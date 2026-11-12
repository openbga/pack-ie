//! Severity routing and the per-alert rate limit.

use std::time::{Duration, Instant};

use super::*;
use crate::limit::{AlertKey, RateLimiter};
use crate::{Labels, Target};

#[test]
fn the_limiter_allows_one_message_per_alert_and_window() {
    let start = Instant::now();
    let minutes = |m: u64| start + Duration::from_mins(m);
    let mut limiter = RateLimiter::new(Duration::from_mins(15));
    let key = |channel: &str| AlertKey::of(&raised(Severity::Critical, channel));
    assert!(limiter.allow(key("a"), minutes(0)));
    assert!(!limiter.allow(key("a"), minutes(1)));
    assert!(!limiter.allow(key("a"), minutes(14)));
    assert!(
        limiter.allow(key("b"), minutes(1)),
        "another scope has its own window"
    );
    assert!(limiter.allow(key("a"), minutes(15)), "the window is over");
    assert!(!limiter.allow(key("a"), minutes(20)), "and starts again");
}

#[test]
fn raised_and_cleared_share_one_window() {
    let notifier = notifier(&routing());
    let start = Instant::now();
    let raise = raised(Severity::Critical, "chan-a");
    let mut clear = raise.clone();
    clear.state = AlertState::Cleared;
    let plan = |alert: &Alert, minutes: u64| {
        notifier
            .inner
            .plan(alert, start + Duration::from_mins(minutes))
    };
    assert_eq!(plan(&raise, 0), [Target::Email, Target::Webhook]);
    assert!(plan(&clear, 5).is_empty());
    assert_eq!(plan(&clear, 16), [Target::Email, Target::Webhook]);
    let other_code = alert(
        AlertKind::EndpointDown,
        Severity::Critical,
        AlertState::Active,
        "chan-a",
    );
    assert!(
        !plan(&other_code, 17).is_empty(),
        "another code is another alert"
    );
}

#[test]
fn severities_route_to_their_targets() {
    let notifier = notifier(&routing());
    let now = Instant::now();
    let plan = |severity, channel| notifier.inner.plan(&raised(severity, channel), now);
    assert_eq!(plan(Severity::Warning, "a"), [Target::Email]);
    assert_eq!(
        plan(Severity::Critical, "b"),
        [Target::Email, Target::Webhook]
    );
    assert!(plan(Severity::Info, "c").is_empty());
}

#[test]
fn an_event_no_target_wants_does_not_use_up_the_window() {
    let notifier = notifier(&json!({
        "webhook": {"url": "https://chat.lab.example/x", "severities": ["critical"]}
    }));
    let now = Instant::now();
    let plan = |severity| notifier.inner.plan(&raised(severity, "a"), now);
    assert!(plan(Severity::Warning).is_empty());
    assert_eq!(plan(Severity::Critical), [Target::Webhook]);
}

#[test]
fn without_targets_nothing_happens() {
    let notifier = notifier(&json!({}));
    let alert = raised(Severity::Critical, "a");
    assert!(notifier.inner.plan(&alert, Instant::now()).is_empty());
    notifier.alert_changed(&alert, &Labels::default());
    assert!(notifier.failing().is_empty());
}
