//! One signed-in SFTP session: host key pinning, credentials, bounded steps and the mapping
//! of SSH and SFTP failures to operator-facing errors.

use std::future::Future;
use std::io::SeekFrom;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use packie_config::{Secret, SecretRef, SecretResolver, SftpAuth, SftpSettings};
use russh::Disconnect;
use russh::client::{self, Handle};
use russh::keys::{
    HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate, decode_secret_key,
};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::fs::Metadata;
use russh_sftp::client::{Config as SftpConfig, SftpSession};
use russh_sftp::protocol::{OpenFlags, StatusCode};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::files::{Content, classify};
use crate::link::{LinkError, Operation};

/// How long closing a connection may take.
const CLOSE_LIMIT: Duration = Duration::from_secs(2);

/// What the server is shown to sign in.
pub(super) enum Credentials {
    Password(Secret),
    Key(Arc<PrivateKey>),
}

/// Resolves the secrets of `auth` (and parses the key) off the async threads.
async fn credentials(
    auth: &SftpAuth,
    secrets: &Arc<dyn SecretResolver>,
) -> Result<Credentials, LinkError> {
    let auth = auth.clone();
    let secrets = Arc::clone(secrets);
    tokio::task::spawn_blocking(move || resolve(&auth, secrets.as_ref()))
        .await
        .unwrap_or_else(|_| {
            Err(LinkError::new(
                Operation::Connect,
                "secret lookup was interrupted",
            ))
        })
}

pub(super) fn resolve(
    auth: &SftpAuth,
    secrets: &dyn SecretResolver,
) -> Result<Credentials, LinkError> {
    let lookup = |reference: &SecretRef| {
        secrets
            .resolve(reference)
            .map_err(|e| LinkError::persistent(Operation::Connect, e.to_string()))
    };
    match auth {
        SftpAuth::Password { password } => Ok(Credentials::Password(lookup(password)?)),
        SftpAuth::Key {
            private_key,
            passphrase,
        } => {
            let key = lookup(private_key)?;
            let passphrase = passphrase.as_ref().map(lookup).transpose()?;
            decode_secret_key(key.expose(), passphrase.as_ref().map(Secret::expose))
                .map(|key| Credentials::Key(Arc::new(key)))
                .map_err(|_| {
                    LinkError::persistent(
                        Operation::Connect,
                        "private key cannot be read (not a supported key, or wrong passphrase)",
                    )
                })
        }
    }
}

/// `SHA256:` fingerprints compare without base64 padding or surrounding space.
fn normalized(fingerprint: &str) -> &str {
    fingerprint.trim().trim_end_matches('=')
}

/// Accepts only the server whose key has the pinned fingerprint, and remembers the one it
/// saw so a mismatch can be reported.
pub(super) struct Pinned {
    presented: Arc<Mutex<Option<String>>>,
    expected: String,
}

impl client::Handler for Pinned {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let presented = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256),
            PublicKeyOrCertificate::Certificate(certificate) => {
                certificate.public_key().fingerprint(HashAlg::Sha256)
            }
        }
        .to_string();
        let matches = normalized(&presented) == normalized(&self.expected);
        if let Ok(mut seen) = self.presented.lock() {
            *seen = Some(presented);
        }
        Ok(matches)
    }
}

/// An SSH failure as an operator-facing error: I/O failures keep their kind, the rest is
/// retryable.
fn ssh_failure(error: &russh::Error) -> LinkError {
    match error {
        russh::Error::IO(io) => LinkError::io(Operation::Connect, io),
        other => LinkError::new(Operation::Connect, other.to_string()),
    }
}

fn timed_out(operation: Operation) -> LinkError {
    LinkError::new(operation, "operation timed out")
}

/// An SFTP failure as an operator-facing error. Server messages are not repeated: they can
/// contain paths.
pub(super) fn sftp_failure(operation: Operation, error: &SftpError) -> LinkError {
    match error {
        SftpError::Status(status) => match status.status_code {
            StatusCode::PermissionDenied => LinkError::persistent(operation, "permission denied"),
            StatusCode::NoSuchFile => LinkError::persistent(operation, "remote path not found"),
            code => LinkError::new(operation, format!("server returned {code}")),
        },
        SftpError::Timeout => timed_out(operation),
        SftpError::IO(_) => LinkError::new(operation, "connection lost"),
        other => LinkError::new(operation, other.to_string()),
    }
}

pub(super) fn is_missing(error: &SftpError) -> bool {
    matches!(error, SftpError::Status(status) if status.status_code == StatusCode::NoSuchFile)
}

/// Runs `step` for at most `limit`.
async fn within<T>(
    limit: Duration,
    operation: Operation,
    step: impl Future<Output = Result<T, LinkError>>,
) -> Result<T, LinkError> {
    tokio::time::timeout(limit, step)
        .await
        .unwrap_or_else(|_| Err(timed_out(operation)))
}

