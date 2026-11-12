//! Protocol-aware steps over real loopback sockets: HL7 v2 over MLLP and ASTM E1381.
//! `send_message` runs on one actor and `expect_message` on the other, concurrently.
#![cfg(test)]

use std::sync::Arc;
use std::time::Duration;

use packie_sim::actors::conn::Conn;
use packie_sim::actors::tcp::{TcpClient, TcpServer};
use packie_sim::ports::PortLease;
use packie_sim::protocol::{expect_message, send_message};
use packie_sim::steps::{ExpectMessageArgs, SendMessageArgs};

const WAIT: Duration = Duration::from_secs(5);

async fn pair() -> (Arc<Conn>, Arc<Conn>) {
    let mut lease = PortLease::reserve_loopback().unwrap();
    let port = lease.port();
    let server = TcpServer::new("127.0.0.1", port, 16, None).unwrap();
    server.adopt(lease.take_socket().unwrap());
    server.listen().unwrap();
    let client = TcpClient::new(None, None).unwrap();
    client.connect("127.0.0.1", port, WAIT).await.unwrap();
    let (srv, _) = server.accept(WAIT).await.unwrap().unwrap();
    // The actors own the connections; leaking them for the test's lifetime is fine.
    let cli = client.current().unwrap();
    std::mem::forget((server, client));
    (cli, srv)
}

fn send(json: serde_json::Value) -> SendMessageArgs {
    serde_json::from_value(json).unwrap()
}

fn expect(json: serde_json::Value) -> ExpectMessageArgs {
    serde_json::from_value(json).unwrap()
}

const ORU: [&str; 4] = [
    "MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|CTRL-1|P|2.5.1",
    "PID|1||PID-TEST-002~ALT^x||TEST^PATIENT",
    "OBR|1||SID-0002|GLU^Glucose",
    "OBX|1|NM|GLU^Glucose||95|mg/dL",
];

