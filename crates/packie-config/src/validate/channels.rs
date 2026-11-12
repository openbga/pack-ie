//! Checks of channels (§6): bridges, message channels, destinations, policies, and the
//! filters and mappings compiled by `packie-transform` (§8).

use std::collections::{BTreeMap, HashSet};

use packie_core::{AckLevel, ChannelId, EndpointId, EndpointKind};
use rust_decimal::Decimal;

use super::{Endpoints, MAX_MESSAGE, ProblemCode, Role, Validator, count};
use crate::channel::{
    BridgeChannel, BufferPolicy, ChannelConfig, ConnectStrategy, DeliveryOverrides, DeliveryPolicy,
    Destination, MessageChannel, Retention, RetryPolicy, TapFramer,
};
use crate::endpoint::EndpointConfig;

impl Validator {
    pub(super) fn channels(&mut self, channels: &[ChannelConfig], endpoints: &Endpoints<'_>) {
        let mut ids: HashSet<ChannelId> = HashSet::new();
        for (i, channel) in channels.iter().enumerate() {
            let path = format!("channels[{i}]");
            if !ids.insert(channel.id()) {
                self.duplicate(format!("{path}.channel_id"), "channel id");
            }
            self.label(&path, channel.label());
            self.tags(&path, channel.tags());
            match channel {
                ChannelConfig::Bridge(bridge) => self.bridge(&path, bridge, endpoints),
                ChannelConfig::Message(message) => self.message(&path, message, endpoints),
            }
        }
        self.shared_exclusive(channels, endpoints);
    }

