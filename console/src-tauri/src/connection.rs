//! The shared connection hub: the one engine connection, the subscription table and the
//! state the window sees. The background loop that keeps the connection alive is in
//! `supervisor.rs`; the commands in `commands.rs` are thin calls into this type.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use packie_core::Timestamp;
use packie_ipc::{ClientError, IpcClient, Request, Response, Subscription, Topic};

use crate::endpoint::{self, DiskFiles, ResolveError, ResolveInput, Resolved};
use crate::error::CommandError;
use crate::state::{Backoff, ConnectionState, Emitter, EventPayload, Phase};

/// How long a request to the engine may take before the console gives up on it.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The current connection, as requests see it.
#[derive(Clone)]
pub(crate) struct Live {
    /// Counts connections from 1; tells a stale failure from a current one.
    pub(crate) generation: u64,
    pub(crate) client: Arc<IpcClient>,
    pub(crate) endpoint: String,
}

/// A running forwarder of one engine stream.
pub(crate) struct Forward {
    pub(crate) engine_id: u64,
    pub(crate) generation: u64,
    pub(crate) task: JoinHandle<()>,
}

/// The subscription table. Locked while subscriptions are (re)opened so that a new
/// connection and a new `ipc_subscribe` never interleave.
#[derive(Default)]
pub(crate) struct Core {
    next_id: u64,
    /// Console id to topic: what to re-open after a reconnect.
    pub(crate) topics: BTreeMap<u64, Topic>,
    /// Console id to the forwarder on the current connection.
    pub(crate) forwarders: HashMap<u64, Forward>,
}

pub(crate) struct Inner {
    pub(crate) emitter: Arc<dyn Emitter>,
    pub(crate) backoff: Backoff,
    config_file: Option<PathBuf>,
    pub(crate) state: Mutex<ConnectionState>,
    override_endpoint: Mutex<Option<String>>,
    pub(crate) live: Mutex<Option<Live>>,
    pub(crate) core: tokio::sync::Mutex<Core>,
    /// Wakes the supervisor to reconnect now.
    pub(crate) wake: Notify,
    /// Highest connection generation reported broken.
    pub(crate) lost: watch::Sender<u64>,
}

