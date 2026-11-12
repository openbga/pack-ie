//! The SFTP source: polls the remote directory over a fresh connection per batch and hands
//! out complete files.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use packie_config::{OnConsumed, SecretResolver, SftpSettings};
use packie_core::{EndpointId, Timestamp};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::fs::Metadata;
use tokio::time::Instant;

use super::paths::{absolute, join_relative, normalized_relative, remote_root, split_relative};
use super::session::{Session, is_missing, sftp_failure};
use crate::files::{
    Content, FileSource, FileUnit, HARD_CAP_BYTES, is_candidate, suffix_candidates,
};
use crate::link::{BoxFuture, LinkError, Operation};

/// What identifies one state of a file: a change of either makes it a new, unstable file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Version {
    size: u64,
    mtime: u32,
}

/// A file as listed.
struct Listed {
    /// Path relative to `remote_directory`, with `/` separators.
    path: String,
    version: Version,
}

/// How long a file has looked the same.
struct Sighting {
    version: Version,
    /// When this version was first listed.
    since: Instant,
    /// How many listings showed this version.
    listings: u32,
}

/// A polling source of complete files in `remote_directory`.
pub(crate) fn source(
    settings: SftpSettings,
    secrets: Arc<dyn SecretResolver>,
    endpoint_id: EndpointId,
) -> Box<dyn FileSource> {
    Box::new(SftpSource::new(settings, secrets, endpoint_id))
}

struct SftpSource {
    settings: SftpSettings,
    secrets: Arc<dyn SecretResolver>,
    endpoint_id: EndpointId,
    /// The candidates of the latest listing.
    seen: HashMap<String, Sighting>,
    /// Files returned by `next` and not yet consumed.
    outstanding: HashMap<String, Version>,
    /// The latest version consumed per file with `on_consumed = leave`: that version is never
    /// returned again. At most one entry per listed path.
    left: HashMap<String, Version>,
    /// The largest file read into memory.
    max_bytes: u64,
}

impl FileSource for SftpSource {
    fn next(&mut self) -> BoxFuture<'_, Result<FileUnit, LinkError>> {
        Box::pin(self.next_unit())
    }

    fn consumed(&mut self, unit: &FileUnit) -> BoxFuture<'_, Result<(), LinkError>> {
        let name = unit.name.clone();
        Box::pin(async move { self.finish(&name).await })
    }

    fn set_max_file_bytes(&mut self, max_bytes: u64) {
        self.max_bytes = max_bytes;
    }
}

impl SftpSource {
    fn new(
        settings: SftpSettings,
        secrets: Arc<dyn SecretResolver>,
        endpoint_id: EndpointId,
    ) -> Self {
        Self {
            settings,
            secrets,
            endpoint_id,
            seen: HashMap::new(),
            outstanding: HashMap::new(),
            left: HashMap::new(),
            max_bytes: HARD_CAP_BYTES,
        }
    }

    /// Polls until a complete file is found.
    async fn next_unit(&mut self) -> Result<FileUnit, LinkError> {
        loop {
            if let Some(unit) = self.poll().await? {
                return Ok(unit);
            }
            tokio::time::sleep(self.wait()).await;
        }
    }

    /// One connection: list, update what is known, read the oldest complete file.
    async fn poll(&mut self) -> Result<Option<FileUnit>, LinkError> {
        let session = Session::connect(&self.settings, &self.secrets).await?;
        let result = self.scan(&session).await;
        session.close().await;
        result
    }

    async fn scan(&mut self, session: &Session) -> Result<Option<FileUnit>, LinkError> {
        let files = self.list(session).await?;
        self.observe(files);
        match self.pick() {
            Some((path, version)) => self.read(session, path, version).await,
            None => Ok(None),
        }
    }

    /// How long to wait before the next listing: files that are not yet stable are looked
    /// at again when they could be, not a whole poll interval later.
    fn wait(&self) -> Duration {
        let poll = Duration::from_secs(u64::from(self.settings.poll_interval_seconds));
        let stable = Duration::from_secs(u64::from(self.settings.stable_seconds))
            .max(Duration::from_secs(1));
        let settling = self
            .seen
            .iter()
            .any(|(path, s)| !self.excluded(path, s.version) && !self.complete(s));
        if settling { poll.min(stable) } else { poll }
    }

