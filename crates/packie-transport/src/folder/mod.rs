//! Folder endpoints (`docs/spec/configuration.md` §5.4): complete files in, files out.
//!
//! This module is the source side; `write` holds the sink.

mod write;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use packie_config::{FolderMode, FolderSettings, OnConsumed};
use packie_core::{EndpointId, Timestamp};
use tokio::sync::mpsc;

use crate::files::{
    Content, FileSink, FileSource, FileUnit, HARD_CAP_BYTES, is_candidate, read_bounded, suffixed,
};
use crate::link::{BoxFuture, LinkError, Operation};

/// A source of complete files in the folder.
pub(crate) fn source(settings: FolderSettings, endpoint_id: EndpointId) -> Box<dyn FileSource> {
    Box::new(FolderSource::new(settings, endpoint_id))
}

/// A writer of files into the folder.
pub(crate) fn sink(settings: FolderSettings) -> Box<dyn FileSink> {
    Box::new(write::FolderSink { settings })
}

/// A file as last listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Seen {
    size: u64,
    modified: SystemTime,
}

/// Change notifications; dropping it stops watching.
struct Watch {
    _watcher: notify::RecommendedWatcher,
    changes: mpsc::Receiver<()>,
}

struct FolderSource {
    settings: FolderSettings,
    endpoint_id: EndpointId,
    watch: Option<Watch>,
    /// Relative name → listing and when it was first seen like that.
    observed: HashMap<String, (Seen, Instant)>,
    /// Returned by `next` (as listed then) and not yet consumed.
    handed_out: HashMap<String, Seen>,
    /// The latest version consumed per name with `on_consumed: leave`; that version is never
    /// returned again. At most one entry per listed name.
    left: HashMap<String, Seen>,
    /// The largest file read into memory.
    max_bytes: u64,
}

impl FolderSource {
    fn new(settings: FolderSettings, endpoint_id: EndpointId) -> Self {
        Self {
            settings,
            endpoint_id,
            watch: None,
            observed: HashMap::new(),
            handed_out: HashMap::new(),
            left: HashMap::new(),
            max_bytes: HARD_CAP_BYTES,
        }
    }

    fn start_watch(&mut self) {
        if self.settings.mode != FolderMode::Watch || self.watch.is_some() {
            return;
        }
        let (tx, changes) = mpsc::channel(1);
        let watcher = notify::recommended_watcher(move |_: notify::Result<notify::Event>| {
            // A full channel already means "look again".
            let _ = tx.try_send(());
        });
        let recursive = if self.settings.recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };
        let started = watcher.and_then(|mut w| {
            notify::Watcher::watch(&mut w, &self.settings.path, recursive).map(|()| w)
        });
        match started {
            Ok(watcher) => {
                self.watch = Some(Watch {
                    _watcher: watcher,
                    changes,
                });
            }
            Err(error) => tracing::warn!(
                event = "folder.watch_unavailable",
                endpoint_id = %self.endpoint_id,
                %error,
                "Folder change notifications unavailable; polling instead"
            ),
        }
    }

    /// Updates observations from a listing; returns complete candidates, oldest first, and
    /// how long until the next one may become complete.
    fn complete(
        &mut self,
        listing: Vec<(String, Seen)>,
    ) -> (Vec<(String, Seen)>, Option<Duration>) {
        let stable = Duration::from_secs(u64::from(self.settings.stable_seconds));
        let now = Instant::now();
        let present: HashSet<&String> = listing.iter().map(|(name, _)| name).collect();
        self.observed.retain(|name, _| present.contains(name));
        self.left.retain(|name, _| present.contains(name));
        let mut ready = Vec::new();
        let mut wait: Option<Duration> = None;
        for (name, seen) in listing {
            if self.handed_out.contains_key(&name) || self.left.get(&name) == Some(&seen) {
                continue;
            }
            let since = match self.observed.get(&name) {
                Some((previous, since)) if *previous == seen => *since,
                _ => {
                    self.observed.insert(name.clone(), (seen, now));
                    now
                }
            };
            let age = now.saturating_duration_since(since);
            if age >= stable {
                ready.push((name, seen));
            } else {
                let remaining = stable.saturating_sub(age);
                wait = Some(wait.map_or(remaining, |w| w.min(remaining)));
            }
        }
        ready.sort_by(|a, b| a.1.modified.cmp(&b.1.modified).then_with(|| a.0.cmp(&b.0)));
        (ready, wait)
    }

    async fn wait(&mut self, until_stable: Option<Duration>) {
        let poll = Duration::from_secs(u64::from(self.settings.poll_interval_seconds.max(1)));
        let delay = until_stable.map_or(poll, |d| d.min(poll));
        match &mut self.watch {
            Some(watch) => {
                tokio::select! {
                    _ = watch.changes.recv() => {}
                    () = tokio::time::sleep(delay) => {}
                }
            }
            None => tokio::time::sleep(delay).await,
        }
    }

    /// Forgets that `name` is out; with `leave`, remembers the version that was read as
    /// the only consumed one of that name.
    fn mark_consumed(&mut self, name: String) {
        let handed_out = self.handed_out.remove(&name);
        if self.settings.on_consumed == OnConsumed::Leave
            && let Some(seen) = handed_out
        {
            self.left.insert(name, seen);
        }
    }

    async fn try_take(&mut self) -> Result<Result<FileUnit, Option<Duration>>, LinkError> {
        let settings = self.settings.clone();
        let listing = blocking(move || list_folder(&settings)).await?;
        let (ready, wait) = self.complete(listing);
        for (name, seen) in ready {
            let path = self.settings.path.join(&name);
            let exclusive = self.settings.exclusive_open_check;
            let limit = self.max_bytes;
            let Some(content) = blocking(move || read_if_free(&path, exclusive, limit)).await?
            else {
                continue;
            };
            self.handed_out.insert(name.clone(), seen);
            let modified = timestamp(seen.modified);
            return Ok(Ok(FileUnit::new(name, content, modified)));
        }
        Ok(Err(wait))
    }
}

