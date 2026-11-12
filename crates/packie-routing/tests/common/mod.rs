//! Shared harness for the scenario-shaped tests: an engine context in a temporary state
//! directory, configuration builders, polling assertions, scripted peers and the
//! message-channel rig over the durable store. Timers are short and jitter is 0 so runs
//! are deterministic.

#![allow(
    dead_code,
    unreachable_pub,
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used
)]

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use packie_config::{ApplyOutcome, ApplyReport, ConnectStrategy, TapFramer};
use packie_config::{Bootstrap, ConfigDiff, RuntimeConfig};
use packie_core::{
    AckLevel, BridgeSide, ChannelState, ConnectorId, DeliveryId, DeliveryState, EndpointKind,
    EndpointState, MessageId, SessionState, StopReason, Timestamp,
};
use packie_core::{ChannelId, EndpointId};
use packie_diagnostics::{ChannelSnapshot, EndpointSnapshot, TrafficTap};
use packie_diagnostics::{EndpointRef, FlowSnapshot};
use packie_engine::{ChannelRegistry, EngineContext};
use packie_hl7::{Message, mllp};
use packie_ipc::messages::{
    BodyRequest, CloseRequest, DeliveryRecord, MessageBody, MessageList, MessageQuery,
    MessageRecord, ResendRequest, ResendResult,
};
use packie_routing::memory::{MemoryListener, MemoryNetwork};
use packie_routing::message::store::{
    AttemptFinish, DestinationPolicy, Identifiers, NewDelivery, PendingDelivery, QueueCounts,
    Receipt, ReceiptOutcome, Reconciliation, RetentionPolicy, StoreError, StoreResult,
    StoredMessage,
};
use packie_routing::message::{MessageStore, SqliteStore};
use packie_routing::{RegistryOptions, RoutingRegistry};
use packie_store::{OpenOptions, Store};
use packie_transport::{BoxFuture, MapSecrets, SystemTransports, Transports};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::sync::{Notify, watch};

/// How long polling assertions wait.
pub const WITHIN: Duration = Duration::from_secs(10);

pub struct Harness {
    pub registry: RoutingRegistry,
    pub context: EngineContext,
    pub config: RuntimeConfig,
    pub dir: Arc<tempfile::TempDir>,
    /// What the last apply reported per channel.
    pub report: ApplyReport,
    options: RegistryOptions,
}

pub fn context(dir: &tempfile::TempDir) -> EngineContext {
    let bootstrap: Bootstrap = serde_json::from_value(json!({
        "node": {"node_id": "test-node", "display_name": "Test node"},
        "paths": {"state_directory": dir.path(), "log_directory": dir.path().join("logs")},
    }))
    .expect("bootstrap");
    EngineContext {
        bootstrap: Arc::new(bootstrap),
        state_dir: dir.path().to_owned(),
        tap: TrafficTap::new(),
        snapshot_now: Arc::new(Notify::new()),
        test_hooks: None,
        secrets: Arc::new(packie_config::MapSecrets::default()),
    }
}

impl Harness {
    pub fn new(transports: Arc<dyn Transports>) -> Self {
        Self::with_options(RegistryOptions::new(transports))
    }

    pub fn with_options(options: RegistryOptions) -> Self {
        let dir = Arc::new(tempfile::tempdir().expect("tempdir"));
        Self::in_dir(dir, options)
    }

    /// A fresh registry on an existing state directory (an engine restart).
    pub fn in_dir(dir: Arc<tempfile::TempDir>, options: RegistryOptions) -> Self {
        let context = context(&dir);
        Self {
            registry: RoutingRegistry::new(&context, options.clone()),
            context,
            config: RuntimeConfig::empty(),
            dir,
            report: ApplyReport::default(),
            options,
        }
    }

    /// Restarts: shuts this registry down and builds a new one on the same state.
    pub async fn restart_engine(self) -> Self {
        self.registry.shutdown(Duration::from_millis(200)).await;
        let mut next = Self::in_dir(Arc::clone(&self.dir), self.options.clone());
        next.config.config_version = 0;
        next
    }