    /// Lists the candidate files, recursively when configured.
    async fn list(&self, session: &Session) -> Result<Vec<Listed>, LinkError> {
        let root = remote_root(&self.settings);
        let mut directories = vec![String::new()];
        let mut files = Vec::new();
        while let Some(relative) = directories.pop() {
            let entries = match session.list_dir(&absolute(root, &relative)).await {
                Ok(entries) => entries,
                Err(error) if !relative.is_empty() => {
                    tracing::warn!(
                        event = "sftp.directory_skipped",
                        endpoint_id = %self.endpoint_id,
                        cause = %sftp_failure(Operation::Read, &error).cause,
                    );
                    continue;
                }
                Err(error) => return Err(sftp_failure(Operation::Read, &error)),
            };
            for (name, metadata) in entries {
                self.sort_entry(&relative, &name, &metadata, &mut directories, &mut files);
            }
        }
        Ok(files)
    }

    /// Files that qualify are listed; sub-directories (other than the processed one) are
    /// queued when `recursive`. Links and other special entries are ignored.
    fn sort_entry(
        &self,
        directory: &str,
        name: &str,
        metadata: &Metadata,
        directories: &mut Vec<String>,
        files: &mut Vec<Listed>,
    ) {
        let settings = &self.settings;
        let path = join_relative(directory, name);
        if metadata.is_dir() {
            if settings.recursive && path != normalized_relative(&settings.processed_directory) {
                directories.push(path);
            }
        } else if metadata.is_regular()
            && is_candidate(name, &settings.file_pattern, &settings.ignore_suffixes)
        {
            let version = Version {
                size: metadata.len(),
                mtime: metadata.mtime.unwrap_or(0),
            };
            files.push(Listed { path, version });
        }
    }

    /// Records a listing. Everything it does is synchronous, so a dropped `next` loses
    /// nothing.
    fn observe(&mut self, files: Vec<Listed>) {
        let now = Instant::now();
        let mut seen = HashMap::with_capacity(files.len());
        for file in files {
            let sighting = match self.seen.remove(&file.path) {
                Some(before) if before.version == file.version => Sighting {
                    listings: before.listings.saturating_add(1),
                    ..before
                },
                _ => Sighting {
                    version: file.version,
                    since: now,
                    listings: 1,
                },
            };
            seen.insert(file.path, sighting);
        }
        self.outstanding.retain(|path, _| seen.contains_key(path));
        self.left.retain(|path, _| seen.contains_key(path));
        self.seen = seen;
    }

    /// Returned and not consumed, or left in place: not to be returned.
    fn excluded(&self, path: &str, version: Version) -> bool {
        self.outstanding.get(path) == Some(&version) || self.left.get(path) == Some(&version)
    }

    /// Size and modified time unchanged across two listings at least `stable_seconds`
    /// apart (without a stability period, one listing is enough).
    fn complete(&self, sighting: &Sighting) -> bool {
        let stable = Duration::from_secs(u64::from(self.settings.stable_seconds));
        stable.is_zero() || (sighting.listings >= 2 && sighting.since.elapsed() >= stable)
    }

    /// The oldest-modified (then by name) complete file not yet returned.
    fn pick(&self) -> Option<(String, Version)> {
        self.seen
            .iter()
            .filter(|(path, s)| !self.excluded(path, s.version) && self.complete(s))
            .min_by(|a, b| (a.1.version.mtime, a.0).cmp(&(b.1.version.mtime, b.0)))
            .map(|(path, s)| (path.clone(), s.version))
    }

    /// Reads the chosen file. A file that vanished or changed while it was read is not
    /// returned; a changed one starts its stability period again.
    async fn read(
        &mut self,
        session: &Session,
        path: String,
        listed: Version,
    ) -> Result<Option<FileUnit>, LinkError> {
        let remote = absolute(remote_root(&self.settings), &path);
        let content = match session.read_bounded(&remote, self.max_bytes).await {
            Ok(Some(content)) => content,
            Ok(None) => return Ok(None),
            Err(error) => return Err(sftp_failure(Operation::Read, &error)),
        };
        let after = session
            .stat(&remote)
            .await
            .map_err(|e| sftp_failure(Operation::Read, &e))?;
        let Some(now) = after.map(|m| Version {
            size: m.len(),
            mtime: m.mtime.unwrap_or(0),
        }) else {
            return Ok(None);
        };
        let complete = match &content {
            Content::Bytes(bytes) => bytes.len() as u64 == listed.size,
            Content::TooLarge(_) => true,
        };
        if now != listed || !complete {
            self.seen.insert(
                path,
                Sighting {
                    version: now,
                    since: Instant::now(),
                    listings: 1,
                },
            );
            return Ok(None);
        }
        tracing::debug!(
            event = "sftp.file_read",
            endpoint_id = %self.endpoint_id,
            bytes = listed.size,
            too_large = matches!(content, Content::TooLarge(_)),
        );
        self.outstanding.insert(path.clone(), listed);
        let modified = Timestamp::from_unix_millis(i64::from(listed.mtime) * 1000)
            .unwrap_or_else(Timestamp::now);
        Ok(Some(FileUnit::new(path, content, modified)))
    }

