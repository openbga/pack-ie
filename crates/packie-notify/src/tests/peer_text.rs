//! A relay or receiver cannot put its own text into failure texts, logs or alerts: only a
//! fixed category and the numeric status appear.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicU16;

use serde_json::Value;
use tokio::net::TcpListener;

use super::net::{
    Refusal, auth_mail_settings, http_listener_with_body, mail_settings, smtp_listener,
    webhook_settings,
};
use super::*;

const SENTINEL: &str = "SENTINEL-SECRET-123";

/// Runs a test message against `settings` and returns the one failure text, after checking
/// that the sentinel is in none of the places a failure shows up.
async fn failure_text(settings: &Value) -> String {
    let notifier = notifier(settings);
    let results = notifier.test().await;
    assert_eq!(results.len(), 1, "{results:?}");
    assert!(!results[0].ok, "{results:?}");
    let failing = notifier.failing();
    assert_eq!(failing.len(), 1);
    assert_eq!(failing[0].error, results[0].detail);
    for text in [&results[0].detail, &failing[0].error] {
        assert!(!text.contains(SENTINEL), "{text}");
    }
    // The caller names the target (`target: error`); the text must not repeat it.
    let detail = &results[0].detail;
    assert!(
        !detail.starts_with("webhook:") && !detail.starts_with("email:"),
        "{detail}"
    );
    detail.clone()
}

async fn smtp_failure(refusal: Refusal, authenticate: bool) -> String {
    let (addr, _messages) = smtp_listener(refusal).await;
    let settings = if authenticate {
        auth_mail_settings(addr)
    } else {
        mail_settings(addr)
    };
    failure_text(&settings).await
}

#[tokio::test]
async fn a_refused_login_names_the_code_and_not_the_relays_text() {
    let reply = "535 5.7.8 SENTINEL-SECRET-123 bad credentials\r\n";
    let text = smtp_failure(Some(("AUTH", reply)), true).await;
    assert_eq!(text, "SMTP relay refused the credentials (535)");
}

#[tokio::test]
async fn a_refused_sender_names_the_code_and_not_the_relays_text() {
    let reply = "554 SENTINEL-SECRET-123 rejected\r\n";
    let text = smtp_failure(Some(("MAIL", reply)), false).await;
    assert_eq!(text, "SMTP relay refused the message (554)");
}

#[tokio::test]
async fn a_deferral_names_the_code_and_not_the_relays_text() {
    let reply = "451 4.3.0 SENTINEL-SECRET-123 try later\r\n";
    let text = smtp_failure(Some(("MAIL", reply)), false).await;
    assert_eq!(text, "SMTP relay deferred the message (451)");
}

#[tokio::test]
async fn an_unreadable_greeting_is_a_fixed_text() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        while let Ok((mut stream, _)) = listener.accept().await {
            let _ = stream.write_all(b"SENTINEL-SECRET-123\r\n").await;
        }
    });
    let text = failure_text(&mail_settings(addr)).await;
    assert_eq!(text, "the SMTP relay sent a reply that could not be read");
}

#[tokio::test]
async fn an_unreachable_relay_is_a_fixed_text() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener);
    assert_eq!(
        failure_text(&mail_settings(addr)).await,
        "cannot connect to the SMTP relay"
    );
}

#[tokio::test]
async fn a_webhook_error_body_never_reaches_the_failure_text() {
    let status = Arc::new(AtomicU16::new(500));
    let (addr, _requests) = http_listener_with_body(status, SENTINEL).await;
    assert_eq!(
        failure_text(&webhook_settings(addr, None)).await,
        "the receiver answered HTTP 500"
    );
}

#[tokio::test]
async fn a_webhook_redirect_is_a_status_failure() {
    let status = Arc::new(AtomicU16::new(302));
    let (addr, _requests) = http_listener_with_body(status, SENTINEL).await;
    let text = failure_text(&webhook_settings(addr, None)).await;
    assert_eq!(text, "the receiver answered HTTP 302");
}

#[tokio::test]
async fn garbage_instead_of_http_is_a_fixed_text() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(answer_with_garbage(stream));
        }
    });
    let text = failure_text(&webhook_settings(addr, None)).await;
    assert_eq!(
        text,
        "the webhook receiver sent a reply that could not be read"
    );
}

/// Reads the whole request, answers with a line that is not HTTP and keeps the connection
/// until the client closes it: closing with the request unread would reset the connection
/// while the client still writes (a network error, not a reply).
async fn answer_with_garbage(mut stream: tokio::net::TcpStream) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    while !complete_request(&request) {
        match stream.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&buffer[..n]),
        }
    }
    let _ = stream.write_all(b"SENTINEL-SECRET-123\r\n\r\n").await;
    while matches!(stream.read(&mut buffer).await, Ok(n) if n > 0) {}
}

/// Whether `request` holds the headers and the `Content-Length` body of an HTTP request.
fn complete_request(request: &[u8]) -> bool {
    let text = String::from_utf8_lossy(request);
    let Some(end) = text.find("\r\n\r\n") else {
        return false;
    };
    let length = text[..end]
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    request.len() >= end + 4 + length
}
