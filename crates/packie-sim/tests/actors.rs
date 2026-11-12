//! Actor-level tests: no engine involved. Real loopback sockets, a real pseudo-terminal
//! pair and a real temporary folder.
#![cfg(test)]

use std::sync::Arc;
use std::time::Duration;

use packie_sim::actors::conn::{Conn, ConnState, Waited};
use packie_sim::actors::folder::{ContentRule, FolderActor, Selector};
use packie_sim::actors::tcp::{ConnectOutcome, TcpClient, TcpServer};
use packie_sim::payload::{Generate, Pattern, PayloadSpec};
use packie_sim::ports::PortLease;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(5);

/// A connected client/server pair: `(server, client, server-side conn, client-side conn)`.
async fn connected() -> (TcpServer, TcpClient, Arc<Conn>, Arc<Conn>) {
    let mut lease = PortLease::reserve_loopback().unwrap();
    let port = lease.port();
    let server = TcpServer::new("127.0.0.1", port, 16, None).unwrap();
    server.adopt(lease.take_socket().unwrap());
    server.listen().unwrap();
    let client = TcpClient::new(None, None).unwrap();
    let outcome = client.connect("127.0.0.1", port, WAIT).await.unwrap();
    assert!(matches!(outcome, ConnectOutcome::Connected));
    let (accepted, peer) = server.accept(WAIT).await.unwrap().expect("a connection");
    assert!(peer.ip().is_loopback());
    let client_conn = client.current().expect("client connection");
    (server, client, accepted, client_conn)
}