#[tokio::test(flavor = "multi_thread")]
async fn hl7_message_is_framed_acknowledged_in_order_and_matched() {
    let (cli, srv) = pair().await;
    let sender = send(serde_json::json!({
        "protocol": "hl7_mllp", "segments": ORU, "expect_reply": ["CA", "AA"]
    }));
    let receiver = expect(serde_json::json!({
        "protocol": "hl7_mllp", "reply": ["CA", "AA"], "reply_delay_ms": 30,
        "assert": [
            {"path": "MSH.10", "equals": "CTRL-1"},
            {"path": "MSH.9", "equals": "ORU^R01"},
            {"path": "PID.3[1].1", "equals": "ALT"},
            {"path": "OBX.5", "matches": "^9[0-9]$"},
            {"path": "OBX.9", "exists": false}
        ]
    }));
    let (sent, received) = tokio::join!(
        send_message(&cli, &sender, WAIT),
        expect_message(&srv, &receiver, WAIT)
    );
    sent.unwrap();
    received.unwrap();
    assert_eq!(cli.buffered(), 0, "every reply was consumed");
    assert_eq!(srv.buffered(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn hl7_failures_say_what_was_expected_and_observed() {
    // A wrong code in the reply.
    let (cli, srv) = pair().await;
    let sender =
        send(serde_json::json!({"protocol": "hl7_mllp", "segments": ORU, "expect_reply": "AA"}));
    let receiver = expect(serde_json::json!({"protocol": "hl7_mllp", "reply": "AR"}));
    let (sent, received) = tokio::join!(
        send_message(&cli, &sender, WAIT),
        expect_message(&srv, &receiver, WAIT)
    );
    received.unwrap();
    let err = sent.unwrap_err();
    assert!(err.contains("expected MSA-1 AA, observed \"AR\""), "{err}");

    // A failed content assertion shows the message.
    let (cli, srv) = pair().await;
    let sender =
        send(serde_json::json!({"protocol": "hl7_mllp", "segments": ORU, "expect_reply": "none"}));
    let receiver = expect(serde_json::json!({
        "protocol": "hl7_mllp", "reply": "none", "assert": [{"path": "MSH.10", "equals": "CTRL-9"}]
    }));
    let (sent, received) = tokio::join!(
        send_message(&cli, &sender, WAIT),
        expect_message(&srv, &receiver, WAIT)
    );
    sent.unwrap();
    let err = received.unwrap_err();
    assert!(
        err.contains("assert MSH.10: expected equal to \"CTRL-9\", observed \"CTRL-1\""),
        "{err}"
    );
    assert!(err.contains("message: MSH|"), "{err}");

    // No reply at all: the sender times out.
    let (cli, srv) = pair().await;
    let sender =
        send(serde_json::json!({"protocol": "hl7_mllp", "segments": ORU, "reply_timeout_ms": 300}));
    let receiver = expect(serde_json::json!({"protocol": "hl7_mllp", "reply": "none"}));
    let (sent, received) = tokio::join!(
        send_message(&cli, &sender, WAIT),
        expect_message(&srv, &receiver, WAIT)
    );
    received.unwrap();
    assert!(
        sent.unwrap_err()
            .contains("no complete MLLP frame within 300 ms")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_segment_list_without_msh_is_sent_as_is() {
    let (cli, srv) = pair().await;
    let sender = send(serde_json::json!({
        "protocol": "hl7_mllp", "segments": ["ZZZ|this is not an HL7 message"], "expect_reply": "none"
    }));
    send_message(&cli, &sender, WAIT).await.unwrap();
    srv.expect_bytes(b"\x0bZZZ|this is not an HL7 message\r\x1c\r", WAIT)
        .await
        .unwrap();
}

const RECORDS: [&str; 5] = [
    "H|\\^&|||SIM^1.0|||||||P|1|20260101120000",
    "P|1|PID-TEST-001|||TEST^PATIENT||19700101|F",
    "O|1|SID-0001||^^^GLU|R",
    "R|1|^^^GLU|95|mg/dL||N||F||||20260101115900",
    "L|1|N",
];

#[tokio::test(flavor = "multi_thread")]
async fn astm_transfer_with_a_forced_retransmission() {
    let (cli, srv) = pair().await;
    let sender = send(serde_json::json!({"protocol": "astm", "records": RECORDS}));
    let receiver = expect(serde_json::json!({
        "protocol": "astm", "reply": "ack", "nak_frames": 2,
        "assert": [
            {"path": "O.3", "equals": "SID-0001"},
            {"path": "R.4", "equals": "95"},
            {"path": "P.3", "matches": "^PID-TEST-"},
            {"path": "L.2", "equals": "1"}
        ]
    }));
    let (sent, received) = tokio::join!(
        send_message(&cli, &sender, WAIT),
        expect_message(&srv, &receiver, WAIT)
    );
    sent.unwrap();
    received.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn astm_partial_transfers_leave_the_line_open() {
    // `partial.frames` stops after n frames without EOT: the receiver never completes.
    let (cli, srv) = pair().await;
    let sender =
        send(serde_json::json!({"protocol": "astm", "records": RECORDS, "partial": {"frames": 2}}));
    let receiver = expect(serde_json::json!({"protocol": "astm"}));
    let (sent, received) = tokio::join!(
        send_message(&cli, &sender, WAIT),
        expect_message(&srv, &receiver, Duration::from_millis(500))
    );
    sent.unwrap();
    assert!(
        received
            .unwrap_err()
            .contains("nothing complete arrived within 500 ms")
    );

    // `omit_eot` sends every frame, then stops.
    let (cli, srv) = pair().await;
    let sender = send(
        serde_json::json!({"protocol": "astm", "records": RECORDS, "partial": {"omit_eot": true}}),
    );
    let receiver = expect(serde_json::json!({"protocol": "astm"}));
    let (sent, received) = tokio::join!(
        send_message(&cli, &sender, WAIT),
        expect_message(&srv, &receiver, Duration::from_millis(500))
    );
    sent.unwrap();
    assert!(received.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn astm_receiver_that_never_answers_enq_makes_the_sender_time_out() {
    let (cli, srv) = pair().await;
    let sender =
        send(serde_json::json!({"protocol": "astm", "records": RECORDS, "reply_timeout_ms": 300}));
    let receiver = expect(serde_json::json!({"protocol": "astm", "reply": "none"}));
    let (sent, received) = tokio::join!(
        send_message(&cli, &sender, WAIT),
        expect_message(&srv, &receiver, WAIT)
    );
    received.unwrap();
    assert!(sent.unwrap_err().contains("no reply byte within 300 ms"));
}
