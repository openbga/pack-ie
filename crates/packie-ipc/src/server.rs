//! The IPC server: accepts local connections and dispatches requests to a [`Handler`].

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use packie_core::{Actor, ActorOrigin};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

use crate::frame::{FrameError, read_frame, write_all};
use crate::protocol::{ClientFrame, ErrorCode, Event, IpcError, Request, Response, ServerFrame};
use crate::transport::{Listener, PeerIdentity, Stream, TransportError, peer_identity};

/// Frames queued for one client before the engine waits for it.
const OUTBOX: usize = 256;

/// What the engine implements to serve IPC. The server handles framing, request ids and
/// subscription bookkeeping (including [`Request::Unsubscribe`]); the handler answers
/// everything else.
pub trait Handler: Send + Sync + 'static {
    /// Answers one request. [`Request::Subscribe`] and [`Request::Unsubscribe`] never
    /// reach it.
    fn handle(&self, request: Request) -> impl Future<Output = Result<Response, IpcError>> + Send;

    /// Like [`handle`](Self::handle), with the connection's [`ClientInfo`]: the OS account
    /// behind it and what the client said about itself in [`Request::Hello`]. The server
    /// calls this one; the default ignores the client.
    fn handle_from(
        &self,
        request: Request,
        client: &ClientInfo,
    ) -> impl Future<Output = Result<Response, IpcError>> + Send {
        let _ = client;
        self.handle(request)
    }

    /// Opens a stream for `topic`. The stream ends when the returned receiver's sender is
    /// dropped; the server drops the receiver when the client unsubscribes or leaves, so
    /// a forwarding task should stop when its `send` fails.
    ///
    /// # Errors
    ///
    /// An [`IpcError`] sent to the client when the stream cannot be opened.
    fn subscribe(&self, topic: crate::protocol::Topic) -> Result<mpsc::Receiver<Event>, IpcError>;

    /// Like [`subscribe`](Self::subscribe), with the connection's [`ClientInfo`], so a
    /// content stream (live traffic) can be attributed and audited. The server calls this
    /// one; the default ignores the client.
    ///
    /// # Errors
    ///
    /// As for [`subscribe`](Self::subscribe).
    fn subscribe_from(
        &self,
        topic: crate::protocol::Topic,
        client: &ClientInfo,
    ) -> Result<mpsc::Receiver<Event>, IpcError> {
        let _ = client;
        self.subscribe(topic)
    }

    /// A connection was refused because the OS could not say who is behind it; `reason`
    /// is operator text. The default does nothing.
    fn refused(&self, reason: &str) {
        let _ = reason;
    }
}

/// Who is behind a connection: the OS account read from the connection, and what the client
/// said about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInfo {
    /// The `client` of its [`Request::Hello`], such as `packie-console 0.1.0`.
    pub name: Option<String>,
    /// The OS account, with the origin the hello declared (`unknown` before it).
    pub actor: Actor,
}

impl ClientInfo {
    /// A connection from `actor` that has not said hello yet.
    #[must_use]
    pub fn new(actor: Actor) -> Self {
        Self { name: None, actor }
    }

    /// Records the hello's client name and the origin it implies.
    pub fn hello(&mut self, client: &str) {
        self.name = Some(client.to_owned());
        self.actor.origin = match self.origin().as_str() {
            "console" => ActorOrigin::Console,
            "cli" => ActorOrigin::Cli,
            "sim" => ActorOrigin::Sim,
            _ => ActorOrigin::Unknown,
        };
    }

    /// The origin recorded in `operator.command` events: the first word of the client
    /// name without its `packie-` prefix (`console`, `cli`, `sim`), or `unknown`.
    #[must_use]
    pub fn origin(&self) -> String {
        self.name
            .as_deref()
            .and_then(|n| n.split_whitespace().next())
            .map(|word| word.strip_prefix("packie-").unwrap_or(word).to_owned())
            .filter(|word| !word.is_empty())
            .unwrap_or_else(|| "unknown".to_owned())
    }
}