    /// Applies `document` (endpoints and channels) as the next version.
    pub async fn apply(&mut self, document: Value) -> ConfigDiff {
        let mut document = document;
        document["config_version"] = json!(self.config.config_version + 1);
        document["generated_at"] = json!("2026-11-06T00:00:00.000000000Z");
        let config = RuntimeConfig::from_value(document).expect("valid runtime document");
        let diff = ConfigDiff::between(&self.config, &config);
        self.report = self.registry.apply(&config, &diff).await;
        self.config = config;
        diff
    }

    pub fn channel(&self, id: ChannelId) -> ChannelSnapshot {
        self.registry
            .snapshot()
            .channels
            .into_iter()
            .find(|c| c.id == id)
            .expect("channel in snapshot")
    }

    pub fn endpoint(&self, id: EndpointId) -> EndpointSnapshot {
        self.registry
            .snapshot()
            .endpoints
            .into_iter()
            .find(|e| e.id == id)
            .expect("endpoint in snapshot")
    }

    pub fn status(&self, id: ChannelId) -> Status {
        Status::of(&self.channel(id))
    }

    /// Waits until `check` holds for the bridge's status; panics with the last status.
    pub async fn until(
        &self,
        id: ChannelId,
        what: &str,
        check: impl Fn(&Status) -> bool,
    ) -> Status {
        let mut last = None;
        let found = try_until(|| {
            let status = self.status(id);
            if check(&status) {
                return Some(status);
            }
            last = Some(status);
            None
        })
        .await;
        found.unwrap_or_else(|| panic!("timed out waiting for {what}; last status: {last:#?}"))
    }

    /// Waits until the bridge runs and its two sides are paired.
    pub async fn running(&self, id: ChannelId) {
        self.until(id, "running and bridged", |s| {
            s.state == ChannelState::Running && s.session_state == SessionState::Bridged
        })
        .await;
    }
}

