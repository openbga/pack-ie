//! Probes of TCP client and listener endpoints.

use std::io;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use packie_config::{TcpClientSettings, TcpListenerSettings};
use tokio::net::TcpStream;

use super::Run;
use crate::link::{LinkError, Operation, describe_io};
use crate::listener::bind_socket;
use crate::tcp::connect_one;

/// Resolve the host, connect to the first address that answers, close without sending.
pub(super) async fn tcp_client(run: &mut Run, settings: &TcpClientSettings) {
    let Some(addrs) = run
        .step("resolve", resolve(&settings.host, settings.port))
        .await
    else {
        return;
    };
    let limit = Duration::from_millis(u64::from(settings.connect_timeout_ms.max(1)));
    let Some(latency) = run.step("connect", connect(&addrs, limit)).await else {
        return;
    };
    run.succeeded(format!(
        "{}:{} accepted a connection in {} ms.",
        settings.host,
        settings.port,
        latency.as_millis()
    ));
}

/// The addresses of `host`, at least one.
pub(crate) async fn resolve(host: &str, port: u16) -> Result<(Vec<SocketAddr>, String), LinkError> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| LinkError::io(Operation::Connect, &e))?
        .collect();
    let detail = match addrs.as_slice() {
        [] => {
            return Err(LinkError::new(
                Operation::Connect,
                "host name did not resolve",
            ));
        }
        [only] => format!("resolved to {}", only.ip()),
        [first, rest @ ..] => format!("resolved to {} and {} more", first.ip(), rest.len()),
    };
    Ok((addrs, detail))
}

/// Connects to the first of `addrs` that answers within `limit` each, and drops the
/// connection. The result is how long that connection took.
async fn connect(addrs: &[SocketAddr], limit: Duration) -> Result<(Duration, String), LinkError> {
    let mut last = LinkError::new(Operation::Connect, "connection timed out");
    for addr in addrs {
        let started = Instant::now();
        match tokio::time::timeout(limit, connect_one(*addr)).await {
            Ok(Ok(stream)) => return Ok(connected(stream, *addr, started.elapsed())),
            Ok(Err(error)) => last = LinkError::io(Operation::Connect, &error),
            Err(_) => last = LinkError::new(Operation::Connect, "connection timed out"),
        }
    }
    Err(last)
}

fn connected(stream: TcpStream, addr: SocketAddr, latency: Duration) -> (Duration, String) {
    drop(stream);
    (
        latency,
        format!(
            "connected to {addr} in {} ms, then closed",
            latency.as_millis()
        ),
    )
}

/// Bind the configured address and port as the listener does, then release it.
pub(super) async fn tcp_listener(run: &mut Run, settings: &TcpListenerSettings) {
    let addr = SocketAddr::new(settings.bind_address, settings.port);
    let bind = async {
        let socket = bind_socket(addr).map_err(|e| bind_failure(addr, &e))?;
        drop(socket);
        Ok(((), format!("{addr} can be bound, then released")))
    };
    if run.step("bind", bind).await.is_some() {
        run.succeeded(format!("This computer can listen on {addr}."));
    }
}

fn bind_failure(addr: SocketAddr, error: &io::Error) -> LinkError {
    let cause = match error.kind() {
        io::ErrorKind::AddrInUse => format!(
            "port {} on {} is already in use (by this engine or another program)",
            addr.port(),
            addr.ip()
        ),
        io::ErrorKind::AddrNotAvailable => {
            format!("{} is not an address of this computer", addr.ip())
        }
        _ => describe_io(error),
    };
    let mut link = LinkError::io(Operation::Listen, error);
    link.cause = cause;
    link
}
