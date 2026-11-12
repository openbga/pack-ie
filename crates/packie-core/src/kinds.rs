//! The state and kind vocabulary shared by configuration, routing, diagnostics and IPC.
//!
//! Each enum serializes as the `snake_case` text given here; `as_str` returns the same
//! text for structured log fields and `FromStr` parses it.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ParseError;

macro_rules! wire_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal, )+
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS,
        )]
        pub enum $name {
            $( $(#[$vmeta])* #[serde(rename = $text)] $variant, )+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The serialized text.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = ParseError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                match text {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(ParseError::Enum),
                }
            }
        }
    };
}

wire_enum! {
    /// Direction of traffic relative to the engine.
    pub enum Direction {
        /// Received from an endpoint.
        Inbound => "inbound",
        /// Sent to an endpoint.
        Outbound => "outbound",
    }
}

wire_enum! {
    /// One of the two sides of a bridge channel.
    pub enum BridgeSide {
        /// The `endpoint_a` side.
        A => "a",
        /// The `endpoint_b` side.
        B => "b",
    }
}

impl BridgeSide {
    /// The other side.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

wire_enum! {
    /// Transport of an endpoint.
    pub enum EndpointKind {
        /// A serial port (physical or virtual).
        Serial => "serial",
        /// An outbound TCP connection.
        TcpClient => "tcp_client",
        /// A TCP listener accepting connections.
        TcpListener => "tcp_listener",
        /// A local or shared folder.
        Folder => "folder",
        /// A directory on an SFTP server.
        Sftp => "sftp",
    }
}

impl EndpointKind {
    /// Whether the endpoint carries a live byte stream (and so can be bridged).
    #[must_use]
    pub const fn is_stream(self) -> bool {
        matches!(self, Self::Serial | Self::TcpClient | Self::TcpListener)
    }
}

wire_enum! {
    /// Protocol spoken by a connector or observed by a bridge tap.
    pub enum ProtocolKind {
        /// ASTM E1381/E1394 (LIS01-A2/LIS02-A2).
        Astm => "astm",
        /// Olympus/Beckman AU400/AU640 fixed-width records.
        Au => "au",
        /// HL7 v2.x over MLLP.
        Hl7 => "hl7_mllp",
        /// Delimited files (CSV and similar).
        Delimited => "delimited_file",
        /// Raw bytes, no parsing.
        Raw => "raw",
    }
}

wire_enum! {
    /// Kind of channel.
    pub enum ChannelKind {
        /// Live byte stream between two endpoints.
        Bridge => "bridge",
        /// Store-and-forward messages from one source to one or more destinations.
        Message => "message",
    }
}

wire_enum! {
    /// Supervision state of a channel.
    pub enum ChannelState {
        /// First start, not yet running.
        Starting => "starting",
        /// Working normally.
        Running => "running",
        /// A failure occurred; waiting for the next attempt.
        Retrying => "retrying",
        /// Running with a problem the operator should see (for example buffer pressure).
        Degraded => "degraded",
        /// Not running; see the [`StopReason`].
        Stopped => "stopped",
    }
}

wire_enum! {
    /// Connection state of an endpoint.
    pub enum EndpointState {
        /// Disabled in configuration.
        Disabled => "disabled",
        /// Opening or connecting.
        Connecting => "connecting",
        /// Listening or watching, waiting for a peer or file.
        Listening => "listening",
        /// Connected or open.
        Connected => "connected",
        /// A failure occurred; waiting for the next attempt.
        Retrying => "retrying",
        /// Closed and not currently trying.
        Disconnected => "disconnected",
        /// Failed in a way retrying will not fix (for example invalid settings).
        Error => "error",
    }
}

wire_enum! {
    /// State of a bridge channel's live session.
    pub enum SessionState {
        /// Waiting for one or both sides to attach.
        WaitingForPeer => "waiting_for_peer",
        /// Both sides attached; bytes flow.
        Bridged => "bridged",
        /// One side closed; the close is propagating.
        Closing => "closing",
        /// No session.
        Closed => "closed",
    }
}

wire_enum! {
    /// State of one message to one destination.
    pub enum DeliveryState {
        /// Waiting to be sent.
        Queued => "queued",
        /// An attempt is in flight.
        Sending => "sending",
        /// The destination acknowledged the message at protocol level.
        Acknowledged => "acknowledged",
        /// The destination answered with a negative acknowledgement.
        Rejected => "rejected",
        /// The whole message was sent on an open connection and no required acknowledgement
        /// came in time; it may have been processed. Needs an operator decision.
        TimedOut => "timed_out",
        /// The message may or may not have arrived (for example the connection dropped
        /// after sending); needs an operator decision.
        OutcomeUnknown => "outcome_unknown",
        /// Retries are exhausted.
        Failed => "failed",
        /// Closed by an operator with a reason.
        Cancelled => "cancelled",
    }
}

impl DeliveryState {
    /// No further automatic attempt will be made.
    #[must_use]
    pub const fn is_final(self) -> bool {
        !matches!(self, Self::Queued | Self::Sending)
    }