/// A bound IPC endpoint, ready to serve.
#[derive(Debug)]
pub struct IpcServer {
    listener: Listener,
}

impl IpcServer {
    /// Binds `endpoint` (a socket path on Unix, a pipe name on Windows). Fails when
    /// another engine already serves it; a stale socket file is replaced.
    ///
    /// # Errors
    ///
    /// [`TransportError::InUse`] when another engine serves `endpoint`;
    /// [`TransportError::Io`] when binding fails.
    pub fn bind(endpoint: &str) -> Result<Self, TransportError> {
        Self::bind_shared(endpoint, None)
    }

    /// Like [`bind`](Self::bind); on Unix, members of `group` may connect as well (socket
    /// `0660` owned by the group). When the group does not exist or cannot be given the
    /// socket, the socket stays owner-only and a warning is logged.
    ///
    /// # Errors
    ///
    /// As for [`bind`](Self::bind).
    pub fn bind_shared(endpoint: &str, group: Option<&str>) -> Result<Self, TransportError> {
        Ok(Self {
            listener: Listener::bind(endpoint, group)?,
        })
    }

    /// Serves connections until the returned future is dropped; dropping it closes every
    /// connection.
    ///
    /// # Errors
    ///
    /// [`TransportError::Io`] when accepting connections fails.
    pub async fn serve<H: Handler>(self, handler: Arc<H>) -> Result<(), TransportError> {
        self.serve_identified(handler, peer_identity).await
    }

    /// [`serve`](Self::serve) with the function that reads a connection's OS identity, so a
    /// test can make it fail.
    async fn serve_identified<H, F>(
        mut self,
        handler: Arc<H>,
        identify: F,
    ) -> Result<(), TransportError>
    where
        H: Handler,
        F: Fn(&Stream) -> Result<PeerIdentity, String>,
    {
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    let stream = accepted?;
                    match identify(&stream) {
                        Ok(peer) => {
                            connections.spawn(connection(stream, peer, Arc::clone(&handler)));
                        }
                        Err(reason) => refuse(&*handler, &reason),
                    }
                }
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
            }
        }
    }
}

/// Drops a connection whose OS identity cannot be read: every action must be attributable.
fn refuse<H: Handler>(handler: &H, reason: &str) {
    tracing::warn!(
        event = "ipc.refused",
        error_summary = reason,
        "Management connection refused: {reason}"
    );
    handler.refused(reason);
}

async fn connection<S, H>(stream: S, peer: PeerIdentity, handler: Arc<H>)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
    H: Handler,
{
    let actor = Actor {
        name: peer.name,
        uid_or_sid: peer.uid_or_sid,
        origin: ActorOrigin::Unknown,
        pid: peer.pid,
    };
    tracing::info!(
        event = "ipc.connected",
        operator_account = %actor.name,
        uid_or_sid = %actor.uid_or_sid,
        pid = actor.pid,
        "Management connection from {}",
        actor.name
    );
    let (mut reader, mut writer) = tokio::io::split(stream);
    let (outbox, mut outgoing) = mpsc::channel::<ServerFrame>(OUTBOX);

    let write = write_frames(&mut writer, &mut outgoing);
    let mut session = Session {
        handler: &*handler,
        client: ClientInfo::new(actor),
        outbox: outbox.clone(),
        subscriptions: HashMap::new(),
        next_subscription: 1,
    };
    let read = session.read_frames(&mut reader);
    tokio::select! {
        () = read => {}
        () = write => {}
    }
    session.close();
    tracing::debug!(
        event = "ipc.client_disconnected",
        operator_account = %session.client.actor.name,
        "IPC client disconnected"
    );
}

async fn write_frames<W: AsyncWrite + Unpin>(
    writer: &mut W,
    outgoing: &mut mpsc::Receiver<ServerFrame>,
) {
    if let Some(error) = write_all(writer, outgoing).await {
        tracing::debug!(event = "ipc.write_failed", %error, "IPC client write failed");
    }
}

