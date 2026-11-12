//! The in-memory log ring: the newest events, queryable with filters and followable live.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use packie_core::{ChannelId, EndpointId, MessageId, Timestamp};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use ts_rs::TS;

/// Severity of a log event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    /// Very detailed tracing.
    Trace,
    /// Debugging detail.
    Debug,
    /// Normal operation.
    Info,
    /// Something unexpected the engine handled.
    Warn,
    /// A failure.
    Error,
}

impl From<Level> for LogLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::TRACE => Self::Trace,
            Level::DEBUG => Self::Debug,
            Level::INFO => Self::Info,
            Level::WARN => Self::Warn,
            _ => Self::Error,
        }
    }
}

/// One structured log event as served over IPC. `channel_id`, `endpoint_id` and
/// `message_id` are lifted from the event or its enclosing spans; every other field is in
/// `fields` as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LogEvent {
    /// Position in the ring; increases by one per event since the engine started.
    #[ts(type = "number")]
    pub seq: u64,
    /// When the event was recorded.
    pub at: Timestamp,
    /// Severity.
    pub level: LogLevel,
    /// Module that logged it, such as `packie_routing::bridge`.
    pub target: String,
    /// The message text.
    pub message: String,
    /// The catalog event (`channel.started`, `listener.peer_rejected`, ...), when set.
    pub event: Option<String>,
    /// Channel the event concerns.
    pub channel_id: Option<ChannelId>,
    /// Endpoint the event concerns.
    pub endpoint_id: Option<EndpointId>,
    /// Message the event concerns.
    pub message_id: Option<MessageId>,
    /// Other fields.
    pub fields: BTreeMap<String, String>,
}

/// Selects log events. Absent criteria match everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct LogFilter {
    /// Events at this level or more severe.
    pub min_level: Option<LogLevel>,
    /// Events at or after this time.
    pub since: Option<Timestamp>,
    /// Events before this time.
    pub until: Option<Timestamp>,
    /// Events about this channel.
    pub channel_id: Option<ChannelId>,
    /// Events about this endpoint.
    pub endpoint_id: Option<EndpointId>,
    /// Case-insensitive text in the message, target or any field.
    pub text: Option<String>,
    /// Events with this catalog event name.
    pub event: Option<String>,
    /// Events with `seq` greater than this, for paging forward.
    #[ts(type = "number | null")]
    pub after_seq: Option<u64>,
    /// Events with `seq` less than this, for paging back to older events.
    #[ts(type = "number | null")]
    pub before_seq: Option<u64>,
    /// At most this many events (the newest). Default 500.
    pub limit: Option<u32>,
}

impl LogFilter {
    /// Whether `event` matches every criterion except `limit`.
    #[must_use]
    pub fn matches(&self, event: &LogEvent) -> bool {
        self.min_level.is_none_or(|min| event.level >= min)
            && self.since.is_none_or(|since| event.at >= since)
            && self.until.is_none_or(|until| event.at < until)
            && self
                .channel_id
                .is_none_or(|id| event.channel_id == Some(id))
            && self
                .endpoint_id
                .is_none_or(|id| event.endpoint_id == Some(id))
            && self.after_seq.is_none_or(|seq| event.seq > seq)
            && self.before_seq.is_none_or(|seq| event.seq < seq)
            && self
                .event
                .as_deref()
                .is_none_or(|name| event.event.as_deref() == Some(name))
            && self
                .text
                .as_deref()
                .is_none_or(|text| contains_text(event, text))
    }
}

fn contains_text(event: &LogEvent, text: &str) -> bool {
    let needle = text.to_lowercase();
    let hit = |s: &str| s.to_lowercase().contains(&needle);
    hit(&event.message) || hit(&event.target) || event.fields.iter().any(|(k, v)| hit(k) || hit(v))
}

const DEFAULT_LIMIT: usize = 500;
const LIVE_CAPACITY: usize = 1024;

