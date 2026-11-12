//! The step catalog (scenarios.md §7) as typed values.
//!
//! Every step object is `{do, actor, id?, note?, ...fields}`. [`parse_step`] strips the
//! common fields and deserializes the rest into [`Action`], tagged by `do`; each variant's
//! argument struct rejects unknown fields, so a misspelled field fails at load.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::matcher::Matcher;
use crate::payload::{Generate, PayloadSpec};
use crate::scenario::{Step, from_json};

/// Steps with no fields.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

/// `accept`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptArgs {
    /// Wait limit.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Required peer address (`ip` or `ip:port`).
    #[serde(default)]
    pub expect_peer: Option<String>,
    /// `connection` (default) or `none`.
    #[serde(default)]
    pub expect: AcceptExpect,
}

/// Outcome of `accept`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptExpect {
    /// A connection arrives.
    #[default]
    Connection,
    /// No connection arrives.
    None,
}

/// `connect` (TCP client with host and port, or serial peer without).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectArgs {
    /// Target host.
    #[serde(default)]
    pub host: Option<String>,
    /// Target port.
    #[serde(default)]
    pub port: Option<u16>,
    /// `ok` (default) or `refused`.
    #[serde(default)]
    pub expect: ConnectExpect,
}

/// Outcome of a TCP `connect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectExpect {
    /// Established.
    #[default]
    Ok,
    /// Refused.
    Refused,
}

/// `send`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendArgs {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    hex: Option<String>,
    #[serde(default)]
    base64: Option<String>,
    #[serde(default)]
    generate: Option<Generate>,
    /// Split the write into chunks of this size.
    #[serde(default)]
    pub chunk_size: Option<usize>,
    /// Pause between chunks.
    #[serde(default)]
    pub chunk_delay_ms: Option<u64>,
    /// Return at once; finish with `await_send`.
    #[serde(default, rename = "async")]
    pub is_async: bool,
    /// Name of the asynchronous operation.
    #[serde(default)]
    pub name: Option<String>,
}

impl SendArgs {
    /// The payload fields.
    #[must_use]
    pub fn payload(&self) -> PayloadSpec {
        PayloadSpec::from_parts(
            self.text.clone(),
            self.hex.clone(),
            self.base64.clone(),
            self.generate.clone(),
        )
    }
}

/// Wire protocols of `send_message` and `expect_message`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// HL7 v2 over MLLP.
    Hl7Mllp,
    /// ASTM E1381.
    Astm,
}

/// One acknowledgement code or several, or `none`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codes {
    /// No acknowledgement.
    None,
    /// Codes in order.
    List(Vec<String>),
}

impl<'de> Deserialize<'de> for Codes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match Value::deserialize(d)? {
            Value::String(s) if s == "none" => Ok(Self::None),
            Value::String(s) => Ok(Self::List(vec![s])),
            Value::Array(items) => items
                .into_iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| D::Error::custom("codes are strings"))
                })
                .collect::<Result<_, _>>()
                .map(Self::List),
            _ => Err(D::Error::custom("a code, an array of codes, or `none`")),
        }
    }
}

/// `send_message`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SendMessageArgs {
    /// The protocol.
    pub protocol: Protocol,
    /// HL7 segments.
    #[serde(default)]
    pub segments: Option<Vec<String>>,
    /// ASTM records.
    #[serde(default)]
    pub records: Option<Vec<String>>,
    /// HL7: reply codes to expect (default `AA`).
    #[serde(default)]
    pub expect_reply: Option<Codes>,
    /// Wait limit for each reply.
    #[serde(default)]
    pub reply_timeout_ms: Option<u64>,
    /// ASTM partial transfer.
    #[serde(default)]
    pub partial: Option<Partial>,
    /// Return at once.
    #[serde(default, rename = "async")]
    pub is_async: bool,
    /// Name of the asynchronous operation.
    #[serde(default)]
    pub name: Option<String>,
}

/// ASTM partial transfer.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partial {
    /// Stop after this many frames, leaving the transfer open.
    #[serde(default)]
    pub frames: Option<u32>,
    /// Send every frame but not EOT.
    #[serde(default)]
    pub omit_eot: bool,
}

