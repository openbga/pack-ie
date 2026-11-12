//! What a notification says. Built only from the alert's catalog fields, never from its
//! `details` and never from message content, so a notification cannot carry patient data:
//! every field below is an operator-chosen name, a catalog text, a time or a count.

use packie_diagnostics::{Alert, AlertState};
use serde_json::{Value, json};

/// The PHI-free content of one notification.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Content {
    /// The node's display name.
    pub(crate) node: String,
    /// Alert code (`endpoint_down`, …) or `test`.
    pub(crate) code: String,
    /// `info` (test only), `warning` or `critical`.
    pub(crate) severity: &'static str,
    /// `raised`, `cleared` or `test`.
    pub(crate) state: &'static str,
    /// The channel the alert concerns.
    pub(crate) channel_id: Option<String>,
    /// The channel's operator-chosen label.
    pub(crate) channel_label: Option<String>,
    /// The endpoint the alert concerns.
    pub(crate) endpoint_id: Option<String>,
    /// The endpoint's operator-chosen label.
    pub(crate) endpoint_label: Option<String>,
    /// The destination the alert concerns.
    pub(crate) destination_id: Option<String>,
    /// The catalog's one-sentence summary.
    pub(crate) summary: String,
    /// The catalog's next action.
    pub(crate) next_action: String,
    /// When the alert was first raised; absent for a test message.
    pub(crate) first_seen_at: Option<String>,
    /// When the alert was last seen; absent for a test message.
    pub(crate) last_seen_at: Option<String>,
    /// How often the alert occurred.
    pub(crate) occurrences: u32,
}

/// `text` with control characters turned into spaces, so it stays on one line.
pub(crate) fn blank_controls(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

impl Content {
    /// The content of `alert` as raised on `node`; labels come from [`Content::labelled`].
    pub(crate) fn from_alert(node: &str, alert: &Alert) -> Self {
        let code = serde_json::to_value(alert.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{:?}", alert.kind));
        Self {
            node: node.to_owned(),
            code,
            severity: alert.severity.as_str(),
            state: match alert.state {
                AlertState::Active => "raised",
                AlertState::Cleared => "cleared",
            },
            channel_id: alert.channel_id.map(|id| id.to_string()),
            endpoint_id: alert.endpoint_id.map(|id| id.to_string()),
            destination_id: alert.destination_id.map(|id| id.to_string()),
            summary: alert.summary.clone(),
            next_action: alert.next_action.clone(),
            first_seen_at: Some(alert.first_seen.to_string()),
            last_seen_at: Some(alert.last_seen.to_string()),
            occurrences: alert.occurrences,
            ..Self::default()
        }
    }

    /// The content of the message sent by the settings' test action.
    pub(crate) fn test(node: &str) -> Self {
        Self {
            node: node.to_owned(),
            code: "test".to_owned(),
            severity: "info",
            state: "test",
            summary: "Test message from the notification settings; no alert was raised.".to_owned(),
            next_action: "None. Receiving this message means the target works.".to_owned(),
            ..Self::default()
        }
    }

    /// Adds the operator-chosen names of the alert's channel and endpoint.
    pub(crate) fn labelled(mut self, labels: &crate::Labels) -> Self {
        self.channel_label.clone_from(&labels.channel);
        self.endpoint_label.clone_from(&labels.endpoint);
        self
    }

    /// The channel, else the endpoint, else the destination the alert concerns.
    fn scope(&self) -> Option<&str> {
        self.channel_id
            .as_deref()
            .or(self.endpoint_id.as_deref())
            .or(self.destination_id.as_deref())
    }

    /// `[PACK-IE <node>] <SEVERITY> <code> raised|cleared: <scope>`; control characters in the
    /// node name become spaces so the subject stays one header line.
    pub(crate) fn subject(&self) -> String {
        let mut subject = format!(
            "[PACK-IE {}] {} {} {}",
            blank_controls(&self.node),
            self.severity.to_ascii_uppercase(),
            self.code,
            self.state
        );
        if let Some(scope) = self.scope() {
            subject.push_str(": ");
            subject.push_str(scope);
        }
        subject
    }

    /// The plain-text mail body.
    pub(crate) fn text(&self) -> String {
        let mut lines = vec![
            format!("Node: {}", self.node),
            format!("Alert: {} ({}, {})", self.code, self.severity, self.state),
        ];
        let scopes = [
            ("Channel", &self.channel_id, &self.channel_label),
            ("Endpoint", &self.endpoint_id, &self.endpoint_label),
            ("Destination", &self.destination_id, &None),
        ];
        for (kind, id, label) in scopes {
            match (id, label) {
                (Some(id), Some(label)) => lines.push(format!("{kind}: {label} ({id})")),
                (Some(id), None) => lines.push(format!("{kind}: {id}")),
                _ => {}
            }
        }
        lines.push(format!("Summary: {}", self.summary));
        lines.push(format!("Next action: {}", self.next_action));
        if let (Some(first), Some(last)) = (&self.first_seen_at, &self.last_seen_at) {
            lines.push(format!("First seen: {first}"));
            lines.push(format!("Last seen: {last}"));
            lines.push(format!("Occurrences: {}", self.occurrences));
        }
        lines.push(String::new());
        lines.push("Sent by Positive ACK. Open the console to investigate.".to_owned());
        lines.join("\n")
    }

    /// The webhook body.
    pub(crate) fn json(&self) -> Value {
        json!({
            "node": self.node,
            "code": self.code,
            "severity": self.severity,
            "state": self.state,
            "channel_id": self.channel_id,
            "channel_label": self.channel_label,
            "endpoint_id": self.endpoint_id,
            "endpoint_label": self.endpoint_label,
            "destination_id": self.destination_id,
            "summary": self.summary,
            "next_action": self.next_action,
            "first_seen_at": self.first_seen_at,
            "last_seen_at": self.last_seen_at,
            "occurrences": self.occurrences,
        })
    }
}
