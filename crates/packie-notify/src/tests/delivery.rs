//! Delivery to local webhook and SMTP listeners, and failure tracking.

use std::sync::atomic::{AtomicU16, Ordering};

use tokio::net::TcpListener;

use super::net::{http_listener, mail_settings, next, smtp_listener, webhook_settings};
use super::*;
use crate::{EMAIL, Labels, WEBHOOK};

#[tokio::test]
async fn the_webhook_posts_json_with_the_bearer_token() {
    let (addr, mut requests) = http_listener(Arc::new(AtomicU16::new(200))).await;
    let notifier = notifier(&webhook_settings(addr, Some("webhook-token")));
    let results = notifier.test().await;
    assert_eq!(results.len(), 1);
    assert_eq!(
        (results[0].target, results[0].ok),
        (WEBHOOK, true),
        "{results:?}"
    );
    let request = next(&mut requests).await;
    let (head, body) = request.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("POST /hooks/packie HTTP/1.1"), "{head}");
    let head = head.to_ascii_lowercase();
    assert!(head.contains("content-type: application/json"), "{head}");
    assert!(head.contains("authorization: bearer tok-123"), "{head}");
    let body: Value = serde_json::from_str(body).unwrap();
    assert_eq!(body["node"], "Lab One");
    assert_eq!(body["code"], "test");
    assert!(notifier.failing().is_empty());
}

#[tokio::test]
async fn no_bearer_header_without_a_bearer_secret() {
    let (addr, mut requests) = http_listener(Arc::new(AtomicU16::new(200))).await;
    assert!(notifier(&webhook_settings(addr, None)).test().await[0].ok);
    assert!(
        !next(&mut requests)
            .await
            .to_ascii_lowercase()
            .contains("authorization")
    );
}

#[tokio::test]
async fn a_failing_target_is_reported_until_a_send_succeeds() {
    let status = Arc::new(AtomicU16::new(500));
    let (addr, _requests) = http_listener(Arc::clone(&status)).await;
    let notifier = notifier(&webhook_settings(addr, None));
    let results = notifier.test().await;
    assert!(!results[0].ok);
    assert_eq!(results[0].detail, "the receiver answered HTTP 500");
    let first = notifier.failing();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].target, WEBHOOK);
    assert_eq!(first[0].error, results[0].detail);
    let _ = notifier.test().await;
    assert_eq!(
        notifier.failing()[0].since,
        first[0].since,
        "since the first failure"
    );
    status.store(200, Ordering::SeqCst);
    assert!(notifier.test().await[0].ok);
    assert!(notifier.failing().is_empty());
}

#[tokio::test]
async fn a_missing_secret_fails_the_target_by_name_only() {
    let (addr, _requests) = http_listener(Arc::new(AtomicU16::new(200))).await;
    let mut value = webhook_settings(addr, Some("absent-token"));
    value["rate_limit_minutes"] = json!(1);
    let results = notifier(&value).test().await;
    assert!(!results[0].ok);
    assert_eq!(results[0].detail, "secret absent-token is not set");
}

#[tokio::test]
async fn an_unreachable_receiver_is_a_failure_not_a_panic() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let notifier = notifier(&webhook_settings(addr, None));
    let results = notifier.test().await;
    assert!(!results[0].ok);
    assert_eq!(results[0].detail, "cannot connect to the webhook receiver");
    assert_eq!(notifier.failing().len(), 1);
}

#[tokio::test]
async fn alert_changes_send_in_the_background_and_are_rate_limited() {
    let (addr, mut requests) = http_listener(Arc::new(AtomicU16::new(200))).await;
    let notifier = notifier(&webhook_settings(addr, None));
    notifier.alert_changed(&raised(Severity::Critical, "chan-a"), &Labels::default());
    let request = next(&mut requests).await;
    let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["channel_id"], "chan-a");
    assert_eq!(body["state"], "raised");
    assert!(!request.contains("10.0.0.5"));
    // The same alert again is dropped; the next message to arrive is the other alert's.
    notifier.alert_changed(&raised(Severity::Critical, "chan-a"), &Labels::default());
    notifier.alert_changed(&raised(Severity::Critical, "chan-b"), &Labels::default());
    let request = next(&mut requests).await;
    assert!(request.contains("\"channel_id\":\"chan-b\""), "{request}");
}

#[tokio::test]
async fn mail_goes_through_smtp_to_every_recipient_address() {
    let (addr, mut messages) = smtp_listener(None).await;
    let notifier = notifier(&mail_settings(addr));
    notifier.alert_changed(&raised(Severity::Critical, "chan-a"), &Labels::default());
    let message = next(&mut messages).await;
    assert!(
        message.contains("Subject: [PACK-IE Lab One] CRITICAL no_peer_connected raised: chan-a"),
        "{message}"
    );
    assert!(message.contains("Channel: chan-a"));
    assert!(!message.contains("10.0.0.5"));
    let results = notifier.test().await;
    assert_eq!(
        (results[0].target, results[0].ok),
        (EMAIL, true),
        "{results:?}"
    );
    assert!(
        next(&mut messages)
            .await
            .contains("Subject: [PACK-IE Lab One] INFO test test")
    );
}

#[tokio::test]
async fn a_refused_recipient_marks_email_failing() {
    let (addr, _messages) = smtp_listener(Some(("RCPT", "550 no such mailbox\r\n"))).await;
    let notifier = notifier(&mail_settings(addr));
    let results = notifier.test().await;
    assert!(!results[0].ok);
    assert_eq!(results[0].detail, "SMTP relay refused the message (550)");
    assert_eq!(notifier.failing()[0].target, EMAIL);
}
