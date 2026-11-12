//! Filesystem helpers shared by backup, verify and restore: owner-only creation, durable
//! syncing, tree walking, copying, removal and hashing.

use std::fs;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{Error, Result};

/// Creates `path` and its parents readable by the owner only.
pub(crate) fn private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(Error::io(path))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(Error::io(path))?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path).map_err(Error::io(path))?;
    Ok(())
}

/// Creates (truncating) an owner-only file.
pub(crate) fn private_create(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path).map_err(Error::io(path))
}

/// Flushes a file or directory to disk where the OS allows (Unix; Windows flushes on close
/// of the writing handle only).
pub(crate) fn sync_path(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)
        .and_then(|entry| entry.sync_all())
        .map_err(Error::io(path))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Copies one file owner-only and syncs it.
pub(crate) fn copy_file(src: &Path, dest: &Path) -> Result<()> {
    let mut input = fs::File::open(src).map_err(Error::io(src))?;
    let mut output = private_create(dest)?;
    std::io::copy(&mut input, &mut output)
        .and_then(|_| output.sync_all())
        .map_err(Error::io(dest))
}

/// Everything below `root` (not `root` itself) with its type, relative to `root`; a
/// directory comes before its contents.
pub(crate) fn tree(root: &Path) -> Result<Vec<(PathBuf, fs::FileType)>> {
    let mut found = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        let absolute = root.join(&relative);
        for entry in fs::read_dir(&absolute).map_err(Error::io(&absolute))? {
            let entry = entry.map_err(Error::io(&absolute))?;
            let kind = entry.file_type().map_err(Error::io(&entry.path()))?;
            let child = relative.join(entry.file_name());
            if kind.is_dir() {
                pending.push(child.clone());
            }
            found.push((child, kind));
        }
    }
    Ok(found)
}

/// Whether `path` ends in `.extension` (exact case).
pub(crate) fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|found| found == extension)
}

/// Removes a directory tree, clearing read-only marks (sealed audit copies) where the OS
/// refuses to delete read-only files.
pub(crate) fn remove_tree(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    for entry in fs::read_dir(path)? {
        let child = entry?.path();
        if child.is_dir() {
            remove_tree(&child)?;
        } else {
            let mut permissions = fs::metadata(&child)?.permissions();
            #[allow(
                clippy::permissions_set_readonly_false,
                reason = "Windows only: the flag is the sole read-only mark there"
            )]
            permissions.set_readonly(false);
            fs::set_permissions(&child, permissions)?;
        }
    }
    fs::remove_dir_all(path)
}

/// Empties `dir` and keeps it.
pub(crate) fn remove_children(dir: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            remove_tree(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// A relative path with `/` separators on every platform.
pub(crate) fn slashed(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// `relative` as a path inside a backup, refusing absolute paths and `..`.
pub(crate) fn inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    let path = Path::new(relative);
    path.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| dir.join(path))
}

/// Size and SHA-256 (lowercase hex) of an opened file; `name` labels I/O errors.
pub(crate) fn hash_open(mut file: fs::File, name: &str) -> Result<(u64, String)> {
    let mut digest = Sha256::new();
    let bytes = std::io::copy(&mut file, &mut digest).map_err(Error::io(Path::new(name)))?;
    Ok((bytes, format!("{:x}", digest.finalize())))
}

/// Size and SHA-256 (lowercase hex) of one file.
pub(crate) fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = fs::File::open(path).map_err(Error::io(path))?;
    let mut digest = Sha256::new();
    let bytes = std::io::copy(&mut file, &mut digest).map_err(Error::io(path))?;
    Ok((bytes, format!("{:x}", digest.finalize())))
}

/// Opens `relative` under `root` for reading only when it is a regular file reached
/// through real directories: no component may be a symbolic link, and the file may not be
/// a device, FIFO or socket. The opened file is checked to be the one inspected, so a
/// component swapped for a link meanwhile is refused too. Never blocks on a FIFO.
pub(crate) fn open_regular(root: &Path, relative: &str) -> Result<fs::File> {
    let refused = || {
        Error::Invalid(format!(
            "{relative} is a symbolic link or not a regular file; a backup holds only regular files"
        ))
    };
    let path = inside(root, relative).ok_or_else(refused)?;
    let components: Vec<Component<'_>> = Path::new(relative).components().collect();
    let mut current = root.to_owned();
    let mut inspected = None;
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(Error::io(&current))?;
        let last = index + 1 == components.len();
        let kind = metadata.file_type();
        if kind.is_symlink() || (last && !kind.is_file()) || (!last && !kind.is_dir()) {
            return Err(refused());
        }
        inspected = Some(metadata);
    }
    let file = no_follow_open(&path)?;
    let opened = file.metadata().map_err(Error::io(&path))?;
    if !opened.is_file() || !same_file(inspected.as_ref(), &opened) {
        return Err(refused());
    }
    Ok(file)
}

#[cfg(unix)]
fn no_follow_open(path: &Path) -> Result<fs::File> {
    use nix::fcntl::OFlag;
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags((OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK).bits())
        .open(path)
        .map_err(Error::io(path))
}

#[cfg(not(unix))]
fn no_follow_open(path: &Path) -> Result<fs::File> {
    fs::File::open(path).map_err(Error::io(path))
}

#[cfg(unix)]
fn same_file(inspected: Option<&fs::Metadata>, opened: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    inspected.is_some_and(|i| i.dev() == opened.dev() && i.ino() == opened.ino())
}

#[cfg(not(unix))]
fn same_file(inspected: Option<&fs::Metadata>, opened: &fs::Metadata) -> bool {
    inspected.is_some_and(|i| i.len() == opened.len())
}

/// Copies a listed backup file (opened with [`open_regular`]) to `dest`, owner-only and
/// read-only when the source is, hashing while copying: a file that no longer matches its
/// manifest entry is refused and its copy removed.
pub(crate) fn copy_listed(root: &Path, entry: &crate::FileEntry, dest: &Path) -> Result<()> {
    let source = root.join(&entry.path);
    let mut input = open_regular(root, &entry.path)?;
    let read_only = input
        .metadata()
        .map_err(Error::io(&source))?
        .permissions()
        .readonly();
    let mut output = private_create(dest)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        let read = std::io::Read::read(&mut input, &mut buffer).map_err(Error::io(&source))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        std::io::Write::write_all(&mut output, &buffer[..read]).map_err(Error::io(dest))?;
        bytes += read as u64;
    }
    output.sync_all().map_err(Error::io(dest))?;
    drop(output);
    if bytes != entry.bytes || format!("{:x}", digest.finalize()) != entry.sha256 {
        let _ = fs::remove_file(dest);
        return Err(changed(&entry.path));
    }
    if read_only {
        let mut permissions = fs::metadata(dest).map_err(Error::io(dest))?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(dest, permissions).map_err(Error::io(dest))?;
    }
    Ok(())
}

fn changed(path: &str) -> Error {
    Error::Invalid(format!(
        "{path} changed after the backup was verified; nothing was restored"
    ))
}

/// The bytes of a listed backup file, refused unless they still match its entry.
pub(crate) fn read_listed(root: &Path, entry: &crate::FileEntry) -> Result<Vec<u8>> {
    let mut file = open_regular(root, &entry.path)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut bytes)
        .map_err(Error::io(&root.join(&entry.path)))?;
    let sha256 = format!("{:x}", Sha256::digest(&bytes));
    if bytes.len() as u64 != entry.bytes || sha256 != entry.sha256 {
        return Err(changed(&entry.path));
    }
    Ok(bytes)
}
