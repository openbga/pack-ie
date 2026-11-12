//! Verifying a backup without changing it.

use std::collections::BTreeSet;
use std::path::Path;

use packie_ipc::audit::AuditVerification;
use rusqlite::{Connection, OpenFlags};

use crate::fsutil;
use crate::manifest::{self, MANIFEST, Manifest};
use crate::{AUDIT, Result, STORE};

/// What [`verify`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verification {
    /// No problem found.
    pub ok: bool,
    /// The manifest, when it could be read.
    pub manifest: Option<Manifest>,
    /// Files whose size and hash were checked.
    pub files_checked: u32,
    /// Every problem, for the operator.
    pub problems: Vec<String>,
    /// The audit chain of the copy, when it has one.
    pub audit: Option<AuditVerification>,
}

/// Checks a backup directory: the manifest is present and readable; it holds only
/// directories and regular files (a symbolic link, device, FIFO or socket anywhere is a
/// problem, named by its path); every listed file is reached without following links and
/// has its size and SHA-256, and no unlisted file is present; the store copy passes
/// `PRAGMA integrity_check` and every body file it references is listed; the audit copy's
/// chain verifies and ends at the head the manifest recorded. Opens databases read-only.
///
/// # Errors
///
/// Only when the backup directory itself cannot be walked or the audit copy cannot be read;
/// every finding about the backup's content is a problem in the [`Verification`].
pub fn verify(dir: &Path) -> Result<Verification> {
    let manifest = match Manifest::read(dir) {
        Ok(manifest) => manifest,
        Err(error) => {
            return Ok(Verification {
                ok: false,
                manifest: None,
                files_checked: 0,
                problems: vec![format!(
                    "{MANIFEST} cannot be read ({error}); the backup is incomplete or not a backup"
                )],
                audit: None,
            });
        }
    };
    let mut problems = Vec::new();
    let (files_checked, listed) = check_files(dir, &manifest, &mut problems)?;
    if manifest.store.is_some() {
        check_store(&dir.join(STORE), &listed, &mut problems);
    }
    let audit = if manifest.audit.is_some() {
        Some(check_audit(&dir.join(AUDIT), &manifest, &mut problems)?)
    } else {
        None
    };
    Ok(Verification {
        ok: problems.is_empty(),
        manifest: Some(manifest),
        files_checked,
        problems,
        audit,
    })
}

/// Hashes every listed file and reports unlisted ones; returns how many files matched and
/// the listed paths.
fn check_files(
    dir: &Path,
    manifest: &Manifest,
    problems: &mut Vec<String>,
) -> Result<(u32, BTreeSet<String>)> {
    let mut checked = 0u32;
    let mut listed = BTreeSet::new();
    for entry in &manifest.files {
        listed.insert(entry.path.clone());
        if fsutil::inside(dir, &entry.path).is_none() {
            problems.push(format!("{} is not a path inside the backup", entry.path));
            continue;
        }
        match fsutil::open_regular(dir, &entry.path)
            .and_then(|file| fsutil::hash_open(file, &entry.path))
        {
            Ok((bytes, sha256)) if bytes == entry.bytes && sha256 == entry.sha256 => checked += 1,
            Ok(_) => problems.push(format!("{} changed since the backup was taken", entry.path)),
            Err(crate::Error::Invalid(reason)) => problems.push(reason),
            Err(_) => problems.push(format!("{} is missing or unreadable", entry.path)),
        }
    }
    for (path, kind) in fsutil::tree(dir)? {
        if !kind.is_dir() && !kind.is_file() {
            problems.push(format!(
                "{} is a symbolic link or special file; a backup holds only regular files",
                fsutil::slashed(&path)
            ));
        }
    }
    for extra in manifest::walk(dir)?
        .into_iter()
        .filter(|path| !listed.contains(path))
    {
        problems.push(format!("{extra} is not listed in the manifest"));
    }
    Ok((checked, listed))
}

/// The integrity check result and the body files the database references.
fn read_store(database: &Path) -> rusqlite::Result<(String, Vec<String>)> {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    let bodies = connection
        .prepare("SELECT DISTINCT file_path FROM body_contents WHERE file_path IS NOT NULL")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok((integrity, bodies))
}

fn check_store(store: &Path, listed: &BTreeSet<String>, problems: &mut Vec<String>) {
    match read_store(&store.join(packie_store::DATABASE_FILE)) {
        Ok((integrity, bodies)) => {
            if integrity != "ok" {
                problems.push(format!(
                    "the store copy fails its integrity check: {integrity}"
                ));
            }
            let missing = bodies
                .iter()
                .filter(|body| !listed.contains(&format!("{STORE}/bodies/{body}")))
                .count();
            if missing > 0 {
                problems.push(format!(
                    "{missing} body files the store references are not in the backup"
                ));
            }
        }
        Err(error) => problems.push(format!("the store copy cannot be read: {error}")),
    }
}

fn check_audit(
    audit: &Path,
    manifest: &Manifest,
    problems: &mut Vec<String>,
) -> Result<AuditVerification> {
    let report = packie_audit::verify_dir(audit)?;
    if !report.ok {
        problems.extend(
            report
                .problems
                .iter()
                .map(|p| format!("audit {}: {}", p.file, p.message)),
        );
    }
    let recorded = manifest.audit.as_ref().and_then(|a| a.head_seq);
    if report.last_seq != recorded {
        let copy = report.last_seq.map_or_else(
            || "holds no events".to_owned(),
            |seq| format!("ends at event {seq}"),
        );
        let expected = recorded.map_or_else(|| "none".to_owned(), |seq| format!("event {seq}"));
        problems.push(format!(
            "the audit copy {copy}; the manifest recorded {expected} as the last"
        ));
    }
    Ok(report)
}
