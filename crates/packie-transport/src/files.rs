//! File endpoints as message sources and destinations.

use std::io::{self, Read as _};

use packie_config::OnExists;
use packie_core::Timestamp;

use crate::link::{BoxFuture, LinkError, Operation};

/// The largest file a source reads into memory when no limit was set with
/// [`FileSource::set_max_file_bytes`]: 256 MiB.
pub(crate) const HARD_CAP_BYTES: u64 = 256 * 1024 * 1024;

/// One complete file read from a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileUnit {
    /// Path relative to the source directory, with `/` separators.
    pub name: String,
    /// The whole content; empty when [`too_large`](Self::too_large) is set.
    pub bytes: Vec<u8>,
    /// Modified time when it was found complete.
    pub modified: Timestamp,
    /// `Some(size)` when the file is over the source's size limit: it was not read
    /// (`bytes` is empty). `size` is what the file system or server reported, or the limit
    /// plus one when the file turned out larger than reported.
    pub too_large: Option<u64>,
}

impl FileUnit {
    pub(crate) fn new(name: String, content: Content, modified: Timestamp) -> Self {
        let (bytes, too_large) = match content {
            Content::Bytes(bytes) => (bytes, None),
            Content::TooLarge(size) => (Vec::new(), Some(size)),
        };
        Self {
            name,
            bytes,
            modified,
            too_large,
        }
    }
}

/// What reading a file under a size limit produced.
pub(crate) enum Content {
    Bytes(Vec<u8>),
    /// Over the limit; the size reported or, for a file that grew, the limit plus one.
    TooLarge(u64),
}

/// Reads `reader` unless `declared` (the size it was reported to have) is over `limit`, and
/// stops at `limit + 1` bytes whatever the reported size was.
pub(crate) fn read_bounded(
    reader: impl io::Read,
    declared: u64,
    limit: u64,
) -> io::Result<Content> {
    if declared > limit {
        return Ok(Content::TooLarge(declared));
    }
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(classify(bytes, declared, limit))
}

/// `bytes` as content, or too large when it exceeds `limit`.
pub(crate) fn classify(bytes: Vec<u8>, declared: u64, limit: u64) -> Content {
    let read = bytes.len() as u64;
    if read > limit {
        Content::TooLarge(declared.max(read))
    } else {
        Content::Bytes(bytes)
    }
}

/// Complete files from a folder or SFTP directory, one at a time, oldest modified first
/// (then by name). A file is complete when it matches `file_pattern`, has no ignored
/// suffix, and its size and modified time have not changed for `stable_seconds` (folders
/// with `exclusive_open_check` also require an exclusive open).
pub trait FileSource: Send {
    /// The next complete file, waiting (watching or polling) until one exists. Cancel-safe:
    /// a file returned is not returned again unless [`consumed`](Self::consumed) fails.
    fn next(&mut self) -> BoxFuture<'_, Result<FileUnit, LinkError>>;

    /// Called after the file's content is committed: moves it to `processed_directory`,
    /// deletes it or leaves it, per `on_consumed`. A file that is left is not returned
    /// again while this source lives.
    fn consumed(&mut self, unit: &FileUnit) -> BoxFuture<'_, Result<(), LinkError>>;

    /// Sets the largest file, in bytes, that is read into memory (the source's
    /// `max_file_bytes`). A larger file is returned as a unit with
    /// [`FileUnit::too_large`] set and no content, so it can be recorded and consumed
    /// without holding it in memory. Without this call the limit is 256 MiB. A source
    /// that ignores it keeps that cap.
    fn set_max_file_bytes(&mut self, max_bytes: u64) {
        let _ = max_bytes;
    }
}

/// Writes files to a folder or SFTP directory.
pub trait FileSink: Send + Sync {
    /// Writes `bytes` as `name` (a file name, no directories) per `write_mode`: with
    /// `atomic_rename` to a uniquely named, exclusively created `.<random>.part` file in the
    /// same directory first, then published, so readers never see a partial file and
    /// concurrent writers never share a temporary. `on_exists` decides what happens when
    /// `name` exists; `fail` and `suffix` never replace a file, also not one created
    /// meanwhile. Returns the name actually written (different from `name` with `suffix`).
    fn write<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
        on_exists: OnExists,
    ) -> BoxFuture<'a, Result<String, LinkError>>;
}

