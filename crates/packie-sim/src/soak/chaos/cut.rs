//! What may happen to deliveries when a destination goes away, and the checks of it.
//!
//! A delivery that was still `queued` when the destination's endpoint went down must stay
//! `queued`: waiting is not failing (`message-channels.md` §5). The one delivery whose
//! attempt was in flight at that moment (the whole message written, no acknowledgement yet)
//! legitimately ends `outcome_unknown` with the attempt result `connection_lost_after_send`
//! (§4); it is never retried by itself unless `retry_unacknowledged` is set, so afterwards it
//! stays listed as needing a decision or is resolved. At most one delivery per destination
//! can be in that position, because a destination has one attempt in flight.

use packie_core::{ChannelId, MessageId};
use packie_ipc::messages::{MAX_PAGE, MessageFilter, MessageQuery};
use packie_ipc::{Request, Response};
use serde_json::{Value, json};

use crate::soak::chan::ChannelRt;
use crate::soak::rig::Rig;
use crate::soak::util::parse_utc_ms;

/// Clock difference allowed between the soak's cut time and the engine's attempt time.
const CLOCK_SLACK_MS: u64 = 2_000;
/// Pages of unresolved deliveries read at most; there should be a handful.
const MAX_PAGES: u32 = 5;

/// A delivery that needs a decision, as the Messages list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Listed {
    pub(super) message_id: String,
    pub(super) delivery_id: String,
    pub(super) control_id: String,
    pub(super) state: String,
}

/// The last attempt of a delivery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Attempt {
    /// When it started, Unix milliseconds.
    pub(super) started_ms: Option<u64>,
    pub(super) result: String,
}

fn unresolved_filter(ch: &ChannelRt) -> Option<MessageFilter> {
    let mut filter: MessageFilter = serde_json::from_value(json!({
        "delivery_states": ["rejected", "timed_out", "outcome_unknown", "failed"]
    }))
    .ok()?;
    filter.channels = vec![ch.plan.id.parse::<ChannelId>().ok()?];
    Some(filter)
}

/// The deliveries of the channel that need a decision; `None` when the engine cannot say.
pub(super) async fn list_unresolved(rig: &Rig, ch: &ChannelRt) -> Option<Vec<Listed>> {
    let filter = unresolved_filter(ch)?;
    let mut listed = Vec::new();
    for page in 0..MAX_PAGES {
        let query = MessageQuery {
            filter: filter.clone(),
            offset: page * MAX_PAGE,
            limit: MAX_PAGE,
            ..MessageQuery::default()
        };
        let Ok(Response::Messages(list)) = rig.call(Request::GetMessages(Box::new(query))).await
        else {
            return None;
        };
        let value = serde_json::to_value(&list).ok()?;
        let messages = value["messages"].as_array().cloned().unwrap_or_default();
        listed.extend(messages.iter().flat_map(listed_from));
        if (u64::from(page) + 1) * u64::from(MAX_PAGE) >= value["total"].as_u64().unwrap_or(0) {
            break;
        }
    }
    Some(listed)
}

/// The unresolved deliveries of one message row.
fn listed_from(message: &Value) -> Vec<Listed> {
    let text = |v: &Value| v.as_str().unwrap_or_default().to_owned();
    message["deliveries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| {
            matches!(
                d["state"].as_str(),
                Some("rejected" | "timed_out" | "outcome_unknown" | "failed")
            )
        })
        .map(|d| Listed {
            message_id: text(&message["message_id"]),
            delivery_id: text(&d["delivery_id"]),
            control_id: text(&message["control_id"]),
            state: text(&d["state"]),
        })
        .collect()
}

/// The deliveries of `now` that `before` did not have.
pub(super) fn new_since(before: &[Listed], now: &[Listed]) -> Vec<Listed> {
    now.iter()
        .filter(|n| !before.iter().any(|b| b.delivery_id == n.delivery_id))
        .cloned()
        .collect()
}