/// `await_send`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AwaitSendArgs {
    /// The asynchronous operation.
    pub name: String,
    /// `complete` (default) or `pending`.
    #[serde(default)]
    pub expect: AwaitExpect,
    /// For `pending`: how long it must stay unfinished.
    #[serde(default)]
    pub for_ms: Option<u64>,
    /// For `complete`: wait limit.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Outcome of `await_send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwaitExpect {
    /// Finished successfully.
    #[default]
    Complete,
    /// Not finished.
    Pending,
}

/// `expect`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectArgs {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    hex: Option<String>,
    #[serde(default)]
    base64: Option<String>,
    #[serde(default)]
    generate: Option<Generate>,
    /// Wait limit.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

impl ExpectArgs {
    /// The payload fields.
    #[must_use]
    pub fn payload(&self) -> PayloadSpec {
        PayloadSpec::from_parts(
            self.text.clone(),
            self.hex.clone(),
            self.base64.clone(),
            self.generate.clone(),
        )
    }
}

/// One `assert` entry of `expect_message`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageAssert {
    /// A message path such as `MSH.10` or `R.3`.
    pub path: String,
    /// Exact text.
    #[serde(default)]
    pub equals: Option<String>,
    /// Regular expression.
    #[serde(default)]
    pub matches: Option<String>,
    /// Whether the field has a value.
    #[serde(default)]
    pub exists: Option<bool>,
}

/// `expect_message`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectMessageArgs {
    /// The protocol.
    pub protocol: Protocol,
    /// HL7: code or codes to send as ACK (default `AA`); ASTM: `ack` (default) or `none`.
    #[serde(default)]
    pub reply: Option<Codes>,
    /// HL7: pause between replies.
    #[serde(default)]
    pub reply_delay_ms: Option<u64>,
    /// ASTM: NAK the first n frames once.
    #[serde(default)]
    pub nak_frames: Option<u32>,
    /// Checks on the received message.
    #[serde(default)]
    pub assert: Vec<MessageAssert>,
    /// Wait limit.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Steps taking only `timeout_ms`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeoutArgs {
    /// Wait limit.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// Steps taking only `for_ms`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForArgs {
    /// Window length.
    #[serde(default)]
    pub for_ms: Option<u64>,
}

/// `close`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseArgs {
    /// Reset instead of a graceful close.
    #[serde(default)]
    pub abort: bool,
}

/// `wait`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitArgs {
    /// Milliseconds to sleep.
    pub ms: u64,
}

/// `write_file`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteFileArgs {
    /// File name inside the folder.
    pub name: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    hex: Option<String>,
    #[serde(default)]
    base64: Option<String>,
    #[serde(default)]
    generate: Option<Generate>,
    /// Write under a temporary name, then rename (default true).
    #[serde(default = "yes")]
    pub atomic: bool,
}

const fn yes() -> bool {
    true
}

impl WriteFileArgs {
    /// The payload fields.
    #[must_use]
    pub fn payload(&self) -> PayloadSpec {
        PayloadSpec::from_parts(
            self.text.clone(),
            self.hex.clone(),
            self.base64.clone(),
            self.generate.clone(),
        )
    }
}

/// `expect_file`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectFileArgs {
    /// Exact relative name.
    #[serde(default)]
    pub name: Option<String>,
    /// Glob relative to the folder.
    #[serde(default)]
    pub pattern: Option<String>,
    /// Exact content.
    #[serde(default)]
    pub equals: Option<PayloadSpec>,
    /// Substrings the content must contain.
    #[serde(default)]
    pub contains: Option<Vec<String>>,
    /// Wait limit.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// `expect_no_file`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExpectNoFileArgs {
    /// Glob relative to the folder.
    pub pattern: String,
    /// Window length.
    #[serde(default)]
    pub for_ms: Option<u64>,
}

/// `delete_file`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteFileArgs {
    /// File name inside the folder.
    pub name: String,
}

/// Result of an operator command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandExpect {
    /// Accepted.
    #[default]
    Accepted,
    /// Rejected.
    Rejected,
}

/// `pause_channel`, `resume_channel`, `restart_channel`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelCommandArgs {
    /// The channel id.
    pub channel: String,
    /// Expected result.
    #[serde(default)]
    pub expect: CommandExpect,
    /// Stable reason code of a rejection.
    #[serde(default)]
    pub reason_code: Option<String>,
}

/// Which destinations a resend targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destinations {
    /// All.
    All,
    /// The listed ones.
    List(Vec<String>),
}

