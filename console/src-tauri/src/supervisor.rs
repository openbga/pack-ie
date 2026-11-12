//! The background loop that keeps one connection to the engine alive: resolve, connect,
//! greet, re-open the subscriptions, publish the state, wait for the connection to break,
//! back off, repeat.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use packie_ipc::{ClientError, IpcClient, PROTOCOL_VERSION, Request, Response};

use crate::connection::{Hub, Live, REQUEST_TIMEOUT, lock};
use crate::endpoint::resolve_endpoint;
use crate::state::{ConnectionState, EngineInfo, Phase, jitter_percent};

/// How long connecting may take (Windows named pipes can be busy for a while).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// A failed attempt: where it tried, and why it failed.
struct Failed {
    endpoint: Option<String>,
    message: String,
}

impl Failed {
    fn new(endpoint: &str, message: impl Into<String>) -> Self {
        Self {
            endpoint: Some(endpoint.to_owned()),
            message: message.into(),
        }
    }
}

impl Hub {
    /// Runs until the application exits.
    pub(crate) async fn run(self) {
        let mut lost = self.inner.lost.subscribe();
        let mut generation = 0_u64;
        let mut failures = 0_u32;
        let mut immediate = true;
        loop {
            if !immediate {
                self.pause(failures).await;
            }
            generation += 1;
            match self.attempt(generation, failures).await {
                Ok(()) => {
                    failures = 0;
                    immediate = self.hold(generation, &mut lost).await;
                }
                Err(failed) => {
                    failures = failures.saturating_add(1);
                    immediate = false;
                    tracing::debug!(error = %failed.message, attempt = failures, "engine connection attempt failed");
                    self.publish(ConnectionState::next(
                        Phase::Disconnected,
                        failed.endpoint,
                        Some(failed.message),
                        failures,
                    ));
                }
            }
        }
    }

    /// Keeps the connection until it breaks (returns `false`) or the endpoint settings
    /// change (returns `true`: reconnect at once).
    async fn hold(&self, generation: u64, lost: &mut watch::Receiver<u64>) -> bool {
        let settings_changed = tokio::select! {
            _ = async { let _ = lost.wait_for(|g| *g >= generation).await; } => false,
            () = self.inner.wake.notified() => true,
        };
        let endpoint = self.connection().endpoint;
        self.detach().await;
        if !settings_changed {
            let place = endpoint
                .as_deref()
                .map_or_else(String::new, |e| format!(" at {e}"));
            tracing::warn!(
                endpoint = endpoint.as_deref().unwrap_or(""),
                "engine connection lost"
            );
            self.publish(ConnectionState::next(
                Phase::Disconnected,
                endpoint,
                Some(format!("the connection to the engine{place} was lost")),
                0,
            ));
        }
        settings_changed
    }

    /// Waits out the backoff; a settings change cuts it short.
    async fn pause(&self, failures: u32) {
        let delay = self.inner.backoff.delay(failures, jitter_percent());
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = self.inner.wake.notified() => {}
        }
    }

    /// One attempt: leaves the hub connected, or says why not.
    async fn attempt(&self, generation: u64, failures: u32) -> Result<(), Failed> {
        let resolved = resolve_endpoint(&self.resolve_input(), &crate::endpoint::DiskFiles)
            .map_err(|error| Failed {
                endpoint: None,
                message: error.to_string(),
            })?;
        if let Some(loaded) = &resolved.bootstrap {
            tracing::debug!(bootstrap = %loaded.path.display(), "engine endpoint from the bootstrap file");
        }
        let endpoint = resolved.endpoint;
        let previous_error = lock(&self.inner.state).error.clone();
        self.publish(ConnectionState::next(
            Phase::Reconnecting,
            Some(endpoint.clone()),
            previous_error,
            failures,
        ));
        let client =
            match tokio::time::timeout(CONNECT_TIMEOUT, IpcClient::connect(&endpoint)).await {
                Ok(Ok(client)) => client,
                Ok(Err(error)) => return Err(Failed::new(&endpoint, error.to_string())),
                Err(_) => return Err(Failed::new(&endpoint, "timed out connecting to the engine")),
            };
        let engine = greet(&client)
            .await
            .map_err(|m| Failed::new(&endpoint, m))?;
        self.attach(client, engine, generation, endpoint).await
    }

    /// Re-opens every recorded subscription on `client`, then makes it the live connection
    /// and publishes `connected`.
    async fn attach(
        &self,
        client: IpcClient,
        engine: EngineInfo,
        generation: u64,
        endpoint: String,
    ) -> Result<(), Failed> {
        let fail = |message: String| Failed::new(&endpoint, message);
        let mut core = self.inner.core.lock().await;
        let mut opened = Vec::new();
        for (id, topic) in &core.topics {
            match tokio::time::timeout(REQUEST_TIMEOUT, client.subscribe(topic.clone())).await {
                Ok(Ok(stream)) => opened.push((*id, stream)),
                Ok(Err(ClientError::Server(error))) => {
                    tracing::warn!(subscription = *id, %error, "the engine refused a subscription");
                }
                Ok(Err(error)) => return Err(fail(error.to_string())),
                Err(_) => return Err(fail("the engine did not answer in time".into())),
            }
        }
        *lock(&self.inner.live) = Some(Live {
            generation,
            client: Arc::new(client),
            endpoint: endpoint.clone(),
        });
        let mut state = ConnectionState::next(Phase::Connected, Some(endpoint), None, 0);
        state.engine = Some(engine);
        self.publish(state);
        for (id, stream) in opened {
            let forward = self.forward(id, generation, stream);
            core.forwarders.insert(id, forward);
        }
        Ok(())
    }

    /// Drops the live connection and stops its forwarders.
    async fn detach(&self) {
        let mut core = self.inner.core.lock().await;
        for (_, forward) in core.forwarders.drain() {
            forward.task.abort();
        }
        *lock(&self.inner.live) = None;
    }
}

/// Says hello and checks the engine speaks this protocol.
async fn greet(client: &IpcClient) -> Result<EngineInfo, String> {
    let hello = Request::Hello {
        client: format!("packie-console {}", env!("CARGO_PKG_VERSION")),
    };
    match tokio::time::timeout(REQUEST_TIMEOUT, client.request(hello)).await {
        Err(_) => Err("the engine did not answer the greeting in time".into()),
        Ok(Err(error)) => Err(error.to_string()),
        Ok(Ok(Response::Hello {
            engine_version,
            protocol_version,
            node_id,
            node_name,
            capabilities,
            actor,
        })) if protocol_version == PROTOCOL_VERSION => Ok(EngineInfo {
            engine_version,
            protocol_version,
            node_id,
            node_name,
            capabilities,
            actor,
        }),
        Ok(Ok(Response::Hello {
            protocol_version, ..
        })) => Err(format!(
            "the engine speaks protocol version {protocol_version} but this console needs \
             version {PROTOCOL_VERSION}; update the console or the engine"
        )),
        Ok(Ok(_)) => Err("the engine sent an unexpected reply to the greeting".into()),
    }
}