    /// Looks up a referenced endpoint, reporting a missing or disabled one.
    fn reference<'a>(
        &mut self,
        path: &str,
        id: EndpointId,
        channel_enabled: bool,
        endpoints: &Endpoints<'a>,
    ) -> Option<&'a EndpointConfig> {
        let Some(endpoint) = endpoints.get(&id) else {
            self.err(
                ProblemCode::UnknownEndpoint,
                path,
                format!("no endpoint {id}"),
            );
            return None;
        };
        if channel_enabled && !endpoint.enabled {
            self.err(
                ProblemCode::EndpointDisabled,
                path,
                format!("endpoint {id} is disabled; enable it or disable this channel"),
            );
        }
        Some(endpoint)
    }

    fn bridge(&mut self, path: &str, bridge: &BridgeChannel, endpoints: &Endpoints<'_>) {
        let mut kinds = Vec::new();
        for (field, id) in [
            ("endpoint_a", bridge.endpoint_a),
            ("endpoint_b", bridge.endpoint_b),
        ] {
            let at = format!("{path}.{field}");
            let Some(endpoint) = self.reference(&at, id, bridge.enabled, endpoints) else {
                continue;
            };
            if endpoint.kind().is_stream() {
                kinds.push(endpoint.kind());
            } else {
                self.err(
                    ProblemCode::EndpointKindNotAllowed,
                    at,
                    "a bridge needs a serial, tcp_client or tcp_listener endpoint",
                );
            }
        }
        if bridge.endpoint_a == bridge.endpoint_b {
            self.err(
                ProblemCode::SameEndpoint,
                format!("{path}.endpoint_b"),
                "must differ from endpoint_a",
            );
        }
        self.connection(path, bridge, &kinds);
        self.buffer(&format!("{path}.buffer"), &bridge.buffer);
        self.retry(&format!("{path}.retry"), &bridge.retry);
        let tap = &bridge.tap;
        if tap.record_messages && tap.framer == TapFramer::None {
            self.err(
                ProblemCode::TapInvalid,
                format!("{path}.tap.record_messages"),
                "recording needs a framer",
            );
        }
        self.range(
            &format!("{path}.tap"),
            "max_message_bytes",
            tap.max_message_bytes,
            1024..=MAX_MESSAGE,
        );
        self.retention_limits(
            &format!("{path}.tap.retention"),
            tap.retention.max_age_days,
            tap.retention.max_messages,
            tap.record_messages,
        );
    }

    fn connection(&mut self, path: &str, bridge: &BridgeChannel, kinds: &[EndpointKind]) {
        let c = &bridge.connection;
        let path = format!("{path}.connection");
        let receiver = format!("{path}.receiver");
        match (c.connect_strategy, c.receiver) {
            (ConnectStrategy::ReceiverThenSender, None) => self.err(
                ProblemCode::ReceiverRequired,
                receiver,
                "required for receiver_then_sender",
            ),
            (ConnectStrategy::Parallel | ConnectStrategy::WaitForListenerThenPair, Some(_)) => self
                .err(
                    ProblemCode::ReceiverRequired,
                    receiver,
                    "only used by receiver_then_sender",
                ),
            _ => {}
        }
        if c.connect_strategy == ConnectStrategy::WaitForListenerThenPair
            && kinds.len() == 2
            && !kinds.contains(&EndpointKind::TcpListener)
        {
            self.err(
                ProblemCode::ListenerStrategy,
                format!("{path}.connect_strategy"),
                "wait_for_listener_then_pair needs a tcp_listener endpoint",
            );
        }
        self.positive(&path, "pairing_timeout_ms", c.pairing_timeout_ms);
    }

    fn buffer(&mut self, path: &str, buffer: &BufferPolicy) {
        self.positive(path, "max_buffered_segments", buffer.max_buffered_segments);
        self.positive(path, "max_bytes", buffer.max_bytes);
        self.positive(path, "block_timeout_ms", buffer.block_timeout_ms);
    }

    fn retry(&mut self, path: &str, retry: &RetryPolicy) {
        let checks = [
            (
                "initial_delay_ms",
                retry.initial_delay_ms == 0,
                "must be greater than 0",
            ),
            (
                "max_delay_ms",
                retry.max_delay_ms < retry.initial_delay_ms,
                "must be at least initial_delay_ms",
            ),
            (
                "multiplier",
                retry.multiplier < Decimal::ONE,
                "must be at least 1.0",
            ),
            (
                // Stored and sent as a JSON number; four places survive the round trip exactly.
                "multiplier",
                retry.multiplier.normalize().scale() > 4,
                "must have at most 4 decimal places",
            ),
            (
                "jitter_percent",
                retry.jitter_percent > 100,
                "must be from 0 to 100",
            ),
            (
                "max_consecutive_failures_before_alert",
                retry.max_consecutive_failures_before_alert == 0,
                "must be greater than 0",
            ),
        ];
        for (field, bad, message) in checks {
            if bad {
                self.err(
                    ProblemCode::RetryInvalid,
                    format!("{path}.{field}"),
                    message,
                );
            }
        }
    }

    fn message(&mut self, path: &str, channel: &MessageChannel, endpoints: &Endpoints<'_>) {
        let source_path = format!("{path}.source");
        let source = &channel.source;
        let endpoint_path = format!("{source_path}.endpoint");
        if let Some(endpoint) =
            self.reference(&endpoint_path, source.endpoint, channel.enabled, endpoints)
        {
            self.protocol_fits(&source_path, &source.protocol, endpoint.kind());
        }
        self.protocol(&source_path, &source.protocol, Role::Source);
        let schema = source.protocol.schema();
        self.filters(&format!("{path}.filters"), &channel.filters, &schema);
        self.retry(&format!("{path}.retry"), &channel.retry);
        self.delivery_policy(&format!("{path}.delivery"), &channel.delivery);
        self.range(
            path,
            "destinations",
            count(channel.destinations.len()),
            1..=20,
        );
        let mut ids = HashSet::new();
        for (i, destination) in channel.destinations.iter().enumerate() {
            let at = format!("{path}.destinations[{i}]");
            if !ids.insert(destination.id) {
                self.duplicate(format!("{at}.destination_id"), "destination id");
            }
            self.destination(&at, channel, destination, endpoints, &schema);
        }
        self.retention(&format!("{path}.retention"), &channel.retention);
    }

    fn destination(
        &mut self,
        at: &str,
        channel: &MessageChannel,
        d: &Destination,
        endpoints: &Endpoints<'_>,
        source: &packie_transform::Schema,
    ) {
        self.label(at, &d.label);
        let enabled = channel.enabled && d.enabled;
        let endpoint_path = format!("{at}.endpoint");
        if let Some(endpoint) = self.reference(&endpoint_path, d.endpoint, enabled, endpoints) {
            self.protocol_fits(at, &d.protocol, endpoint.kind());
        }
        self.protocol(at, &d.protocol, Role::Destination);
        self.filters(&format!("{at}.filters"), &d.filters, source);
        match &d.mapping {
            None if d.protocol.kind() != channel.source.protocol.kind() => self.err(
                ProblemCode::PassthroughProtocol,
                format!("{at}.mapping"),
                "without a mapping the destination must use the source's protocol",
            ),
            Some(mapping) => {
                self.mapping(
                    &format!("{at}.mapping"),
                    mapping,
                    source,
                    &d.protocol.schema(),
                );
            }
            None => {}
        }
        self.delivery_overrides(&format!("{at}.delivery"), &d.delivery);
        let required = d.effective_delivery(&channel.delivery).ack_required;
        let highest = d.protocol.highest_ack_level();
        let path = format!("{at}.delivery.ack_required");
        if required == AckLevel::None {
            self.schema(path, "must be handed_off, received or accepted");
        } else if required > highest {
            self.err(
                ProblemCode::AckLevelUnreachable,
                path,
                d.protocol.ack_limit(),
            );
        }
    }

    fn delivery_policy(&mut self, path: &str, p: &DeliveryPolicy) {
        self.range(path, "max_attempts", p.max_attempts, 1..=100);
        self.retry(&format!("{path}.retry"), &p.retry);
        self.range(
            path,
            "send_timeout_seconds",
            p.send_timeout_seconds,
            1..=3600,
        );
        self.range(path, "ack_timeout_seconds", p.ack_timeout_seconds, 1..=3600);
    }

    fn delivery_overrides(&mut self, path: &str, o: &DeliveryOverrides) {
        if let Some(n) = o.max_attempts {
            self.range(path, "max_attempts", n, 1..=100);
        }
        if let Some(retry) = &o.retry {
            self.retry(&format!("{path}.retry"), retry);
        }
        if let Some(n) = o.send_timeout_seconds {
            self.range(path, "send_timeout_seconds", n, 1..=3600);
        }
        if let Some(n) = o.ack_timeout_seconds {
            self.range(path, "ack_timeout_seconds", n, 1..=3600);
        }
    }

    fn retention(&mut self, path: &str, r: &Retention) {
        self.retention_limits(path, r.max_age_days, r.max_messages, true);
    }

    /// The limits of a message channel's or a recording tap's retention; unbounded is a
    /// warning when messages are stored (`stores`).
    fn retention_limits(
        &mut self,
        path: &str,
        max_age_days: Option<u32>,
        max_messages: Option<u64>,
        stores: bool,
    ) {
        if let Some(days) = max_age_days {
            self.range(path, "max_age_days", days, 1..=36_500);
        }
        if let Some(messages) = max_messages {
            self.range(path, "max_messages", messages, 1000..=1_000_000_000);
        }
        if stores && max_age_days.is_none() && max_messages.is_none() {
            self.warn(
                ProblemCode::RetentionUnbounded,
                path,
                "messages are kept forever",
            );
        }
    }

    fn shared_exclusive(&mut self, channels: &[ChannelConfig], endpoints: &Endpoints<'_>) {
        let exclusive = |id: &EndpointId| {
            endpoints.get(id).is_some_and(|e| {
                matches!(e.kind(), EndpointKind::Serial | EndpointKind::TcpListener)
            })
        };
        let mut users: BTreeMap<EndpointId, usize> = BTreeMap::new();
        for channel in channels.iter().filter(|c| c.enabled()) {
            for id in channel.endpoint_ids().into_iter().filter(exclusive) {
                *users.entry(id).or_default() += 1;
            }
        }
        for (id, n) in users.into_iter().filter(|(_, n)| *n > 1) {
            self.warn(
                ProblemCode::SharedExclusiveEndpoint,
                "channels",
                format!("{n} enabled channels use endpoint {id}; only one holds it at a time"),
            );
        }
    }

    /// Filter expressions (§8.1), compiled by `packie-transform` against the model of the
    /// message they filter.
    fn filters(
        &mut self,
        path: &str,
        filters: &[serde_json::Value],
        schema: &packie_transform::Schema,
    ) {
        if let Err(problems) = packie_transform::Filters::compile(filters, schema, path) {
            for problem in problems {
                self.err(ProblemCode::FilterInvalid, problem.path, problem.message);
            }
        }
    }

    /// A mapping (§8.2), compiled by `packie-transform`: `from` paths against the source
    /// model, `to` paths against the destination's.
    fn mapping(
        &mut self,
        path: &str,
        mapping: &serde_json::Value,
        source: &packie_transform::Schema,
        target: &packie_transform::Schema,
    ) {
        if let Err(problems) = packie_transform::Mapping::compile(mapping, source, target, path) {
            for problem in problems {
                self.err(ProblemCode::MappingInvalid, problem.path, problem.message);
            }
        }
    }
}
