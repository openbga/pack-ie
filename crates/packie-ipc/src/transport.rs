//! Local transports: a Unix domain socket, or a Windows named pipe. Both are reachable
//! only from this computer; access is restricted by the OS. The socket is `0600`, or
//! `0660` owned by the configured group (`ipc.group`) when that group exists; the pipe
//! rejects remote clients and has an explicit DACL: `LocalSystem` and administrators have
//! full access and members of the local group named by `ipc.group` (`PACK-IE Operators` when it
//! is the cross-platform default `packie`) may read and write, so the unelevated console can
//! attach to the service running as `LocalSystem`. Without that group only administrators
//! can connect.

use std::io;

/// The IPC endpoint could not be bound or reached.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// Another engine already serves this endpoint.
    #[error("another engine is already serving {0}")]
    InUse(String),
    /// Binding, accepting or connecting failed.
    #[error("IPC endpoint {endpoint}: {source}")]
    Io {
        /// The endpoint.
        endpoint: String,
        /// The cause.
        source: io::Error,
    },
}

fn io_error(endpoint: &str) -> impl FnOnce(io::Error) -> TransportError + '_ {
    move |source| TransportError::Io {
        endpoint: endpoint.to_owned(),
        source,
    }
}

/// The OS account behind an accepted connection, read from the connection itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeerIdentity {
    /// Account name; `uid <n>` when the OS has no name for the uid.
    pub(crate) name: String,
    /// The uid (Unix) or SID (Windows) as text.
    pub(crate) uid_or_sid: String,
    /// The client's process id, when the OS reports it.
    pub(crate) pid: Option<u32>,
}

/// The parts of the Windows pipe policy that need no OS call, so they are tested on every
/// platform.
#[cfg(any(windows, test))]
mod pipe_policy {
    /// The cross-platform default of `ipc.group` (a Unix group name).
    const DEFAULT_GROUP: &str = "packie";
    /// The local group that the Windows installer creates and the pipe admits by default.
    const WINDOWS_GROUP: &str = "PACK-IE Operators";

    /// The Windows group for a configured `ipc.group`: the cross-platform default `packie`
    /// means [`WINDOWS_GROUP`]; any other value is used as written (`group` or
    /// `DOMAIN\group`).
    pub(super) fn group_name(configured: &str) -> &str {
        if configured == DEFAULT_GROUP {
            WINDOWS_GROUP
        } else {
            configured
        }
    }

    /// The pipe's protected DACL: `LocalSystem` and administrators have full access, the
    /// group of `group_sid` (SID string) may read and write. Without a group only the
    /// first two remain. A malformed SID text is dropped, never spliced into the SDDL.
    pub(super) fn pipe_sddl(group_sid: Option<&str>) -> String {
        let mut sddl = String::from("D:P(A;;GA;;;SY)(A;;GA;;;BA)");
        if let Some(sid) = group_sid.filter(|sid| is_sid_text(sid)) {
            sddl.push_str("(A;;GRGW;;;");
            sddl.push_str(sid);
            sddl.push(')');
        }
        sddl
    }

    /// `S-1-5-21-…`: the letter `S` and digit groups separated by dashes.
    fn is_sid_text(text: &str) -> bool {
        text.strip_prefix("S-").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .split('-')
                    .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        })
    }

    /// `DOMAIN\account` as Windows reports it, or just `account` when it has no domain.
    pub(super) fn account_name(domain: &str, account: &str) -> String {
        if domain.is_empty() {
            account.to_owned()
        } else {
            format!(r"{domain}\{account}")
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_cross_platform_default_group_is_the_windows_group() {
            assert_eq!(group_name("packie"), "PACK-IE Operators");
            assert_eq!(group_name("Lab Staff"), "Lab Staff");
            assert_eq!(group_name(r"LAB\Operators"), r"LAB\Operators");
            assert_eq!(group_name("PACKIE"), "PACKIE");
        }

        #[test]
        fn the_dacl_grants_the_group_instead_of_all_users() {
            assert_eq!(
                pipe_sddl(Some("S-1-5-21-1-2-3-1004")),
                "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;S-1-5-21-1-2-3-1004)"
            );
            assert!(!pipe_sddl(Some("S-1-5-21-1-2-3-1004")).contains("BU"));
        }

        #[test]
        fn without_a_group_only_system_and_administrators_remain() {
            assert_eq!(pipe_sddl(None), "D:P(A;;GA;;;SY)(A;;GA;;;BA)");
        }

        #[test]
        fn a_malformed_sid_is_never_spliced_into_the_dacl() {
            for bad in [
                "",
                "S-",
                "BU",
                "S-1-5-)(A;;GA;;;WD",
                "S-1--5",
                "s-1-5",
                "S-1-5-x",
            ] {
                assert_eq!(pipe_sddl(Some(bad)), pipe_sddl(None), "{bad}");
            }
        }

        #[test]
        fn account_names_are_domain_qualified_when_windows_says_so() {
            assert_eq!(account_name("LABPC", "alice"), r"LABPC\alice");
            assert_eq!(account_name("", "alice"), "alice");
        }
    }
}