impl<'de> Deserialize<'de> for Destinations {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match Value::deserialize(d)? {
            Value::String(s) if s == "all" => Ok(Self::All),
            Value::Array(items) => items
                .into_iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| D::Error::custom("destination ids are strings"))
                })
                .collect::<Result<_, _>>()
                .map(Self::List),
            _ => Err(D::Error::custom("`all` or an array of destination ids")),
        }
    }
}

/// Resend mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResendMode {
    /// Reuse the message.
    #[default]
    SameMessage,
    /// Create a new message.
    NewMessage,
}

/// `resend`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResendArgs {
    /// The channel id.
    pub channel: String,
    /// Selector over message fields.
    pub message: BTreeMap<String, Matcher>,
    /// Destinations.
    pub destinations: Destinations,
    /// Mode.
    #[serde(default)]
    pub mode: ResendMode,
    /// Acknowledge the duplicate risk.
    #[serde(default)]
    pub confirm_duplicate_risk: bool,
    /// Expected result.
    #[serde(default)]
    pub expect: CommandExpect,
    /// Stable reason code of a rejection.
    #[serde(default)]
    pub reason_code: Option<String>,
}

/// `close_delivery`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseDeliveryArgs {
    /// The channel id.
    pub channel: String,
    /// Selector over delivery fields.
    pub delivery: BTreeMap<String, Matcher>,
    /// Operator reason.
    pub reason: String,
    /// Expected result.
    #[serde(default)]
    pub expect: CommandExpect,
    /// Stable reason code of a rejection.
    #[serde(default)]
    pub reason_code: Option<String>,
}

/// `validate_config`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateConfigArgs {
    /// Inline document.
    #[serde(default)]
    pub runtime: Option<Value>,
    /// Document file relative to the scenario file.
    #[serde(default)]
    pub runtime_file: Option<String>,
    /// `valid` (default) or `invalid`.
    #[serde(default)]
    pub expect: ValidateExpect,
    /// Problem codes that must all be present.
    #[serde(default)]
    pub problems: Vec<String>,
}

/// Outcome of `validate_config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidateExpect {
    /// Valid.
    #[default]
    Valid,
    /// Invalid.
    Invalid,
}

/// `apply_config`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyConfigArgs {
    /// Inline document.
    #[serde(default)]
    pub runtime: Option<Value>,
    /// Document file relative to the scenario file.
    #[serde(default)]
    pub runtime_file: Option<String>,
    /// `applied` (default), `rejected` or `partial`.
    #[serde(default)]
    pub expect: ApplyExpect,
    /// Expected per-channel outcomes.
    #[serde(default)]
    pub outcomes: BTreeMap<String, String>,
    /// Problem codes of a rejection.
    #[serde(default)]
    pub problems: Vec<String>,
}

/// Outcome of `apply_config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplyExpect {
    /// Applied.
    #[default]
    Applied,
    /// Rejected.
    Rejected,
    /// Partly applied.
    Partial,
}

/// What an assertion looks at.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Target {
    /// A channel.
    Channel {
        /// Its id.
        channel: String,
    },
    /// An endpoint.
    Endpoint {
        /// Its id.
        endpoint: String,
    },
    /// The node.
    Node {
        /// Always `true`.
        node: bool,
    },
}

/// `assert_snapshot`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertSnapshotArgs {
    /// What to inspect.
    pub target: Target,
    /// Path to matcher.
    pub expect: BTreeMap<String, Matcher>,
    /// Poll window.
    #[serde(default)]
    pub within_ms: Option<u64>,
}

/// `assert_alert` scope.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertScope {
    /// Channel id.
    #[serde(default)]
    pub channel: Option<String>,
    /// Endpoint id.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Destination id.
    #[serde(default)]
    pub destination: Option<String>,
}

/// `assert_alert`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertAlertArgs {
    /// Alert code.
    pub code: String,
    /// Scope filter.
    #[serde(default)]
    pub scope: AlertScope,
    /// Whether an active alert must exist (default true).
    #[serde(default = "yes")]
    pub present: bool,
    /// Field checks on the alert.
    #[serde(default)]
    pub expect: BTreeMap<String, Matcher>,
    /// Poll window.
    #[serde(default)]
    pub within_ms: Option<u64>,
}

