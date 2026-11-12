//! Log files that rotate at local midnight and at a size limit, with retention by age.
//!
//! Files are `packie.YYYY-MM-DD.log`, then `packie.YYYY-MM-DD.1.log`, `.2.log` … when a file
//! would grow past the limit. After a restart writing continues in the newest file of the
//! day (append, never truncate). Files of days older than the retention are deleted when a
//! new day starts and at open.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use jiff::civil::Date;

/// A size- and date-rotating log file.
#[derive(Debug)]
pub(crate) struct RotatingFile {
    dir: PathBuf,
    max_bytes: u64,
    retention_days: u32,
    date: Date,
    index: u32,
    file: File,
    written: u64,
}

/// Today in the local time zone: names files, decides midnight rotation and anchors
/// retention, so all three agree.
fn today() -> Date {
    jiff::Zoned::now().date()
}

/// The first log file of today (local date) in `dir`, `packie.YYYY-MM-DD.log`; later files of
/// the day are `packie.YYYY-MM-DD.N.log`.
#[must_use]
pub fn first_log_file_today(dir: &Path) -> std::path::PathBuf {
    dir.join(file_name(today(), 0))
}

fn file_name(date: Date, index: u32) -> String {
    if index == 0 {
        format!("packie.{date}.log")
    } else {
        format!("packie.{date}.{index}.log")
    }
}

/// The date and index of a log file name.
fn parse_name(name: &str) -> Option<(Date, u32)> {
    let middle = name.strip_prefix("packie.")?.strip_suffix(".log")?;
    let (date, index) = match middle.split_once('.') {
        Some((date, index)) => (date, index.parse().ok()?),
        None => (middle, 0),
    };
    Some((date.parse().ok()?, index))
}

fn open_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

impl RotatingFile {
    /// Opens (or continues) today's file in `dir`.
    pub(crate) fn open(dir: &Path, max_bytes: u64, retention_days: u32) -> io::Result<Self> {
        Self::open_on(dir, max_bytes, retention_days, today())
    }

    pub(crate) fn open_on(
        dir: &Path,
        max_bytes: u64,
        retention_days: u32,
        date: Date,
    ) -> io::Result<Self> {
        let index = Self::newest_index(dir, date);
        let path = dir.join(file_name(date, index));
        let file = open_append(&path)?;
        let written = file.metadata()?.len();
        let mut rotating = Self {
            dir: dir.to_owned(),
            max_bytes: max_bytes.max(1),
            retention_days,
            date,
            index,
            file,
            written,
        };
        rotating.prune();
        Ok(rotating)
    }

    fn newest_index(dir: &Path, date: Date) -> u32 {
        fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|e| parse_name(&e.file_name().to_string_lossy()))
            .filter(|(d, _)| *d == date)
            .map(|(_, index)| index)
            .max()
            .unwrap_or(0)
    }

    fn switch(&mut self, date: Date, index: u32) -> io::Result<()> {
        let file = open_append(&self.dir.join(file_name(date, index)))?;
        self.written = file.metadata()?.len();
        self.file = file;
        self.date = date;
        self.index = index;
        Ok(())
    }

    /// Deletes files of days before the retention window.
    fn prune(&mut self) {
        let Ok(span) = jiff::Span::new().try_days(i64::from(self.retention_days)) else {
            return;
        };
        let Ok(oldest) = self.date.checked_sub(span) else {
            return;
        };
        for entry in fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
        {
            let expired =
                parse_name(&entry.file_name().to_string_lossy()).is_some_and(|(d, _)| d < oldest);
            if expired {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    /// Writes `buf`, rotating first when the day changed or the file would exceed the
    /// limit (a non-empty file only, so one oversized line still gets written).
    pub(crate) fn write_on(&mut self, buf: &[u8], date: Date) -> io::Result<usize> {
        if date != self.date {
            self.switch(date, Self::newest_index(&self.dir, date))?;
            self.prune();
        }
        let len = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if self.written > 0 && self.written.saturating_add(len) > self.max_bytes {
            self.switch(self.date, self.index + 1)?;
        }
        self.file.write_all(buf)?;
        self.written = self.written.saturating_add(len);
        Ok(buf.len())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_on(buf, today())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn rotates_at_size_and_day_and_continues_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let day: Date = "2026-11-06".parse().unwrap();
        let mut file = RotatingFile::open_on(dir.path(), 10, 30, day).unwrap();
        file.write_on(b"123456", day).unwrap();
        file.write_on(b"789", day).unwrap();
        file.write_on(b"abcdef", day).unwrap();
        assert_eq!(
            names(dir.path()),
            ["packie.2026-11-06.1.log", "packie.2026-11-06.log"]
        );
        assert_eq!(
            fs::read(dir.path().join("packie.2026-11-06.log")).unwrap(),
            b"123456789"
        );

        // A restart continues the newest file of the day.
        drop(file);
        let mut again = RotatingFile::open_on(dir.path(), 10, 30, day).unwrap();
        again.write_on(b"gh", day).unwrap();
        assert_eq!(
            fs::read(dir.path().join("packie.2026-11-06.1.log")).unwrap(),
            b"abcdefgh"
        );

        let next: Date = "2026-11-07".parse().unwrap();
        again.write_on(b"x", next).unwrap();
        assert!(dir.path().join("packie.2026-11-07.log").exists());
    }

    #[test]
    fn deletes_days_past_retention() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "packie.2026-10-01.log",
            "packie.2026-10-01.3.log",
            "packie.2026-11-01.log",
            "other.log",
        ] {
            fs::write(dir.path().join(name), "x").unwrap();
        }
        let day: Date = "2026-11-06".parse().unwrap();
        let _file = RotatingFile::open_on(dir.path(), 1000, 30, day).unwrap();
        assert_eq!(
            names(dir.path()),
            [
                "other.log",
                "packie.2026-11-01.log",
                "packie.2026-11-06.log"
            ]
        );
    }

    #[test]
    fn names_parse() {
        let day: Date = "2026-11-06".parse().unwrap();
        assert_eq!(parse_name("packie.2026-11-06.log"), Some((day, 0)));
        assert_eq!(parse_name("packie.2026-11-06.12.log"), Some((day, 12)));
        assert_eq!(parse_name("packie.log"), None);
    }
}
