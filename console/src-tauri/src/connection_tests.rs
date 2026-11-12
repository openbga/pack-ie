//! The hub and supervisor against an in-process engine on a temporary Unix socket.
#![cfg(unix)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use packie_ipc::{
    ErrorCode, Event, Handler, IpcError, IpcServer, PROTOCOL_VERSION, Request, Response, Topic,
};

use crate::connection::Hub;
use crate::endpoint::save_override;
use crate::error::CommandError;
use crate::state::{Backoff, ConnectionState, Emitter, EventPayload, Phase};

const WAIT: Duration = Duration::from_secs(10);

/// An engine that greets, refuses channel commands and streams what the test pushes.
struct FakeEngine {
    protocol: u32,
    streams: Mutex<Vec<mpsc::Sender<Event>>>,
}

impl FakeEngine {
    fn new(protocol: u32) -> Arc<Self> {
        Arc::new(Self {
            protocol,
            streams: Mutex::new(Vec::new()),
        })
    }

    async fn push(&self, event: Event) {
        let streams = self.streams.lock().unwrap().clone();
        for stream in streams {
            let _ = stream.send(event.clone()).await;
        }
    }
}

impl Handler for FakeEngine {
    async fn handle(&self, request: Request) -> Result<Response, IpcError> {
        match request {
            Request::Hello { .. } => Ok(Response::Hello {
                engine_version: "9.9".into(),
                protocol_version: self.protocol,
                node_id: "n1".into(),
                node_name: "Node".into(),
                capabilities: Vec::new(),
                actor: Some(Box::new(packie_core::Actor::system())),
            }),
            Request::GetConfigHistory => Ok(Response::ConfigHistory {
                versions: Vec::new(),
            }),
            _ => Err(IpcError::new(ErrorCode::NotFound, "no such channel")),
        }
    }

    fn subscribe(&self, topic: Topic) -> Result<mpsc::Receiver<Event>, IpcError> {
        if topic != Topic::Alerts {
            return Err(IpcError::new(ErrorCode::NotFound, "no such topic"));
        }
        let (tx, rx) = mpsc::channel(16);
        self.streams.lock().unwrap().push(tx);
        Ok(rx)
    }
}

struct Collector {
    states: mpsc::UnboundedSender<ConnectionState>,
    events: mpsc::UnboundedSender<(u64, Event)>,
}

impl Emitter for Collector {
    fn connection(&self, state: &ConnectionState) {
        let _ = self.states.send(state.clone());
    }

    fn event(&self, payload: &EventPayload<'_>) {
        let _ = self
            .events
            .send((payload.subscription, payload.event.clone()));
    }
}

fn serve(endpoint: &str, engine: &Arc<FakeEngine>) -> JoinHandle<()> {
    let server = IpcServer::bind(endpoint).unwrap();
    let engine = Arc::clone(engine);
    tokio::spawn(async move {
        let _ = server.serve(engine).await;
    })
}

async fn stop(server: JoinHandle<()>) {
    server.abort();
    let _ = server.await;
}

struct Rig {
    hub: Hub,
    states: mpsc::UnboundedReceiver<ConnectionState>,
    events: mpsc::UnboundedReceiver<(u64, Event)>,
    dir: tempfile::TempDir,
    config_file: std::path::PathBuf,
    supervisor: JoinHandle<()>,
}

impl Rig {
    fn socket(&self, name: &str) -> String {
        self.dir.path().join(name).to_string_lossy().into_owned()
    }

    async fn state_where(
        &mut self,
        what: &str,
        test: impl Fn(&ConnectionState) -> bool,
    ) -> ConnectionState {
        tokio::time::timeout(WAIT, async {
            let mut seen = self.states.recv().await.expect("emitter closed");
            while !test(&seen) {
                seen = self.states.recv().await.expect("emitter closed");
            }
            seen
        })
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {what}"))
    }

    async fn phase(&mut self, phase: Phase) -> ConnectionState {
        self.state_where(&format!("{phase:?}"), |s| s.state == phase)
            .await
    }