/// `assert_log`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertLogArgs {
    /// Event name.
    pub event: String,
    /// Field checks.
    #[serde(default)]
    pub fields: BTreeMap<String, Matcher>,
    /// Count check (default at least 1).
    #[serde(default)]
    pub count: Option<Matcher>,
    /// Poll window.
    #[serde(default)]
    pub within_ms: Option<u64>,
}

/// Store tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Table {
    /// Messages.
    Messages,
    /// Deliveries.
    Deliveries,
    /// Delivery attempts.
    Attempts,
    /// Alerts.
    Alerts,
}

/// `assert_store`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertStoreArgs {
    /// The table.
    pub table: Table,
    /// Row selection.
    #[serde(default, rename = "where")]
    pub where_: BTreeMap<String, Matcher>,
    /// Count check (default at least 1).
    #[serde(default)]
    pub count: Option<Matcher>,
    /// Checks every selected row must pass.
    #[serde(default)]
    pub each: BTreeMap<String, Matcher>,
    /// Poll window.
    #[serde(default)]
    pub within_ms: Option<u64>,
}

/// Every step type.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    /// Bind the server's port.
    Listen(NoArgs),
    /// Close the listening socket.
    StopListening(NoArgs),
    /// Claim a connection.
    Accept(AcceptArgs),
    /// Connect.
    Connect(ConnectArgs),
    /// Write bytes.
    Send(SendArgs),
    /// Protocol-aware send.
    SendMessage(SendMessageArgs),
    /// Finish an asynchronous operation.
    AwaitSend(AwaitSendArgs),
    /// Read exact bytes.
    Expect(ExpectArgs),
    /// Protocol-aware receive.
    ExpectMessage(ExpectMessageArgs),
    /// The connection ended.
    ExpectClosed(TimeoutArgs),
    /// The connection stays open.
    ExpectOpen(ForArgs),
    /// Nothing arrives.
    ExpectNoData(ForArgs),
    /// Stop draining the socket.
    PauseReading(NoArgs),
    /// Drain again.
    ResumeReading(NoArgs),
    /// Close or unplug.
    Close(CloseArgs),
    /// Sleep.
    Wait(WaitArgs),
    /// Write a file.
    WriteFile(WriteFileArgs),
    /// Wait for a file.
    ExpectFile(ExpectFileArgs),
    /// No matching file appears.
    ExpectNoFile(ExpectNoFileArgs),
    /// Remove a file.
    DeleteFile(DeleteFileArgs),
    /// Start the engine.
    StartEngine(NoArgs),
    /// Stop the engine gracefully.
    StopEngine(NoArgs),
    /// Kill the engine.
    CrashEngine(NoArgs),
    /// Pause a channel.
    PauseChannel(ChannelCommandArgs),
    /// Resume a channel.
    ResumeChannel(ChannelCommandArgs),
    /// Restart a channel.
    RestartChannel(ChannelCommandArgs),
    /// Resend a message.
    Resend(ResendArgs),
    /// Close a delivery.
    CloseDelivery(CloseDeliveryArgs),
    /// Validate a configuration.
    ValidateConfig(ValidateConfigArgs),
    /// Apply a configuration.
    ApplyConfig(ApplyConfigArgs),
    /// Hold store commits.
    HoldStoreCommits(NoArgs),
    /// Release store commits.
    ReleaseStoreCommits(NoArgs),
    /// Check a snapshot.
    AssertSnapshot(AssertSnapshotArgs),
    /// Check alerts.
    AssertAlert(AssertAlertArgs),
    /// Check log events.
    AssertLog(AssertLogArgs),
    /// Check store rows.
    AssertStore(AssertStoreArgs),
}

/// Which actors may run a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorRule {
    /// Only `engine`.
    Engine,
    /// A `tcp_server`.
    TcpServer,
    /// A `tcp_client` or `serial_peer`.
    Connector,
    /// Any actor with a connection.
    Connection,
    /// A `folder`.
    Folder,
    /// Any actor or `engine`.
    Any,
}

