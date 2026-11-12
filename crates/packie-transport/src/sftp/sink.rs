//! The SFTP sink: uploads that never share a temporary and never replace unasked.

use std::sync::Arc;

use packie_config::{OnExists, SecretResolver, SftpSettings, WriteMode};
use packie_core::EndpointId;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::OpenFlags;

use super::paths::{absolute, remote_root};
use super::session::{Session, sftp_failure};
use crate::files::{FileSink, Stored, no_free_output_name, output_exists, suffix_candidates};
use crate::link::{BoxFuture, LinkError, Operation};

/// Attempts at replacing a name that another writer keeps re-creating.
const REPLACE_ATTEMPTS: u32 = 3;

/// A writer of files to `remote_directory`.
pub(crate) fn sink(
    settings: SftpSettings,
    secrets: Arc<dyn SecretResolver>,
    endpoint_id: EndpointId,
) -> Box<dyn FileSink> {
    Box::new(SftpSink {
        settings,
        secrets,
        endpoint_id,
    })
}

struct SftpSink {
    settings: SftpSettings,
    secrets: Arc<dyn SecretResolver>,
    endpoint_id: EndpointId,
}

impl FileSink for SftpSink {
    fn write<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
        on_exists: OnExists,
    ) -> BoxFuture<'a, Result<String, LinkError>> {
        Box::pin(self.write_file(name, bytes, on_exists))
    }
}

/// A file name without directories.
fn check_name(name: &str) -> Result<(), LinkError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return Err(LinkError::persistent(
            Operation::Write,
            "file name must be a plain name without directories",
        ));
    }
    Ok(())
}

impl SftpSink {
    async fn write_file(
        &self,
        name: &str,
        bytes: &[u8],
        on_exists: OnExists,
    ) -> Result<String, LinkError> {
        check_name(name)?;
        let session = Session::connect(&self.settings, &self.secrets).await?;
        let written = self.place(&session, name, bytes, on_exists).await;
        session.close().await;
        let written = written?;
        tracing::debug!(
            event = "sftp.file_written",
            endpoint_id = %self.endpoint_id,
            bytes = bytes.len(),
            on_exists = ?on_exists,
        );
        Ok(written)
    }

    /// Writes per `on_exists` and returns the final name.
    async fn place(
        &self,
        session: &Session,
        name: &str,
        bytes: &[u8],
        on_exists: OnExists,
    ) -> Result<String, LinkError> {
        let root = remote_root(&self.settings);
        match on_exists {
            OnExists::Overwrite => {
                self.store(session, name, bytes, true).await?;
                Ok(name.to_owned())
            }
            // Appending is always a direct write to the existing file: a rename would
            // replace it. This holds in `atomic_rename` mode too.
            OnExists::Append => {
                self.append(session, &absolute(root, name), bytes).await?;
                Ok(name.to_owned())
            }
            OnExists::Fail => {
                if session.exists(&absolute(root, name)).await? {
                    return Err(output_exists());
                }
                match self.store(session, name, bytes, false).await? {
                    Stored::Done => Ok(name.to_owned()),
                    Stored::Taken => Err(output_exists()),
                }
            }
            OnExists::Suffix => self.place_suffixed(session, name, bytes).await,
        }
    }