/// Forwards one subscription's events until either side goes away.
async fn forward(
    subscription: u64,
    mut events: mpsc::Receiver<Event>,
    outbox: mpsc::Sender<ServerFrame>,
) {
    while let Some(event) = events.recv().await {
        let frame = ServerFrame::Event {
            subscription,
            event,
        };
        if outbox.send(frame).await.is_err() {
            break;
        }
    }
}

struct Session<'a, H> {
    handler: &'a H,
    client: ClientInfo,
    outbox: mpsc::Sender<ServerFrame>,
    subscriptions: HashMap<u64, JoinHandle<()>>,
    next_subscription: u64,
}

impl<H: Handler> Session<'_, H> {
    /// Reads and dispatches frames until the client leaves or breaks the framing.
    async fn read_frames<R: AsyncRead + Unpin>(&mut self, reader: &mut R) {
        loop {
            let error = match read_frame(reader).await {
                Ok(Some(body)) if self.dispatch(&body).await => continue,
                Ok(_) => return,
                Err(error) => error,
            };
            tracing::debug!(event = "ipc.read_failed", %error, "IPC client read failed");
            if let FrameError::TooLarge(_) = error {
                self.reply_error(0, ErrorCode::BadRequest, &error.to_string())
                    .await;
            }
            return;
        }
    }

    /// Handles one frame; `false` when the client is gone.
    async fn dispatch(&mut self, body: &[u8]) -> bool {
        // Read the id first so a malformed request still gets an answer it can match.
        let value: Value = match serde_json::from_slice(body) {
            Ok(value) => value,
            Err(error) => {
                return self
                    .reply_error(0, ErrorCode::BadRequest, &error.to_string())
                    .await;
            }
        };
        let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
        let frame: ClientFrame = match serde_json::from_value(value) {
            Ok(frame) => frame,
            Err(error) => {
                return self
                    .reply_error(id, ErrorCode::BadRequest, &error.to_string())
                    .await;
            }
        };
        let result = match frame.request {
            Request::Subscribe { topic } => return self.subscribe(id, topic).await,
            Request::Unsubscribe { subscription } => self.unsubscribe(subscription),
            request => {
                if let Request::Hello { client } = &request {
                    self.client.hello(client);
                }
                self.handler.handle_from(request, &self.client).await
            }
        };
        self.answer(id, result).await
    }

    /// Queues the answer to request `id`; `false` when the client is gone.
    async fn answer(&self, id: u64, result: Result<Response, IpcError>) -> bool {
        let frame = match result {
            Ok(result) => ServerFrame::Response { id, result },
            Err(error) => ServerFrame::Error { id, error },
        };
        self.outbox.send(frame).await.is_ok()
    }

    async fn reply_error(&self, id: u64, code: ErrorCode, message: &str) -> bool {
        let error = IpcError::new(code, format!("Malformed request: {message}"));
        self.answer(id, Err(error)).await
    }

    /// Opens a subscription and answers request `id`; `false` when the client is gone.
    async fn subscribe(&mut self, id: u64, topic: crate::protocol::Topic) -> bool {
        let events = match self.handler.subscribe_from(topic, &self.client) {
            Ok(events) => events,
            Err(error) => return self.answer(id, Err(error)).await,
        };
        let subscription = self.next_subscription;
        self.next_subscription += 1;
        // Queue the answer before any event, so the client knows the id first.
        if !self
            .answer(id, Ok(Response::Subscribed { subscription }))
            .await
        {
            return false;
        }
        let task = tokio::spawn(forward(subscription, events, self.outbox.clone()));
        self.subscriptions.insert(subscription, task);
        true
    }

    fn unsubscribe(&mut self, subscription: u64) -> Result<Response, IpcError> {
        match self.subscriptions.remove(&subscription) {
            Some(task) => {
                task.abort();
                Ok(Response::Done)
            }
            None => Err(IpcError::new(
                ErrorCode::NotFound,
                format!("No subscription {subscription} on this connection."),
            )),
        }
    }

    fn close(&mut self) {
        for (_, task) in self.subscriptions.drain() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClientInfo;

    #[test]
    fn origin_is_the_client_kind() {
        use packie_core::ActorOrigin;

        let origin = |name: Option<&str>| {
            let mut client = ClientInfo::new(packie_core::Actor::system());
            if let Some(name) = name {
                client.hello(name);
            }
            (client.origin(), client.actor.origin)
        };
        assert_eq!(
            origin(Some("packie-console 0.1.0")),
            ("console".to_owned(), ActorOrigin::Console)
        );
        assert_eq!(
            origin(Some("packie-cli 0.1.0")),
            ("cli".to_owned(), ActorOrigin::Cli)
        );
        assert_eq!(
            origin(Some("scripts")),
            ("scripts".to_owned(), ActorOrigin::Unknown)
        );
        assert_eq!(origin(None), ("unknown".to_owned(), ActorOrigin::System));
    }

    /// Records refused connections and counts the requests that reach the handler.
    #[cfg(unix)]
    #[derive(Default)]
    struct Recorder {
        refused: std::sync::Mutex<Vec<String>>,
        handled: std::sync::atomic::AtomicUsize,
    }

    #[cfg(unix)]
    impl super::Handler for Recorder {
        async fn handle(
            &self,
            _request: crate::Request,
        ) -> Result<crate::Response, crate::IpcError> {
            self.handled
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::Response::Done)
        }

        fn subscribe(
            &self,
            _topic: crate::Topic,
        ) -> Result<tokio::sync::mpsc::Receiver<crate::Event>, crate::IpcError> {
            Err(crate::IpcError::not_implemented("Streams"))
        }

        fn refused(&self, reason: &str) {
            self.refused.lock().unwrap().push(reason.to_owned());
        }
    }

    /// The identity lookup of the test server: the first connection's fails the way a
    /// failing `getsockopt(SO_PEERCRED)` does, later ones read as a fixed account.
    #[cfg(unix)]
    fn first_unreadable() -> impl Fn(&super::Stream) -> Result<super::PeerIdentity, String> {
        let first = std::sync::atomic::AtomicBool::new(true);
        move |_| {
            if first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
                Err(format!("the peer's credentials cannot be read: {error}"))
            } else {
                Ok(super::PeerIdentity {
                    name: "lab".into(),
                    uid_or_sid: "501".into(),
                    pid: Some(4242),
                })
            }
        }
    }

    /// Acceptance L-03: a connection whose OS identity cannot be read is closed before any
    /// request reaches the handler and reported once, with the OS error, to the handler
    /// (which records `access_refused`); a later connection is served normally.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_connection_without_an_identity_is_refused_before_any_request() {
        use std::sync::Arc;
        use std::sync::atomic::Ordering;

        use crate::{IpcClient, IpcServer, Request, Response};

        let dir = tempfile::tempdir().unwrap();
        let endpoint = dir
            .path()
            .join("packie.sock")
            .to_string_lossy()
            .into_owned();
        let server = IpcServer::bind(&endpoint).unwrap();
        let handler = Arc::new(Recorder::default());
        let serving =
            tokio::spawn(server.serve_identified(Arc::clone(&handler), first_unreadable()));
        let hello = || Request::Hello {
            client: "packie-cli test".into(),
        };

        let refused = IpcClient::connect(&endpoint).await.unwrap();
        assert!(
            refused.request(hello()).await.is_err(),
            "the connection is dropped"
        );
        assert_eq!(handler.handled.load(Ordering::SeqCst), 0, "no request ran");
        assert_eq!(
            *handler.refused.lock().unwrap(),
            ["the peer's credentials cannot be read: permission denied"]
        );

        let good = IpcClient::connect(&endpoint).await.unwrap();
        assert_eq!(good.request(hello()).await.unwrap(), Response::Done);
        assert_eq!(handler.handled.load(Ordering::SeqCst), 1);
        assert_eq!(handler.refused.lock().unwrap().len(), 1, "refused once");
        serving.abort();
    }
}
