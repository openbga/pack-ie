//! Local HTTP and SMTP listeners, and the settings that point at them.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

/// An HTTP listener that records each request (head and body) and answers with `status`.
pub(super) async fn http_listener(
    status: Arc<AtomicU16>,
) -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    http_listener_with_body(status, "").await
}

/// Like [`http_listener`], but every answer carries `body`.
pub(super) async fn http_listener_with_body(
    status: Arc<AtomicU16>,
    body: &'static str,
) -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let (tx, code) = (tx.clone(), status.load(Ordering::SeqCst));
            tokio::spawn(async move {
                let request = read_http_request(&mut stream).await;
                let reply = format!(
                    "HTTP/1.1 {code} {body}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = tx.send(request);
            });
        }
    });
    (addr, rx)
}

async fn read_http_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0_u8; 1024];
    loop {
        let n = stream.read(&mut buf).await.unwrap_or(0);
        data.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&data).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let length = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                })
                .unwrap_or(0);
            if body.len() >= length || n == 0 {
                return text;
            }
        } else if n == 0 {
            return text;
        }
    }
}

/// Settings with a webhook at `addr`, authenticated by the secret `bearer` when given.
pub(super) fn webhook_settings(addr: SocketAddr, bearer: Option<&str>) -> Value {
    let mut webhook = json!({"url": format!("http://{addr}/hooks/packie"), "timeout_seconds": 5});
    if let Some(name) = bearer {
        webhook["bearer"] = json!({"secret": name});
    }
    json!({"webhook": webhook})
}

/// The next recorded request or message, or a panic after ten seconds.
pub(super) async fn next(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap()
}

/// A command prefix (`RCPT`, `AUTH`, ...) and the complete negative reply sent to it.
pub(super) type Refusal = Option<(&'static str, &'static str)>;

/// Speaks just enough SMTP (it advertises `AUTH PLAIN`) to accept one message per
/// connection, or, with a `refusal`, to answer one command with a negative reply; sends
/// each received DATA section to the receiver.
pub(super) async fn smtp_listener(
    refusal: Refusal,
) -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(smtp_session(stream, refusal, tx.clone()));
        }
    });
    (addr, rx)
}

/// The reply to one SMTP command outside DATA.
fn command_reply(command: &str, refusal: Refusal) -> &'static [u8] {
    if let Some((_, reply)) = refusal.filter(|(prefix, _)| command.starts_with(prefix)) {
        return reply.as_bytes();
    }
    match command.split_whitespace().next() {
        Some("EHLO") => b"250-test\r\n250 AUTH PLAIN\r\n",
        Some("AUTH") => b"235 authenticated\r\n",
        Some("DATA") => b"354 go ahead\r\n",
        _ => b"250 ok\r\n",
    }
}

async fn smtp_session(stream: TcpStream, refusal: Refusal, tx: mpsc::UnboundedSender<String>) {
    let (read, mut write) = stream.into_split();
    let mut lines = BufReader::new(read);
    let _ = write.write_all(b"220 test ESMTP\r\n").await;
    let (mut data, mut in_data) = (String::new(), false);
    let mut line = String::new();
    loop {
        line.clear();
        if lines.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }
        let reply: &[u8] = if in_data {
            if line == ".\r\n" {
                in_data = false;
                let _ = tx.send(std::mem::take(&mut data));
                b"250 queued\r\n"
            } else {
                data.push_str(&line);
                continue;
            }
        } else {
            let command = line.to_ascii_uppercase();
            if command.starts_with("DATA") {
                in_data = true;
            } else if command.starts_with("QUIT") {
                let _ = write.write_all(b"221 bye\r\n").await;
                return;
            }
            command_reply(&command, refusal)
        };
        let _ = write.write_all(reply).await;
    }
}

/// Settings with plain e-mail to the SMTP listener at `addr`.
pub(super) fn mail_settings(addr: SocketAddr) -> Value {
    json!({"email": {"host": "127.0.0.1", "port": addr.port(), "security": "none",
        "from": "packie@lab.example", "to": ["oncall@lab.example"]}})
}

/// Plain e-mail to the SMTP listener at `addr`, authenticating with the secret `smtp-password`.
pub(super) fn auth_mail_settings(addr: SocketAddr) -> Value {
    let mut settings = mail_settings(addr);
    settings["email"]["username"] = json!("packie");
    settings["email"]["password"] = json!({"secret": "smtp-password"});
    settings
}