#[cfg(unix)]
pub(crate) use unix::{Listener, Stream, connect, peer_identity};
#[cfg(windows)]
pub(crate) use windows::{
    Listener, Stream, connect, peer_identity, this_process_account as windows_account,
};

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    use nix::unistd::{Group, Uid, User};
    use tokio::net::{UnixListener, UnixStream};

    use super::{PeerIdentity, TransportError, io_error};

    pub(crate) type Stream = UnixStream;

    #[derive(Debug)]
    pub(crate) struct Listener {
        listener: UnixListener,
        path: PathBuf,
        endpoint: String,
    }

    impl Listener {
        pub(crate) fn bind(endpoint: &str, group: Option<&str>) -> Result<Self, TransportError> {
            let path = PathBuf::from(endpoint);
            // A live engine answers; a stale socket from a crash does not.
            let live = path.exists() && std::os::unix::net::UnixStream::connect(&path).is_ok();
            if live {
                return Err(TransportError::InUse(endpoint.to_owned()));
            }
            if path.exists() {
                std::fs::remove_file(&path).map_err(io_error(endpoint))?;
            }
            let listener = UnixListener::bind(&path).map_err(io_error(endpoint))?;
            let mode = match group.map(|name| share(&path, name)) {
                Some(Ok(())) => 0o660,
                Some(Err(reason)) => {
                    tracing::warn!(
                        event = "ipc.group_unavailable",
                        group = group.unwrap_or_default(),
                        error_summary = %reason,
                        "IPC socket is owner-only: {reason}"
                    );
                    0o600
                }
                None => 0o600,
            };
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                .map_err(io_error(endpoint))?;
            Ok(Self {
                listener,
                path,
                endpoint: endpoint.to_owned(),
            })
        }

        pub(crate) async fn accept(&mut self) -> Result<Stream, TransportError> {
            let (stream, _) = self
                .listener
                .accept()
                .await
                .map_err(io_error(&self.endpoint))?;
            Ok(stream)
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    /// Gives the socket to `group`, so its members can connect.
    fn share(path: &Path, group: &str) -> Result<(), String> {
        let found =
            Group::from_name(group).map_err(|e| format!("cannot look up group {group}: {e}"))?;
        let Some(found) = found else {
            return Err(format!("group {group} does not exist"));
        };
        std::os::unix::fs::chown(path, None, Some(found.gid.as_raw()))
            .map_err(|e| format!("cannot give the socket to group {group}: {e}"))
    }

    /// The peer's uid from the socket (`SO_PEERCRED` on Linux, `getpeereid` and
    /// `LOCAL_PEERPID` on macOS) and its account name; an error when the OS cannot say.
    pub(crate) fn peer_identity(stream: &Stream) -> Result<PeerIdentity, String> {
        let credentials = stream
            .peer_cred()
            .map_err(|e| format!("the peer's credentials cannot be read: {e}"))?;
        let uid = credentials.uid();
        let name = User::from_uid(Uid::from_raw(uid))
            .ok()
            .flatten()
            .map_or_else(|| format!("uid {uid}"), |user| user.name);
        Ok(PeerIdentity {
            name,
            uid_or_sid: uid.to_string(),
            pid: credentials.pid().and_then(|pid| u32::try_from(pid).ok()),
        })
    }

    pub(crate) async fn connect(endpoint: &str) -> Result<Stream, TransportError> {
        UnixStream::connect(endpoint)
            .await
            .map_err(io_error(endpoint))
    }
}

#[cfg(windows)]
mod windows {
    use std::time::Duration;

    use interprocess::os::windows::ToWtf16;
    use interprocess::os::windows::named_pipe::tokio::{DuplexPipeStream, PipeListener};
    use interprocess::os::windows::named_pipe::{PipeListenerOptions, pipe_mode};
    use interprocess::os::windows::security_descriptor::SecurityDescriptor;
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
    use winsafe::{self as w, co};

    use super::pipe_policy::{account_name, group_name, pipe_sddl};
    use super::{PeerIdentity, TransportError, io_error};

    /// `ERROR_PIPE_BUSY`: every instance is in use; wait and retry.
    const PIPE_BUSY: i32 = 231;
    /// `ERROR_ACCESS_DENIED`: on the first instance, another process owns the name.
    const ACCESS_DENIED: i32 = 5;

    /// The server end of one connected pipe instance.
    pub(crate) type Stream = DuplexPipeStream<pipe_mode::Bytes>;

    #[derive(Debug)]
    pub(crate) struct Listener {
        pipe: PipeListener<pipe_mode::Bytes, pipe_mode::Bytes>,
        endpoint: String,
    }

    /// The security descriptor of [`pipe_sddl`]: protected (nothing inherited),
    /// `LocalSystem` and administrators have full access and the members of the operators
    /// group (`group_sid`) may read and write. The service runs as `LocalSystem` while the
    /// console runs as the interactive user. Remote clients are rejected by the pipe itself
    /// (`PIPE_REJECT_REMOTE_CLIENTS`), not by this list.
    fn pipe_security(group_sid: Option<&str>) -> std::io::Result<SecurityDescriptor> {
        let sddl = pipe_sddl(group_sid)
            .to_wtf_16()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        SecurityDescriptor::deserialize(&sddl)
    }

    /// The SID text of the local group `name`; an error when it does not exist or names
    /// something other than a group.
    fn group_sid(name: &str) -> Result<String, String> {
        let (_, sid, kind) = w::LookupAccountName(None, name)
            .map_err(|e| format!("group {name} cannot be resolved: {e}"))?;
        let is_group = kind == co::SID_NAME_USE::Group
            || kind == co::SID_NAME_USE::Alias
            || kind == co::SID_NAME_USE::WellKnownGroup;
        if !is_group {
            return Err(format!("{name} is not a group"));
        }
        w::ConvertSidToStringSid(&sid).map_err(|e| format!("the SID of group {name}: {e}"))
    }

    /// The SID of the group that may use the pipe besides administrators: `None` (with a
    /// warning when a group was configured) when there is none or it cannot be resolved.
    fn shared_group_sid(group: Option<&str>) -> Option<String> {
        let name = group_name(group?);
        match group_sid(name) {
            Ok(sid) => Some(sid),
            Err(reason) => {
                tracing::warn!(
                    event = "ipc.group_unavailable",
                    group = name,
                    error_summary = %reason,
                    "IPC pipe is limited to administrators: {reason}"
                );
                None
            }
        }
    }

    impl Listener {
        /// Creates the first instance, which fails when another engine owns the name;
        /// every accepted client is handed a connected instance and a fresh one waits.
        /// Members of the local `group` (`packie` means `PACK-IE Operators`) may connect besides
        /// administrators; when it does not exist, administrators only.
        pub(crate) fn bind(endpoint: &str, group: Option<&str>) -> Result<Self, TransportError> {
            let mut options = PipeListenerOptions::new();
            options.path = endpoint
                .to_wtf_16()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
                .map_err(io_error(endpoint))?;
            options.accept_remote = false;
            options.security_descriptor = Some(
                pipe_security(shared_group_sid(group).as_deref()).map_err(io_error(endpoint))?,
            );
            let pipe = options
                .create_tokio_duplex::<pipe_mode::Bytes>()
                .map_err(|e| match e.raw_os_error() {
                    Some(ACCESS_DENIED) => TransportError::InUse(endpoint.to_owned()),
                    _ => io_error(endpoint)(e),
                })?;
            Ok(Self {
                pipe,
                endpoint: endpoint.to_owned(),
            })
        }

        pub(crate) async fn accept(&mut self) -> Result<Stream, TransportError> {
            self.pipe.accept().await.map_err(io_error(&self.endpoint))
        }
    }

    /// The pipe client's account: the user SID of the client process's token (found from
    /// `GetNamedPipeClientProcessId`) and its `DOMAIN\user` name; an error when Windows
    /// cannot say. The client's own claims are never used.
    ///
    /// The process id is read from the connection, then the process is opened by id, so a
    /// client that exits and whose id is reused between those two calls would be
    /// misattributed. The window is microseconds, Windows delays id reuse, and only members
    /// of the operators group can connect; impersonating the client would close it but has
    /// no safe API here (neither the tokio pipe stream nor `winsafe` expose it).
    pub(crate) fn peer_identity(stream: &Stream) -> Result<PeerIdentity, String> {
        let pid = stream
            .client_process_id()
            .map_err(|e| format!("the pipe client's process id cannot be read: {e}"))?;
        let (sid, name) = process_user(pid)
            .map_err(|e| format!("the account of pipe client process {pid} cannot be read: {e}"))?;
        Ok(PeerIdentity {
            name,
            uid_or_sid: sid,
            pid: Some(pid),
        })
    }

    /// The account this process runs as, from its own token: `(name, SID text)`.
    pub(crate) fn this_process_account() -> Result<(String, String), String> {
        let pid = std::process::id();
        process_user(pid)
            .map(|(sid, name)| (name, sid))
            .map_err(|e| format!("the account of this process cannot be read: {e}"))
    }

    /// The SID text and account name of the user of process `pid`. The name falls back to
    /// the SID text when Windows cannot resolve it (for example an unreachable domain).
    fn process_user(pid: u32) -> Result<(String, String), co::ERROR> {
        let process = w::HPROCESS::OpenProcess(co::PROCESS::QUERY_LIMITED_INFORMATION, false, pid)?;
        let token = process.OpenProcessToken(co::TOKEN::QUERY)?;
        let w::TokenInfo::User(user) =
            token.GetTokenInformation(co::TOKEN_INFORMATION_CLASS::User)?
        else {
            return Err(co::ERROR::UNIDENTIFIED_ERROR);
        };
        let sid = user.User.Sid().ok_or(co::ERROR::INVALID_SID)?;
        let text = w::ConvertSidToStringSid(sid)?;
        let name = w::LookupAccountSid(None, sid).map_or_else(
            |_| text.clone(),
            |(account, domain, _)| account_name(&domain, &account),
        );
        Ok((text, name))
    }

    pub(crate) async fn connect(endpoint: &str) -> Result<NamedPipeClient, TransportError> {
        for _ in 0..50 {
            let busy = match ClientOptions::new().open(endpoint) {
                Ok(client) => return Ok(client),
                Err(e) if e.raw_os_error() == Some(PIPE_BUSY) => e,
                Err(e) => return Err(io_error(endpoint)(e)),
            };
            tracing::debug!(event = "ipc.pipe_busy", error = %busy, "IPC pipe busy; retrying");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err(io_error(endpoint)(std::io::Error::other(
            "all pipe instances are busy",
        )))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_dacl_parses_with_and_without_a_group() {
            pipe_security(None).unwrap();
            pipe_security(Some("S-1-5-32-545")).unwrap();
        }

        #[test]
        fn a_builtin_group_resolves_and_a_missing_one_does_not() {
            assert_eq!(group_sid("Administrators").unwrap(), "S-1-5-32-544");
            assert!(group_sid("No Such Group packie-test").is_err());
            assert!(shared_group_sid(Some("No Such Group packie-test")).is_none());
            assert!(shared_group_sid(None).is_none());
        }

        #[tokio::test]
        async fn a_client_is_attributed_to_its_account_and_process() {
            let name = format!(r"\\.\pipe\packie-identity-test-{}", std::process::id());
            let mut listener = Listener::bind(&name, None).unwrap();
            let client = connect(&name).await.unwrap();
            let stream = listener.accept().await.unwrap();
            let peer = peer_identity(&stream).unwrap();
            assert_eq!(peer.pid, Some(std::process::id()));
            assert!(peer.uid_or_sid.starts_with("S-1-5-"), "{peer:?}");
            assert!(!peer.name.is_empty());
            drop(client);
        }

        #[tokio::test]
        async fn a_second_engine_gets_in_use() {
            let name = format!(r"\\.\pipe\packie-transport-test-{}", std::process::id());
            let first = Listener::bind(&name, None).unwrap();
            assert!(matches!(
                Listener::bind(&name, None),
                Err(TransportError::InUse(_))
            ));
            drop(first);
        }
    }
}
