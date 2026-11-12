//! Routing, rate limiting, content, and delivery against local listeners (no real servers).

use std::collections::HashMap;
use std::sync::Arc;

use packie_config::{MapSecrets, NotificationSettings};
use packie_core::{AlertId, ChannelId, Severity, Timestamp};
use packie_diagnostics::{Alert, AlertKind, AlertState};
use serde_json::{Value, json};

use crate::Notifier;

mod ca_file;
mod content;
mod delivery;
mod net;
mod peer_text;
mod routing;

fn alert(kind: AlertKind, severity: Severity, state: AlertState, channel: &str) -> Alert {
    Alert {
        id: AlertId::generate(),
        kind,
        severity,
        state,
        channel_id: ChannelId::new(channel).ok(),
        endpoint_id: None,
        destination_id: None,
        summary: "The channel has no peer for 5 minutes.".to_owned(),
        next_action: "Check the analyzer connection.".to_owned(),
        first_seen: Timestamp::now(),
        last_seen: Timestamp::now(),
        cleared_at: None,
        occurrences: 2,
        details: json!({"peer_address": "10.0.0.5", "count": 3}),
    }
}

fn raised(severity: Severity, channel: &str) -> Alert {
    alert(
        AlertKind::NoPeerConnected,
        severity,
        AlertState::Active,
        channel,
    )
}

fn settings(value: &Value) -> NotificationSettings {
    serde_json::from_value(value.clone()).unwrap()
}

fn notifier(value: &Value) -> Notifier {
    let secrets = MapSecrets::new(HashMap::from([
        ("webhook-token".to_owned(), "tok-123".to_owned()),
        ("smtp-password".to_owned(), "pw-456".to_owned()),
    ]));
    Notifier::new(settings(value), Arc::new(secrets), "Lab One".to_owned())
}

/// E-mail for warnings and criticals, webhook for criticals only.
fn routing() -> Value {
    json!({
        "email": {"host": "smtp.lab.example", "port": 587, "from": "packie@lab.example",
                  "to": ["oncall@lab.example"], "severities": ["warning", "critical"]},
        "webhook": {"url": "https://chat.lab.example/hooks/packie", "severities": ["critical"]}
    })
}