    async fn event(&mut self) -> (u64, Event) {
        tokio::time::timeout(WAIT, self.events.recv())
            .await
            .expect("timed out waiting for an event")
            .expect("emitter closed")
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.supervisor.abort();
    }
}

/// Starts a hub whose stored override names `socket` and runs its supervisor.
fn rig(socket: &str) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let config_file = dir.path().join("console.json");
    let socket = dir.path().join(socket).to_string_lossy().into_owned();
    save_override(&config_file, Some(&socket));
    let (states_tx, states) = mpsc::unbounded_channel();
    let (events_tx, events) = mpsc::unbounded_channel();
    let emitter = Arc::new(Collector {
        states: states_tx,
        events: events_tx,
    });
    let backoff = Backoff {
        base: Duration::from_millis(20),
        cap: Duration::from_millis(100),
    };
    let hub = Hub::new(emitter, Some(config_file.clone()), backoff);
    let supervisor = tokio::spawn(hub.clone().run());
    Rig {
        hub,
        states,
        events,
        dir,
        config_file,
        supervisor,
    }
}

#[tokio::test]
async fn connects_greets_and_publishes_states() {
    let engine = FakeEngine::new(PROTOCOL_VERSION);
    let mut rig = rig("a.sock");
    let socket = rig.socket("a.sock");
    // The first attempt finds no engine.
    let failed = rig
        .state_where("a failed attempt", |s| s.attempt >= 1)
        .await;
    assert_eq!(failed.state, Phase::Disconnected);
    assert_eq!(failed.endpoint.as_deref(), Some(socket.as_str()));
    assert!(failed.error.is_some());

    let server = serve(&socket, &engine);
    let connected = rig.phase(Phase::Connected).await;
    assert_eq!(connected.attempt, 0);
    assert_eq!(connected.error, None);
    let info = connected.engine.clone().unwrap();
    assert_eq!(
        (info.engine_version.as_str(), info.node_name.as_str()),
        ("9.9", "Node")
    );
    assert_eq!(info.protocol_version, PROTOCOL_VERSION);
    assert_eq!(
        info.actor.map(|actor| actor.name.clone()),
        Some("system".to_owned())
    );
    assert_eq!(rig.hub.connection(), connected);
    stop(server).await;
}

#[tokio::test]
async fn requests_pass_through_and_engine_errors_keep_their_code() {
    let engine = FakeEngine::new(PROTOCOL_VERSION);
    let mut rig = rig("a.sock");
    let server = serve(&rig.socket("a.sock"), &engine);
    rig.phase(Phase::Connected).await;

    let answer = rig.hub.request(Request::GetConfigHistory).await.unwrap();
    assert_eq!(
        answer,
        Response::ConfigHistory {
            versions: Vec::new()
        }
    );

    let refused = rig.hub.request(Request::GetSnapshot).await.unwrap_err();
    assert!(matches!(refused, CommandError::Ipc { error } if error.code == ErrorCode::NotFound));

    let misuse = rig
        .hub
        .request(Request::Subscribe {
            topic: Topic::Alerts,
        })
        .await;
    assert!(matches!(misuse, Err(CommandError::Internal { .. })));

    let refused = rig.hub.subscribe(Topic::Snapshots).await.unwrap_err();
    assert!(matches!(refused, CommandError::Ipc { .. }));
    stop(server).await;
}

#[tokio::test]
async fn subscription_events_carry_the_console_id() {
    let engine = FakeEngine::new(PROTOCOL_VERSION);
    let mut rig = rig("a.sock");
    let server = serve(&rig.socket("a.sock"), &engine);
    rig.phase(Phase::Connected).await;

    let first = rig.hub.subscribe(Topic::Alerts).await.unwrap();
    let second = rig.hub.subscribe(Topic::Alerts).await.unwrap();
    assert_ne!(first, second);
    engine.push(Event::Lagged { missed: 1 }).await;
    let mut got = vec![rig.event().await, rig.event().await];
    got.sort_by_key(|(id, _)| *id);
    assert_eq!(
        got,
        vec![
            (first, Event::Lagged { missed: 1 }),
            (second, Event::Lagged { missed: 1 })
        ]
    );

    rig.hub.unsubscribe(first).await;
    engine.push(Event::Lagged { missed: 2 }).await;
    assert_eq!(rig.event().await, (second, Event::Lagged { missed: 2 }));
    assert!(rig.events.try_recv().is_err());
    stop(server).await;
}

