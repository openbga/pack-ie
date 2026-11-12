//! The engine's JSON log files, read as they grow: event counts by level, panics, the
//! last startup reconciliation, and excerpts around a point in time for the report.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::util::parse_utc_ms;

/// A line of the engine log that the report may quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) unix_ms: u64,
    pub(crate) level: String,
    pub(crate) text: String,
}

/// Longest quoted line, characters.
const QUOTE: usize = 320;

/// The timestamp, level and event of a log line.
fn parse(line: &str) -> Option<Line> {
    let value: Value = serde_json::from_str(line).ok()?;
    let unix_ms = parse_utc_ms(value.get("timestamp")?.as_str()?)?;
    Some(Line {
        unix_ms,
        level: value.get("level")?.as_str()?.to_uppercase(),
        text: line.chars().take(QUOTE).collect(),
    })
}

fn event_of(line: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    value.get("event")?.as_str().map(str::to_owned)
}

/// Log files of a directory, oldest name first.
fn log_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("packie.") && n.contains(".log"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

/// Incremental reader of the log directory.
#[derive(Debug, Default, Clone)]
pub(crate) struct LogScan {
    dir: PathBuf,
    offsets: HashMap<PathBuf, u64>,
    /// Lines per `(level, event)`.
    pub(crate) counts: BTreeMap<(String, String), u64>,
    /// Lines that mention a panic.
    pub(crate) panics: Vec<String>,
    /// Bytes read so far.
    pub(crate) bytes: u64,
    /// The newest `reconciliation.completed` line, with its time.
    pub(crate) reconciliation: Option<Line>,
    /// Messages removed by retention, per channel.
    pub(crate) purged: BTreeMap<String, u64>,
}

impl LogScan {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            ..Self::default()
        }
    }

    /// Reads what the engine logged since the last call.
    pub(crate) fn refresh(&mut self) {
        for path in log_files(&self.dir) {
            let offset = self.offsets.get(&path).copied().unwrap_or(0);
            let Some((text, end)) = read_from(&path, offset) else {
                continue;
            };
            self.offsets.insert(path, end);
            self.bytes += text.len() as u64;
            for line in text.lines() {
                self.take(line);
            }
        }
    }

    fn take(&mut self, line: &str) {
        if line.contains("panicked") && self.panics.len() < 20 {
            self.panics.push(line.chars().take(QUOTE).collect());
        }
        let Some(parsed) = parse(line) else { return };
        let event = event_of(line).unwrap_or_default();
        if event == "reconciliation.completed" {
            self.reconciliation = Some(parsed.clone());
        }
        if event == "retention.purged" {
            self.take_purge(line);
        }
        *self.counts.entry((parsed.level, event)).or_default() += 1;
    }

    fn take_purge(&mut self, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let channel = value.get("channel_id").and_then(Value::as_str);
        let messages = value.get("messages").and_then(Value::as_u64);
        if let (Some(channel), Some(messages)) = (channel, messages) {
            *self.purged.entry(channel.to_owned()).or_default() += messages;
        }
    }

    /// Lines of one level.
    pub(crate) fn level_total(&self, level: &str) -> u64 {
        self.counts
            .iter()
            .filter(|((l, _), _)| l == level)
            .map(|(_, n)| n)
            .sum()
    }
}

/// The complete lines of `path` after `offset`, and the new offset. A file that shrank is
/// read again from its start.
fn read_from(path: &Path, offset: u64) -> Option<(String, u64)> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = if len < offset { 0 } else { offset };
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let complete = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    bytes.truncate(complete);
    Some((
        String::from_utf8_lossy(&bytes).into_owned(),
        start + complete as u64,
    ))
}

/// The time and level of a log line without parsing all of it: the engine writes
/// `{"timestamp":"...","level":"...",` first.
fn quick(line: &str) -> Option<(u64, &str)> {
    let stamp = line.split_once("\"timestamp\":\"")?.1;
    let (stamp, rest) = stamp.split_once('"')?;
    let level = rest.split_once("\"level\":\"")?.1.split('"').next()?;
    Some((parse_utc_ms(stamp)?, level))
}

/// For each `(from, to)` window of Unix milliseconds, the engine log lines inside it.
pub(crate) fn excerpts(dir: &Path, windows: &[(u64, u64)]) -> Vec<Vec<Line>> {
    let mut found: Vec<Vec<Line>> = windows.iter().map(|_| Vec::new()).collect();
    if windows.is_empty() {
        return found;
    }
    for path in log_files(dir) {
        let Some((text, _)) = read_from(&path, 0) else {
            continue;
        };
        collect(&mut found, windows, &text);
    }
    found
}