impl FileSource for FolderSource {
    fn next(&mut self) -> BoxFuture<'_, Result<FileUnit, LinkError>> {
        Box::pin(async move {
            self.start_watch();
            loop {
                match self.try_take().await? {
                    Ok(unit) => return Ok(unit),
                    Err(until_stable) => self.wait(until_stable).await,
                }
            }
        })
    }

    fn consumed(&mut self, unit: &FileUnit) -> BoxFuture<'_, Result<(), LinkError>> {
        let name = unit.name.clone();
        Box::pin(async move {
            let settings = self.settings.clone();
            let file = name.clone();
            blocking(move || consume_file(&settings, &file)).await?;
            self.mark_consumed(name);
            Ok(())
        })
    }

    fn set_max_file_bytes(&mut self, max_bytes: u64) {
        self.max_bytes = max_bytes;
    }
}

/// Runs blocking file work off the async threads.
pub(crate) async fn blocking<T, F>(work: F) -> Result<T, LinkError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, LinkError> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| LinkError::new(Operation::Read, "file task stopped"))?
}

fn timestamp(time: SystemTime) -> Timestamp {
    let millis = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Timestamp::from_unix_millis(millis).unwrap_or_else(Timestamp::now)
}

fn folder_error(path: &Path, error: &io::Error) -> LinkError {
    let mut link = LinkError::io(Operation::Open, error);
    link.cause = format!("folder {}: {}", path.display(), link.cause);
    link
}

/// How many candidate files the folder holds now, as a source would list them.
pub(crate) fn count_files(settings: &FolderSettings) -> Result<usize, LinkError> {
    list_folder(settings).map(|files| files.len())
}

