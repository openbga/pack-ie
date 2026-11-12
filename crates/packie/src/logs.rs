//! `packie logs`: recent log events from the engine's ring, optionally followed live.

use clap::Args;
use packie_core::{ChannelId, EndpointId, Timestamp};
use packie_diagnostics::{LogEvent, LogFilter, LogLevel};
use packie_ipc::{Event, Request, Response, Subscription, Topic};

use crate::client::{Target, ask, failed, unexpected};
use crate::output::{eprint_line, json_text, print_line, short_time};
use crate::{Failure, args};

#[derive(Debug, Args)]
pub(crate) struct LogsArgs {
    /// Keep printing new events until Ctrl-C.
    #[arg(long, short, conflicts_with = "until")]
    follow: bool,
    /// Minimum level: trace, debug, info, warn or error.
    #[arg(long, value_parser = args::log_level)]
    level: Option<LogLevel>,
    /// Only events about this channel id.
    #[arg(long)]
    channel: Option<ChannelId>,
    /// Only events about this endpoint id.
    #[arg(long)]
    endpoint: Option<EndpointId>,
    /// Only events containing this text (case-insensitive) in the message, target or fields.
    #[arg(long)]
    text: Option<String>,
    /// Only this catalog event, for example `listener.peer_rejected`.
    #[arg(long)]
    event: Option<String>,
    /// Only events at or after this time: RFC 3339 (2026-11-06T08:00:00Z) or an age such
    /// as 15m, 2h or 7d.
    #[arg(long, value_parser = args::time)]
    since: Option<Timestamp>,
    /// Only events before this time (same forms as --since).
    #[arg(long, value_parser = args::time)]
    until: Option<Timestamp>,
    /// How many of the newest matching events to print first.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..))]
    limit: u32,
    /// Print one JSON object per line instead of text.
    #[arg(long)]
    json: bool,
}

impl LogsArgs {
    fn filter(&self) -> LogFilter {
        LogFilter {
            min_level: self.level,
            since: self.since,
            until: self.until,
            channel_id: self.channel,
            endpoint_id: self.endpoint,
            text: self.text.clone(),
            event: self.event.clone(),
            after_seq: None,
            before_seq: None,
            limit: Some(self.limit),
        }
    }
}

pub(crate) async fn logs(target: &Target, args: &LogsArgs) -> Result<(), Failure> {
    let filter = args.filter();
    let client = target.connect().await?;
    // Subscribe before reading history, so no event falls between the two. The engine
    // applies the filter to both.
    let live = if args.follow {
        let topic = Topic::Logs {
            filter: filter.clone(),
        };
        Some(client.subscribe(topic).await.map_err(failed)?)
    } else {
        None
    };
    let Response::Logs { events } = ask(&client, Request::GetLogs { filter }).await? else {
        return Err(unexpected());
    };
    for event in &events {
        print_line(&render(event, args.json)?);
    }
    if let Some(live) = live {
        return follow(live, events.last().map_or(0, |e| e.seq), args.json).await;
    }
    if events.is_empty() {
        eprint_line("No log events match. The engine keeps recent events in memory.");
    }
    Ok(())
}

/// Prints events as they arrive until Ctrl-C. Events already printed from the history
/// (`last_seq` and older) are skipped.
async fn follow(mut live: Subscription, last_seq: u64, json: bool) -> Result<(), Failure> {
    eprint_line("Following the log; press Ctrl-C to stop.");
    let interrupt = tokio::signal::ctrl_c();
    tokio::pin!(interrupt);
    loop {
        tokio::select! {
            _ = &mut interrupt => return Ok(()),
            next = live.next() => match next {
                Some(Event::Log { event }) if event.seq > last_seq => {
                    print_line(&render(&event, json)?);
                }
                Some(Event::Lagged { missed }) => {
                    eprint_line(&format!("... {missed} events were skipped because the output fell behind ..."));
                }
                Some(_) => {}
                None => return Err(Failure::new("the engine closed the connection")),
            },
        }
    }
}

fn render(event: &LogEvent, json: bool) -> Result<String, Failure> {
    if json {
        serde_json::to_string(event).map_err(|e| Failure::new(e.to_string()))
    } else {
        Ok(log_line(event))
    }
}

/// `2026-11-06 12:00:00Z INFO  target message event=… channel_id=… key=value`.
fn log_line(event: &LogEvent) -> String {
    let mut parts = vec![format!(
        "{} {:<5} {} {}",
        short_time(event.at),
        json_text(&event.level).to_uppercase(),
        event.target,
        event.message
    )];
    parts.extend(event.event.iter().map(|name| format!("event={name}")));
    parts.extend(event.channel_id.map(|id| format!("channel_id={id}")));
    parts.extend(event.endpoint_id.map(|id| format!("endpoint_id={id}")));
    parts.extend(event.message_id.map(|id| format!("message_id={id}")));
    parts.extend(
        event
            .fields
            .iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use packie_core::Timestamp;
    use packie_diagnostics::{LogEvent, LogLevel};

    use super::log_line;

    #[test]
    fn a_log_line_shows_time_level_text_and_ids() {
        let event = LogEvent {
            seq: 7,
            at: Timestamp::parse("2026-11-06T12:00:00Z").unwrap(),
            level: LogLevel::Warn,
            target: "packie_routing".to_owned(),
            message: "Peer refused".to_owned(),
            event: Some("listener.peer_rejected".to_owned()),
            channel_id: Some("orders".parse().unwrap()),
            endpoint_id: None,
            message_id: None,
            fields: BTreeMap::from([("peer".to_owned(), "10.0.0.1".to_owned())]),
        };
        assert_eq!(
            log_line(&event),
            "2026-11-06 12:00:00Z WARN  packie_routing Peer refused event=listener.peer_rejected channel_id=orders peer=10.0.0.1"
        );
    }
}
