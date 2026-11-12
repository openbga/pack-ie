//! The SFTP probe: connect, check the pinned host key, sign in, count the entries of the
//! remote directory, disconnect. Nothing is written, no file is opened.

use std::time::{Duration, Instant};

use packie_config::{SecretResolver, SftpAuth, SftpSettings};

use super::paths::{absolute, remote_root};
use super::session::{
    Credentials, Session, authenticate, handshake, open_subsystem, resolve, sftp_failure,
};
use crate::files::is_candidate;
use crate::link::{LinkError, Operation};
use crate::probe::Run;

pub(crate) async fn probe(run: &mut Run, settings: &SftpSettings, secrets: &dyn SecretResolver) {
    let Some(credentials) = run.step("credentials", lookup(settings, secrets)).await else {
        return;
    };
    let host = settings.host.as_str();
    if run
        .step("resolve", crate::probe::resolve(host, settings.port))
        .await
        .is_none()
    {
        return;
    }
    let Some(mut ssh) = run.step("connect", connect(settings)).await else {
        return;
    };
    let limit = Duration::from_millis(u64::from(settings.operation_timeout_ms));
    let signin = authenticate(&mut ssh, &settings.username, &credentials, limit);
    let signed_in = async {
        signin.await?;
        Ok(((), format!("signed in as {}", settings.username)))
    };
    if run.step("authenticate", signed_in).await.is_none() {
        return;
    }
    let Some(sftp) = run.step("open sftp", subsystem(&ssh, limit)).await else {
        return;
    };
    let session = Session { ssh, sftp, limit };
    let listed = run.step("list", list(&session, settings)).await.is_some();
    let started = Instant::now();
    session.close().await;
    run.note("disconnect", true, "closed", started.elapsed());
    if listed {
        run.succeeded(format!(
            "{}:{} accepted the host key and credentials and lists {}.",
            settings.host, settings.port, settings.remote_directory
        ));
    }
}

async fn lookup(
    settings: &SftpSettings,
    secrets: &dyn SecretResolver,
) -> Result<(Credentials, String), LinkError> {
    let credentials = resolve(&settings.auth, secrets)?;
    let detail = match settings.auth {
        SftpAuth::Password { .. } => "password found",
        SftpAuth::Key { .. } => "private key found and readable",
    };
    Ok((credentials, detail.to_owned()))
}

async fn connect(
    settings: &SftpSettings,
) -> Result<(russh::client::Handle<super::session::Pinned>, String), LinkError> {
    let ssh = handshake(settings).await?;
    let detail = format!(
        "connected; host key matches the pinned fingerprint {}",
        settings.host_key_fingerprint.trim()
    );
    Ok((ssh, detail))
}

async fn subsystem(
    ssh: &russh::client::Handle<super::session::Pinned>,
    limit: Duration,
) -> Result<(russh_sftp::client::SftpSession, String), LinkError> {
    let sftp = open_subsystem(ssh, limit).await?;
    Ok((sftp, "SFTP subsystem started".to_owned()))
}

/// Counts the entries of the remote directory: files a source would take, and directories.
/// Names are not reported. Only the top level is listed.
async fn list(session: &Session, settings: &SftpSettings) -> Result<((), String), LinkError> {
    let path = absolute(remote_root(settings), "");
    let entries = session
        .list_dir(&path)
        .await
        .map_err(|e| sftp_failure(Operation::Read, &e))?;
    let directories = entries.iter().filter(|(_, m)| m.is_dir()).count();
    let files = entries
        .iter()
        .filter(|(name, m)| {
            m.is_regular() && is_candidate(name, &settings.file_pattern, &settings.ignore_suffixes)
        })
        .count();
    let scope = if settings.recursive {
        " (top level only)"
    } else {
        ""
    };
    Ok((
        (),
        format!(
            "{path} is readable; {files} file{} match {}, {directories} sub-director{}{scope}",
            if files == 1 { "" } else { "s" },
            settings.file_pattern,
            if directories == 1 { "y" } else { "ies" },
        ),
    ))
}
