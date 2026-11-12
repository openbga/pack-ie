//! `notifications.ca_file` seen from the notifier: which targets it affects.

use std::path::Path;
use std::sync::atomic::AtomicU16;

use super::net::{http_listener, next, webhook_settings};
use super::*;

fn with_ca(mut value: Value, path: &Path) -> Value {
    value["ca_file"] = json!(path);
    value
}

#[tokio::test]
async fn an_unusable_ca_file_fails_every_tls_send_naming_the_file() {
    let missing = std::env::temp_dir().join("packie-notify-ca-absent.pem");
    let notifier = notifier(&with_ca(routing(), &missing));
    let results = notifier.test().await;
    assert_eq!(results.len(), 2);
    for result in &results {
        assert!(!result.ok, "{}", result.target);
        assert!(
            result
                .detail
                .starts_with(&format!("CA file {} cannot be read", missing.display())),
            "{}",
            result.detail
        );
    }
    let failing = notifier.failing();
    assert_eq!(failing.len(), 2);
    assert!(
        failing
            .iter()
            .all(|f| f.error.contains("packie-notify-ca-absent.pem"))
    );
}

#[tokio::test]
async fn an_unusable_ca_file_does_not_touch_targets_without_tls() {
    let missing = std::env::temp_dir().join("packie-notify-ca-absent.pem");
    let (addr, mut requests) = http_listener(Arc::new(AtomicU16::new(200))).await;
    let notifier = notifier(&with_ca(webhook_settings(addr, None), &missing));
    assert!(notifier.test().await[0].ok);
    assert!(next(&mut requests).await.starts_with("POST /hooks/packie"));
}