/// The ring buffer of recent log events. Cheap to clone; clones share the buffer.
#[derive(Clone)]
pub struct LogRing {
    inner: Arc<RingInner>,
}

struct RingInner {
    capacity: usize,
    events: Mutex<VecDeque<LogEvent>>,
    next_seq: AtomicU64,
    live: broadcast::Sender<LogEvent>,
}

impl RingInner {
    fn events(&self) -> MutexGuard<'_, VecDeque<LogEvent>> {
        self.events.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl fmt::Debug for LogRing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LogRing")
            .field("capacity", &self.inner.capacity)
            .finish_non_exhaustive()
    }
}

impl LogRing {
    /// A ring holding the newest `capacity` events (at least one).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            inner: Arc::new(RingInner {
                capacity,
                events: Mutex::new(VecDeque::with_capacity(capacity.min(4096))),
                next_seq: AtomicU64::new(1),
                live: broadcast::channel(LIVE_CAPACITY).0,
            }),
        }
    }

    /// The `tracing` layer that records into this ring.
    #[must_use]
    pub fn layer(&self) -> RingLayer {
        RingLayer { ring: self.clone() }
    }

    /// Matching events, oldest first, at most `filter.limit` (the newest of them).
    #[must_use]
    pub fn query(&self, filter: &LogFilter) -> Vec<LogEvent> {
        let limit = filter
            .limit
            .map_or(DEFAULT_LIMIT, |l| usize::try_from(l).unwrap_or(usize::MAX));
        let events = self.inner.events();
        let mut matched: Vec<LogEvent> = events
            .iter()
            .rev()
            .filter(|e| filter.matches(e))
            .take(limit)
            .cloned()
            .collect();
        matched.reverse();
        matched
    }

    /// Every event recorded from now on. Slow receivers miss events (`Lagged`) rather
    /// than slowing the engine.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<LogEvent> {
        self.inner.live.subscribe()
    }

    fn push(&self, mut event: LogEvent) {
        event.seq = self.inner.next_seq.fetch_add(1, Ordering::Relaxed);
        // Only a followed ring pays for a second copy.
        let live = (self.inner.live.receiver_count() > 0).then(|| event.clone());
        {
            let mut events = self.inner.events();
            if events.len() == self.inner.capacity {
                events.pop_front();
            }
            events.push_back(event);
        }
        if let Some(event) = live {
            // The receivers may have gone since the check.
            let _ = self.inner.live.send(event);
        }
    }
}

/// `tracing` layer that records events into a [`LogRing`].
#[derive(Debug, Clone)]
pub struct RingLayer {
    ring: LogRing,
}

/// Fields recorded on a span, kept in its extensions.
#[derive(Default)]
struct SpanFields(Fields);

#[derive(Default)]
struct Fields {
    message: Option<String>,
    event: Option<String>,
    channel_id: Option<ChannelId>,
    endpoint_id: Option<EndpointId>,
    message_id: Option<MessageId>,
    other: BTreeMap<String, String>,
}

impl Fields {
    fn set(&mut self, name: &str, value: String) {
        match name {
            "message" => self.message = Some(value),
            "event" => self.event = Some(value),
            "channel_id" => self.lift(name, value, |f, id| f.channel_id = Some(id)),
            "endpoint_id" => self.lift(name, value, |f, id| f.endpoint_id = Some(id)),
            "message_id" => self.lift(name, value, |f, id| f.message_id = Some(id)),
            _ => {
                self.other.insert(name.to_owned(), value);
            }
        }
    }

    /// Stores a typed id, or the raw text when it does not parse.
    fn lift<T: std::str::FromStr>(&mut self, name: &str, value: String, set: fn(&mut Self, T)) {
        match value.parse() {
            Ok(id) => set(self, id),
            Err(_) => {
                self.other.insert(name.to_owned(), value);
            }
        }
    }

