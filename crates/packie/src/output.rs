//! Terminal output: lines to stdout and stderr, plain text tables and the one way times,
//! durations and sizes are written.

use std::io::{ErrorKind, Write};

use packie_core::Timestamp;
use serde::Serialize;

use crate::Failure;

/// Writes one line to stdout. A closed pipe (`packie logs | head`) ends the command quietly.
pub(crate) fn print_line(line: &str) {
    if let Err(error) = writeln!(std::io::stdout().lock(), "{line}")
        && error.kind() == ErrorKind::BrokenPipe
    {
        std::process::exit(0);
    }
}

/// Writes one line to stderr.
pub(crate) fn eprint_line(line: &str) {
    let _ = writeln!(std::io::stderr().lock(), "{line}");
}

/// Prints `value` as pretty JSON.
pub(crate) fn print_json<T: Serialize>(value: &T) -> Result<(), Failure> {
    let text = serde_json::to_string_pretty(value).map_err(|e| Failure::new(e.to_string()))?;
    print_line(&text);
    Ok(())
}

/// Prints every line of `lines`.
pub(crate) fn print_lines(lines: &[String]) {
    for line in lines {
        print_line(line);
    }
}

/// Prints rows as left-aligned columns under `headers`, two spaces apart.
pub(crate) fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    print_lines(&table(headers, rows));
}

/// Rows as left-aligned columns under `headers`, two spaces apart, one line each.
pub(crate) fn table(headers: &[&str], rows: &[Vec<String>]) -> Vec<String> {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.chars().count()).collect();
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |cells: &mut dyn Iterator<Item = &str>| {
        let padded: Vec<String> = cells
            .zip(&widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        padded.join("  ").trim_end().to_owned()
    };
    let mut lines = vec![line(&mut headers.iter().copied())];
    lines.extend(
        rows.iter()
            .map(|row| line(&mut row.iter().map(String::as_str))),
    );
    lines
}

/// `1d 2h`, `3h 4m`, `5m 6s` or `7s`.
pub(crate) fn duration(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    let (days, hours, minutes, seconds) =
        (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    match (days, hours, minutes) {
        (0, 0, 0) => format!("{seconds}s"),
        (0, 0, _) => format!("{minutes}m {seconds}s"),
        (0, _, _) => format!("{hours}h {minutes}m"),
        _ => format!("{days}d {hours}h"),
    }
}

/// `2026-11-06 12:00:00Z`: the one time format of tables, details and log lines (UTC,
/// seconds precision).
pub(crate) fn short_time(at: Timestamp) -> String {
    at.as_jiff().strftime("%Y-%m-%d %H:%M:%SZ").to_string()
}

/// [`short_time`], or `-` when there is no time.
pub(crate) fn opt_time(at: Option<Timestamp>) -> String {
    at.map_or_else(|| "-".to_owned(), short_time)
}

/// The text, or `-` when there is none.
pub(crate) fn opt_text(text: Option<&str>) -> String {
    text.map_or_else(|| "-".to_owned(), str::to_owned)
}

/// The serialized name of a `snake_case` enum value.
pub(crate) fn json_text<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(text)) => text,
        _ => String::new(),
    }
}

/// `1 message`, `2 messages`.
pub(crate) fn count(n: u64, singular: &str, plural: &str) -> String {
    format!("{n} {}", if n == 1 { singular } else { plural })
}

/// Creates the export file `path`, which must not exist yet, readable by its owner only:
/// exports and reports can hold message content and identifiers. An existing file is
/// refused rather than overwritten.
pub(crate) fn create_export_file(path: &std::path::Path) -> Result<std::fs::File, Failure> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|e| {
        if e.kind() == ErrorKind::AlreadyExists {
            Failure::new(format!(
                "{} already exists; choose a new file name (an export never overwrites).",
                path.display()
            ))
        } else {
            Failure::new(format!("cannot create {}: {e}", path.display()))
        }
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use packie_core::Timestamp;

    use super::{count, duration, opt_time, short_time, table};

    #[test]
    fn durations_use_the_two_largest_units() {
        assert_eq!(duration(Duration::from_secs(7)), "7s");
        assert_eq!(duration(Duration::from_secs(306)), "5m 6s");
        assert_eq!(duration(Duration::from_mins(184)), "3h 4m");
        assert_eq!(duration(Duration::from_hours(26)), "1d 2h");
    }

    #[test]
    fn tables_align_columns_and_trim_line_ends() {
        let rows = vec![
            vec!["a".to_owned(), "long cell".to_owned(), String::new()],
            vec!["bbb".to_owned(), "x".to_owned(), "end".to_owned()],
        ];
        assert_eq!(
            table(&["ID", "NAME", "NOTE"], &rows),
            [
                "ID   NAME       NOTE",
                "a    long cell",
                "bbb  x          end"
            ]
        );
    }

    #[test]
    fn times_have_seconds_precision_in_utc() {
        let at = Timestamp::parse("2026-11-06T14:03:07.123456789+02:00").unwrap();
        assert_eq!(short_time(at), "2026-11-06 12:03:07Z");
        assert_eq!(opt_time(Some(at)), "2026-11-06 12:03:07Z");
        assert_eq!(opt_time(None), "-");
    }

    #[test]
    fn counts_pluralize() {
        assert_eq!(count(1, "message", "messages"), "1 message");
        assert_eq!(count(0, "delivery", "deliveries"), "0 deliveries");
    }
}
