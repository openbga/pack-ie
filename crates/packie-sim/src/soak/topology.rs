//! The soak topology: which channels exist, which ports and folders they use, and the
//! runtime document that describes them.
//!
//! | Channels | Count | Shape |
//! | --- | --- | --- |
//! | `bridge-01`..`bridge-20` | 20 | TCP listener (the instrument connects) to TCP client (the sink listens). `bridge-01` and `bridge-02` carry HL7 over MLLP with an HL7 tap that records messages. Every fifth bridge has a small buffer so a stalled sink produces back-pressure. |
//! | `hl7-01`..`hl7-07` | 7 | HL7 MLLP in, store, HL7 MLLP out to a peer that acknowledges. |
//! | `astm-01`, `astm-02` | 2 | ASTM over TCP in, mapped to HL7 MLLP out. |
//! | `folder-01` | 1 | Delimited file folder in, delimited file folder out. |
//!
//! A config apply changes one channel through [`Knobs`]: [`Change::Label`] and
//! [`Change::Policy`] are applied in place, [`Change::Replace`] restarts the channel.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tokio::net::TcpListener;

use crate::error::SimError;
use crate::ports::PortLease;

/// What a channel is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A bridge carrying newline separated records.
    Bridge,
    /// A bridge carrying MLLP framed HL7 with a recording tap.
    TapBridge,
    /// HL7 in, HL7 out.
    Hl7,
    /// ASTM in, HL7 out.
    Astm,
    /// Delimited file in, delimited file out.
    Folder,
}

impl Kind {
    pub(crate) fn is_bridge(self) -> bool {
        matches!(self, Self::Bridge | Self::TapBridge)
    }

    /// The letter that starts the control ids of this kind.
    pub(crate) fn letter(self) -> char {
        match self {
            Self::Bridge => 'B',
            Self::TapBridge => 'T',
            Self::Hl7 => 'H',
            Self::Astm => 'A',
            Self::Folder => 'F',
        }
    }
}

/// One channel of the topology.
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    /// The channel id in the runtime document.
    pub(crate) id: String,
    /// Position in the topology, used in ids and seeds.
    pub(crate) index: usize,
    pub(crate) kind: Kind,
    /// The port the engine listens on (bridge side a, message source).
    pub(crate) in_port: u16,
    /// The port of the peer the engine connects to (bridge side b, message destination).
    pub(crate) out_port: u16,
    /// Folder channels: the source directory.
    pub(crate) in_dir: PathBuf,
    /// Folder channels: the destination directory.
    pub(crate) out_dir: PathBuf,
    /// Whether the bridge buffer is small.
    pub(crate) small_buffer: bool,
}

impl Plan {
    /// The prefix of every control id of this channel, such as `H03`.
    pub(crate) fn token(&self) -> String {
        format!("{}{:02}", self.kind.letter(), self.index + 1)
    }

    /// The control id of record `seq`.
    pub(crate) fn control_id(&self, seq: u64) -> String {
        format!("{}-{seq:09}", self.token())
    }

    /// The sequence number in a control id of this channel.
    pub(crate) fn parse_control_id(&self, id: &str) -> Option<u64> {
        let rest = id.strip_prefix(&self.token())?.strip_prefix('-')?;
        rest.parse().ok()
    }

    /// The endpoint ids `(first, second)`: bridge sides a and b, or message source and
    /// destination.
    pub(crate) fn endpoint_ids(&self) -> (String, String) {
        let (first, second) = if self.kind.is_bridge() {
            ("a", "b")
        } else {
            ("in", "out")
        };
        (
            format!("{}-{first}", self.id),
            format!("{}-{second}", self.id),
        )
    }
}

/// Which part of a channel a config apply changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    /// The label: applied in place.
    Label,
    /// A delivery policy value (message channels): applied in place.
    Policy,
    /// A connector or connection value: the channel restarts.
    Replace,
}

impl Change {
    /// The outcome the engine reports for the changed channel.
    pub(crate) fn expected_outcome(self) -> &'static str {
        match self {
            Self::Label | Self::Policy => "updated",
            Self::Replace => "restarted",
        }
    }
}

/// The values a config apply toggles, per channel.
#[derive(Debug, Clone, Default)]
pub(crate) struct Knobs {
    pub(crate) label_rev: u32,
    pub(crate) policy_flip: bool,
    pub(crate) replace_flip: bool,
}

impl Knobs {
    pub(crate) fn apply(&mut self, change: Change) {
        match change {
            Change::Label => self.label_rev += 1,
            Change::Policy => self.policy_flip = !self.policy_flip,
            Change::Replace => self.replace_flip = !self.replace_flip,
        }
    }
}

/// Values of the document that do not change during a run.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DocOptions {
    pub(crate) retention_messages: u64,
}