/// The last attempt of `delivery`, read from the message record.
pub(super) async fn last_attempt(rig: &Rig, delivery: &Listed) -> Attempt {
    let Ok(message_id) = delivery.message_id.parse::<MessageId>() else {
        return Attempt::default();
    };
    let Ok(Response::Message { message }) = rig.call(Request::GetMessage { message_id }).await
    else {
        return Attempt::default();
    };
    let Ok(value) = serde_json::to_value(&*message) else {
        return Attempt::default();
    };
    attempt_of(&value, &delivery.delivery_id)
}

fn attempt_of(record: &Value, delivery_id: &str) -> Attempt {
    let last = record["deliveries"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|d| d["delivery_id"].as_str() == Some(delivery_id))
        .and_then(|d| d["attempt_log"].as_array())
        .and_then(|log| log.last());
    Attempt {
        started_ms: last
            .and_then(|a| a["started_at"].as_str())
            .and_then(parse_utc_ms),
        result: last
            .and_then(|a| a["result"].as_str())
            .unwrap_or_default()
            .to_owned(),
    }
}

/// What is wrong with the deliveries that became unresolved while the destination was down
/// (cut at `cut_ms`, Unix milliseconds). Empty when only the one in-flight attempt did.
pub(super) fn judge_at_cut(new: &[(Listed, Attempt)], cut_ms: u64) -> Vec<String> {
    let mut problems = Vec::new();
    if new.len() > 1 {
        problems.push(format!(
            "{} deliveries became unresolved, but a destination has one attempt in flight at most; the others were queued and must have stayed queued",
            new.len()
        ));
    }
    for (delivery, attempt) in new {
        let who = format!("{} ({})", delivery.control_id, delivery.state);
        if delivery.state != "outcome_unknown" {
            problems.push(format!(
                "{who}: a delivery that was waiting for the destination changed state; only an attempt in flight at the cut may end as outcome_unknown"
            ));
        } else if attempt
            .started_ms
            .is_some_and(|at| at > cut_ms + CLOCK_SLACK_MS)
        {
            problems.push(format!(
                "{who}: its attempt started after the destination went down, so it was queued at the cut and must have stayed queued"
            ));
        } else if !attempt.result.is_empty() && attempt.result != "connection_lost_after_send" {
            problems.push(format!(
                "{who}: its last attempt ended `{}`; an attempt in flight at the cut ends `connection_lost_after_send`",
                attempt.result
            ));
        }
    }
    problems
}