/// Cheap handle to the shared state; Tauri manages one.
#[derive(Clone)]
pub(crate) struct Hub {
    pub(crate) inner: Arc<Inner>,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Hub {
    /// `config_file` is `console.json`; the endpoint override stored there is loaded.
    pub(crate) fn new(
        emitter: Arc<dyn Emitter>,
        config_file: Option<PathBuf>,
        backoff: Backoff,
    ) -> Self {
        let stored = config_file.as_deref().and_then(endpoint::load_override);
        Self {
            inner: Arc::new(Inner {
                emitter,
                backoff,
                config_file,
                state: Mutex::new(ConnectionState::initial()),
                override_endpoint: Mutex::new(stored),
                live: Mutex::new(None),
                core: tokio::sync::Mutex::new(Core::default()),
                wake: Notify::new(),
                lost: watch::channel(0).0,
            }),
        }
    }

    pub(crate) fn connection(&self) -> ConnectionState {
        lock(&self.inner.state).clone()
    }

    /// Sets or clears the endpoint override and asks the supervisor to reconnect now.
    pub(crate) fn configure(&self, endpoint: Option<String>) {
        let endpoint = endpoint
            .map(|e| e.trim().to_owned())
            .filter(|e| !e.is_empty());
        if let Some(file) = &self.inner.config_file {
            endpoint::save_override(file, endpoint.as_deref());
        }
        *lock(&self.inner.override_endpoint) = endpoint;
        self.inner.wake.notify_one();
    }

    pub(crate) fn resolve_input(&self) -> ResolveInput {
        ResolveInput::from_process(lock(&self.inner.override_endpoint).clone())
    }

    /// The bootstrap document the endpoint resolves from, when it does.
    pub(crate) fn bootstrap(&self) -> Result<Option<serde_json::Value>, CommandError> {
        match endpoint::resolve_endpoint(&self.resolve_input(), &DiskFiles) {
            Ok(Resolved {
                bootstrap: Some(loaded),
                ..
            }) => serde_json::to_value(&loaded.document)
                .map(Some)
                .map_err(|e| CommandError::internal(e.to_string())),
            Ok(_) | Err(ResolveError::NotConfigured) => Ok(None),
            Err(error) => Err(CommandError::internal(error.to_string())),
        }
    }

    /// The bootstrap file the endpoint resolves from, when it does.
    pub(crate) fn bootstrap_path(&self) -> Option<std::path::PathBuf> {
        match endpoint::resolve_endpoint(&self.resolve_input(), &DiskFiles) {
            Ok(Resolved {
                bootstrap: Some(loaded),
                ..
            }) => Some(loaded.path),
            _ => None,
        }
    }

    pub(crate) fn live(&self) -> Option<Live> {
        lock(&self.inner.live).clone()
    }

    /// Reports connection `generation` broken; the supervisor reconnects.
    pub(crate) fn mark_lost(&self, generation: u64) {
        self.inner
            .lost
            .send_modify(|lost| *lost = (*lost).max(generation));
    }

    fn not_connected(&self) -> CommandError {
        let state = self.connection();
        let place = state
            .endpoint
            .map_or_else(String::new, |e| format!(" at {e}"));
        let cause = state.error.map_or_else(String::new, |e| format!(": {e}"));
        CommandError::disconnected(format!("not connected to the engine{place}{cause}"))
    }

    /// Sends one request on the current connection.
    pub(crate) async fn request(&self, request: Request) -> Result<Response, CommandError> {
        if matches!(
            request,
            Request::Subscribe { .. } | Request::Unsubscribe { .. }
        ) {
            return Err(CommandError::internal(
                "subscriptions are managed with ipc_subscribe and ipc_unsubscribe",
            ));
        }
        let live = self.live().ok_or_else(|| self.not_connected())?;
        match tokio::time::timeout(REQUEST_TIMEOUT, live.client.request(request)).await {
            Err(_) => Err(CommandError::internal("the engine did not answer in time")),
            Ok(Ok(response)) => Ok(response),
            Ok(Err(error)) => {
                if CommandError::is_connection_failure(&error) {
                    self.mark_lost(live.generation);
                }
                Err(CommandError::from_client(error, &live.endpoint))
            }
        }
    }

    /// Opens a subscription that survives reconnects; returns its console id. While there is
    /// no connection the topic is only recorded and opened when one is made.
    pub(crate) async fn subscribe(&self, topic: Topic) -> Result<u64, CommandError> {
        let mut core = self.inner.core.lock().await;
        core.next_id += 1;
        let id = core.next_id;
        if let Some(live) = self.live() {
            let opened =
                tokio::time::timeout(REQUEST_TIMEOUT, live.client.subscribe(topic.clone())).await;
            match opened {
                Ok(Ok(subscription)) => {
                    let forward = self.forward(id, live.generation, subscription);
                    core.forwarders.insert(id, forward);
                }
                Ok(Err(ClientError::Server(error))) => return Err(CommandError::Ipc { error }),
                // The supervisor will reconnect and open the recorded topic.
                Ok(Err(_)) | Err(_) => self.mark_lost(live.generation),
            }
        }
        core.topics.insert(id, topic);
        Ok(id)
    }

    /// Forgets subscription `id` and stops it at the engine when connected.
    pub(crate) async fn unsubscribe(&self, id: u64) {
        let mut core = self.inner.core.lock().await;
        core.topics.remove(&id);
        let Some(forward) = core.forwarders.remove(&id) else {
            return;
        };
        forward.task.abort();
        let Some(live) = self.live().filter(|l| l.generation == forward.generation) else {
            return;
        };
        let stop = Request::Unsubscribe {
            subscription: forward.engine_id,
        };
        let sent = tokio::time::timeout(REQUEST_TIMEOUT, live.client.request(stop)).await;
        if !matches!(sent, Ok(Ok(_))) {
            tracing::debug!(
                subscription = id,
                "the engine did not confirm the unsubscribe"
            );
        }
    }

    /// Forwards the engine stream under console id `id`; reports the connection lost when
    /// the stream ends.
    pub(crate) fn forward(&self, id: u64, generation: u64, mut stream: Subscription) -> Forward {
        let engine_id = stream.id;
        let hub = self.clone();
        let task = tokio::spawn(async move {
            while let Some(event) = stream.next().await {
                hub.inner.emitter.event(&EventPayload {
                    subscription: id,
                    event: &event,
                });
            }
            hub.mark_lost(generation);
        });
        Forward {
            engine_id,
            generation,
            task,
        }
    }

    /// Records and announces `next`; `since` moves only when the phase changes.
    pub(crate) fn publish(&self, mut next: ConnectionState) {
        {
            let mut current = lock(&self.inner.state);
            next.since = if current.state == next.state {
                current.since
            } else {
                Timestamp::now()
            };
            *current = next.clone();
        }
        tracing::debug!(
            state = ?next.state,
            endpoint = next.endpoint.as_deref().unwrap_or(""),
            attempt = next.attempt,
            "engine connection state"
        );
        self.inner.emitter.connection(&next);
    }
}

impl ConnectionState {
    /// A state to publish; `since` is set by [`Hub::publish`].
    pub(crate) fn next(
        state: Phase,
        endpoint: Option<String>,
        error: Option<String>,
        attempt: u32,
    ) -> Self {
        Self {
            state,
            endpoint,
            error,
            attempt,
            engine: None,
            since: Timestamp::now(),
        }
    }
}