/// The ports, folders and listeners of a run.
pub(crate) struct Topology {
    pub(crate) plans: Vec<Plan>,
    /// Peer listeners, by channel id, taken once by the peer that serves them.
    pub(crate) listeners: BTreeMap<String, TcpListener>,
    _leases: Vec<PortLease>,
}

/// The number of bridges, message channels of each kind.
const BRIDGES: usize = 20;
const HL7_CHANNELS: usize = 7;
const ASTM_CHANNELS: usize = 2;

fn lease_pair() -> Result<(PortLease, PortLease), SimError> {
    Ok((
        PortLease::reserve_loopback()?,
        PortLease::reserve_loopback()?,
    ))
}

fn listen(lease: &mut PortLease) -> Result<TcpListener, SimError> {
    lease
        .take_socket()
        .ok_or_else(|| SimError::other("the port has no socket"))?
        .listen(1024)
        .map_err(|e| SimError::io(format!("listen on port {}", lease.port()), e))
}

/// Collects the plans while the topology is allocated.
struct Builder<'a> {
    work: &'a Path,
    plans: Vec<Plan>,
    listeners: BTreeMap<String, TcpListener>,
    leases: Vec<PortLease>,
}

impl Builder<'_> {
    fn add(&mut self, id: String, kind: Kind, small_buffer: bool) -> Result<(), SimError> {
        let (mut engine_side, mut peer_side) = lease_pair()?;
        engine_side.release_socket();
        let (in_port, out_port) = (engine_side.port(), peer_side.port());
        let (in_dir, out_dir) = self.folders(&id, kind)?;
        if kind == Kind::Folder {
            // A folder channel has no TCP peers; the leases are kept unused.
            peer_side.release_socket();
        } else {
            self.listeners.insert(id.clone(), listen(&mut peer_side)?);
        }
        self.leases.push(engine_side);
        self.leases.push(peer_side);
        self.plans.push(Plan {
            id,
            index: self.plans.len(),
            kind,
            in_port,
            out_port,
            in_dir,
            out_dir,
            small_buffer,
        });
        Ok(())
    }

    /// The source and destination folders of a folder channel, created; empty paths for
    /// every other kind.
    fn folders(&self, id: &str, kind: Kind) -> Result<(PathBuf, PathBuf), SimError> {
        if kind != Kind::Folder {
            return Ok((PathBuf::new(), PathBuf::new()));
        }
        let (source, destination) = (
            self.work.join(id).join("in"),
            self.work.join(id).join("out"),
        );
        for dir in [&source, &destination] {
            std::fs::create_dir_all(dir)
                .map_err(|e| SimError::io(format!("create {}", dir.display()), e))?;
        }
        Ok((source, destination))
    }
}

impl Topology {
    /// Reserves every port and creates the folders under `work`.
    pub(crate) fn allocate(work: &Path) -> Result<Self, SimError> {
        let mut b = Builder {
            work,
            plans: Vec::new(),
            listeners: BTreeMap::new(),
            leases: Vec::new(),
        };
        for n in 1..=BRIDGES {
            let kind = if n <= 2 {
                Kind::TapBridge
            } else {
                Kind::Bridge
            };
            b.add(format!("bridge-{n:02}"), kind, n % 5 == 0)?;
        }
        for n in 1..=HL7_CHANNELS {
            b.add(format!("hl7-{n:02}"), Kind::Hl7, false)?;
        }
        for n in 1..=ASTM_CHANNELS {
            b.add(format!("astm-{n:02}"), Kind::Astm, false)?;
        }
        b.add("folder-01".to_owned(), Kind::Folder, false)?;
        Ok(Self {
            plans: b.plans,
            listeners: b.listeners,
            _leases: b.leases,
        })
    }
}

fn retry() -> Value {
    json!({ "initial_delay_ms": 500, "max_delay_ms": 5000, "multiplier": 2.0,
            "jitter_percent": 20, "max_consecutive_failures_before_alert": 3 })
}

fn label(plan: &Plan, knobs: &Knobs) -> String {
    let base = match plan.kind {
        Kind::Bridge => "Bridge",
        Kind::TapBridge => "Bridge with HL7 tap",
        Kind::Hl7 => "HL7 channel",
        Kind::Astm => "ASTM channel",
        Kind::Folder => "Folder channel",
    };
    format!("{base} {:02} r{}", plan.index + 1, knobs.label_rev)
}

fn tcp_listener(id: &str, port: u16) -> Value {
    json!({ "endpoint_id": id, "label": id, "kind": "tcp_listener",
            "settings": { "bind_address": "127.0.0.1", "port": port } })
}