/// What is wrong with the tolerated deliveries after the destination came back. The engine
/// does not retry an `outcome_unknown` delivery by itself, so it stays one or an operator
/// (or `retry_unacknowledged`) has resolved it; turning into another unresolved state, or
/// vanishing from the list while still unresolved, is a fault.
pub(super) fn judge_after(tolerated: &[Listed], now: &[Listed]) -> Vec<String> {
    tolerated
        .iter()
        .filter_map(|t| {
            let still = now.iter().find(|n| n.delivery_id == t.delivery_id)?;
            (still.state != "outcome_unknown").then(|| {
                format!(
                    "{}: after the destination returned, the delivery that was in flight at the cut is `{}` instead of staying outcome_unknown or being resolved",
                    t.control_id, still.state
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(id: &str, state: &str) -> Listed {
        Listed {
            message_id: format!("m-{id}"),
            delivery_id: id.to_owned(),
            control_id: format!("H01-{id}"),
            state: state.to_owned(),
        }
    }

    fn attempt(started_ms: u64, result: &str) -> Attempt {
        Attempt {
            started_ms: Some(started_ms),
            result: result.to_owned(),
        }
    }

    #[test]
    fn one_attempt_in_flight_at_the_cut_is_tolerated() {
        let new = [(
            listed("1", "outcome_unknown"),
            attempt(9_500, "connection_lost_after_send"),
        )];
        assert!(judge_at_cut(&new, 10_000).is_empty());
        // An attempt record that is not readable does not fail the check.
        let unknown = [(listed("1", "outcome_unknown"), Attempt::default())];
        assert!(judge_at_cut(&unknown, 10_000).is_empty());
        assert!(judge_at_cut(&[], 10_000).is_empty());
    }

    #[test]
    fn a_second_unresolved_delivery_fails() {
        let new = [
            (
                listed("1", "outcome_unknown"),
                attempt(9_500, "connection_lost_after_send"),
            ),
            (
                listed("2", "outcome_unknown"),
                attempt(9_600, "connection_lost_after_send"),
            ),
        ];
        let problems = judge_at_cut(&new, 10_000);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("one attempt in flight at most"));
    }

    #[test]
    fn a_queued_delivery_that_changed_state_fails() {
        for state in ["failed", "timed_out", "rejected"] {
            let new = [(listed("1", state), attempt(9_500, "write_failed"))];
            let problems = judge_at_cut(&new, 10_000);
            assert!(
                problems.iter().any(|p| p.contains("changed state")),
                "{state}: {problems:?}"
            );
        }
    }

    #[test]
    fn an_attempt_that_began_after_the_cut_was_not_in_flight() {
        let new = [(
            listed("1", "outcome_unknown"),
            attempt(20_000, "connection_lost_after_send"),
        )];
        assert!(judge_at_cut(&new, 10_000)[0].contains("started after"));
        // The clock slack is allowed.
        let close = [(
            listed("1", "outcome_unknown"),
            attempt(11_000, "connection_lost_after_send"),
        )];
        assert!(judge_at_cut(&close, 10_000).is_empty());
    }

    #[test]
    fn the_attempt_result_must_be_a_lost_connection() {
        let new = [(
            listed("1", "outcome_unknown"),
            attempt(9_900, "ack_timeout"),
        )];
        assert!(judge_at_cut(&new, 10_000)[0].contains("connection_lost_after_send"));
    }

    #[test]
    fn afterwards_the_delivery_stays_unknown_or_is_resolved() {
        let tolerated = [listed("1", "outcome_unknown")];
        assert!(judge_after(&tolerated, &[listed("1", "outcome_unknown")]).is_empty());
        assert!(
            judge_after(&tolerated, &[]).is_empty(),
            "resolved or resent"
        );
        let problems = judge_after(&tolerated, &[listed("1", "failed")]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("`failed`"));
    }

    #[test]
    fn only_new_deliveries_are_counted() {
        let before = [listed("1", "outcome_unknown")];
        let now = [
            listed("1", "outcome_unknown"),
            listed("2", "outcome_unknown"),
        ];
        assert_eq!(
            new_since(&before, &now),
            vec![listed("2", "outcome_unknown")]
        );
    }

    #[test]
    fn a_message_row_yields_its_unresolved_deliveries() {
        let row = json!({ "message_id": "m1", "control_id": "H01-1", "deliveries": [
            { "delivery_id": "d1", "state": "acknowledged" },
            { "delivery_id": "d2", "state": "outcome_unknown" } ] });
        assert_eq!(
            listed_from(&row),
            vec![Listed {
                message_id: "m1".into(),
                delivery_id: "d2".into(),
                control_id: "H01-1".into(),
                state: "outcome_unknown".into()
            }]
        );
    }

    #[test]
    fn the_last_attempt_is_read_from_the_message_record() {
        let record = json!({ "deliveries": [{ "delivery_id": "d1", "attempt_log": [
            { "attempt_no": 1, "started_at": "2026-11-07T05:39:18.580414000Z", "result": "write_failed" },
            { "attempt_no": 2, "started_at": "2026-11-07T05:39:20.000000000Z", "result": "connection_lost_after_send" } ] }] });
        let a = attempt_of(&record, "d1");
        assert_eq!(a.result, "connection_lost_after_send");
        assert_eq!(a.started_ms, parse_utc_ms("2026-11-07T05:39:20Z"));
        assert_eq!(attempt_of(&record, "other"), Attempt::default());
    }
}