    /// Stores under the first free of `name`, `name-1`, `name-2`, ...
    async fn place_suffixed(
        &self,
        session: &Session,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, LinkError> {
        let root = remote_root(&self.settings);
        for candidate in suffix_candidates(name) {
            if session.exists(&absolute(root, &candidate)).await? {
                continue;
            }
            if let Stored::Done = self.store(session, &candidate, bytes, false).await? {
                return Ok(candidate);
            }
        }
        Err(no_free_output_name())
    }

    /// Writes `name` per `write_mode`. With `replace` an existing file is replaced
    /// (removed first, because SFTP renames may refuse to overwrite); without it the
    /// file is never replaced and a taken name is reported as [`Stored::Taken`].
    async fn store(
        &self,
        session: &Session,
        name: &str,
        bytes: &[u8],
        replace: bool,
    ) -> Result<Stored, LinkError> {
        let root = remote_root(&self.settings);
        let target = absolute(root, name);
        match self.settings.write_mode {
            WriteMode::Direct => direct(session, &target, bytes, replace).await,
            WriteMode::AtomicRename => {
                let part = absolute(root, &temp_name());
                atomic(session, &part, &target, bytes, replace).await
            }
        }
    }

    /// Appends to `path`, creating it when missing.
    async fn append(&self, session: &Session, path: &str, bytes: &[u8]) -> Result<(), LinkError> {
        let size = session
            .stat(path)
            .await
            .map_err(|e| sftp_failure(Operation::Write, &e))?
            .map_or(0, |m| m.len());
        let flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::APPEND;
        session
            .put(path, bytes, flags, size)
            .await
            .map_err(|e| e.error)
    }
}

/// A random name for an upload in progress: `.<32 hex digits>.part`.
fn temp_name() -> String {
    format!(".{}.part", uuid::Uuid::now_v7().simple())
}

/// Writes the final name directly.
async fn direct(
    session: &Session,
    target: &str,
    bytes: &[u8],
    replace: bool,
) -> Result<Stored, LinkError> {
    let flags = if replace {
        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE
    } else {
        OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE
    };
    match session.put(target, bytes, flags, 0).await {
        Ok(()) => Ok(Stored::Done),
        Err(failed) if replace => Err(failed.error),
        Err(failed) if failed.opened => {
            // The exclusive create succeeded, so the partial file is ours.
            let _ = session.remove(target).await;
            Err(failed.error)
        }
        Err(failed) => {
            // A refused exclusive create is a taken name (SFTP v3 has no status for it).
            if session.exists_or_unknown(target).await {
                return Ok(Stored::Taken);
            }
            Err(failed.error)
        }
    }
}

/// Uploads under the exclusively created `part`, then renames it to `target`. `part` has a
/// random name no other writer uses, so removing it (unless it was published) only ever
/// removes this call's own upload.
async fn atomic(
    session: &Session,
    part: &str,
    target: &str,
    bytes: &[u8],
    replace: bool,
) -> Result<Stored, LinkError> {
    let flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE;
    if let Err(failed) = session.put(part, bytes, flags, 0).await {
        let _ = session.remove(part).await;
        return Err(failed.error);
    }
    let renamed = rename_into_place(session, part, target, replace).await;
    if !matches!(renamed, Ok(Stored::Done)) {
        let _ = session.remove(part).await;
    }
    renamed
}

async fn rename_into_place(
    session: &Session,
    part: &str,
    target: &str,
    replace: bool,
) -> Result<Stored, LinkError> {
    let failure = |e: &SftpError| sftp_failure(Operation::Write, e);
    let attempts = if replace { REPLACE_ATTEMPTS } else { 1 };
    let mut last = None;
    for _ in 0..attempts {
        if replace {
            session.remove(target).await.map_err(|e| failure(&e))?;
        }
        match session.rename(part, target).await {
            Ok(()) => return Ok(Stored::Done),
            Err(error) => last = Some(error),
        }
    }
    let taken = !replace && session.exists_or_unknown(target).await;
    match last {
        Some(_) if taken => Ok(Stored::Taken),
        Some(error) => Err(failure(&error)),
        None => Ok(Stored::Done),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::is_candidate;

    #[test]
    fn temporaries_are_unique_hidden_part_files() {
        let (first, second) = (temp_name(), temp_name());
        assert_ne!(first, second);
        assert!(first.starts_with('.') && first.ends_with(".part"));
        assert!(!is_candidate(&first, "*", &[]));
    }

    #[test]
    fn names_with_directories_are_refused() {
        assert!(check_name("orders.csv").is_ok());
        for bad in ["", ".", "..", "a/b.csv", "a\\b.csv", "/etc/passwd"] {
            let error = check_name(bad).unwrap_err();
            assert!(error.persistent, "{bad}");
        }
    }
}