/// A signed-in SFTP session; the SSH connection lives as long as it.
pub(super) struct Session {
    pub(super) ssh: Handle<Pinned>,
    pub(super) sftp: SftpSession,
    /// `operation_timeout_ms`.
    pub(super) limit: Duration,
}

impl Session {
    /// Connects, checks the pinned host key, signs in and opens the SFTP subsystem.
    pub(super) async fn connect(
        settings: &SftpSettings,
        secrets: &Arc<dyn SecretResolver>,
    ) -> Result<Self, LinkError> {
        let credentials = credentials(&settings.auth, secrets).await?;
        let limit = Duration::from_millis(u64::from(settings.operation_timeout_ms));
        let mut ssh = handshake(settings).await?;
        authenticate(&mut ssh, &settings.username, &credentials, limit).await?;
        let sftp = open_subsystem(&ssh, limit).await?;
        Ok(Self { ssh, sftp, limit })
    }

    /// Ends the session; failures only mean it was already gone.
    pub(super) async fn close(self) {
        let closing = async {
            let _ = self.sftp.close().await;
            let _ = self
                .ssh
                .disconnect(Disconnect::ByApplication, "", "en")
                .await;
        };
        let _ = tokio::time::timeout(CLOSE_LIMIT, closing).await;
    }

    /// One SFTP step, bounded by the operation timeout.
    pub(super) async fn step<T>(
        &self,
        step: impl Future<Output = Result<T, SftpError>>,
    ) -> Result<T, SftpError> {
        tokio::time::timeout(self.limit, step)
            .await
            .unwrap_or(Err(SftpError::Timeout))
    }