fn counter(bytes: u64, seed: u32) -> Vec<u8> {
    Generate {
        pattern: Pattern::Counter32,
        bytes,
        seed,
    }
    .bytes()
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn tcp_send_and_expect_work_in_both_directions() {
    let (server, client, srv, cli) = connected().await;
    cli.write(b"hello from client\n", None, Duration::ZERO)
        .await
        .unwrap();
    srv.expect_bytes(b"hello from client\n", WAIT)
        .await
        .unwrap();
    srv.write(&[0x0b, 0x00, 0xff, 0x1c, 0x0d], None, Duration::ZERO)
        .await
        .unwrap();
    cli.expect_bytes(&[0x0b, 0x00, 0xff, 0x1c, 0x0d], WAIT)
        .await
        .unwrap();

    // Bytes beyond what a step expects stay for the next step.
    cli.write(b"abcdef", None, Duration::ZERO).await.unwrap();
    srv.expect_bytes(b"abc", WAIT).await.unwrap();
    srv.expect_bytes(b"def", WAIT).await.unwrap();
    assert_eq!(srv.buffered(), 0);
    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn expect_reports_mismatch_and_timeout_precisely() {
    let (server, client, srv, cli) = connected().await;
    cli.write(b"hello wrong world", None, Duration::ZERO)
        .await
        .unwrap();
    let err = srv
        .expect_bytes(b"hello right world", WAIT)
        .await
        .unwrap_err();
    assert!(err.contains("first difference at byte 6"), "{err}");
    assert!(err.contains("right"), "{err}");
    assert!(err.contains("wrong"), "{err}");

    let started = Instant::now();
    let err = cli
        .expect_bytes(b"never sent", Duration::from_millis(200))
        .await
        .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(err.contains("timed out after 200 ms"), "{err}");
    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn close_by_peer_keeps_buffered_bytes_readable() {
    let (server, client, srv, cli) = connected().await;
    cli.write(b"last words", None, Duration::ZERO)
        .await
        .unwrap();
    cli.close(false).await;
    assert_eq!(cli.state(), ConnState::ClosedByActor);
    let state = srv
        .wait_until(Instant::now() + WAIT, |_, s| {
            (!s.is_open()).then(|| s.clone())
        })
        .await;
    assert!(matches!(state, Waited::Ready(ConnState::ClosedByPeer(_))));
    srv.expect_bytes(b"last words", WAIT).await.unwrap();
    // A write after the actor closed fails the step.
    assert!(cli.write(b"x", None, Duration::ZERO).await.is_err());
    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn abort_resets_the_connection() {
    let (server, client, srv, cli) = connected().await;
    cli.close(true).await;
    let state = srv
        .wait_until(Instant::now() + WAIT, |_, s| {
            (!s.is_open()).then(|| s.clone())
        })
        .await;
    let Waited::Ready(ConnState::ClosedByPeer(cause)) = state else {
        panic!("expected a close, got {state:?}");
    };
    assert!(
        cause.contains("reset") || cause.contains("end of stream"),
        "{cause}"
    );
    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn refused_connect_listen_again_and_unclaimed_queue() {
    let mut lease = PortLease::reserve_loopback().unwrap();
    let port = lease.port();
    let server = TcpServer::new("127.0.0.1", port, 16, None).unwrap();
    // A bound socket that does not listen would not refuse on every platform.
    lease.release_socket();
    let client = TcpClient::new(None, None).unwrap();
    // Not listening yet: refused.
    assert!(matches!(
        client.connect("127.0.0.1", port, WAIT).await.unwrap(),
        ConnectOutcome::Refused
    ));
    server.listen().unwrap();
    assert!(server.listen().is_err(), "listening twice is an error");
    assert!(matches!(
        client.connect("127.0.0.1", port, WAIT).await.unwrap(),
        ConnectOutcome::Connected
    ));
    // Nobody claimed it yet: it sits in the unclaimed queue.
    let deadline = Instant::now() + WAIT;
    while server.unclaimed() == 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(server.unclaimed(), 1);
    // `stop_listening` resets queued connections and frees the port.
    server.stop_listening().await.unwrap();
    assert_eq!(server.unclaimed(), 0);
    let conn = client.current().unwrap();
    let state = conn
        .wait_until(Instant::now() + WAIT, |_, s| {
            (!s.is_open()).then(|| s.clone())
        })
        .await;
    assert!(matches!(state, Waited::Ready(ConnState::ClosedByPeer(_))));
    assert!(matches!(
        client.connect("127.0.0.1", port, WAIT).await.unwrap(),
        ConnectOutcome::Refused
    ));
    // ... and `listen` binds again.
    server.listen().unwrap();
    assert!(matches!(
        client.connect("127.0.0.1", port, WAIT).await.unwrap(),
        ConnectOutcome::Connected
    ));
    assert!(server.accept(WAIT).await.unwrap().is_some());
    assert!(
        server
            .accept(Duration::from_millis(100))
            .await
            .unwrap()
            .is_none()
    );
    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn generated_counter_pattern_survives_a_large_transfer() {
    let (server, client, srv, cli) = connected().await;
    let spec = Generate {
        pattern: Pattern::Counter32,
        bytes: 1 << 20,
        seed: 5,
    };
    let data = spec.bytes().unwrap();
    assert_eq!(&data[..8], &[5, 0, 0, 0, 6, 0, 0, 0]);
    let writer = {
        let data = data.clone();
        let cli = Arc::clone(&cli);
        tokio::spawn(async move { cli.write(&data, Some(64 * 1024), Duration::ZERO).await })
    };
    srv.expect_bytes(&data, Duration::from_secs(20))
        .await
        .unwrap();
    writer.await.unwrap().unwrap();

    // A lost, duplicated or reordered word is a mismatch at its offset.
    let mut damaged = counter(4096, 1);
    damaged.drain(100..104);
    cli.write(&damaged, None, Duration::ZERO).await.unwrap();
    let err = srv.expect_bytes(&counter(4096, 1), WAIT).await.unwrap_err();
    assert!(err.contains("first difference at byte 100"), "{err}");
    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_reading_holds_the_sender_back_until_resumed() {
    let mut lease = PortLease::reserve_loopback().unwrap();
    let port = lease.port();
    let server = TcpServer::new("127.0.0.1", port, 16, Some(16 * 1024)).unwrap();
    server.adopt(lease.take_socket().unwrap());
    server.listen().unwrap();
    let client = TcpClient::new(None, None).unwrap();
    client.connect("127.0.0.1", port, WAIT).await.unwrap();
    let (srv, _) = server.accept(WAIT).await.unwrap().unwrap();
    let cli = client.current().unwrap();

    srv.set_reading(false);
    assert!(!srv.is_reading());
    let data = counter(32 << 20, 9);
    let writer = {
        let (data, cli) = (data.clone(), Arc::clone(&cli));
        // Small writes cannot hand the whole payload to a large Windows socket buffer
        // in one call; the paused receiver must eventually close its receive window.
        tokio::spawn(async move { cli.write(&data, Some(64 * 1024), Duration::ZERO).await })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !writer.is_finished(),
        "the sender must be held back while nobody reads"
    );
    assert_eq!(
        srv.buffered(),
        0,
        "a paused reader takes nothing from the socket"
    );

    srv.set_reading(true);
    srv.expect_bytes(&data, Duration::from_secs(30))
        .await
        .unwrap();
    writer.await.unwrap().unwrap();
    client.shutdown().await;
    server.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn chunked_writes_deliver_everything_in_order() {
    let (server, client, srv, cli) = connected().await;
    let started = Instant::now();
    cli.write(b"0123456789", Some(3), Duration::from_millis(40))
        .await
        .unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "three pauses between four chunks"
    );
    srv.expect_bytes(b"0123456789", WAIT).await.unwrap();
    client.shutdown().await;
    server.shutdown().await;
}

#[cfg(unix)]
mod serial {
    use std::io::{Read, Write};
    use std::sync::Arc;

    use packie_sim::actors::serial::{SerialPair, SerialPeer};
    use serialport::SerialPort;

    use super::*;

    /// Opens the engine end. A baud rate of 0 means "pseudo-terminal" to `serialport`; macOS
    /// refuses any other rate on one.
    fn open(path: &std::path::Path) -> Box<dyn SerialPort> {
        let baud = if cfg!(target_os = "macos") { 0 } else { 9600 };
        serialport::new(path.to_string_lossy(), baud)
            .timeout(Duration::from_millis(500))
            .open()
            .expect("open the engine end")
    }

    async fn on_thread<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        tokio::task::spawn_blocking(f).await.unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn serial_pair_round_trips_binary_bytes_and_survives_an_unplug() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("inst.tty");
        let pair = Arc::new(SerialPair::create(link.clone()).unwrap());
        assert_eq!(pair.engine_path(), link);
        let peer = SerialPeer::new(Arc::clone(&pair));
        peer.connect().unwrap();
        assert!(peer.connect().is_err(), "already plugged in");
        let conn = peer.current().unwrap();

        let mut engine_end = open(&link);
        let payload: Vec<u8> = (0..=255).collect();
        let sent = payload.clone();
        engine_end = on_thread(move || {
            engine_end.write_all(&sent).unwrap();
            engine_end.flush().unwrap();
            engine_end
        })
        .await;
        conn.expect_bytes(&payload, WAIT).await.unwrap();

        conn.write(&[0x06, 0x00, 0x80, 0xff], None, Duration::ZERO)
            .await
            .unwrap();
        let (mut engine_end, got) = on_thread(move || {
            let mut buf = [0_u8; 4];
            engine_end.read_exact(&mut buf).unwrap();
            (engine_end, buf)
        })
        .await;
        assert_eq!(got, [0x06, 0x00, 0x80, 0xff]);

        // Unplug: the link disappears and the engine's old handle stops delivering.
        peer.unplug().await;
        assert!(!link.exists(), "the link is removed");
        assert_eq!(conn.state(), ConnState::ClosedByActor);
        let after = on_thread(move || {
            let mut buf = [0_u8; 1];
            engine_end.read(&mut buf).map_or(0, |n| n)
        })
        .await;
        assert_eq!(after, 0, "no data after the unplug");

        // Plug back in: a new link, and a fresh round trip.
        peer.connect().unwrap();
        assert!(link.exists());
        let mut again = open(&link);
        let conn = peer.current().unwrap();
        on_thread(move || again.write_all(b"after-replug").unwrap()).await;
        conn.expect_bytes(b"after-replug", WAIT).await.unwrap();
        peer.shutdown().await;
        assert!(!link.exists());
    }
}

mod folder {
    use super::*;

    /// Writes `late.out` after 200 ms.
    fn write_later(folder: FolderActor) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            folder.write_file("late.out", b"payload", true).unwrap();
        })
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn written_files_complete_and_match() {
        let dir = tempfile::tempdir().unwrap();
        let folder = FolderActor::new(dir.path().to_owned());
        folder
            .write_file("orders/o-1.csv", b"id,qty\r\n1,2\r\n", true)
            .unwrap();
        folder.write_file("plain.txt", b"x", false).unwrap();
        let leftovers: Vec<String> = folder.matching("orders/*");
        assert_eq!(
            leftovers,
            ["orders/o-1.csv"],
            "the temporary name is gone after the rename"
        );

        let by_name = folder
            .expect_file(
                &Selector::Name("orders/o-1.csv".into()),
                &ContentRule::Equals(b"id,qty\r\n1,2\r\n".to_vec()),
                WAIT,
            )
            .await
            .unwrap();
        assert_eq!(by_name, "orders/o-1.csv");
        let by_pattern = folder
            .expect_file(
                &Selector::Pattern("orders/o-*.csv".into()),
                &ContentRule::Contains(vec!["id,qty".into(), "1,2".into()]),
                WAIT,
            )
            .await
            .unwrap();
        assert_eq!(by_pattern, "orders/o-1.csv");
        folder
            .expect_file(&Selector::Pattern("*.txt".into()), &ContentRule::Any, WAIT)
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_file_that_arrives_late_is_waited_for_and_a_wrong_one_fails_with_both_contents() {
        let dir = tempfile::tempdir().unwrap();
        let folder = FolderActor::new(dir.path().to_owned());
        let writer = write_later(folder.clone());
        folder
            .expect_file(
                &Selector::Name("late.out".into()),
                &ContentRule::Equals(b"payload".to_vec()),
                WAIT,
            )
            .await
            .unwrap();
        writer.await.unwrap();

        let err = folder
            .expect_file(
                &Selector::Name("late.out".into()),
                &ContentRule::Equals(b"different".to_vec()),
                Duration::from_millis(200),
            )
            .await
            .unwrap_err();
        assert!(err.contains("expected 9 bytes"), "{err}");
        assert!(err.contains("payload"), "{err}");
        let err = folder
            .expect_file(
                &Selector::Pattern("missing-*".into()),
                &ContentRule::Any,
                Duration::from_millis(100),
            )
            .await
            .unwrap_err();
        assert!(err.contains("no matching file"), "{err}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn expect_no_file_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let folder = FolderActor::new(dir.path().to_owned());
        folder
            .expect_no_file("*.csv", Duration::from_millis(100))
            .await
            .unwrap();
        folder.write_file("a.csv", b"1", true).unwrap();
        let err = folder
            .expect_no_file("*.csv", Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(err.contains("a.csv"), "{err}");
        folder.delete_file("a.csv").unwrap();
        folder
            .expect_no_file("*.csv", Duration::from_millis(100))
            .await
            .unwrap();
        assert!(folder.delete_file("a.csv").is_err());
    }

    #[test]
    fn payload_forms_decode_exactly() {
        let spec = |text: Option<&str>, hex: Option<&str>, b64: Option<&str>| {
            PayloadSpec::from_parts(
                text.map(str::to_owned),
                hex.map(str::to_owned),
                b64.map(str::to_owned),
                None,
            )
        };
        assert_eq!(
            spec(Some("a\u{000b}"), None, None).bytes().unwrap(),
            vec![b'a', 0x0b]
        );
        assert_eq!(
            spec(None, Some("0b 4d-53"), None).bytes().unwrap(),
            vec![0x0b, 0x4d, 0x53]
        );
        assert_eq!(
            spec(None, None, Some("CzFT")).bytes().unwrap(),
            vec![0x0b, 0x31, 0x53]
        );
    }
}

/// A peer that resets every connection as soon as it accepts it (an engine rejecting a second
/// session) must never turn `connect` into an error: the reset may arrive before `connect`
/// looks at the result (macOS, loopback), and the step still expects a connection that is
/// then closed by the peer.
#[tokio::test(flavor = "multi_thread")]
async fn a_peer_that_resets_on_accept_never_makes_connect_fail() {
    let mut lease = PortLease::reserve_loopback().unwrap();
    let port = lease.port();
    let listener = lease.take_socket().unwrap().listen(128).unwrap();
    let rejecter = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let _ = stream.set_zero_linger();
            drop(stream);
        }
    });
    let client = TcpClient::new(None, None).unwrap();
    for attempt in 0..300 {
        let outcome = client.connect("127.0.0.1", port, WAIT).await;
        assert!(
            matches!(outcome, Ok(ConnectOutcome::Connected)),
            "attempt {attempt}: {outcome:?}"
        );
        let conn = client.current().unwrap();
        let closed = conn
            .wait_until(Instant::now() + WAIT, |_, s| {
                (!s.is_open()).then(|| s.clone())
            })
            .await;
        assert!(
            matches!(closed, Waited::Ready(ConnState::ClosedByPeer(_))),
            "attempt {attempt}: the connection ends closed by the peer"
        );
    }
    rejecter.abort();
}