    /// Nothing is left for anyone to do: acknowledged, or closed by an operator. Retention
    /// may purge a message only when all its deliveries are resolved.
    #[must_use]
    pub const fn is_resolved(self) -> bool {
        matches!(self, Self::Acknowledged | Self::Cancelled)
    }
}

wire_enum! {
    /// Why a channel is `stopped`.
    pub enum StopReason {
        /// Paused by an operator.
        OperatorPaused => "operator_paused",
        /// `enabled` is false in configuration.
        DisabledByConfig => "disabled_by_config",
        /// Removed by a configuration apply.
        RemovedByConfig => "removed_by_config",
        /// Being replaced by a configuration apply.
        ReplacedByConfig => "replaced_by_config",
        /// The engine is shutting down.
        Shutdown => "shutdown",
        /// This build of the engine cannot run the channel.
        NotImplemented => "not_implemented",
        /// A message channel waits for the message store, which could not be opened; the
        /// engine retries opening it and starts the channel when it opens.
        StoreUnavailable => "store_unavailable",
    }
}

impl StopReason {
    /// The stop is what an operator or the configuration asked for, not a problem.
    #[must_use]
    pub const fn is_intentional(self) -> bool {
        !matches!(self, Self::NotImplemented | Self::StoreUnavailable)
    }
}

wire_enum! {
    /// An operator action on a channel.
    pub enum ChannelCommand {
        /// Stop with reason `operator_paused` until resumed.
        Pause => "pause",
        /// Undo a pause.
        Resume => "resume",
        /// Stop and start again from the active definition.
        Restart => "restart",
    }
}

wire_enum! {
    /// Severity of an alert or log-level summary.
    pub enum Severity {
        /// For information.
        Info => "info",
        /// Needs attention soon.
        Warning => "warning",
        /// Needs attention now.
        Critical => "critical",
    }
}

wire_enum! {
    /// Which way a stored message travelled: received by a message channel, or observed
    /// by a bridge tap in one direction.
    pub enum MessageDirection {
        /// Received by a message channel's source.
        Inbound => "inbound",
        /// Observed on a bridge from A to B.
        AToB => "a_to_b",
        /// Observed on a bridge from B to A.
        BToA => "b_to_a",
    }
}

wire_enum! {
    /// State of a stored message (`docs/spec/message-channels.md` §3).
    pub enum MessageState {
        /// A multi-frame transfer is in progress; frames are committed as they arrive.
        Receiving => "receiving",
        /// Complete original bytes committed; the pipeline has not finished.
        Received => "received",
        /// The pipeline finished and at least one delivery exists.
        Routed => "routed",
        /// Excluded by a filter or for lack of an enabled destination.
        Filtered => "filtered",
        /// Same source identity and content as an earlier message.
        Duplicate => "duplicate",
        /// Complete bytes that do not parse or validate.
        Invalid => "invalid",
        /// The transfer ended without a complete message.
        Aborted => "aborted",
        /// Recorded from a bridge tap; never processed.
        Observed => "observed",
    }
}

wire_enum! {
    /// How a stored message came to exist.
    pub enum MessageOrigin {
        /// Received from a source connector.
        Received => "received",
        /// Created by an operator `new_message` resend.
        Resend => "resend",
        /// Recorded by a bridge tap.
        Tap => "tap",
    }
}

wire_enum! {
    /// What a delivery has proven (`docs/spec/message-channels.md` §8). Never read a lower
    /// level as processing success.
    pub enum AckLevel {
        /// Nothing proven yet.
        None => "none",
        /// The bytes or the complete file left the engine; no protocol confirmation.
        HandedOff => "handed_off",
        /// The peer's protocol layer confirmed receipt of the whole message.
        Received => "received",
        /// The peer's application accepted it.
        Accepted => "accepted",
    }
}

wire_enum! {
    /// How one delivery attempt ended.
    pub enum AttemptResult {
        /// Required acknowledgement level reached.
        Acknowledged => "acknowledged",
        /// Negative acknowledgement.
        Rejected => "rejected",
        /// Writing failed before the last byte.
        WriteFailed => "write_failed",
        /// The whole message could not be written in time.
        SendTimeout => "send_timeout",
        /// Sent, but no required acknowledgement in time.
        AckTimeout => "ack_timeout",
        /// The connection ended after the last byte, before the acknowledgement.
        ConnectionLostAfterSend => "connection_lost_after_send",
        /// The engine stopped or crashed during the attempt.
        EngineStopped => "engine_stopped",
        /// The destination mapping failed; nothing was sent.
        TransformError => "transform_error",
    }
}

wire_enum! {
    /// Roll-up of a message's deliveries.
    pub enum DeliverySummary {
        /// No deliveries.
        None => "none",
        /// Some `queued` or `sending`, none needing attention.
        Pending => "pending",
        /// Some `rejected`, `failed`, `timed_out` or `outcome_unknown`.
        Attention => "attention",
        /// Every delivery `acknowledged` or `cancelled`.
        Complete => "complete",
    }
}

impl DeliverySummary {
    /// The summary of deliveries in `states`.
    #[must_use]
    pub fn of(states: impl IntoIterator<Item = DeliveryState>) -> Self {
        let mut summary = Self::None;
        for state in states {
            let this = if state.is_resolved() {
                Self::Complete
            } else if state.is_final() {
                Self::Attention
            } else {
                Self::Pending
            };
            summary = match (summary, this) {
                (Self::Attention, _) | (_, Self::Attention) => Self::Attention,
                (Self::Pending, _) | (_, Self::Pending) => Self::Pending,
                _ => Self::Complete,
            };
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_wire<T>(all: &[T])
    where
        T: Copy + fmt::Display + FromStr<Err = ParseError> + Serialize + PartialEq + fmt::Debug,
    {
        for value in all {
            let text = value.to_string();
            assert_eq!(
                serde_json::to_value(value).unwrap(),
                serde_json::json!(text)
            );
            assert_eq!(text.parse::<T>(), Ok(*value));
        }
        assert_eq!("Nope".parse::<T>(), Err(ParseError::Enum));
    }

    #[test]
    fn serde_display_and_parse_agree() {
        assert_wire(Direction::ALL);
        assert_wire(BridgeSide::ALL);
        assert_wire(EndpointKind::ALL);
        assert_wire(ProtocolKind::ALL);
        assert_wire(ChannelKind::ALL);
        assert_wire(ChannelState::ALL);
        assert_wire(EndpointState::ALL);
        assert_wire(SessionState::ALL);
        assert_wire(DeliveryState::ALL);
        assert_wire(StopReason::ALL);
        assert_wire(Severity::ALL);
        assert_wire(MessageDirection::ALL);
        assert_wire(MessageState::ALL);
        assert_wire(MessageOrigin::ALL);
        assert_wire(AckLevel::ALL);
        assert_wire(AttemptResult::ALL);
        assert_wire(DeliverySummary::ALL);
        assert_wire(ChannelCommand::ALL);
    }

    #[test]
    fn delivery_summary_rolls_up() {
        use DeliveryState::*;
        assert_eq!(DeliverySummary::of([]), DeliverySummary::None);
        assert_eq!(
            DeliverySummary::of([Acknowledged, Cancelled]),
            DeliverySummary::Complete
        );
        assert_eq!(
            DeliverySummary::of([Acknowledged, Queued]),
            DeliverySummary::Pending
        );
        assert_eq!(
            DeliverySummary::of([Sending, TimedOut]),
            DeliverySummary::Attention
        );
        assert_eq!(
            DeliverySummary::of([Rejected, Acknowledged]),
            DeliverySummary::Attention
        );
    }

    #[test]
    fn snake_case_for_multi_word_values() {
        assert_eq!(EndpointKind::TcpListener.as_str(), "tcp_listener");
        assert_eq!(DeliveryState::OutcomeUnknown.as_str(), "outcome_unknown");
        assert_eq!(SessionState::WaitingForPeer.as_str(), "waiting_for_peer");
    }

    #[test]
    fn delivery_finality_and_resolution() {
        use DeliveryState::*;
        let pending: Vec<_> = DeliveryState::ALL
            .iter()
            .filter(|s| !s.is_final())
            .collect();
        assert_eq!(pending, [&Queued, &Sending]);
        let resolved: Vec<_> = DeliveryState::ALL
            .iter()
            .filter(|s| s.is_resolved())
            .collect();
        assert_eq!(resolved, [&Acknowledged, &Cancelled]);
    }

    #[test]
    fn typescript_is_a_string_union() {
        let cfg = ts_rs::Config::default();
        assert_eq!(
            ChannelState::decl(&cfg),
            r#"type ChannelState = "starting" | "running" | "retrying" | "degraded" | "stopped";"#
        );
    }

    #[test]
    fn stream_endpoints() {
        let streams: Vec<_> = EndpointKind::ALL.iter().filter(|k| k.is_stream()).collect();
        assert_eq!(
            streams,
            [
                &EndpointKind::Serial,
                &EndpointKind::TcpClient,
                &EndpointKind::TcpListener
            ]
        );
    }
}
