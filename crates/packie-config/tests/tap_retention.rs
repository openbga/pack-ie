//! `tap.retention` of bridge channels (`docs/spec/configuration.md` §6.2): defaults,
//! ranges and the unbounded warning.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::path::Path;

use packie_config::{ChannelConfig, ProblemCode, RuntimeConfig, TapRetention, load_runtime_file};
use serde_json::{Value, json};

/// The repository's sample document; channel 0 is a bridge with an ASTM tap.
fn sample() -> Value {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../config/runtime.json"
    ));
    serde_json::to_value(load_runtime_file(path).unwrap()).unwrap()
}

fn tap_retention(config: &RuntimeConfig) -> TapRetention {
    match &config.channels[0] {
        ChannelConfig::Bridge(bridge) => bridge.tap.retention.clone(),
        ChannelConfig::Message(_) => panic!("channel 0 is a bridge"),
    }
}

/// The document with channel 0's tap set to `tap`.
fn with_tap(tap: Value) -> Value {
    let mut value = sample();
    value["channels"][0]["tap"] = tap;
    value
}

#[test]
fn a_tap_keeps_thirty_days_by_default_and_with_a_partial_retention() {
    let config = RuntimeConfig::from_value(with_tap(json!({"framer": "astm"}))).unwrap();
    assert_eq!(tap_retention(&config), TapRetention::default());
    assert_eq!(tap_retention(&config).max_age_days, Some(30));
    assert_eq!(tap_retention(&config).max_messages, None);
    let partial = with_tap(json!({"framer": "astm", "retention": {"max_messages": 5000}}));
    let config = RuntimeConfig::from_value(partial).unwrap();
    assert_eq!(tap_retention(&config).max_age_days, Some(30));
    assert_eq!(tap_retention(&config).max_messages, Some(5000));
}

#[test]
fn tap_retention_limits_are_checked() {
    let value = with_tap(json!({
        "framer": "astm",
        "retention": {"max_age_days": 0, "max_messages": 10}
    }));
    let errors = RuntimeConfig::from_value(value).unwrap_err();
    let paths: Vec<&str> = errors.problems().iter().map(|p| p.path.as_str()).collect();
    assert!(
        paths.contains(&"channels[0].tap.retention.max_age_days"),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"channels[0].tap.retention.max_messages"),
        "{paths:?}"
    );
    let unknown = with_tap(json!({"framer": "astm", "retention": {"days": 3}}));
    assert!(RuntimeConfig::from_value(unknown).is_err());
}

#[test]
fn an_unbounded_retention_warns_only_when_the_tap_records() {
    let unbounded = json!({"max_age_days": null, "max_messages": null});
    let warnings = |tap: Value| {
        let text = serde_json::to_string(&with_tap(tap)).unwrap();
        let (_, warnings) = RuntimeConfig::parse_with_warnings(&text).unwrap();
        warnings
            .into_iter()
            .filter(|w| w.code == ProblemCode::RetentionUnbounded)
            .map(|w| w.path)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        warnings(json!({"framer": "astm", "record_messages": true, "retention": unbounded})),
        ["channels[0].tap.retention"]
    );
    assert!(warnings(json!({"framer": "astm", "retention": unbounded})).is_empty());
}
