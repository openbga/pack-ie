//! The folder sink: files written without ever following links or sharing a temporary.

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::Path;

use packie_config::{FolderSettings, OnExists, WriteMode};

use super::blocking;
use crate::files::{FileSink, Stored, no_free_output_name, output_exists, suffix_candidates};
use crate::link::{BoxFuture, LinkError, Operation};

/// A writer of files into the folder.
pub(super) struct FolderSink {
    pub(super) settings: FolderSettings,
}

impl FileSink for FolderSink {
    fn write<'a>(
        &'a self,
        name: &'a str,
        bytes: &'a [u8],
        on_exists: OnExists,
    ) -> BoxFuture<'a, Result<String, LinkError>> {
        let dir = self.settings.path.clone();
        let mode = self.settings.write_mode;
        let name = name.to_owned();
        let bytes = bytes.to_vec();
        Box::pin(blocking(move || {
            write_file(&dir, &name, &bytes, on_exists, mode)
        }))
    }
}

/// Why a file could not be written.
#[derive(Debug, thiserror::Error)]
enum WriteError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("the target is a symbolic link")]
    Symlink,
}

impl From<WriteError> for LinkError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::Io(error) => Self::io(Operation::Write, &error),
            WriteError::Symlink => Self::persistent(
                Operation::Write,
                "the target is a symbolic link; files are not written through links",
            ),
        }
    }
}

fn write_file(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    on_exists: OnExists,
    mode: WriteMode,
) -> Result<String, LinkError> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(LinkError::persistent(
            Operation::Write,
            "the output file name must be a plain name without directories",
        ));
    }
    let publish = |name: &str, replace: bool| {
        publish(dir, name, bytes, mode, replace).map_err(LinkError::from)
    };
    match on_exists {
        OnExists::Overwrite => publish(name, true).map(|_| name.to_owned()),
        OnExists::Fail => match publish(name, false)? {
            Stored::Done => Ok(name.to_owned()),
            Stored::Taken => Err(output_exists()),
        },
        OnExists::Suffix => {
            for candidate in suffix_candidates(name) {
                if let Stored::Done = publish(&candidate, false)? {
                    return Ok(candidate);
                }
            }
            Err(no_free_output_name())
        }
        // Appending is always a direct write to the existing file (a rename would replace
        // it), also in `atomic_rename` mode; only the first write creates it.
        OnExists::Append => {
            for _ in 0..3 {
                if append_existing(&dir.join(name), bytes)? {
                    return Ok(name.to_owned());
                }
                if let Stored::Done = publish(name, false)? {
                    return Ok(name.to_owned());
                }
            }
            Err(LinkError::new(
                Operation::Write,
                "the output file kept changing while it was appended to",
            ))
        }
    }
}

/// Writes `name` per `mode`. Without `replace` an existing entry of that name (a file or a
/// link) is never touched. With it, `atomic_rename` swaps the name for the new file (a link
/// is replaced, not followed), while `direct` rewrites the file in place and refuses links.
fn publish(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    mode: WriteMode,
    replace: bool,
) -> Result<Stored, WriteError> {
    let target = dir.join(name);
    match (mode, replace) {
        (WriteMode::AtomicRename, _) => publish_via_temp(dir, &target, bytes, replace),
        (WriteMode::Direct, true) => {
            let mut options = OpenOptions::new();
            options.write(true).create(true).truncate(true);
            let mut file = open_without_links(&mut options, &target)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            Ok(Stored::Done)
        }
        (WriteMode::Direct, false) => create_exclusive(&target, bytes),
    }
}

/// Creates `target` exclusively (an existing file or link of that name is `Taken`, and
/// never followed) and writes it; a failed write removes the file this call created.
fn create_exclusive(target: &Path, bytes: &[u8]) -> Result<Stored, WriteError> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(target) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Ok(Stored::Taken),
        Err(e) => return Err(e.into()),
    };
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(error) = written {
        drop(file);
        let _ = std::fs::remove_file(target);
        return Err(error.into());
    }
    Ok(Stored::Done)
}

/// Writes a uniquely named, exclusively created temporary file in `dir`, then moves it to
/// `target`. The temporary is removed when anything fails; no other file is ever opened or
/// truncated. Without `replace` the move fails if the name exists (`persist_noclobber`:
/// `renameat2(RENAME_NOREPLACE)` / `renamex_np(RENAME_EXCL)` where available, otherwise a
/// hard link followed by unlink, or `MoveFileEx` without replacement on Windows).
fn publish_via_temp(
    dir: &Path,
    target: &Path,
    bytes: &[u8],
    replace: bool,
) -> Result<Stored, WriteError> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(".").suffix(".part").rand_bytes(12);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        // Like `File::create`: the umask decides, not the owner-only default of temp files.
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    let mut temp = builder.tempfile_in(dir)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    if replace {
        // Windows refuses a replace for a moment while another writer replaces the same name.
        packie_config::persist_replacing(temp, target)?;
        return Ok(Stored::Done);
    }
    match temp.persist_noclobber(target) {
        Ok(_) => Ok(Stored::Done),
        Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => Ok(Stored::Taken),
        Err(e) => Err(e.error.into()),
    }
}

/// Appends to `target`; `false` when it does not exist. Links are refused.
fn append_existing(target: &Path, bytes: &[u8]) -> Result<bool, WriteError> {
    let mut options = OpenOptions::new();
    options.append(true);
    match open_without_links(&mut options, target) {
        Ok(mut file) => {
            file.write_all(bytes)?;
            file.sync_all()?;
            Ok(true)
        }
        Err(WriteError::Io(e)) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Opens `target` with `options`, refusing a symbolic link. On Unix the open itself does
/// not follow links (`O_NOFOLLOW`), so nothing can be swapped in between a check and the
/// open; elsewhere the link is checked just before opening.
fn open_without_links(
    options: &mut OpenOptions,
    target: &Path,
) -> Result<std::fs::File, WriteError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits());
    }
    #[cfg(not(unix))]
    if is_link(target) {
        return Err(WriteError::Symlink);
    }
    options.open(target).map_err(|error| {
        if is_link(target) {
            WriteError::Symlink
        } else {
            WriteError::Io(error)
        }
    })
}

fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}
