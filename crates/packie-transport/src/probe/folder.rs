//! Probes of folder endpoints.

use std::fs::{self, OpenOptions};
use std::path::Path;

use packie_config::FolderSettings;

use super::Run;
use crate::folder::{blocking, count_files};
use crate::link::{LinkError, Operation};

/// Check the path, count the files a source would take, and create and delete a hidden
/// empty file.
pub(super) async fn probe(run: &mut Run, settings: &FolderSettings) {
    let path = settings.path.clone();
    let shown = path.display().to_string();
    if run
        .step("find folder", blocking(move || find(&path)))
        .await
        .is_none()
    {
        return;
    }
    let owned = settings.clone();
    if run
        .step("read directory", blocking(move || count(&owned)))
        .await
        .is_none()
    {
        return;
    }
    let path = settings.path.clone();
    if run
        .step("write test", blocking(move || write_test(&path)))
        .await
        .is_some()
    {
        run.succeeded(format!("The folder {shown} can be read and written."));
    }
}

fn error(operation: Operation, io: &std::io::Error) -> LinkError {
    LinkError::io(operation, io)
}

fn find(path: &Path) -> Result<((), String), LinkError> {
    let meta = fs::metadata(path).map_err(|e| error(Operation::Open, &e))?;
    if !meta.is_dir() {
        return Err(LinkError::persistent(
            Operation::Open,
            format!("{} is not a directory", path.display()),
        ));
    }
    Ok(((), format!("{} exists and is a directory", path.display())))
}

fn count(settings: &FolderSettings) -> Result<((), String), LinkError> {
    let files = count_files(settings)?;
    let scope = if settings.recursive {
        ", including sub-directories"
    } else {
        ""
    };
    Ok((
        (),
        format!(
            "readable; {files} file{} match {}{scope}",
            if files == 1 { "" } else { "s" },
            settings.file_pattern
        ),
    ))
}

/// Creates a hidden, empty `.part` file (ignored by sources) exclusively and removes it.
/// Endpoints have no read-only role, so this is always checked: a folder used as a
/// destination needs it.
fn write_test(dir: &Path) -> Result<((), String), LinkError> {
    let probe = dir.join(format!(
        ".packie-probe-{}.part",
        uuid::Uuid::now_v7().simple()
    ));
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| error(Operation::Write, &e))?;
    fs::remove_file(&probe).map_err(|e| {
        let mut link = error(Operation::Write, &e);
        link.cause = format!(
            "created the test file but could not delete it: {}",
            link.cause
        );
        link
    })?;
    Ok((
        (),
        "writable (an empty hidden test file was created and deleted; the endpoint has no \
         read-only setting, so writing is always checked)"
            .to_owned(),
    ))
}