    /// The metadata of `path`, or `None` when it does not exist.
    pub(super) async fn stat(&self, path: &str) -> Result<Option<Metadata>, SftpError> {
        match self.step(self.sftp.metadata(path)).await {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if is_missing(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Whether `path` exists; a failed check is a write failure.
    pub(super) async fn exists(&self, path: &str) -> Result<bool, LinkError> {
        self.stat(path)
            .await
            .map(|m| m.is_some())
            .map_err(|e| sftp_failure(Operation::Write, &e))
    }

    /// Whether `path` exists, counting a failed check as "no". Only for explaining a failure
    /// that already happened.
    pub(super) async fn exists_or_unknown(&self, path: &str) -> bool {
        self.exists(path).await.unwrap_or(false)
    }

    pub(super) async fn list_dir(&self, path: &str) -> Result<Vec<(String, Metadata)>, SftpError> {
        let entries = self.step(self.sftp.read_dir(path)).await?;
        Ok(entries.map(|e| (e.file_name(), e.metadata())).collect())
    }

    pub(super) async fn rename(&self, from: &str, to: &str) -> Result<(), SftpError> {
        self.step(self.sftp.rename(from, to)).await
    }

    /// Removes a file; one that is already gone counts as removed.
    pub(super) async fn remove(&self, path: &str) -> Result<(), SftpError> {
        match self.step(self.sftp.remove_file(path)).await {
            Err(error) if !is_missing(&error) => Err(error),
            _ => Ok(()),
        }
    }

    /// Creates `relative` below `root`, directory by directory.
    pub(super) async fn make_dirs(&self, root: &str, relative: &str) -> Result<(), SftpError> {
        let mut path = root.to_owned();
        for part in relative.split('/').filter(|p| !p.is_empty()) {
            path.push('/');
            path.push_str(part);
            if self.stat(&path).await?.is_none() {
                self.step(self.sftp.create_dir(path.as_str())).await?;
            }
        }
        Ok(())
    }

    /// Reads `remote` unless its size, as the server reports it for the open handle, is over
    /// `limit`, and stops one byte past `limit` whatever it reported. `None` when the file is
    /// gone. The whole exchange is bounded by the operation timeout.
    pub(super) async fn read_bounded(
        &self,
        remote: &str,
        limit: u64,
    ) -> Result<Option<Content>, SftpError> {
        self.step(async {
            let mut file = match self.sftp.open(remote).await {
                Ok(file) => file,
                Err(error) if is_missing(&error) => return Ok(None),
                Err(error) => return Err(error),
            };
            let declared = file.metadata().await?.len();
            if declared > limit {
                return Ok(Some(Content::TooLarge(declared)));
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(limit.saturating_add(1))
                .read_to_end(&mut bytes)
                .await?;
            // The content is complete; a close failure does not change it.
            let _ = file.close().await;
            Ok(Some(classify(bytes, declared, limit)))
        })
        .await
    }

    /// Opens `path` with `flags`, writes `bytes` from `offset` and closes it: one write
    /// operation as far as the timeout is concerned.
    pub(super) async fn put(
        &self,
        path: &str,
        bytes: &[u8],
        flags: OpenFlags,
        offset: u64,
    ) -> Result<(), PutError> {
        let io = |e: std::io::Error| LinkError::io(Operation::Write, &e);
        let opened = AtomicBool::new(false);
        let write = async {
            let mut file = self
                .sftp
                .open_with_flags(path, flags)
                .await
                .map_err(|e| sftp_failure(Operation::Write, &e))?;
            opened.store(true, Ordering::Relaxed);
            if offset > 0 {
                file.seek(SeekFrom::Start(offset)).await.map_err(io)?;
            }
            file.write_all(bytes).await.map_err(io)?;
            file.shutdown().await.map_err(io)
        };
        within(self.limit, Operation::Write, write)
            .await
            .map_err(|error| PutError {
                error,
                opened: opened.load(Ordering::Relaxed),
            })
    }
}

/// A failed [`Session::put`].
pub(super) struct PutError {
    pub(super) error: LinkError,
    /// The server opened the file. With an exclusive create, `false` means it was refused
    /// (the file is somebody else's) and `true` that a partial file of ours is left.
    pub(super) opened: bool,
}

pub(super) async fn handshake(settings: &SftpSettings) -> Result<Handle<Pinned>, LinkError> {
    let presented = Arc::new(Mutex::new(None));
    let handler = Pinned {
        presented: Arc::clone(&presented),
        expected: settings.host_key_fingerprint.clone(),
    };
    let address = (settings.host.as_str(), settings.port);
    let connecting = client::connect(Arc::new(client::Config::default()), address, handler);
    let limit = Duration::from_millis(u64::from(settings.connect_timeout_ms));
    match tokio::time::timeout(limit, connecting).await {
        Ok(Ok(handle)) => Ok(handle),
        Ok(Err(error)) => {
            Err(mismatch(settings, &presented).unwrap_or_else(|| ssh_failure(&error)))
        }
        Err(_) => Err(LinkError::new(Operation::Connect, "connection timed out")),
    }
}

/// The host key mismatch error, if the server showed another key than the pinned one.
fn mismatch(settings: &SftpSettings, presented: &Mutex<Option<String>>) -> Option<LinkError> {
    let seen = presented.lock().ok()?.clone()?;
    (normalized(&seen) != normalized(&settings.host_key_fingerprint)).then(|| {
        LinkError::persistent(
            Operation::Connect,
            format!(
                "host key mismatch: expected {}, got {seen}",
                settings.host_key_fingerprint.trim()
            ),
        )
    })
}

pub(super) async fn authenticate(
    ssh: &mut Handle<Pinned>,
    username: &str,
    credentials: &Credentials,
    limit: Duration,
) -> Result<(), LinkError> {
    let attempt = async {
        let result = match credentials {
            Credentials::Password(password) => {
                ssh.authenticate_password(username, password.expose()).await
            }
            Credentials::Key(key) => {
                let hash = rsa_hash(ssh, key).await;
                let key = PrivateKeyWithHashAlg::new(Arc::clone(key), hash);
                ssh.authenticate_publickey(username, key).await
            }
        };
        result.map_err(|e| ssh_failure(&e))
    };
    if within(limit, Operation::Connect, attempt).await?.success() {
        Ok(())
    } else {
        Err(LinkError::persistent(
            Operation::Connect,
            "authentication rejected by the server",
        ))
    }
}

/// The signature hash an RSA key should use with this server; other keys need none.
async fn rsa_hash(ssh: &Handle<Pinned>, key: &PrivateKey) -> Option<HashAlg> {
    if !key.algorithm().is_rsa() {
        return None;
    }
    let negotiated = tokio::time::timeout(CLOSE_LIMIT, ssh.best_supported_rsa_hash()).await;
    negotiated.ok()?.ok()?.flatten()
}

pub(super) async fn open_subsystem(
    ssh: &Handle<Pinned>,
    limit: Duration,
) -> Result<SftpSession, LinkError> {
    let open = async {
        let channel = ssh
            .channel_open_session()
            .await
            .map_err(|e| ssh_failure(&e))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|e| ssh_failure(&e))?;
        let config = SftpConfig {
            request_timeout_secs: limit.as_secs().max(1),
            ..SftpConfig::default()
        };
        SftpSession::new_with_config(channel.into_stream(), config)
            .await
            .map_err(|e| sftp_failure(Operation::Connect, &e))
    };
    within(limit, Operation::Connect, open).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_compare_without_padding() {
        assert_eq!(normalized(" SHA256:abc= "), "SHA256:abc");
        assert_eq!(normalized("SHA256:abc"), "SHA256:abc");
    }
}
