//! The operator commands against a live engine: `packie run` with one HL7 message channel
//! (a listener source, a destination the test provides), synthetic messages, then
//! `status`, `messages` and `logs` as an operator would use them. All data is synthetic.
#![cfg(test)]

mod common;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use common::{WAIT, eventually};

/// An MLLP frame around `body`.
fn frame(body: &str) -> Vec<u8> {
    let mut bytes = vec![0x0B];
    bytes.extend_from_slice(body.as_bytes());
    bytes.extend_from_slice(&[0x1C, 0x0D]);
    bytes
}

/// A synthetic result message.
fn oru(control_id: &str) -> Vec<u8> {
    oru_valued(control_id, "5.4")
}

/// The same message with another result value: a correction (a revision of the first).
fn oru_valued(control_id: &str, value: &str) -> Vec<u8> {
    frame(&format!(
        "MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|{control_id}|P|2.5.1\r\
         PID|1||MRN-00000||TEST^PATIENT\rOBR|1||SID-00000\rOBX|1|NM|GLU||{value}|mmol/L\r"
    ))
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The destination system: acknowledges every message `AA`, except control ids starting
/// with `REJ`, which it rejects (`AR`) until `accept_all` is set.
struct Destination {
    port: u16,
    received: Arc<Mutex<Vec<String>>>,
    accept_all: Arc<AtomicBool>,
}

impl Destination {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let this = Self {
            port,
            received: Arc::default(),
            accept_all: Arc::default(),
        };
        let (received, accept_all) = (this.received.clone(), this.accept_all.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (received, accept_all) = (received.clone(), accept_all.clone());
                std::thread::spawn(move || serve(stream, &received, &accept_all));
            }
        });
        this
    }

    fn received(&self) -> usize {
        self.received.lock().unwrap().len()
    }
}

fn serve(mut stream: TcpStream, received: &Mutex<Vec<String>>, accept_all: &AtomicBool) {
    let mut buffered = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => buffered.extend_from_slice(&chunk[..n]),
        }
        while let Some(end) = buffered.iter().position(|b| *b == 0x1C) {
            if buffered.len() < end + 2 {
                break;
            }
            let text = String::from_utf8_lossy(&buffered[1..end]).into_owned();
            buffered.drain(..end + 2);
            let control = text.split('|').nth(9).unwrap_or("").to_owned();
            let code = if control.starts_with("REJ") && !accept_all.load(Ordering::SeqCst) {
                "AR"
            } else {
                "AA"
            };
            received.lock().unwrap().push(control.clone());
            let ack = format!(
                "MSH|^~\\&|LIS|LISFAC|PACKIE|PACKIEFAC|20260101120001||ACK|A-{control}|P|2.5.1\r\
                 MSA|{code}|{control}\r"
            );
            if stream.write_all(&frame(&ack)).is_err() {
                return;
            }
        }
    }
}

/// A running engine with the `orders` channel applied; stopped when dropped.
struct Rig {
    dir: tempfile::TempDir,
    bootstrap: PathBuf,
    source_port: u16,
    destination: Destination,
    engine: Child,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.engine.kill();
        let _ = self.engine.wait();
    }
}

fn command(bootstrap: &Path, dir: &Path, args: &[&str]) -> Command {
    let mut command = common::packie();
    command
        .args(["--bootstrap", bootstrap.to_str().unwrap()])
        .args(args)
        .current_dir(dir);
    command
}

fn runtime_document(source_port: u16, destination_port: u16) -> Value {
    json!({
        "schema_version": 1,
        "endpoints": [
            {"endpoint_id": "lab-in", "label": "Lab in", "kind": "tcp_listener",
             "settings": {"bind_address": "127.0.0.1", "port": source_port}},
            {"endpoint_id": "lis-out", "label": "LIS out", "kind": "tcp_client",
             "settings": {"host": "127.0.0.1", "port": destination_port, "connect_timeout_ms": 500}},
        ],
        "channels": [{
            "kind": "message", "channel_id": "orders", "label": "Orders",
            "source": {"endpoint": "lab-in", "protocol": "hl7_mllp", "settings": {}},
            "destinations": [{
                "destination_id": "lis", "label": "LIS", "endpoint": "lis-out",
                "protocol": "hl7_mllp", "settings": {},
            }],
            "delivery": {"max_attempts": 2, "ack_timeout_seconds": 2, "send_timeout_seconds": 2},
        }],
    })
}