#[tokio::test]
async fn reconnects_and_resumes_subscriptions_under_the_same_ids() {
    let first_engine = FakeEngine::new(PROTOCOL_VERSION);
    let mut rig = rig("a.sock");
    let socket = rig.socket("a.sock");
    let server = serve(&socket, &first_engine);
    rig.phase(Phase::Connected).await;
    let kept = rig.hub.subscribe(Topic::Alerts).await.unwrap();

    stop(server).await;
    let lost = rig.phase(Phase::Disconnected).await;
    assert!(lost.error.unwrap().contains("lost"));
    // Requests fail fast while there is no connection.
    let result = rig.hub.request(Request::GetConfigHistory).await;
    assert!(matches!(result, Err(CommandError::Disconnected { .. })));
    // A subscription made during the outage is opened on reconnect.
    let during = rig.hub.subscribe(Topic::Alerts).await.unwrap();
    assert!(during > kept);
    let retrying = rig.state_where("a retry", |s| s.attempt >= 1).await;
    assert!(retrying.error.is_some());

    let second_engine = FakeEngine::new(PROTOCOL_VERSION);
    let server = serve(&socket, &second_engine);
    let again = rig.phase(Phase::Connected).await;
    assert_eq!(again.attempt, 0);
    second_engine.push(Event::Lagged { missed: 7 }).await;
    let mut got = vec![rig.event().await, rig.event().await];
    got.sort_by_key(|(id, _)| *id);
    assert_eq!(
        got,
        vec![
            (kept, Event::Lagged { missed: 7 }),
            (during, Event::Lagged { missed: 7 })
        ]
    );
    assert!(rig.hub.request(Request::GetConfigHistory).await.is_ok());
    stop(server).await;
}

#[tokio::test]
async fn an_engine_with_another_protocol_version_is_rejected() {
    let engine = FakeEngine::new(PROTOCOL_VERSION + 1);
    let mut rig = rig("a.sock");
    let server = serve(&rig.socket("a.sock"), &engine);
    let rejected = rig
        .state_where("a protocol error", |s| {
            s.error
                .as_deref()
                .is_some_and(|e| e.contains("protocol version"))
        })
        .await;
    assert_eq!(rejected.state, Phase::Disconnected);
    assert!(rejected.engine.is_none());
    assert!(rig.hub.live().is_none());
    stop(server).await;
}

#[tokio::test]
async fn configure_switches_engines_and_stores_the_override() {
    let (engine_a, engine_b) = (
        FakeEngine::new(PROTOCOL_VERSION),
        FakeEngine::new(PROTOCOL_VERSION),
    );
    let mut rig = rig("a.sock");
    let server_a = serve(&rig.socket("a.sock"), &engine_a);
    let socket_b = rig.socket("b.sock");
    let server_b = serve(&socket_b, &engine_b);
    let on_a = rig.phase(Phase::Connected).await;
    assert_eq!(on_a.endpoint, Some(rig.socket("a.sock")));

    rig.hub.configure(Some(socket_b.clone()));
    let on_b = rig
        .state_where("engine b", |s| {
            s.state == Phase::Connected && s.endpoint.as_deref() == Some(socket_b.as_str())
        })
        .await;
    assert_eq!(on_b.attempt, 0);
    assert_eq!(
        crate::endpoint::load_override(&rig.config_file).as_deref(),
        Some(socket_b.as_str())
    );
    stop(server_a).await;
    stop(server_b).await;
}