/// Candidate files under the folder (relative names with `/`), skipping the processed
/// directory.
fn list_folder(settings: &FolderSettings) -> Result<Vec<(String, Seen)>, LinkError> {
    let mut out = Vec::new();
    let processed = settings.path.join(&settings.processed_directory);
    let mut pending = vec![(settings.path.clone(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| folder_error(&dir, &e))?;
        for entry in entries.filter_map(Result::ok) {
            match inspect(settings, &processed, &entry) {
                Some(Entry::Dir(path, name)) => pending.push((path, format!("{prefix}{name}/"))),
                Some(Entry::File(name, seen)) => out.push((format!("{prefix}{name}"), seen)),
                None => {}
            }
        }
    }
    Ok(out)
}

enum Entry {
    Dir(PathBuf, String),
    File(String, Seen),
}

/// A directory to descend into, a candidate file, or nothing.
fn inspect(
    settings: &FolderSettings,
    processed: &Path,
    entry: &std::fs::DirEntry,
) -> Option<Entry> {
    let path = entry.path();
    let name = entry.file_name().to_string_lossy().into_owned();
    let meta = entry.metadata().ok()?;
    if meta.is_dir() {
        let descend = settings.recursive && path != processed && !name.starts_with('.');
        return descend.then_some(Entry::Dir(path, name));
    }
    let wanted =
        meta.is_file() && is_candidate(&name, &settings.file_pattern, &settings.ignore_suffixes);
    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    wanted.then_some(Entry::File(
        name,
        Seen {
            size: meta.len(),
            modified,
        },
    ))
}

/// The content (not read when the file is over `limit`), or `None` when another process
/// still holds the file.
fn read_if_free(path: &Path, exclusive: bool, limit: u64) -> Result<Option<Content>, LinkError> {
    let read_error = |e: &io::Error| LinkError::io(Operation::Read, e);
    let file = match open_shared_none(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        // A Windows sharing violation: the writer still has it open.
        Err(e) if exclusive && e.raw_os_error() == Some(32) => return Ok(None),
        Err(e) => return Err(read_error(&e)),
    };
    if exclusive {
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
            Err(std::fs::TryLockError::Error(e)) => return Err(read_error(&e)),
        }
    }
    // The size of the open file; the read is bounded independently in case it grows.
    let declared = file.metadata().map_err(|e| read_error(&e))?.len();
    let content = read_bounded(file, declared, limit).map_err(|e| read_error(&e))?;
    Ok(Some(content))
}

/// Opens for reading; on Windows without sharing, so an open writer makes it fail.
fn open_shared_none(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    std::os::windows::fs::OpenOptionsExt::share_mode(&mut options, 0);
    options.open(path)
}

fn consume_file(settings: &FolderSettings, name: &str) -> Result<(), LinkError> {
    match settings.on_consumed {
        OnConsumed::Leave => Ok(()),
        OnConsumed::Delete => std::fs::remove_file(settings.path.join(name))
            .map_err(|e| LinkError::io(Operation::Write, &e)),
        OnConsumed::Move => move_processed(settings, name),
    }
}

fn move_processed(settings: &FolderSettings, name: &str) -> Result<(), LinkError> {
    let write_error = |e: &io::Error| LinkError::io(Operation::Write, e);
    let processed = settings.path.join(&settings.processed_directory);
    let source = settings.path.join(name);
    let target = processed.join(name);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| write_error(&e))?;
    }
    let target = free_name(&target);
    std::fs::rename(&source, target).map_err(|e| write_error(&e))
}

/// `path`, or the first `-n` variant that does not exist.
fn free_name(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_owned();
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    (1..)
        .map(|n| path.with_file_name(suffixed(&name, n)))
        .find(|candidate| !candidate.exists())
        .unwrap_or_else(|| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(on_consumed: &str) -> FolderSettings {
        serde_json::from_value(serde_json::json!({
            "path": "/unused",
            "stable_seconds": 0,
            "on_consumed": on_consumed,
        }))
        .unwrap()
    }

    fn seen(size: u64) -> Seen {
        Seen {
            size,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(size),
        }
    }

    #[test]
    fn leave_keeps_one_version_per_name() {
        let mut source = FolderSource::new(settings("leave"), EndpointId::generate());
        for size in 1..=50 {
            let listing = vec![
                ("a.csv".to_owned(), seen(size)),
                ("b.csv".to_owned(), seen(size)),
            ];
            let (ready, _) = source.complete(listing.clone());
            assert_eq!(ready.len(), 2, "each new version is returned once");
            for (name, _) in ready {
                source.handed_out.insert(name.clone(), seen(size));
                source.mark_consumed(name);
            }
            let (ready, _) = source.complete(listing);
            assert!(ready.is_empty(), "a consumed version is not returned again");
            assert_eq!(source.left.len(), 2, "one entry per name, not per version");
        }
        let (_, _) = source.complete(vec![("a.csv".to_owned(), seen(50))]);
        assert_eq!(source.left.len(), 1, "names no longer listed are pruned");
    }

    #[test]
    fn files_over_the_limit_are_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.csv");
        std::fs::write(&path, [b'x'; 20]).unwrap();
        let size = |content: Option<Content>| match content {
            Some(Content::TooLarge(size)) => Some(size),
            _ => None,
        };
        assert_eq!(size(read_if_free(&path, false, 10).unwrap()), Some(20));
        assert!(matches!(
            read_if_free(&path, false, 20).unwrap(),
            Some(Content::Bytes(bytes)) if bytes.len() == 20
        ));
        assert!(
            read_if_free(&dir.path().join("gone.csv"), false, 10)
                .unwrap()
                .is_none()
        );
    }
}
