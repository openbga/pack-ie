//! What a message says: the PHI-free field set, the subject, the mail and its transport.

use std::collections::BTreeSet;

use packie_core::{ConnectorId, EndpointId};

use super::*;
use crate::Labels;
use crate::content::Content;
use crate::email;

#[test]
fn the_message_holds_only_the_phi_free_field_set() {
    let mut alert = raised(Severity::Critical, "chan-a");
    alert.endpoint_id = EndpointId::new("ep-1").ok();
    alert.destination_id = ConnectorId::new("lis-out").ok();
    let labels = Labels {
        channel: Some("Hematology to LIS".into()),
        endpoint: Some("Analyzer port".into()),
    };
    let content = Content::from_alert("Lab One", &alert).labelled(&labels);
    let json = content.json();
    let keys: BTreeSet<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "node",
            "code",
            "severity",
            "state",
            "channel_id",
            "channel_label",
            "endpoint_id",
            "endpoint_label",
            "destination_id",
            "summary",
            "next_action",
            "first_seen_at",
            "last_seen_at",
            "occurrences"
        ])
    );
    assert_eq!(json["code"], "no_peer_connected");
    assert_eq!(json["severity"], "critical");
    assert_eq!(json["state"], "raised");
    assert_eq!(json["channel_id"], "chan-a");
    assert_eq!(json["endpoint_label"], "Analyzer port");
    assert_eq!(json["occurrences"], 2);
    assert!(
        content
            .text()
            .contains("Channel: Hematology to LIS (chan-a)")
    );
    for text in [json.to_string(), content.text(), content.subject()] {
        assert!(!text.contains("10.0.0.5"), "{text}");
        assert!(!text.contains("peer_address"), "{text}");
        assert!(!text.to_lowercase().contains("details"), "{text}");
    }
}

#[test]
fn the_subject_names_node_severity_code_state_and_scope() {
    let in_channel = raised(Severity::Warning, "chan-a");
    let at_endpoint = Alert {
        state: AlertState::Cleared,
        channel_id: None,
        endpoint_id: EndpointId::new("ep-1").ok(),
        ..in_channel.clone()
    };
    let unscoped = Alert {
        endpoint_id: None,
        ..at_endpoint.clone()
    };
    for (node, alert, subject) in [
        (
            "Lab One",
            &in_channel,
            "[PACK-IE Lab One] WARNING no_peer_connected raised: chan-a",
        ),
        (
            "Lab One",
            &at_endpoint,
            "[PACK-IE Lab One] WARNING no_peer_connected cleared: ep-1",
        ),
        (
            "Lab\r\nBcc: x",
            &unscoped,
            "[PACK-IE Lab  Bcc: x] WARNING no_peer_connected cleared",
        ),
    ] {
        assert_eq!(Content::from_alert(node, alert).subject(), subject);
    }
}

#[test]
fn the_mail_is_plain_text_from_the_settings() {
    let settings = settings(&routing()).email.unwrap();
    let content = Content::from_alert("Lab One", &raised(Severity::Critical, "chan-a"));
    let message = email::build_message(&settings, &content).unwrap();
    let text = String::from_utf8(message.formatted()).unwrap();
    assert!(text.contains("Subject: [PACK-IE Lab One] CRITICAL no_peer_connected raised: chan-a"));
    assert!(text.contains("From: packie@lab.example"));
    assert!(text.contains("To: oncall@lab.example"));
    assert!(text.contains("Content-Type: text/plain"));
    assert!(text.contains("Next action: Check the analyzer connection."));
    assert!(!text.contains("10.0.0.5"));
    let mut bad = settings;
    bad.to = vec!["not an address".to_owned()];
    assert!(email::build_message(&bad, &content).is_err());
}

#[tokio::test]
async fn transports_build_for_every_security_mode() {
    for (security, host) in [
        ("starttls", "smtp.lab.example"),
        ("tls", "smtp.lab.example"),
        ("none", "localhost"),
    ] {
        let value = json!({"email": {"host": host, "port": 25, "security": security,
            "from": "a@b.example", "to": ["c@d.example"]}});
        let email = settings(&value).email.unwrap();
        assert!(email::transport(&email, None, None).is_ok(), "{security}");
    }
}