fn tcp_client(id: &str, port: u16) -> Value {
    json!({ "endpoint_id": id, "label": id, "kind": "tcp_client",
            "settings": { "host": "127.0.0.1", "port": port } })
}

fn folder_endpoint(id: &str, settings: Value) -> Value {
    json!({ "endpoint_id": id, "label": id, "kind": "folder", "settings": settings })
}

fn bridge_channel(plan: &Plan, knobs: &Knobs) -> Value {
    let (a, b) = plan.endpoint_ids();
    let pairing = if knobs.replace_flip { 3500 } else { 3000 };
    let mut channel = json!({
        "channel_id": plan.id, "label": label(plan, knobs), "kind": "bridge",
        "endpoint_a": a, "endpoint_b": b,
        "connection": { "connect_strategy": "parallel", "pairing_timeout_ms": pairing },
        "retry": retry(),
    });
    if plan.small_buffer {
        channel["buffer"] = json!({ "max_buffered_segments": 64, "max_bytes": 65536,
                                    "block_timeout_ms": 2000 });
    }
    if plan.kind == Kind::TapBridge {
        channel["tap"] = json!({ "framer": "hl7_mllp", "record_messages": true });
    }
    channel
}

fn delivery(knobs: &Knobs) -> Value {
    let ack = if knobs.policy_flip { 65 } else { 60 };
    json!({ "max_attempts": 5, "send_timeout_seconds": 30, "ack_timeout_seconds": ack,
            "retry": { "initial_delay_ms": 500, "max_delay_ms": 5000, "multiplier": 2.0,
                       "jitter_percent": 20, "max_consecutive_failures_before_alert": 3 } })
}

/// The mapping of an ASTM result message to an HL7 `ORU^R01`.
fn astm_to_hl7_mapping() -> Value {
    json!({
        "tables": { "flags": { "N": "N", "H": "H", "L": "L" } },
        "rules": [
            { "op": "constant", "value": "ORU^R01", "to": "MSH.9" },
            { "op": "copy", "from": "O.3", "to": "MSH.10" },
            { "op": "copy", "from": "P.3", "to": "PID.3" },
            { "op": "copy", "from": "O.3", "to": "OBR.3" },
            { "op": "for_each", "over": "R", "into": "OBX", "rules": [
                { "op": "sequence", "to": "$.1" },
                { "op": "copy", "from": "$.3", "to": "$.3" },
                { "op": "copy", "from": "$.4", "to": "$.5" },
                { "op": "lookup", "from": "$.7", "table": "flags", "to": "$.8",
                  "on_missing": "skip" } ] } ]
    })
}

fn message_channel(plan: &Plan, knobs: &Knobs, opts: &DocOptions) -> Value {
    let (source, destination) = plan.endpoint_ids();
    let app = if knobs.replace_flip {
        "PACKIE-B"
    } else {
        "PACKIE"
    };
    let (protocol, mapping) = if plan.kind == Kind::Astm {
        ("astm", astm_to_hl7_mapping())
    } else {
        ("hl7_mllp", Value::Null)
    };
    json!({
        "channel_id": plan.id, "label": label(plan, knobs), "kind": "message",
        "retry": retry(),
        "source": { "endpoint": source, "protocol": protocol,
                    "settings": if plan.kind == Kind::Astm { json!({}) }
                                else { json!({ "ack_mode": "original" }) } },
        "destinations": [{
            "destination_id": "lis", "label": "LIS", "endpoint": destination,
            "protocol": "hl7_mllp",
            "settings": { "ack_mode": "original", "sending_application": app,
                          "receiving_application": "LIS" },
            "mapping": mapping,
            "delivery": delivery(knobs),
        }],
        "retention": { "max_age_days": 7, "max_messages": opts.retention_messages },
    })
}

/// The columns of the delimited files, with positions for the destination.
fn file_columns() -> Value {
    json!([
        { "name": "control_id", "type": "string", "required": true, "position": 1 },
        { "name": "sample_id", "type": "string", "position": 2 },
        { "name": "value", "type": "decimal", "position": 3 }
    ])
}

fn folder_channel(plan: &Plan, knobs: &Knobs, opts: &DocOptions) -> Value {
    let (source, destination) = plan.endpoint_ids();
    let line_ending = if knobs.replace_flip { "crlf" } else { "lf" };
    json!({
        "channel_id": plan.id, "label": label(plan, knobs), "kind": "message",
        "retry": retry(),
        "source": { "endpoint": source, "protocol": "delimited_file",
                    "settings": { "delimiter": ",", "header_row": true,
                                  "columns": file_columns() } },
        "destinations": [{
            "destination_id": "out", "label": "Results folder", "endpoint": destination,
            "protocol": "delimited_file",
            "settings": { "delimiter": ",", "header_row": true, "line_ending": line_ending,
                          "columns": file_columns(), "file_name": "{control_id}.csv",
                          "on_exists": "suffix" },
            "mapping": Value::Null,
            "delivery": delivery(knobs),
        }],
        "retention": { "max_age_days": 7, "max_messages": opts.retention_messages },
    })
}