fn collect(found: &mut [Vec<Line>], windows: &[(u64, u64)], text: &str) {
    for line in text.lines() {
        let Some((unix_ms, level)) = quick(line) else {
            continue;
        };
        for (slot, (from, to)) in found.iter_mut().zip(windows) {
            if (*from..=*to).contains(&unix_ms) {
                slot.push(Line {
                    unix_ms,
                    level: level.to_uppercase(),
                    text: line.chars().take(QUOTE).collect(),
                });
            }
        }
    }
}

/// Picks at most `max` lines of `lines` in time order. Warnings and errors that mention
/// `channel` come first, then other lines that mention it, then other warnings and errors,
/// then the rest.
pub(crate) fn pick(mut lines: Vec<Line>, max: usize, channel: Option<&str>) -> Vec<Line> {
    lines.sort_by_key(|l| l.unix_ms);
    let rank = |l: &Line| {
        let urgent = l.level == "ERROR" || l.level == "WARN";
        let about = channel.is_some_and(|c| l.text.contains(c));
        match (urgent, about) {
            (true, true) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (false, false) => 3,
        }
    };
    let mut ranked: Vec<(u8, Line)> = lines.into_iter().map(|l| (rank(&l), l)).collect();
    ranked.sort_by_key(|(r, l)| (*r, l.unix_ms));
    let mut chosen: Vec<Line> = ranked.into_iter().take(max).map(|(_, l)| l).collect();
    chosen.sort_by_key(|l| l.unix_ms);
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ts: &str, level: &str, event: &str) -> String {
        format!(
            r#"{{"timestamp":"{ts}","level":"{level}","message":"m","event":"{event}","target":"t"}}"#
        )
    }

    #[test]
    fn lines_are_counted_by_level_and_event() {
        let dir = tempfile::tempdir().map(|d| d.keep()).unwrap_or_default();
        let text = [
            line("2026-11-07T05:39:18.557870Z", "INFO", "channel.started"),
            line("2026-11-07T05:39:19.000000Z", "ERROR", "store.error"),
            "thread 'x' panicked at src/lib.rs:1".to_owned(),
            line(
                "2026-11-07T05:39:20.000000Z",
                "INFO",
                "reconciliation.completed",
            ),
        ]
        .join("\n");
        let _ = std::fs::write(dir.join("packie.2026-11-07.log"), format!("{text}\n"));
        let mut scan = LogScan::new(dir.clone());
        scan.refresh();
        assert_eq!(scan.level_total("INFO"), 2);
        assert_eq!(scan.level_total("ERROR"), 1);
        assert_eq!(scan.panics.len(), 1);
        assert!(scan.reconciliation.is_some());
        let before = scan.bytes;
        scan.refresh();
        assert_eq!(scan.bytes, before, "nothing is read twice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_partial_last_line_waits_for_its_newline() {
        let dir = tempfile::tempdir().map(|d| d.keep()).unwrap_or_default();
        let path = dir.join("packie.a.log");
        let full = line("2026-11-07T05:39:18.557870Z", "INFO", "a");
        let _ = std::fs::write(&path, &full[..20]);
        let mut scan = LogScan::new(dir.clone());
        scan.refresh();
        assert_eq!(scan.level_total("INFO"), 0);
        let _ = std::fs::write(&path, format!("{full}\n"));
        scan.refresh();
        assert_eq!(scan.level_total("INFO"), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn excerpts_select_by_time_and_prefer_urgent_lines() {
        let dir = tempfile::tempdir().map(|d| d.keep()).unwrap_or_default();
        let text = [
            line("2026-11-07T05:39:10.000000Z", "INFO", "early"),
            line("2026-11-07T05:39:18.000000Z", "INFO", "a"),
            line("2026-11-07T05:39:19.000000Z", "ERROR", "b"),
            line("2026-11-07T05:39:20.000000Z", "INFO", "c"),
        ]
        .join("\n");
        let _ = std::fs::write(dir.join("packie.a.log"), format!("{text}\n"));
        let from = parse_utc_ms("2026-11-07T05:39:17Z").unwrap_or(0);
        let to = parse_utc_ms("2026-11-07T05:39:21Z").unwrap_or(0);
        let found = excerpts(&dir, &[(from, to)]);
        assert_eq!(found[0].len(), 3);
        let picked = pick(found[0].clone(), 2, None);
        assert_eq!(picked.len(), 2);
        assert!(picked.iter().any(|l| l.level == "ERROR"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