impl Action {
    /// The `do` name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Listen(_) => "listen",
            Self::StopListening(_) => "stop_listening",
            Self::Accept(_) => "accept",
            Self::Connect(_) => "connect",
            Self::Send(_) => "send",
            Self::SendMessage(_) => "send_message",
            Self::AwaitSend(_) => "await_send",
            Self::Expect(_) => "expect",
            Self::ExpectMessage(_) => "expect_message",
            Self::ExpectClosed(_) => "expect_closed",
            Self::ExpectOpen(_) => "expect_open",
            Self::ExpectNoData(_) => "expect_no_data",
            Self::PauseReading(_) => "pause_reading",
            Self::ResumeReading(_) => "resume_reading",
            Self::Close(_) => "close",
            Self::Wait(_) => "wait",
            Self::WriteFile(_) => "write_file",
            Self::ExpectFile(_) => "expect_file",
            Self::ExpectNoFile(_) => "expect_no_file",
            Self::DeleteFile(_) => "delete_file",
            Self::StartEngine(_) => "start_engine",
            Self::StopEngine(_) => "stop_engine",
            Self::CrashEngine(_) => "crash_engine",
            Self::PauseChannel(_) => "pause_channel",
            Self::ResumeChannel(_) => "resume_channel",
            Self::RestartChannel(_) => "restart_channel",
            Self::Resend(_) => "resend",
            Self::CloseDelivery(_) => "close_delivery",
            Self::ValidateConfig(_) => "validate_config",
            Self::ApplyConfig(_) => "apply_config",
            Self::HoldStoreCommits(_) => "hold_store_commits",
            Self::ReleaseStoreCommits(_) => "release_store_commits",
            Self::AssertSnapshot(_) => "assert_snapshot",
            Self::AssertAlert(_) => "assert_alert",
            Self::AssertLog(_) => "assert_log",
            Self::AssertStore(_) => "assert_store",
        }
    }

    /// Which actors may run this step.
    #[must_use]
    pub fn actor_rule(&self) -> ActorRule {
        match self {
            Self::Listen(_) | Self::StopListening(_) | Self::Accept(_) => ActorRule::TcpServer,
            Self::Connect(_) => ActorRule::Connector,
            Self::Send(_)
            | Self::SendMessage(_)
            | Self::Expect(_)
            | Self::ExpectMessage(_)
            | Self::ExpectClosed(_)
            | Self::ExpectOpen(_)
            | Self::ExpectNoData(_)
            | Self::PauseReading(_)
            | Self::ResumeReading(_)
            | Self::Close(_) => ActorRule::Connection,
            Self::AwaitSend(_) | Self::Wait(_) => ActorRule::Any,
            Self::WriteFile(_)
            | Self::ExpectFile(_)
            | Self::ExpectNoFile(_)
            | Self::DeleteFile(_) => ActorRule::Folder,
            Self::StartEngine(_)
            | Self::StopEngine(_)
            | Self::CrashEngine(_)
            | Self::PauseChannel(_)
            | Self::ResumeChannel(_)
            | Self::RestartChannel(_)
            | Self::Resend(_)
            | Self::CloseDelivery(_)
            | Self::ValidateConfig(_)
            | Self::ApplyConfig(_)
            | Self::HoldStoreCommits(_)
            | Self::ReleaseStoreCommits(_)
            | Self::AssertSnapshot(_)
            | Self::AssertAlert(_)
            | Self::AssertLog(_)
            | Self::AssertStore(_) => ActorRule::Engine,
        }
    }

    /// The capability the step needs beyond the scenario's own resources.
    #[must_use]
    pub fn needs_test_hooks(&self) -> bool {
        matches!(
            self,
            Self::CrashEngine(_) | Self::HoldStoreCommits(_) | Self::ReleaseStoreCommits(_)
        )
    }
}

/// Parses one substituted step object.
pub fn parse_step(index: usize, value: Value) -> Result<Step, String> {
    let at = |m: String| format!("steps[{index}]: {m}");
    let Value::Object(mut map) = value else {
        return Err(at("a step must be an object".into()));
    };
    let name = map
        .get("do")
        .and_then(Value::as_str)
        .ok_or_else(|| at("`do` is required".into()))?
        .to_owned();
    let actor = take_string(&mut map, "actor").map_err(&at)?;
    let id = take_string(&mut map, "id").ok();
    map.remove("note");
    let actor = actor.ok_or_else(|| at(format!("`actor` is required for `{name}`")))?;
    let action: Action = from_json(Value::Object(map)).map_err(|e| at(format!("{name}: {e}")))?;
    Ok(Step {
        index,
        id: id.flatten(),
        actor,
        action,
    })
}

fn take_string(map: &mut Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match map.remove(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(format!("`{key}` must be a string, found {other}")),
    }
}