fn endpoints_of(plan: &Plan) -> Vec<Value> {
    let (first, second) = plan.endpoint_ids();
    if plan.kind == Kind::Folder {
        return vec![
            folder_endpoint(
                &first,
                json!({ "path": plan.in_dir, "mode": "watch", "poll_interval_seconds": 5,
                        "file_pattern": "*.csv", "stable_seconds": 1,
                        "on_consumed": "delete" }),
            ),
            folder_endpoint(
                &second,
                json!({ "path": plan.out_dir, "write_mode": "atomic_rename" }),
            ),
        ];
    }
    vec![
        tcp_listener(&first, plan.in_port),
        tcp_client(&second, plan.out_port),
    ]
}

fn channel_of(plan: &Plan, knobs: &Knobs, opts: &DocOptions) -> Value {
    match plan.kind {
        Kind::Bridge | Kind::TapBridge => bridge_channel(plan, knobs),
        Kind::Hl7 | Kind::Astm => message_channel(plan, knobs, opts),
        Kind::Folder => folder_channel(plan, knobs, opts),
    }
}

/// The runtime document of every channel in `plans` with the current `knobs`.
pub(crate) fn runtime_document(
    plans: &[Plan],
    knobs: &BTreeMap<String, Knobs>,
    opts: &DocOptions,
) -> Value {
    let default = Knobs::default();
    let (mut endpoints, mut channels) = (Vec::new(), Vec::new());
    for plan in plans {
        let k = knobs.get(&plan.id).unwrap_or(&default);
        endpoints.extend(endpoints_of(plan));
        channels.push(channel_of(plan, k, opts));
    }
    json!({ "schema_version": 1, "endpoints": endpoints, "channels": channels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(kind: Kind, index: usize) -> Plan {
        Plan {
            id: format!("c-{index}"),
            index,
            kind,
            in_port: 1,
            out_port: 2,
            in_dir: PathBuf::from("/tmp/in"),
            out_dir: PathBuf::from("/tmp/out"),
            small_buffer: false,
        }
    }

    #[test]
    fn control_ids_round_trip() {
        let p = plan(Kind::Hl7, 2);
        assert_eq!(p.control_id(42), "H03-000000042");
        assert_eq!(p.parse_control_id("H03-000000042"), Some(42));
        assert_eq!(p.parse_control_id("H04-000000042"), None);
        assert_eq!(p.parse_control_id("H03-x"), None);
    }

    #[test]
    fn a_change_toggles_exactly_one_value() {
        let mut k = Knobs::default();
        k.apply(Change::Label);
        k.apply(Change::Policy);
        k.apply(Change::Replace);
        k.apply(Change::Replace);
        assert_eq!(
            (k.label_rev, k.policy_flip, k.replace_flip),
            (1, true, false)
        );
    }

    #[test]
    fn the_document_has_every_channel_with_its_endpoints() {
        let plans = vec![
            plan(Kind::Bridge, 0),
            plan(Kind::TapBridge, 1),
            plan(Kind::Hl7, 2),
            plan(Kind::Astm, 3),
            plan(Kind::Folder, 4),
        ];
        let doc = runtime_document(
            &plans,
            &BTreeMap::new(),
            &DocOptions {
                retention_messages: 5000,
            },
        );
        assert_eq!(doc["channels"].as_array().map(Vec::len), Some(5));
        assert_eq!(doc["endpoints"].as_array().map(Vec::len), Some(10));
        assert_eq!(doc["channels"][1]["tap"]["framer"], "hl7_mllp");
        assert_eq!(
            doc["channels"][3]["destinations"][0]["mapping"]["rules"][0]["to"],
            "MSH.9"
        );
    }

    #[test]
    fn only_the_changed_channel_differs_after_a_change() {
        let plans = vec![plan(Kind::Hl7, 0), plan(Kind::Hl7, 1)];
        let opts = DocOptions {
            retention_messages: 5000,
        };
        let before = runtime_document(&plans, &BTreeMap::new(), &opts);
        let mut knobs = BTreeMap::new();
        let mut k = Knobs::default();
        k.apply(Change::Replace);
        knobs.insert("c-1".to_owned(), k);
        let after = runtime_document(&plans, &knobs, &opts);
        assert_eq!(before["channels"][0], after["channels"][0]);
        assert_ne!(before["channels"][1], after["channels"][1]);
        assert_eq!(before["endpoints"], after["endpoints"]);
    }
}
