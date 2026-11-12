//! The IPC client used by the CLI and the console.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::frame::{FrameError, read_json, write_all};
use crate::protocol::{ClientFrame, Event, IpcError, Request, Response, ServerFrame, Topic};
use crate::transport::{TransportError, connect};

/// Events buffered per subscription before the reader waits for the consumer.
const EVENT_BUFFER: usize = 256;

/// A client request failed.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The engine could not be reached.
    #[error("cannot reach the engine: {0}")]
    Connect(#[from] TransportError),
    /// The connection broke.
    #[error(transparent)]
    Frame(#[from] FrameError),
    /// The engine closed the connection.
    #[error("the engine closed the connection")]
    Closed,
    /// The engine answered with an error.
    #[error(transparent)]
    Server(#[from] IpcError),
    /// The engine answered with a response of the wrong type.
    #[error("unexpected response from the engine")]
    UnexpectedResponse,
}

type Reply = oneshot::Sender<Result<Response, IpcError>>;

#[derive(Default)]
struct Routes {
    pending: HashMap<u64, Reply>,
    /// Open subscriptions, by id.
    streams: HashMap<u64, mpsc::Sender<Event>>,
    /// Receivers created by the reader for a `Subscribed` answer, until `subscribe`
    /// picks them up.
    opened: HashMap<u64, mpsc::Receiver<Event>>,
}

/// A connection to the engine. Requests may be issued concurrently.
pub struct IpcClient {
    outbox: mpsc::Sender<ClientFrame>,
    routes: Arc<Mutex<Routes>>,
    next_id: AtomicU64,
    tasks: [JoinHandle<()>; 2],
}

impl std::fmt::Debug for IpcClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IpcClient").finish_non_exhaustive()
    }
}

/// An open subscription.
#[derive(Debug)]
pub struct Subscription {
    /// Its id on this connection.
    pub id: u64,
    events: mpsc::Receiver<Event>,
}

impl Subscription {
    /// The next event; `None` when the connection closed.
    pub async fn next(&mut self) -> Option<Event> {
        self.events.recv().await
    }
}

impl IpcClient {
    /// Connects to the engine at `endpoint` (socket path or pipe name).
    ///
    /// # Errors
    ///
    /// [`ClientError::Connect`] when the engine cannot be reached.
    pub async fn connect(endpoint: &str) -> Result<Self, ClientError> {
        let stream = connect(endpoint).await?;
        Ok(Self::from_stream(stream))
    }

    /// Wraps an already connected stream.
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: AsyncRead + AsyncWrite + Send + 'static,
    {
        let (mut reader, mut writer) = tokio::io::split(stream);
        let (outbox, mut outgoing) = mpsc::channel::<ClientFrame>(64);
        let routes = Arc::new(Mutex::new(Routes::default()));

        let writer_task = tokio::spawn(async move {
            let _ = write_all(&mut writer, &mut outgoing).await;
        });
        let reader_routes = Arc::clone(&routes);
        let reader_task = tokio::spawn(async move {
            while let Ok(Some(frame)) = read_json::<ServerFrame, _>(&mut reader).await {
                route(&reader_routes, frame).await;
            }
            // Dropping every reply and stream wakes waiters with "closed".
            let mut routes = reader_routes.lock().unwrap_or_else(PoisonError::into_inner);
            *routes = Routes::default();
        });
        Self {
            outbox,
            routes,
            next_id: AtomicU64::new(1),
            tasks: [writer_task, reader_task],
        }
    }

    /// Sends `request` and waits for its answer.
    ///
    /// # Errors
    ///
    /// [`ClientError::Server`] when the engine answers with an error;
    /// [`ClientError::Closed`] when the connection ends first.
    pub async fn request(&self, request: Request) -> Result<Response, ClientError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (reply, answer) = oneshot::channel();
        self.lock().pending.insert(id, reply);
        if self.outbox.send(ClientFrame { id, request }).await.is_err() {
            self.lock().pending.remove(&id);
            return Err(ClientError::Closed);
        }
        Ok(answer.await.map_err(|_| ClientError::Closed)??)
    }

    /// Opens a subscription to `topic`.
    ///
    /// # Errors
    ///
    /// As for [`request`](Self::request); [`ClientError::UnexpectedResponse`] when the
    /// engine answers with something other than a subscription.
    pub async fn subscribe(&self, topic: Topic) -> Result<Subscription, ClientError> {
        match self.request(Request::Subscribe { topic }).await? {
            Response::Subscribed { subscription } => {
                let events = self
                    .lock()
                    .opened
                    .remove(&subscription)
                    .ok_or(ClientError::Closed)?;
                Ok(Subscription {
                    id: subscription,
                    events,
                })
            }
            _ => Err(ClientError::UnexpectedResponse),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Routes> {
        self.routes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for IpcClient {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

async fn route(routes: &Mutex<Routes>, frame: ServerFrame) {
    let lock = || routes.lock().unwrap_or_else(PoisonError::into_inner);
    match frame {
        ServerFrame::Response { id, result } => {
            let mut routes = lock();
            // Register the stream before the caller learns the id, so no event is lost.
            if let Response::Subscribed { subscription } = &result {
                let (tx, rx) = mpsc::channel(EVENT_BUFFER);
                routes.streams.insert(*subscription, tx);
                routes.opened.insert(*subscription, rx);
            }
            if let Some(reply) = routes.pending.remove(&id) {
                let _ = reply.send(Ok(result));
            }
        }
        ServerFrame::Error { id, error } => {
            if let Some(reply) = lock().pending.remove(&id) {
                let _ = reply.send(Err(error));
            }
        }
        ServerFrame::Event {
            subscription,
            event,
        } => {
            let stream = lock().streams.get(&subscription).cloned();
            if let Some(stream) = stream
                && stream.send(event).await.is_err()
            {
                lock().streams.remove(&subscription);
            }
        }
    }
}