impl Rig {
    /// Starts the engine with no channels, waits until it answers, then validates and
    /// applies the runtime document through the CLI.
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bootstrap = common::write_bootstrap(
            dir.path(),
            &json!({
                "node": {"node_id": "cli-e2e", "display_name": "CLI test node"},
                "paths": {"state_directory": "state", "log_directory": "logs"},
                "ipc": {"group": null},
                "secrets": {"provider": "file", "file": "secrets.json"},
            }),
        );
        let secrets = dir.path().join("secrets.json");
        std::fs::write(&secrets, "{}").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let engine = common::spawn_engine(&bootstrap);
        let rig = Self {
            dir,
            bootstrap,
            source_port: free_port(),
            destination: Destination::start(),
            engine,
        };
        common::wait_for_engine(&rig.bootstrap);
        let empty = rig.ok(&["status"]);
        assert!(
            empty.contains("No channels configured. `packie config apply <file>` adds some."),
            "{empty}"
        );
        rig.apply_runtime();
        rig
    }

    fn apply_runtime(&self) {
        let path = self.dir.path().join("runtime.json");
        let document = runtime_document(self.source_port, self.destination.port);
        std::fs::write(&path, document.to_string()).unwrap();
        let path = path.to_str().unwrap();
        let valid = self.ok(&["config", "validate", path]);
        assert!(
            valid.contains("is valid: 2 endpoints, 1 channels"),
            "{valid}"
        );
        let applied = self.ok(&["config", "apply", path, "--note", "e2e setup"]);
        assert!(applied.contains("orders"), "{applied}");
        eventually("the channel runs", || {
            self.ok(&["status"]).contains("running")
        });
    }

    fn packie(&self, args: &[&str]) -> Output {
        command(&self.bootstrap, self.dir.path(), args)
            .output()
            .unwrap()
    }

    /// Stdout of a command that must succeed.
    fn ok(&self, args: &[&str]) -> String {
        let output = self.packie(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            text(&output.stderr)
        );
        text(&output.stdout)
    }

    /// Stderr of a command that must fail with exit code 1 (not a usage error).
    fn refused(&self, args: &[&str]) -> String {
        let output = self.packie(args);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{args:?}: {}",
            text(&output.stdout)
        );
        text(&output.stderr)
    }

    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.ok(args)).unwrap()
    }

    /// Sends one message to the channel's source and returns the acknowledgement code. The
    /// source serves one sender at a time, so a refused connection is retried.
    fn send(&self, control_id: &str) -> String {
        self.send_bytes(&oru(control_id))
    }

    fn send_bytes(&self, message: &[u8]) -> String {
        let mut code = String::new();
        eventually("the source acknowledges the message", || {
            code = self.try_send(message);
            !code.is_empty()
        });
        code
    }

    fn try_send(&self, message: &[u8]) -> String {
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", self.source_port)) else {
            return String::new();
        };
        stream.set_read_timeout(Some(WAIT)).unwrap();
        if stream.write_all(message).is_err() {
            return String::new();
        }
        let mut reply = Vec::new();
        let mut byte = [0_u8; 1];
        while stream.read_exact(&mut byte).is_ok() {
            reply.push(byte[0]);
            if byte[0] == 0x1C {
                break;
            }
        }
        let reply = String::from_utf8_lossy(&reply).into_owned();
        reply
            .split("MSA|")
            .nth(1)
            .unwrap_or("")
            .chars()
            .take(2)
            .collect()
    }

    /// The stored message with this control id, once its deliveries have settled.
    fn settled(&self, control_id: &str) -> Value {
        let mut found = Value::Null;
        eventually("the message is delivered or rejected", || {
            let page = self.json(&["messages", "list", "--identifier", control_id, "--json"]);
            let message = page["messages"][0].clone();
            let settled = message["deliveries"].as_array().is_some_and(|d| {
                !d.is_empty()
                    && d.iter()
                        .all(|d| !matches!(d["state"].as_str(), Some("queued" | "sending")))
            });
            if settled {
                found = message;
            }
            settled
        });
        found
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn id_of(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap().to_owned()
}

#[test]
fn status_messages_and_logs_against_a_live_engine() {
    let rig = Rig::start();

    let empty = rig.ok(&["messages", "list"]);
    assert!(empty.contains("No messages stored yet"), "{empty}");

    assert_eq!(rig.send("MSG-00001"), "AA");
    assert_eq!(rig.send("REJ-00002"), "AA");
    let accepted = rig.settled("MSG-00001");
    let rejected = rig.settled("REJ-00002");
    assert_eq!(accepted["delivery_summary"], "complete");
    assert_eq!(rejected["delivery_summary"], "attention");

    check_status(&rig);
    check_list(&rig);
    check_show(&rig, &accepted, &rejected);
    check_close(&rig, &accepted, &rejected);
    check_resend(&rig, &accepted, &rejected);
    check_logs(&rig);
    check_channel_commands(&rig);
    check_secrets(&rig);
    check_reports(&rig);
    check_config_verification(&rig);
    check_superseded_resend(&rig);
    check_setup_checks(&rig);
}

fn check_status(rig: &Rig) {
    let status = rig.ok(&["status"]);
    for expected in [
        "CLI test node (cli-e2e)",
        "Channels (1)",
        "Endpoints (2)",
        "orders",
        "Orders",
        "2 received, 1 acked, 1 unresolved, 0 queued",
        "lab-in",
        "tcp_listener",
        "lis-out",
        "tcp_client",
    ] {
        assert!(
            status.contains(expected),
            "missing `{expected}` in:\n{status}"
        );
    }
    let json = rig.json(&["status", "--json"]);
    assert_eq!(json["channels"][0]["channel_id"], "orders");
}

fn check_list(rig: &Rig) {
    let list = rig.ok(&["messages", "list"]);
    for expected in [
        "RECEIVED",
        "MSG-00001",
        "REJ-00002",
        "ORU^R01",
        "complete (lis acknowledged)",
        "attention (lis rejected)",
        "Showing 1-2 of 2 messages.",
    ] {
        assert!(list.contains(expected), "missing `{expected}` in:\n{list}");
    }
    let attention = rig.ok(&["messages", "list", "--attention", "--channel", "orders"]);
    assert!(
        attention.contains("REJ-00002") && !attention.contains("MSG-00001"),
        "{attention}"
    );
    let none = rig.ok(&["messages", "list", "--state", "invalid"]);
    assert!(none.contains("No messages match these filters"), "{none}");
    let prefix = rig.ok(&["messages", "list", "--identifier", "MSG-", "--prefix"]);
    assert!(
        prefix.contains("MSG-00001") && !prefix.contains("REJ-00002"),
        "{prefix}"
    );
    let paged = rig.ok(&["messages", "list", "--limit", "1"]);
    assert!(
        paged.contains("Showing 1-1 of 2 messages. Next page: --offset 1."),
        "{paged}"
    );
    let second = rig.json(&[
        "messages", "list", "--limit", "1", "--offset", "1", "--json",
    ]);
    assert_eq!(second["total"], 2);
    assert_eq!(second["messages"].as_array().unwrap().len(), 1);
    let past = rig.ok(&["messages", "list", "--offset", "50"]);
    assert!(
        past.contains("past the last of 2 matching messages"),
        "{past}"
    );
    let recent = rig.ok(&["messages", "list", "--since", "1h"]);
    assert!(recent.contains("MSG-00001"), "{recent}");
    let future = rig.ok(&["messages", "list", "--since", "2999-01-01T00:00:00Z"]);
    assert!(future.contains("No messages match"), "{future}");
    let bad = rig.packie(&["messages", "list", "--state", "bogus"]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(
        text(&bad.stderr).contains("is not one of: receiving"),
        "{}",
        text(&bad.stderr)
    );
    let short = rig.refused(&["messages", "list", "--text", "ab"]);
    assert!(short.contains("at least 3 characters"), "{short}");
}

/// `--fields` lists the original's fields by filter path; without it nothing of them shows.
fn check_fields(rig: &Rig, message_id: &str) {
    let has_row = |text: &str, path: &str, value: &str| {
        text.lines()
            .any(|line| line.split_whitespace().eq([path, value]))
    };
    let shown = rig.ok(&["messages", "show", message_id, "--fields"]);
    assert!(
        shown.contains("Fields (") && shown.contains("PATH  "),
        "{shown}"
    );
    for (path, value) in [
        ("MSH.9.1", "ORU"),
        ("MSH.9.2", "R01"),
        ("MSH.10", "MSG-00001"),
        ("PID.3", "MRN-00000"),
        ("PID.5.1", "TEST"),
        ("OBX.5", "5.4"),
    ] {
        assert!(has_row(&shown, path, value), "missing {path} in:\n{shown}");
    }
    let plain = rig.ok(&["messages", "show", message_id]);
    assert!(
        !plain.contains("Fields") && !plain.contains("MSH.9.1"),
        "{plain}"
    );
    let json = rig.json(&["messages", "show", message_id, "--fields", "--json"]);
    let listed = json["fields"]["fields"].as_array().unwrap();
    assert!(
        listed
            .iter()
            .any(|f| f["path"] == "MSH.9.1" && f["value"] == "ORU"),
        "{json}"
    );
    assert_eq!(json["fields"]["truncated"], false);
    let without = rig.json(&["messages", "show", message_id, "--json"]);
    assert!(without["fields"].is_null(), "{without}");
}

fn check_show(rig: &Rig, accepted: &Value, rejected: &Value) {
    let rejected_id = id_of(rejected, "message_id");
    let shown = rig.ok(&["messages", "show", &rejected_id]);
    for expected in [
        rejected_id.as_str(),
        "Orders (orders)",
        "control REJ-00002",
        "Delivery     ",
        "LIS (lis)",
        "State        Rejected by destination",
        "ATTEMPT",
        "AR",
    ] {
        assert!(
            shown.contains(expected),
            "missing `{expected}` in:\n{shown}"
        );
    }
    // Content appears only when a body is asked for.
    assert!(!shown.contains("TEST^PATIENT"), "{shown}");

    let accepted_id = id_of(accepted, "message_id");
    let original = rig.ok(&["messages", "show", &accepted_id, "--body", "original"]);
    assert!(
        original.contains("Body: original") && original.contains("TEST^PATIENT"),
        "{original}"
    );
    check_fields(rig, &accepted_id);
    let sent = rig.ok(&[
        "messages",
        "show",
        &accepted_id,
        "--body",
        "lis",
        "--as",
        "hex",
    ]);
    assert!(
        sent.contains("sent with delivery") && sent.contains("00000000"),
        "{sent}"
    );
    let json = rig.json(&[
        "messages",
        "show",
        &accepted_id,
        "--body",
        "original",
        "--json",
    ]);
    assert_eq!(json["summary"]["control_id"], "MSG-00001");
    assert_eq!(json["body"]["which"]["kind"], "original");
    let unknown = rig.refused(&["messages", "show", &accepted_id, "--body", "nowhere"]);
    assert!(unknown.contains("its destinations are: lis"), "{unknown}");
    let missing = rig.refused(&["messages", "show", "01900000-0000-7000-8000-000000000000"]);
    assert!(missing.starts_with("packie: "), "{missing}");

    // Prefixes. The first 8 hex digits of a UUIDv7 are its time, so messages received in the
    // same minute share them: the lookup either finds the one message or names every match.
    let output = rig.packie(&["messages", "show", &accepted_id[..8]]);
    let short = text(&output.stdout) + &text(&output.stderr);
    assert!(
        short.contains(&accepted_id) && (short.contains("matches") || short.contains("Delivery")),
        "{short}"
    );
    let digits: String = accepted_id.chars().filter(|c| *c != '-').take(24).collect();
    let by_prefix = rig.ok(&["messages", "show", &digits]);
    assert!(by_prefix.contains(&accepted_id), "{by_prefix}");
    let too_short = rig.refused(&["messages", "show", &accepted_id[..6]]);
    assert!(too_short.contains("at least 8"), "{too_short}");
}

fn check_close(rig: &Rig, accepted: &Value, rejected: &Value) {
    let accepted_delivery = id_of(&accepted["deliveries"][0], "delivery_id");
    let refused = rig.refused(&[
        "messages",
        "close",
        &accepted_delivery,
        "--reason",
        "no need",
    ]);
    assert!(refused.starts_with("packie: "), "{refused}");
    let usage = rig.packie(&["messages", "close", &accepted_delivery]);
    assert_eq!(usage.status.code(), Some(2), "a reason is required");
    let blank = rig.refused(&["messages", "close", &accepted_delivery, "--reason", "  "]);
    assert!(
        blank.contains("--reason must be 1 to 500 characters"),
        "{blank}"
    );

    let delivery = id_of(&rejected["deliveries"][0], "delivery_id");
    let reason = "Checked with the LIS team";
    let closed = rig.ok(&["messages", "close", &delivery, "--reason", reason]);
    assert!(
        closed.contains(&format!("Closed delivery {delivery}")),
        "{closed}"
    );
    assert!(
        closed.contains("The destination was not contacted"),
        "{closed}"
    );
    let shown = rig.ok(&["messages", "show", &id_of(rejected, "message_id")]);
    assert!(
        shown.contains("State        Closed") && shown.contains(reason),
        "{shown}"
    );
    let attention = rig.ok(&["messages", "list", "--attention"]);
    assert!(attention.contains("No messages match"), "{attention}");
}

fn check_resend(rig: &Rig, accepted: &Value, rejected: &Value) {
    let accepted_id = id_of(accepted, "message_id");
    let before = rig.destination.received();

    let refused = rig.refused(&["messages", "resend", &accepted_id]);
    assert!(refused.contains("--confirm-duplicate-risk"), "{refused}");
    assert_eq!(rig.destination.received(), before, "nothing was sent");

    let resent = rig.ok(&[
        "messages",
        "resend",
        &accepted_id,
        "--destination",
        "lis",
        "--confirm-duplicate-risk",
    ]);
    assert!(
        resent.contains("queued delivery") && resent.contains("Queued 1 delivery"),
        "{resent}"
    );
    eventually("the destination receives the resend", || {
        rig.destination.received() == before + 1
    });

    rig.destination.accept_all.store(true, Ordering::SeqCst);
    let rejected_id = id_of(rejected, "message_id");
    let created = rig.ok(&["messages", "resend", &rejected_id, "--new-message"]);
    assert!(created.contains("Created message"), "{created}");
    eventually("the new message is delivered", || {
        rig.json(&["messages", "list", "--json"])["total"] == 3
            && rig.destination.received() == before + 2
    });
    let unknown = rig.refused(&[
        "messages",
        "resend",
        &accepted_id,
        "--destination",
        "nowhere",
    ]);
    assert!(unknown.starts_with("packie: "), "{unknown}");
}

fn check_logs(rig: &Rig) {
    let recent = rig.ok(&["logs", "--limit", "5"]);
    let lines = recent.lines().count();
    assert!((1..=5).contains(&lines), "{recent}");
    let channel = rig.ok(&["logs", "--channel", "orders", "--limit", "50"]);
    assert!(channel.lines().count() > 0);
    assert!(
        channel.lines().all(|l| l.contains("channel_id=orders")),
        "{channel}"
    );
    let json = rig.ok(&["logs", "--since", "1h", "--limit", "3", "--json"]);
    for line in json.lines() {
        let event: Value = serde_json::from_str(line).unwrap();
        assert!(
            event["seq"].is_u64() && event["message"].is_string(),
            "{line}"
        );
    }
    let none = rig.packie(&["logs", "--text", "zzzz-no-such-text"]);
    assert!(none.status.success() && none.stdout.is_empty());
    assert!(
        text(&none.stderr).contains("No log events match"),
        "{}",
        text(&none.stderr)
    );
    let level = rig.packie(&["logs", "--level", "loud"]);
    assert_eq!(level.status.code(), Some(2));
}

fn channel_state(rig: &Rig) -> String {
    let status = rig.json(&["status", "--json"]);
    let channel = &status["channels"][0];
    format!(
        "{} {}",
        channel["state"].as_str().unwrap(),
        channel["stop_reason"].as_str().unwrap_or("")
    )
    .trim()
    .to_owned()
}

fn check_channel_commands(rig: &Rig) {
    let paused = rig.ok(&["channels", "pause", "orders"]);
    assert!(
        paused.contains("Channel orders paused. No retry is scheduled"),
        "{paused}"
    );
    assert_eq!(channel_state(rig), "stopped operator_paused");
    let status = rig.ok(&["status"]);
    assert!(status.contains("stopped (operator_paused)"), "{status}");

    let again = rig.refused(&["channels", "pause", "orders"]);
    assert!(again.contains("already stopped"), "{again}");
    let restart = rig.refused(&["channels", "restart", "orders"]);
    assert!(restart.contains("packie channels resume"), "{restart}");

    let resumed = rig.ok(&["channels", "resume", "orders"]);
    assert!(
        resumed.contains("Channel orders resumed, starting from configuration version 1."),
        "{resumed}"
    );
    eventually("the channel runs again", || channel_state(rig) == "running");
    let not_paused = rig.refused(&["channels", "resume", "orders"]);
    assert!(
        not_paused.contains("Only a paused channel can be resumed"),
        "{not_paused}"
    );

    let restarted = rig.ok(&["channels", "restart", "orders"]);
    assert!(
        restarted.contains("Channel orders restarted"),
        "{restarted}"
    );
    eventually("the channel runs after the restart", || {
        channel_state(rig) == "running"
    });
    let missing = rig.refused(&["channels", "pause", "nowhere"]);
    assert!(missing.contains("No channel nowhere"), "{missing}");
    let usage = rig.packie(&["channels", "pause"]);
    assert_eq!(usage.status.code(), Some(2));
}

#[cfg(unix)]
#[test]
fn logs_follow_streams_filtered_events_and_ends_cleanly_on_ctrl_c() {
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc;

    let rig = Rig::start();
    let mut follower = command(
        &rig.bootstrap,
        rig.dir.path(),
        &[
            "logs",
            "--follow",
            "--channel",
            "orders",
            "--text",
            "message",
            "--limit",
            "5",
        ],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .unwrap();
    let stdout = follower.stdout.take().unwrap();
    let (sender, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    std::thread::sleep(Duration::from_millis(500));

    assert_eq!(rig.send("MSG-00003"), "AA");
    let deadline = Instant::now() + WAIT;
    let mut live = None;
    while live.is_none() && Instant::now() < deadline {
        if let Ok(line) = lines.recv_timeout(Duration::from_millis(200))
            && line.contains("message_id=")
        {
            live = Some(line);
        }
    }
    let live = live.expect("a live event about the message");
    assert!(live.contains("channel_id=orders"), "{live}");

    let signalled = Command::new("kill")
        .args(["-INT", &follower.id().to_string()])
        .status()
        .unwrap();
    assert!(signalled.success());
    let deadline = Instant::now() + WAIT;
    let exit = loop {
        if let Some(exit) = follower.try_wait().unwrap() {
            break exit;
        }
        assert!(
            Instant::now() < deadline,
            "packie logs --follow did not end on Ctrl-C"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(exit.success(), "{exit:?}");
}

fn check_secrets(rig: &Rig) {
    use std::io::Write;

    let empty = rig.ok(&["secrets", "list"]);
    assert!(empty.contains("No secrets stored"), "{empty}");

    let mut set = command(
        &rig.bootstrap,
        rig.dir.path(),
        &["secrets", "set", "sftp-password"],
    )
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    set.stdin
        .take()
        .unwrap()
        .write_all(b"synthetic-value\n")
        .unwrap();
    let output = set.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert!(text(&output.stdout).contains("Secret sftp-password stored"));
    assert!(!text(&output.stdout).contains("synthetic-value"));

    assert_eq!(rig.ok(&["secrets", "list"]).trim(), "sftp-password");
    let stored = std::fs::read_to_string(rig.dir.path().join("secrets.json")).unwrap();
    assert!(
        stored.contains("synthetic-value"),
        "the file provider holds the value"
    );

    let refused = rig.refused(&["secrets", "set", "Bad Name"]);
    assert!(refused.contains("packie: "), "{refused}");
    let removed = rig.ok(&["secrets", "delete", "sftp-password"]);
    assert!(
        removed.contains("Secret sftp-password removed"),
        "{removed}"
    );
    assert!(rig.ok(&["secrets", "list"]).contains("No secrets stored"));
    let missing = rig.refused(&["secrets", "delete", "sftp-password"]);
    assert!(
        missing.contains("No secret sftp-password is stored"),
        "{missing}"
    );
}

fn check_reports(rig: &Rig) {
    // Export: CSV with a header and one row per message, bodies only when asked for.
    let csv = rig.ok(&["messages", "export", "--channel", "orders"]);
    let mut lines = csv.lines();
    assert!(
        lines
            .next()
            .unwrap()
            .starts_with("message_id,received_at,channel_id"),
        "{csv}"
    );
    let rows = lines.count();
    assert!(rows >= 2, "{csv}");
    assert!(!csv.contains("MSH|"), "no bodies without --bodies: {csv}");
    let jsonl = rig.ok(&["messages", "export", "--format", "jsonl", "--bodies"]);
    let first: Value = serde_json::from_str(jsonl.lines().next().unwrap()).unwrap();
    assert!(first["body"].as_str().unwrap().contains("MSH|"), "{jsonl}");
    let file = rig.dir.path().join("export.csv");
    let saved = rig.ok(&["messages", "export", "--output", file.to_str().unwrap()]);
    assert!(saved.contains("Exported"), "{saved}");
    assert!(std::fs::read_to_string(&file).unwrap().lines().count() > rows);
    // The file is never overwritten.
    let again = rig.refused(&["messages", "export", "--output", file.to_str().unwrap()]);
    assert!(again.contains("export.csv"), "{again}");

    // Transmission verification: per-destination summary and CSV rows with hashes.
    let unknown = rig.refused(&[
        "messages",
        "verify",
        "--channel",
        "no-such",
        "--since",
        "1h",
    ]);
    assert!(unknown.contains("No channel no-such"), "{unknown}");
    let summary = rig.ok(&["messages", "verify", "--channel", "orders", "--since", "1h"]);
    assert!(
        summary.contains("LIS (lis)") && summary.contains("ACCEPTED"),
        "{summary}"
    );
    let rows = rig.ok(&[
        "messages",
        "verify",
        "--channel",
        "orders",
        "--since",
        "1h",
        "--csv",
    ]);
    let header = rows.lines().next().unwrap();
    assert!(
        header.starts_with(
            "message_id,received_at,channel_id,source_endpoint_id,source_peer,config_version,\
             mapping_version,"
        ) && header.contains("ack_control_id"),
        "{rows}"
    );
    let row: Vec<&str> = rows.lines().nth(1).unwrap().split(',').collect();
    assert_eq!((row[2], row[5]), ("orders", "1"), "{rows}");
    let report = rig.json(&[
        "messages",
        "verify",
        "--channel",
        "orders",
        "--since",
        "1h",
        "--json",
    ]);
    assert_eq!(report["rows_limit"], 10_000);
    assert_eq!(report["truncated"], false);
    assert_eq!(
        report["period_rows_total"].as_u64().unwrap(),
        report["rows"].as_array().unwrap().len() as u64
    );
}

fn check_config_verification(rig: &Rig) {
    let history = rig.ok(&["config", "history"]);
    assert!(history.contains("not verified since change"), "{history}");
    let verified = rig.ok(&["config", "verify", "1", "--note", "VAL-TEST-001"]);
    assert!(
        verified.contains("Version 1 is signed off as verified"),
        "{verified}"
    );
    let history = rig.ok(&["config", "history"]);
    assert!(
        !history.lines().nth(1).unwrap().contains("not verified"),
        "{history}"
    );
    let missing = rig.refused(&["config", "verify", "99", "--note", "x"]);
    assert!(missing.contains("Version 99"), "{missing}");
}

/// Serial port listing, read-only endpoint probes, notification tests and `config diff`.
fn check_setup_checks(rig: &Rig) {
    let ports = rig.json(&["endpoints", "list-serial", "--json"]);
    assert!(ports.is_array(), "{ports}");

    let folder = rig.dir.path().join("probe-folder");
    std::fs::create_dir(&folder).unwrap();
    let settings = serde_json::json!({ "path": folder, "file_pattern": "*.txt" }).to_string();
    let report = rig.json(&[
        "endpoints",
        "test",
        "--kind",
        "folder",
        "--settings",
        &settings,
        "--json",
    ]);
    assert_eq!(report["ok"], true, "{report}");
    assert!(std::fs::read_dir(&folder).unwrap().next().is_none());
    let invalid = rig.refused(&[
        "endpoints",
        "test",
        "--kind",
        "tcp_client",
        "--settings",
        "{}",
    ]);
    assert!(invalid.contains("settings"), "{invalid}");

    let notify = rig.ok(&["notify", "test"]);
    assert!(notify.contains("No notification target"), "{notify}");
    let audit = rig.ok(&["audit", "list", "--action", "endpoint_test"]);
    assert!(audit.contains("endpoint_test"), "{audit}");

    let same = rig.ok(&["config", "diff", "1", "active"]);
    assert!(same.contains("configure the same"), "{same}");
    let edited = rig.dir.path().join("edited.json");
    let mut document: Value = serde_json::from_str(&rig.ok(&["config", "export"])).unwrap();
    document["channels"][0]["label"] = Value::from("Renamed channel");
    std::fs::write(&edited, document.to_string()).unwrap();
    let diff = rig.ok(&["config", "diff", "active", edited.to_str().unwrap()]);
    assert!(
        diff.contains("Label: ") && diff.contains("→ Renamed channel"),
        "{diff}"
    );
}

fn check_superseded_resend(rig: &Rig) {
    assert_eq!(rig.send("COR-00001"), "AA");
    let original = rig.settled("COR-00001");
    assert_eq!(rig.send_bytes(&oru_valued("COR-00001", "6.1")), "AA");
    let original_id = id_of(&original, "message_id");
    let shown = rig.json(&["messages", "show", &original_id, "--json"]);
    let newer = shown["revised_by"]
        .as_str()
        .expect("the original knows its revision")
        .to_owned();

    let refused = rig.refused(&[
        "messages",
        "resend",
        &original_id,
        "--confirm-duplicate-risk",
    ]);
    assert!(
        refused.contains("A newer revision") && refused.contains(&newer),
        "{refused}"
    );
    assert!(refused.contains("--superseded-ok"), "{refused}");
    let resent = rig.ok(&[
        "messages",
        "resend",
        &original_id,
        "--confirm-duplicate-risk",
        "--superseded-ok",
        "--reason",
        "partner asked for the original",
    ]);
    assert!(!resent.contains("newer revision"), "{resent}");
}
