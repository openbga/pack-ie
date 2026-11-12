//! Operating system probes of the engine process: open descriptors and disk use. Resident
//! memory and CPU time come from the benchmark module.

use std::path::Path;

/// Open descriptors of a process, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Fds {
    pub(crate) total: u64,
    pub(crate) sockets: u64,
    pub(crate) files: u64,
}

/// The memory `pid` is responsible for, in KiB.
///
/// Resident size is the wrong figure on macOS: it counts pages the allocator freed but the
/// system has not reclaimed, so it follows the largest transient burst and never goes down.
/// The footprint (`phys_footprint`, the figure Activity Monitor shows) counts only dirty,
/// compressed and swapped pages. It comes from `footprint(1)`, because reading it directly
/// needs `proc_pid_rusage`, which needs `unsafe` code. On Linux the anonymous resident
/// memory (`RssAnon`) plays the same part. `None` where neither exists; callers then use
/// the resident size.
pub(crate) fn footprint_kib(pid: u32) -> Option<u64> {
    if let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) {
        let line = status.lines().find(|l| l.starts_with("RssAnon:"))?;
        return line.split_whitespace().nth(1)?.parse().ok();
    }
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let file = std::env::temp_dir().join(format!(
        "packie-soak-footprint-{}-{}.json",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let status = std::process::Command::new("footprint")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-j")
        .arg(&file)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    let text = std::fs::read_to_string(&file).ok();
    let _ = std::fs::remove_file(&file);
    status.success().then_some(())?;
    parse_footprint(&text?)
}

/// The footprint in KiB from the JSON `footprint -j` writes.
pub(crate) fn parse_footprint(json: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let unit = value
        .get("bytes per unit")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1);
    let bytes = value
        .get("processes")?
        .get(0)?
        .get("footprint")?
        .as_u64()?
        .checked_mul(unit)?;
    Some(bytes / 1024)
}

/// Counts the descriptors of `pid`: `/proc/<pid>/fd` where it exists, `lsof` elsewhere.
/// Unavailable on Windows (`None`); handles are not Unix file descriptors.
pub(crate) fn open_fds(pid: u32) -> Option<Fds> {
    if cfg!(windows) {
        return None;
    }
    if let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
        return Some(from_proc(entries));
    }
    let output = std::process::Command::new("lsof")
        .args(["-n", "-P", "-w", "-F", "ft", "-p", &pid.to_string()])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| parse_lsof(&String::from_utf8_lossy(&output.stdout)))
}

fn from_proc(entries: std::fs::ReadDir) -> Fds {
    let mut fds = Fds::default();
    for entry in entries.flatten() {
        fds.total += 1;
        let target = std::fs::read_link(entry.path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if target.starts_with("socket:") {
            fds.sockets += 1;
        } else if target.starts_with('/') {
            fds.files += 1;
        }
    }
    fds
}

/// Parses `lsof -F ft` output: a `p<pid>` line, then for each descriptor an `f<fd>` line
/// and a `t<type>` line. Only numbered descriptors count (not `cwd`, `txt`, `mem`).
pub(crate) fn parse_lsof(output: &str) -> Fds {
    let mut fds = Fds::default();
    let mut numbered = false;
    for line in output.lines() {
        let Some((tag, rest)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "f" => {
                numbered = !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit());
                fds.total += u64::from(numbered);
            }
            "t" if numbered => match rest {
                "IPv4" | "IPv6" | "unix" => fds.sockets += 1,
                "REG" | "VREG" | "DIR" | "VDIR" => fds.files += 1,
                _ => {}
            },
            _ => {}
        }
    }
    fds
}

/// The total size in bytes of the files under `path`.
pub(crate) fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(m) if m.is_dir() => dir_size(&entry.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

/// The number of files directly in `path` whose name starts with `prefix`.
pub(crate) fn count_files(path: &Path, prefix: &str) -> u64 {
    std::fs::read_dir(path).map_or(0, |entries| {
        entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(prefix))
            .count() as u64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_output_is_counted_by_kind() {
        let output = "p123\nfcwd\ntDIR\nftxt\ntREG\nf0\ntCHR\nf3\ntIPv4\nf4\ntIPv6\nf5\ntREG\nf6\ntunix\nf7\ntPIPE\n";
        assert_eq!(
            parse_lsof(output),
            Fds {
                total: 6,
                sockets: 3,
                files: 1
            }
        );
    }

    #[test]
    fn the_footprint_is_read_from_footprint_json() {
        let json = r#"{"unit":"byte","bytes per unit":1,"processes":[{"name":"packie","pid":7,"footprint":11240000}]}"#;
        assert_eq!(parse_footprint(json), Some(10_976));
        let pages = r#"{"bytes per unit":1024,"processes":[{"footprint":2048}]}"#;
        assert_eq!(parse_footprint(pages), Some(2_048));
        assert_eq!(parse_footprint(r#"{"processes":[]}"#), None);
        assert_eq!(parse_footprint("not json"), None);
    }

    #[test]
    fn this_process_has_a_footprint_where_the_system_reports_one() {
        // macOS and Linux have one; elsewhere the caller falls back to the resident size.
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(footprint_kib(std::process::id()).is_some_and(|kib| kib > 100));
        }
    }

    #[test]
    fn this_process_has_descriptors() {
        let fds = open_fds(std::process::id());
        if cfg!(windows) {
            assert_eq!(fds, None);
        } else {
            assert!(fds.is_some_and(|f| f.total >= 3), "{fds:?}");
        }
    }

    #[test]
    fn directories_are_summed_recursively() {
        let dir = tempfile::tempdir().map(|d| d.keep()).unwrap_or_default();
        let _ = std::fs::create_dir_all(dir.join("a/b"));
        let _ = std::fs::write(dir.join("one"), [0_u8; 10]);
        let _ = std::fs::write(dir.join("a/b/two"), [0_u8; 32]);
        assert_eq!(dir_size(&dir), 42);
        assert_eq!(count_files(&dir, "on"), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