/// Free names tried by `on_exists: suffix` and for the processed directory before giving up.
pub(crate) const MAX_SUFFIX: u32 = 1000;

/// `name` with `-n` inserted before the extension: `orders.csv` → `orders-2.csv`.
pub(crate) fn suffixed(name: &str, n: u32) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{stem}-{n}.{ext}"),
        _ => format!("{name}-{n}"),
    }
}

/// `name`, then its `-1`, `-2`, ... variants up to [`MAX_SUFFIX`].
pub(crate) fn suffix_candidates(name: &str) -> impl Iterator<Item = String> + '_ {
    (0..=MAX_SUFFIX).map(move |n| {
        if n == 0 {
            name.to_owned()
        } else {
            suffixed(name, n)
        }
    })
}

/// The outcome of publishing a file that may not replace another.
pub(crate) enum Stored {
    Done,
    /// The name is (or meanwhile became) taken by another file; nothing was written there.
    Taken,
}

/// `on_exists: fail` met an existing file.
pub(crate) fn output_exists() -> LinkError {
    LinkError::new(
        Operation::Write,
        "the output file already exists (on_exists fail)",
    )
}

/// `on_exists: suffix` ran out of names.
pub(crate) fn no_free_output_name() -> LinkError {
    LinkError::new(
        Operation::Write,
        "no free name for the output file (on_exists suffix)",
    )
}

/// Whether `name` matches a glob with `*` (any run) and `?` (one character).
pub(crate) fn glob_match(pattern: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        match p.get(pi) {
            Some('*') => {
                star = Some((pi, ni));
                pi += 1;
            }
            Some(&c) if c == '?' || c == n[ni] => {
                pi += 1;
                ni += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    pi = sp + 1;
                    ni = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Whether a source should skip `name` (a file name, not a path).
pub(crate) fn is_candidate(name: &str, pattern: &str, ignore_suffixes: &[String]) -> bool {
    !name.starts_with('.')
        && !ignore_suffixes.iter().any(|s| name.ends_with(s.as_str()))
        && glob_match(pattern, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("*", "anything.csv"));
        assert!(glob_match("*.csv", "orders.csv"));
        assert!(!glob_match("*.csv", "orders.csv.tmp"));
        assert!(glob_match("res_??.txt", "res_01.txt"));
        assert!(!glob_match("res_??.txt", "res_1.txt"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
    }

    #[test]
    fn candidates_skip_hidden_and_ignored() {
        let ignore = vec![".tmp".to_owned(), ".part".to_owned()];
        assert!(is_candidate("r.csv", "*.csv", &ignore));
        assert!(!is_candidate("r.csv.tmp", "*", &ignore));
        assert!(!is_candidate(".r.csv.part", "*", &ignore));
        assert!(!is_candidate(".hidden", "*", &[]));
    }

    #[test]
    fn reads_stop_at_the_limit_whatever_the_reported_size() {
        let data = [7_u8; 25];
        let bytes = |content: Content| match content {
            Content::Bytes(bytes) => Some(bytes.len()),
            Content::TooLarge(_) => None,
        };
        let size = |content: Content| match content {
            Content::TooLarge(size) => Some(size),
            Content::Bytes(_) => None,
        };
        assert_eq!(bytes(read_bounded(&data[..], 25, 25).unwrap()), Some(25));
        // Reported over the limit: not read at all.
        assert_eq!(size(read_bounded(&data[..], 25, 10).unwrap()), Some(25));
        // Reported small, but longer: cut off at limit + 1.
        assert_eq!(size(read_bounded(&data[..], 5, 10).unwrap()), Some(11));
    }

    #[test]
    fn suffixes_go_before_the_extension() {
        assert_eq!(suffixed("orders.csv", 2), "orders-2.csv");
        assert_eq!(suffixed("README", 1), "README-1");
        assert_eq!(suffixed(".env", 1), ".env-1");
    }
}
