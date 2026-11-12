//! Attaching an endpoint: dialing, or binding a listener and accepting one peer while
//! publishing the listener's refusal counts.

use std::time::Duration;

use packie_core::EndpointState;
use packie_transport::{Acceptor, Bind, Incoming, Link, LinkError, Opener, Operation};

use crate::status::{ChannelShared, Role, SideCell};

/// How often a waiting accept samples the listener's refusal counts.
const SAMPLE: Duration = Duration::from_millis(250);

/// The next allowed peer. While waiting, the transport's refusal counts (peers outside
/// the allowlist, extra peers) are copied into `cell` for the endpoint snapshot.
pub(crate) async fn accept(
    acceptor: &mut Box<dyn Acceptor>,
    cell: Option<&SideCell>,
) -> Result<Incoming, LinkError> {
    loop {
        let accepted = tokio::select! {
            accepted = acceptor.accept() => Some(accepted),
            () = tokio::time::sleep(SAMPLE) => None,
        };
        if let Some(cell) = cell {
            cell.set_transport_rejections(acceptor.rejections());
        }
        if let Some(accepted) = accepted {
            return accepted;
        }
    }
}

/// Dials, or binds `listener` (unless bound) and accepts one allowed peer on it,
/// reporting `connecting` and `listening` for `role`; the caller reports the peer. A
/// failed accept drops the listener so the next attempt binds again.
pub(crate) async fn attach(
    shared: &ChannelShared,
    role: Role,
    opener: &Opener,
    listener: &mut Option<Box<dyn Acceptor>>,
) -> Result<Link, LinkError> {
    match opener {
        Opener::Dial(dialer) => {
            shared.set_endpoint(role, EndpointState::Connecting);
            dialer.dial().await
        }
        Opener::Listen(bind) => {
            ensure_bound(shared, role, bind.as_ref(), listener).await?;
            let Some(acceptor) = listener.as_mut() else {
                return Err(LinkError::new(Operation::Listen, "listener not bound"));
            };
            accept(acceptor, shared.cell(role))
                .await
                .map(|incoming| incoming.link)
                .inspect_err(|_| *listener = None)
        }
    }
}

/// Binds `listener` unless it is bound already; `role` reports `listening` after.
pub(crate) async fn ensure_bound(
    shared: &ChannelShared,
    role: Role,
    bind: &dyn Bind,
    listener: &mut Option<Box<dyn Acceptor>>,
) -> Result<(), LinkError> {
    if listener.is_none() {
        shared.set_endpoint(role, EndpointState::Connecting);
        *listener = Some(bind.bind().await?);
    }
    shared.set_endpoint(role, EndpointState::Listening);
    Ok(())
}