    /// Fills what `self` lacks from `outer` (an enclosing span).
    fn inherit(&mut self, outer: &Self) {
        self.channel_id = self.channel_id.or(outer.channel_id);
        self.endpoint_id = self.endpoint_id.or(outer.endpoint_id);
        self.message_id = self.message_id.or(outer.message_id);
        for (k, v) in &outer.other {
            self.other.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.set(field.name(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.set(field.name(), format!("{value:?}"));
    }
}

impl<S> Layer<S> for RingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            let mut fields = Fields::default();
            attrs.record(&mut fields);
            span.extensions_mut().insert(SpanFields(fields));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(SpanFields(fields)) = span.extensions_mut().get_mut::<SpanFields>()
        {
            values.record(fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        // Innermost span first, so nearer spans win.
        for span in ctx.event_scope(event).into_iter().flatten() {
            if let Some(SpanFields(outer)) = span.extensions().get::<SpanFields>() {
                fields.inherit(outer);
            }
        }
        let metadata = event.metadata();
        self.ring.push(LogEvent {
            seq: 0,
            at: Timestamp::now(),
            level: (*metadata.level()).into(),
            target: metadata.target().to_owned(),
            message: fields.message.unwrap_or_default(),
            event: fields.event,
            channel_id: fields.channel_id,
            endpoint_id: fields.endpoint_id,
            message_id: fields.message_id,
            fields: fields.other,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    fn record(ring: &LogRing, body: impl FnOnce()) {
        let subscriber = tracing_subscriber::registry().with(ring.layer());
        tracing::subscriber::with_default(subscriber, body);
    }

    #[test]
    fn events_inherit_ids_from_spans() {
        let ring = LogRing::new(10);
        let channel = ChannelId::generate();
        let endpoint = EndpointId::generate();
        record(&ring, || {
            let span = tracing::info_span!("channel", channel_id = %channel);
            let _guard = span.enter();
            tracing::warn!(endpoint_id = %endpoint, bytes = 12, "write failed");
        });
        let events = ring.query(&LogFilter::default());
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.message, "write failed");
        assert_eq!(event.level, LogLevel::Warn);
        assert_eq!(event.channel_id, Some(channel));
        assert_eq!(event.endpoint_id, Some(endpoint));
        assert_eq!(event.fields["bytes"], "12");
    }

    #[test]
    fn ring_keeps_the_newest_and_filters() {
        let ring = LogRing::new(3);
        let channel = ChannelId::generate();
        record(&ring, || {
            tracing::info!("one");
            tracing::error!(channel_id = %channel, "two");
            tracing::debug!("three");
            tracing::info!(peer = "10.0.0.7", "four");
        });
        let all = ring.query(&LogFilter::default());
        let messages: Vec<_> = all.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(messages, ["two", "three", "four"]);
        assert_eq!(all[0].seq + 2, all[2].seq);

        let by = |filter: LogFilter| -> Vec<String> {
            ring.query(&filter).into_iter().map(|e| e.message).collect()
        };
        let info_up = LogFilter {
            min_level: Some(LogLevel::Info),
            ..LogFilter::default()
        };
        assert_eq!(by(info_up), ["two", "four"]);
        let channel_only = LogFilter {
            channel_id: Some(channel),
            ..LogFilter::default()
        };
        assert_eq!(by(channel_only), ["two"]);
        let text = LogFilter {
            text: Some("10.0.0".into()),
            ..LogFilter::default()
        };
        assert_eq!(by(text), ["four"]);
        let newest = LogFilter {
            limit: Some(1),
            ..LogFilter::default()
        };
        assert_eq!(by(newest), ["four"]);
        let after = LogFilter {
            after_seq: Some(all[1].seq),
            ..LogFilter::default()
        };
        assert_eq!(by(after), ["four"]);
    }

    #[test]
    fn live_subscribers_receive_new_events() {
        let ring = LogRing::new(10);
        let mut live = ring.subscribe();
        record(&ring, || tracing::info!("hello"));
        assert_eq!(live.try_recv().unwrap().message, "hello");
    }
}