/// Polls `probe` until it yields a value, or `None` after [`WITHIN`].
pub async fn try_until<T>(mut probe: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = tokio::time::Instant::now() + WITHIN;
    loop {
        if let Some(value) = probe() {
            return Some(value);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// Polls `check` until it holds; panics after [`WITHIN`].
pub async fn until_holds(what: &str, mut check: impl FnMut() -> bool) {
    until(what, || check().then_some(())).await;
}

/// Polls `probe` until it yields a value; panics after [`WITHIN`].
pub async fn until<T>(what: &str, probe: impl FnMut() -> Option<T>) -> T {
    try_until(probe)
        .await
        .unwrap_or_else(|| panic!("timed out waiting for {what}"))
}

/// A bridge channel snapshot flattened to the field names of `docs/spec/diagnostics.md`
/// §4.2, so assertions read like the scenario files.
#[derive(Debug, Clone)]
pub struct Status {
    pub state: ChannelState,
    pub stop_reason: Option<StopReason>,
    pub session_state: SessionState,
    pub connect_strategy: ConnectStrategy,
    pub receiver: Option<BridgeSide>,
    pub config_version: u64,
    pub instance_started_at: Option<Timestamp>,
    pub retry_count: u32,
    pub next_retry_at: Option<Timestamp>,
    pub last_error_summary: Option<String>,
    pub recent_errors: usize,
    pub endpoint_a: Endpoint,
    pub endpoint_b: Endpoint,
    pub counters: BridgeCounters,
    pub buffer_a_to_b: Buffer,
    pub buffer_b_to_a: Buffer,
    pub tap_framer: TapFramer,
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub kind: EndpointKind,
    pub state: EndpointState,
    pub consecutive_failure_count: u32,
    pub active_peer: Option<String>,
    pub last_activity_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Copy)]
pub struct BridgeCounters {
    pub bytes_a_to_b: u64,
    pub bytes_b_to_a: u64,
    pub messages_a_to_b: Option<u64>,
    pub messages_b_to_a: Option<u64>,
    pub framing_errors: Option<u64>,
    pub tap_dropped: Option<u64>,
    pub overflow_count: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct Buffer {
    pub segments: u32,
    pub bytes: u64,
    pub max_segments: u32,
    pub max_bytes: u64,
    pub overflowing: bool,
}

impl Status {
    pub fn of(snapshot: &ChannelSnapshot) -> Self {
        let bridge = snapshot.bridge.as_ref().expect("bridge detail");
        let endpoint = |e: &EndpointRef| Endpoint {
            kind: e.kind,
            state: e.state,
            consecutive_failure_count: e.consecutive_failures,
            active_peer: e.active_peer.clone(),
            last_activity_at: e.last_activity,
        };
        let buffer = |f: &FlowSnapshot| Buffer {
            segments: f.buffered_segments,
            bytes: f.buffered_bytes,
            max_segments: f.max_segments,
            max_bytes: f.max_bytes,
            overflowing: f.overflowing,
        };
        Self {
            state: snapshot.state,
            stop_reason: snapshot.stop_reason,
            session_state: bridge.session,
            connect_strategy: bridge.connect_strategy,
            receiver: bridge.receiver,
            config_version: snapshot.config_version,
            instance_started_at: snapshot.instance_started_at,
            retry_count: snapshot.retry_count,
            next_retry_at: snapshot.next_retry_at,
            last_error_summary: snapshot.last_error.as_ref().map(|e| match &e.next_action {
                Some(next) => format!("{} {next}", e.summary),
                None => e.summary.clone(),
            }),
            recent_errors: snapshot.recent_errors.len(),
            endpoint_a: endpoint(&bridge.endpoint_a),
            endpoint_b: endpoint(&bridge.endpoint_b),
            counters: BridgeCounters {
                bytes_a_to_b: bridge.a_to_b.bytes,
                bytes_b_to_a: bridge.b_to_a.bytes,
                messages_a_to_b: bridge.a_to_b.messages,
                messages_b_to_a: bridge.b_to_a.messages,
                framing_errors: bridge.tap.framing_errors,
                tap_dropped: bridge.tap.tap_dropped,
                overflow_count: bridge.a_to_b.overflow_count + bridge.b_to_a.overflow_count,
            },
            buffer_a_to_b: buffer(&bridge.a_to_b),
            buffer_b_to_a: buffer(&bridge.b_to_a),
            tap_framer: bridge.tap.framer,
        }
    }
}

impl Harness {
    /// The outcome the last apply reported for `channel`.
    pub fn outcome(&self, channel: &str) -> ApplyOutcome {
        let id = channel_id(channel);
        self.report
            .channels
            .iter()
            .find(|c| c.channel_id == id)
            .map(|c| c.outcome.clone())
            .expect("channel in the apply report")
    }
}

/// An operator-chosen id.
pub fn id<K>(name: &str) -> packie_core::Name<K> {
    name.parse().unwrap()
}

pub fn listener(name: &str, port: u16) -> Value {
    json!({
        "endpoint_id": name,
        "label": format!("{name} listener"),
        "kind": "tcp_listener",
        "settings": {"bind_address": "127.0.0.1", "port": port},
    })
}

pub fn listener_allowing(name: &str, port: u16, allowed: &[&str]) -> Value {
    json!({
        "endpoint_id": name,
        "label": format!("{name} listener"),
        "kind": "tcp_listener",
        "settings": {"bind_address": "127.0.0.1", "port": port, "allowed_peer_cidrs": allowed},
    })
}

pub fn client(name: &str, port: u16) -> Value {
    json!({
        "endpoint_id": name,
        "label": format!("{name} client"),
        "kind": "tcp_client",
        "settings": {"host": "127.0.0.1", "port": port, "connect_timeout_ms": 500},
    })
}

/// A bridge with short timers and jitter 0; `extra` is merged over the defaults.
pub fn bridge(name: &str, a: &str, b: &str, extra: Value) -> Value {
    let mut channel = json!({
        "kind": "bridge",
        "channel_id": name,
        "label": format!("Bridge {name}"),
        "endpoint_a": a,
        "endpoint_b": b,
        "connection": {"connect_strategy": "parallel", "pairing_timeout_ms": 1000},
        "retry": {
            "initial_delay_ms": 50, "max_delay_ms": 400, "multiplier": 2, "jitter_percent": 0,
            "max_consecutive_failures_before_alert": 3,
        },
    });
    if let (Some(base), Value::Object(extra)) = (channel.as_object_mut(), extra) {
        for (key, value) in extra {
            base.insert(key, value);
        }
    }
    channel
}

pub fn document(endpoints: Vec<Value>, channels: Vec<Value>) -> Value {
    json!({"schema_version": 1, "endpoints": endpoints, "channels": channels})
}

pub fn channel_id(name: &str) -> ChannelId {
    id(name)
}

pub fn endpoint_id(name: &str) -> EndpointId {
    id(name)
}

/// `n` bytes of consecutive little-endian 32-bit counters from `seed` (the scenario
/// `counter32` pattern).
pub fn counter32(n: usize, seed: u32) -> Vec<u8> {
    (0..n / 4)
        .flat_map(|i| (seed.wrapping_add(i as u32)).to_le_bytes())
        .collect()
}

pub async fn send<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) {
    writer.write_all(bytes).await.expect("send");
    writer.flush().await.expect("flush");
}

/// Reads exactly `n` bytes within the time limit.
pub async fn expect_bytes<R: AsyncRead + Unpin>(reader: &mut R, expected: &[u8]) {
    let mut got = vec![0_u8; expected.len()];
    tokio::time::timeout(Duration::from_secs(10), reader.read_exact(&mut got))
        .await
        .expect("bytes in time")
        .expect("read");
    assert!(got == expected, "received bytes differ from what was sent");
}

/// The stream reaches end of stream or reset with nothing left unread.
pub async fn expect_closed<R: AsyncRead + Unpin>(reader: &mut R) {
    let mut byte = [0_u8; 1];
    match tokio::time::timeout(WITHIN, reader.read(&mut byte)).await {
        Ok(Ok(0) | Err(_)) => {}
        Ok(Ok(n)) => panic!("{n} unread bytes before close"),
        Err(_) => panic!("connection still open"),
    }
}

/// The stream stays open with no data for `ms`.
pub async fn expect_open<R: AsyncRead + Unpin>(reader: &mut R, ms: u64) {
    let mut byte = [0_u8; 1];
    if let Ok(result) =
        tokio::time::timeout(Duration::from_millis(ms), reader.read(&mut byte)).await
    {
        panic!("expected an open, silent connection, got {result:?}");
    }
}

/// The host transports (real sockets, folders, serial ports) without secrets.
pub fn system_transports() -> Arc<SystemTransports> {
    Arc::new(SystemTransports::new(Arc::new(MapSecrets::default())))
}

/// A delimited-file folder endpoint, polled every second.
pub fn folder(id: &str, path: &Path) -> Value {
    json!({
        "endpoint_id": id, "label": id, "kind": "folder",
        "settings": {"path": path, "mode": "poll", "poll_interval_seconds": 1, "stable_seconds": 0},
    })
}

// ---------------------------------------------------------------------------------------
// In-memory network peers

/// Connects once the engine's listener at `address` is bound.
pub async fn dial(net: &MemoryNetwork, address: &str) -> DuplexStream {
    dial_from(net, address, IpAddr::V4(Ipv4Addr::LOCALHOST)).await
}

/// [`dial`] from the address `ip`.
pub async fn dial_from(net: &MemoryNetwork, address: &str, ip: IpAddr) -> DuplexStream {
    let deadline = tokio::time::Instant::now() + WITHIN;
    loop {
        match net.connect_from(address, ip) {
            Ok(stream) => return stream,
            Err(error) if tokio::time::Instant::now() >= deadline => panic!("{address}: {error}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(5)).await,
        }
    }
}

/// A peer server at `address`: listens and accepts the engine's destination connection.
pub async fn accept_on(net: &MemoryNetwork, address: &str) -> (MemoryListener, DuplexStream) {
    let mut listener = net.listen(address, 4).unwrap();
    let accepted = tokio::time::timeout(WITHIN, listener.accept())
        .await
        .expect("the engine connected in time")
        .unwrap();
    (listener, accepted.stream)
}

/// A scripted HL7 MLLP peer.
pub struct Peer {
    stream: DuplexStream,
    decoder: mllp::Decoder,
    pending: Vec<Vec<u8>>,
}

impl Peer {
    pub fn new(stream: DuplexStream) -> Self {
        Self {
            stream,
            decoder: mllp::Decoder::new(1 << 20),
            pending: Vec::new(),
        }
    }

    /// An instrument connecting to the engine's listener at `address`.
    pub async fn dial(net: &MemoryNetwork, address: &str) -> Self {
        Self::new(dial(net, address).await)
    }

    /// A LIS accepting the engine's connection at `address`.
    pub async fn accept(net: &MemoryNetwork, address: &str) -> (MemoryListener, Self) {
        let (listener, stream) = accept_on(net, address).await;
        (listener, Self::new(stream))
    }

    pub async fn send(&mut self, payload: &[u8]) {
        let frame = mllp::encode(payload, 1 << 20).unwrap();
        self.write_raw(&frame).await;
    }

    /// Writes `bytes` as they are, in one write (frames, partial frames).
    pub async fn write_raw(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).await.unwrap();
    }

    /// The engine closes the connection with nothing left unread.
    pub async fn expect_closed(&mut self) {
        expect_closed(&mut self.stream).await;
    }

    /// The next frame, or `None` within `ms`.
    pub async fn frame_within(&mut self, ms: u64) -> Option<Vec<u8>> {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(ms);
        let mut buf = vec![0_u8; 4096];
        loop {
            if !self.pending.is_empty() {
                return Some(self.pending.remove(0));
            }
            let n = match tokio::time::timeout_at(deadline, self.stream.read(&mut buf)).await {
                Ok(Ok(n)) if n > 0 => n,
                _ => return None,
            };
            self.pending.extend(frames(&mut self.decoder, &buf[..n]));
        }
    }

    pub async fn frame(&mut self) -> Vec<u8> {
        self.frame_within(5000).await.expect("a frame")
    }

    /// MSA-1 and MSA-2 of the next frame.
    pub async fn msa(&mut self) -> (String, String) {
        let frame = self.frame().await;
        let message = Message::parse(&frame).unwrap();
        (
            message.get("MSA-1").unwrap_or("").to_owned(),
            message.get("MSA-2").unwrap_or("").to_owned(),
        )
    }
}

/// The complete frames `bytes` finish.
fn frames(decoder: &mut mllp::Decoder, bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    decoder.feed(bytes, |event| {
        if let mllp::Event::Message(frame) = event {
            frames.push(frame);
        }
    });
    frames
}

/// A synthetic ORU^R01 with control id `control`, sample `SID-1` and one result.
pub fn oru(control: &str) -> Vec<u8> {
    format!(
        "MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|{control}|P|2.5.1\rPID|1||PID-TEST-1||TEST^PATIENT\rOBR|1||SID-1\rOBX|1|NM|GLU||5.4|mmol/L\r"
    )
    .into_bytes()
}

/// The ACK a LIS answers `control` with.
pub fn ack(code: &str, control: &str) -> Vec<u8> {
    format!("MSH|^~\\&|LIS|LISFAC|PACKIE|PACKIEFAC|20260101120001||ACK|A-{control}|P|2.5.1\rMSA|{code}|{control}\r").into_bytes()
}

// ---------------------------------------------------------------------------------------
// Message channels over the durable store

/// Retry timing for message channels and their deliveries: 50 ms doubling to 200 ms.
pub fn quick_retry() -> Value {
    json!({"initial_delay_ms": 50, "max_delay_ms": 200, "multiplier": 2, "jitter_percent": 0})
}

/// A message channel `orders`: HL7 listener source on endpoint `lab-in`, HL7 client
/// destination `lis` on endpoint `lis-out`. `source` and `destination` override the
/// connector settings; `extra` is merged into the channel.
pub fn hl7_channel(source: Value, destination: Value, extra: Value) -> Value {
    let mut channel = json!({
        "kind": "message",
        "channel_id": "orders",
        "label": "Orders",
        "source": {"endpoint": "lab-in", "protocol": "hl7_mllp", "settings": source},
        "destinations": [{
            "destination_id": "lis", "label": "LIS", "endpoint": "lis-out",
            "protocol": "hl7_mllp", "settings": destination,
        }],
        "retry": quick_retry(),
        "delivery": {
            "max_attempts": 3, "send_timeout_seconds": 1, "ack_timeout_seconds": 1,
            "retry": quick_retry(),
        },
    });
    if let (Some(base), Value::Object(extra)) = (channel.as_object_mut(), extra) {
        for (key, value) in extra {
            base.insert(key, value);
        }
    }
    channel
}

/// The document of [`hl7_channel`]: the instrument listens on :9601, the LIS is :9701.
pub fn hl7_document(channel: Value) -> Value {
    document(
        vec![listener("lab-in", 9601), client("lis-out", 9701)],
        vec![channel],
    )
}

/// The SQLite store under `dir`, optionally with the commit gate (commits wait while the
/// watched value is true).
pub fn open_store(dir: &Path, held: Option<watch::Receiver<bool>>) -> Store {
    let options = OpenOptions {
        commits_held: held,
        ..OpenOptions::default()
    };
    Store::open(dir.join("store"), options).expect("store opens")
}

/// Registry options for `transports` with `store` as the message store.
pub fn options_with_store(transports: Arc<dyn Transports>, store: &Store) -> RegistryOptions {
    let mut options = RegistryOptions::new(transports);
    options.store = Some(Arc::new(SqliteStore::new(store.clone())));
    options
}

/// Every message of `store` oldest first, with its deliveries. A message the retention purge
/// removes between the listing and its read is left out.
pub fn records(store: &Store) -> Vec<MessageRecord> {
    let query = MessageQuery {
        limit: packie_ipc::messages::MAX_PAGE,
        ..MessageQuery::default()
    };
    let mut records: Vec<_> = store
        .query_messages(&query)
        .unwrap()
        .messages
        .iter()
        .filter_map(|m| match store.message(m.message_id) {
            Ok(record) => Some(record),
            Err(packie_store::Error::NotFound) => None,
            Err(error) => panic!("message {} cannot be read: {error}", m.message_id),
        })
        .collect();
    records.sort_by_key(|r| r.summary.seq);
    records
}

/// The deliveries of `records` in message order.
pub fn deliveries_of(records: Vec<MessageRecord>) -> Vec<DeliveryRecord> {
    records.into_iter().flat_map(|r| r.deliveries).collect()
}

/// The engine on the in-memory network with the durable store in the state directory:
/// what the message-channel tests drive. Stored messages and deliveries are read back
/// through the store's own query API.
pub struct MessageRig {
    pub h: Harness,
    pub net: MemoryNetwork,
    pub store: Store,
    latency: Latency,
}

/// Faults a test injects into the store: delays, as on a slow or busy disk, and
/// failures, as of a store that cannot commit.
#[derive(Clone, Default)]
pub struct Latency {
    commit_ms: Arc<AtomicU64>,
    route_ms: Arc<AtomicU64>,
    failing: Arc<AtomicBool>,
}

impl Latency {
    /// While `unavailable`, every mutating store call fails with `Unavailable`.
    pub fn unavailable(&self, unavailable: bool) {
        self.failing.store(unavailable, Ordering::SeqCst);
    }

    /// Every commit of a received unit takes `latency` longer.
    pub fn commit(&self, latency: Duration) {
        self.commit_ms.store(millis(latency), Ordering::SeqCst);
    }

    /// Every pipeline route (after the source's acknowledgement) takes `latency` longer.
    pub fn route(&self, latency: Duration) {
        self.route_ms.store(millis(latency), Ordering::SeqCst);
    }
}

fn millis(latency: Duration) -> u64 {
    u64::try_from(latency.as_millis()).unwrap()
}

/// Registry options with the durable `store` behind the [`Latency`] it returns.
pub fn options_with_latency(
    transports: Arc<dyn Transports>,
    store: &Store,
) -> (RegistryOptions, Latency) {
    let latency = Latency::default();
    let mut options = RegistryOptions::new(transports);
    options.store = Some(Arc::new(FailingStore {
        inner: SqliteStore::new(store.clone()),
        latency: latency.clone(),
    }));
    (options, latency)
}

impl MessageRig {
    /// A fresh state directory, `document` applied.
    pub async fn start(document: Value) -> Self {
        Self::start_in(Arc::new(tempfile::tempdir().unwrap()), None, document).await
    }

    /// `start` in `dir` (kept by the caller to outlive the rig) with an optional
    /// commit gate on the store.
    pub async fn start_in(
        dir: Arc<tempfile::TempDir>,
        held: Option<watch::Receiver<bool>>,
        document: Value,
    ) -> Self {
        let store = open_store(dir.path(), held);
        let net = MemoryNetwork::new();
        let (options, latency) = options_with_latency(Arc::new(net.clone()), &store);
        let mut h = Harness::in_dir(dir, options);
        h.apply(document).await;
        Self {
            h,
            net,
            store,
            latency,
        }
    }

    /// While `unavailable`, every mutating store call fails as a store that cannot
    /// commit does.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.latency.unavailable(unavailable);
    }

    /// Every commit of a received unit takes `latency` longer, as on a slow disk.
    pub fn set_commit_latency(&self, latency: Duration) {
        self.latency.commit(latency);
    }

    /// Every stored message, oldest first.
    pub fn messages(&self) -> Vec<MessageRecord> {
        records(&self.store)
    }

    /// Every delivery, in message order.
    pub fn deliveries(&self) -> Vec<DeliveryRecord> {
        deliveries_of(self.messages())
    }

    /// The original bytes of `message`.
    pub fn body(&self, message: MessageId) -> Vec<u8> {
        self.store.body(message, None).unwrap()
    }

    pub async fn messages_until(
        &self,
        what: &str,
        check: impl Fn(&[MessageRecord]) -> bool,
    ) -> Vec<MessageRecord> {
        until(what, || {
            let messages = self.messages();
            check(&messages).then_some(messages)
        })
        .await
    }

    pub async fn deliveries_until(
        &self,
        what: &str,
        check: impl Fn(&[DeliveryRecord]) -> bool,
    ) -> Vec<DeliveryRecord> {
        until(what, || {
            let deliveries = self.deliveries();
            check(&deliveries).then_some(deliveries)
        })
        .await
    }
}

/// The durable store, except that it can be told to fail every mutating call with
/// `Unavailable`, the way a store whose disk or writer fails does, and to commit received
/// units slowly. Reads pass through.
struct FailingStore {
    inner: SqliteStore,
    latency: Latency,
}

impl FailingStore {
    fn guard<'a, T: Send + 'a>(
        &'a self,
        call: impl FnOnce(&'a SqliteStore) -> BoxFuture<'a, StoreResult<T>>,
    ) -> BoxFuture<'a, StoreResult<T>> {
        if self.latency.failing.load(Ordering::SeqCst) {
            return Box::pin(async { Err(StoreError::Unavailable("injected".into())) });
        }
        call(&self.inner)
    }
}

impl MessageStore for FailingStore {
    fn commit_batch(
        &self,
        units: Vec<(Receipt, Vec<u8>)>,
    ) -> BoxFuture<'_, StoreResult<Vec<ReceiptOutcome>>> {
        self.guard(|s| s.commit_batch(units))
    }

    fn commit_received(
        &self,
        receipt: Receipt,
        bytes: Vec<u8>,
    ) -> BoxFuture<'_, StoreResult<ReceiptOutcome>> {
        let latency = Duration::from_millis(self.latency.commit_ms.load(Ordering::SeqCst));
        Box::pin(async move {
            tokio::time::sleep(latency).await;
            self.guard(|s| s.commit_received(receipt, bytes)).await
        })
    }

    fn begin_transfer(
        &self,
        receipt: Receipt,
        frame: Vec<u8>,
    ) -> BoxFuture<'_, StoreResult<MessageId>> {
        self.guard(|s| s.begin_transfer(receipt, frame))
    }

    fn append_frame(&self, message: MessageId, frame: Vec<u8>) -> BoxFuture<'_, StoreResult<()>> {
        self.guard(|s| s.append_frame(message, frame))
    }

    fn complete_transfer(
        &self,
        message: MessageId,
        bytes: Vec<u8>,
        identifiers: Identifiers,
        duplicate_detection: bool,
    ) -> BoxFuture<'_, StoreResult<ReceiptOutcome>> {
        self.guard(|s| s.complete_transfer(message, bytes, identifiers, duplicate_detection))
    }

    fn abort_transfer(
        &self,
        message: MessageId,
        reason: &'static str,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.guard(|s| s.abort_transfer(message, reason))
    }

    fn record_response(
        &self,
        message: MessageId,
        code: String,
        at: Timestamp,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.guard(|s| s.record_response(message, code, at))
    }

    fn record_invalid(&self, message: MessageId, reason: String) -> BoxFuture<'_, StoreResult<()>> {
        self.guard(|s| s.record_invalid(message, reason))
    }

    fn record_filtered(
        &self,
        message: MessageId,
        reason: String,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.guard(|s| s.record_filtered(message, reason))
    }

    fn route(
        &self,
        message: MessageId,
        deliveries: Vec<NewDelivery>,
        config_version: u64,
    ) -> BoxFuture<'_, StoreResult<Vec<DeliveryId>>> {
        let latency = Duration::from_millis(self.latency.route_ms.load(Ordering::SeqCst));
        Box::pin(async move {
            tokio::time::sleep(latency).await;
            self.guard(|s| s.route(message, deliveries, config_version))
                .await
        })
    }

    fn stored_message(&self, message: MessageId) -> BoxFuture<'_, StoreResult<StoredMessage>> {
        self.inner.stored_message(message)
    }

    fn next_delivery(
        &self,
        channel: ChannelId,
        destination: ConnectorId,
    ) -> BoxFuture<'_, StoreResult<Option<PendingDelivery>>> {
        self.inner.next_delivery(channel, destination)
    }

    fn record_transformed_body(
        &self,
        delivery: DeliveryId,
        body: Vec<u8>,
        mapping_version: u64,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.guard(|s| s.record_transformed_body(delivery, body, mapping_version))
    }

    fn record_transform_error(
        &self,
        delivery: DeliveryId,
        summary: String,
        mapping_version: u64,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.guard(|s| s.record_transform_error(delivery, summary, mapping_version))
    }

    fn begin_attempt(
        &self,
        delivery: DeliveryId,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<u32>> {
        self.guard(|s| s.begin_attempt(delivery, now))
    }

    fn record_ack_progress(
        &self,
        delivery: DeliveryId,
        level: AckLevel,
        code: Option<String>,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<()>> {
        self.guard(|s| s.record_ack_progress(delivery, level, code, now))
    }

    fn finish_attempt(
        &self,
        delivery: DeliveryId,
        finish: AttemptFinish,
    ) -> BoxFuture<'_, StoreResult<DeliveryState>> {
        self.guard(|s| s.finish_attempt(delivery, finish))
    }

    fn resend(
        &self,
        request: ResendRequest,
        active: Vec<DestinationPolicy>,
        config_version: u64,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<ResendResult>> {
        self.guard(|s| s.resend(request, active, config_version, now))
    }

    fn close_delivery(
        &self,
        request: CloseRequest,
        now: Timestamp,
    ) -> BoxFuture<'_, StoreResult<DeliveryRecord>> {
        self.guard(|s| s.close_delivery(request, now))
    }

    fn cancel_unresolved(
        &self,
        channel: ChannelId,
        destination: Option<ConnectorId>,
        reason: &'static str,
    ) -> BoxFuture<'_, StoreResult<u64>> {
        self.guard(|s| s.cancel_unresolved(channel, destination, reason))
    }

    fn reconcile_at_startup(&self) -> BoxFuture<'_, StoreResult<Reconciliation>> {
        self.guard(SqliteStore::reconcile_at_startup)
    }

    fn queue_counts(
        &self,
        channel: ChannelId,
        destination: ConnectorId,
    ) -> StoreResult<QueueCounts> {
        self.inner.queue_counts(channel, destination)
    }

    fn totals(&self) -> Option<packie_store::StoreTotals> {
        self.inner.totals()
    }

    fn purge(&self, policies: Vec<RetentionPolicy>) -> BoxFuture<'_, StoreResult<u64>> {
        self.inner.purge(policies)
    }

    fn query_messages(&self, query: MessageQuery) -> BoxFuture<'_, StoreResult<MessageList>> {
        self.inner.query_messages(query)
    }

    fn transmission_totals(
        &self,
        filter: packie_ipc::messages::MessageFilter,
    ) -> BoxFuture<'_, StoreResult<Vec<packie_ipc::reports::DestinationTransmission>>> {
        self.inner.transmission_totals(filter)
    }

    fn message_record(&self, message: MessageId) -> BoxFuture<'_, StoreResult<MessageRecord>> {
        self.inner.message_record(message)
    }

    fn message_body(&self, request: BodyRequest) -> BoxFuture<'_, StoreResult<MessageBody>> {
        self.inner.message_body(request)
    }
}
