//! SFTP endpoints (`docs/spec/configuration.md` §5.4).
//!
//! Every batch of operations (one poll of a source, one write, one `consumed`) opens its own
//! SSH connection and closes it again: the server key must match the pinned SHA-256
//! fingerprint, the secrets are resolved at that moment, and the connection attempt is
//! bounded by `connect_timeout_ms`. Every SFTP step is bounded by `operation_timeout_ms`.
//! Network failures are retryable; a missing secret, a host key mismatch, rejected
//! credentials, a missing remote directory and permission errors are persistent.
//!
//! Writes never share or reuse a temporary: it has a random name and is created with the
//! exclusive flag (`SSH_FXF_EXCL`), and only a temporary this session created is removed.
//! A file that must not replace another is published with `SSH_FXP_RENAME`, which SFTP v3
//! servers refuse when the new name exists (OpenSSH does so without a race); the
//! `posix-rename@openssh.com` extension, which would overwrite, is never used. A server that
//! overwrites on rename leaves only the window between the existence check and the rename.
//!
//! Reads are bounded by the source's size limit: a file whose reported size is over it is not
//! read, and the read stops at one byte past it whatever the server reported.
//!
//! Nothing here logs or reports file content or secrets.
//!
//! `session` is the signed-in connection, `paths` the remote path rules, `source` and `sink`
//! the two directions.

mod paths;
mod probe;
mod session;
mod sink;
mod source;

pub(crate) use probe::probe;
pub(crate) use sink::sink;
pub(crate) use source::source;