    /// `consumed`: move, delete or leave the file. A failure leaves it eligible to be
    /// returned again.
    async fn finish(&mut self, name: &str) -> Result<(), LinkError> {
        let version = self.outstanding.remove(name);
        if self.settings.on_consumed == OnConsumed::Leave {
            if let Some(version) = version {
                self.left.insert(name.to_owned(), version);
            }
            return Ok(());
        }
        let session = Session::connect(&self.settings, &self.secrets).await?;
        let done = self.dispose(&session, name).await;
        session.close().await;
        done?;
        tracing::debug!(
            event = "sftp.file_consumed",
            endpoint_id = %self.endpoint_id,
            action = ?self.settings.on_consumed,
        );
        Ok(())
    }

    async fn dispose(&self, session: &Session, name: &str) -> Result<(), LinkError> {
        let root = remote_root(&self.settings);
        let from = absolute(root, name);
        let failure = |e: &SftpError| sftp_failure(Operation::Write, e);
        if self.settings.on_consumed == OnConsumed::Delete {
            return session.remove(&from).await.map_err(|e| failure(&e));
        }
        let (directory, file) = split_relative(name);
        let processed = normalized_relative(&self.settings.processed_directory);
        let target_directory = join_relative(&processed, directory);
        session
            .make_dirs(root, &target_directory)
            .await
            .map_err(|e| failure(&e))?;
        let free = free_name(session, &absolute(root, &target_directory), file).await?;
        let to = absolute(&absolute(root, &target_directory), &free);
        match session.rename(&from, &to).await {
            Err(error) if !is_missing(&error) => Err(failure(&error)),
            _ => Ok(()),
        }
    }
}

/// `file`, or the first `file-n` not present in `directory`.
async fn free_name(session: &Session, directory: &str, file: &str) -> Result<String, LinkError> {
    for candidate in suffix_candidates(file) {
        if !session.exists(&absolute(directory, &candidate)).await? {
            return Ok(candidate);
        }
    }
    Err(LinkError::new(
        Operation::Write,
        "no free name in the processed directory",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_settings() -> SftpSettings {
        serde_json::from_value(serde_json::json!({
            "host": "127.0.0.1",
            "port": 22,
            "username": "lab_user",
            "auth": {"method": "password", "password": {"secret": "sftp-pw"}},
            "host_key_fingerprint": "SHA256:synthetic",
            "remote_directory": "/inbound",
            "on_consumed": "leave",
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn leave_keeps_one_version_per_name() {
        let secrets = Arc::new(packie_config::MapSecrets::default());
        let mut source = SftpSource::new(source_settings(), secrets, EndpointId::generate());
        for size in 1..=50_u64 {
            let version = Version { size, mtime: 1 };
            let names = ["a.csv", "b.csv"];
            let listed = |path: &str| Listed {
                path: path.to_owned(),
                version,
            };
            source.observe(names.map(listed).into());
            source
                .outstanding
                .extend(names.map(|p| (p.to_owned(), version)));
            source.finish("a.csv").await.unwrap();
            source.finish("b.csv").await.unwrap();
            assert!(names.iter().all(|path| source.excluded(path, version)));
            let older = Version { size: 0, mtime: 1 };
            assert!(names.iter().all(|path| !source.excluded(path, older)));
            assert_eq!(source.left.len(), 2, "one entry per name, not per version");
        }
        source.observe(vec![Listed {
            path: "a.csv".to_owned(),
            version: Version { size: 50, mtime: 1 },
        }]);
        assert_eq!(source.left.len(), 1, "names no longer listed are pruned");
        source.observe(Vec::new());
        assert!(source.left.is_empty());
    }
}
