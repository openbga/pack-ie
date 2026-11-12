//! Saving files from the window: an audit export, a message export and a report the window
//! built itself. Each asks where to save in the native save dialog, then writes the file. The
//! window never names the path (it cannot write elsewhere): it asks and learns where it went.
//! Exports stream the engine's chunks to the file as they arrive.

use std::fs::OpenOptions;
use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt as _;
use tokio::sync::oneshot;
use packie_ipc::audit::{AuditExportFormat, AuditExportRequest, AuditFilter};
use packie_ipc::messages::MessageFilter;
use packie_ipc::reports::{ExportFormat, ExportRequest};
use packie_ipc::{Request, Response};

use crate::connection::Hub;
use crate::error::CommandError;

/// Where an export went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Exported {
    pub(crate) path: String,
    pub(crate) rows: u64,
}

impl From<Written> for Exported {
    fn from(written: Written) -> Self {
        Self {
            path: written.path.to_string_lossy().into_owned(),
            rows: written.rows,
        }
    }
}

/// Where a report went.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Saved {
    pub(crate) path: String,
}

/// How many numbered variants (`name-1.csv` .. `name-99.csv`) are tried when the chosen name is
/// taken, before giving up.
const MAX_VARIANTS: u32 = 99;

/// Creates a file that did not exist, readable by its owner only from the first byte
/// (`O_EXCL`, mode 0600 at creation, not a chmod afterwards). Exports can hold message content
/// and identifiers. The exclusive create refuses anything already at the path: a file is never
/// truncated and a symbolic link, dangling or not, is never followed.
fn open_new(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

/// `name-3.csv` for `name.csv`.
fn variant(path: &Path, number: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let name = match path.extension() {
        Some(extension) => format!("{stem}-{number}.{}", extension.to_string_lossy()),
        None => format!("{stem}-{number}"),
    };
    path.with_file_name(name)
}

/// Creates the file for an export at `path`, or, when something is already there (the dialog
/// let the operator pick an existing name), at the first free numbered variant of it. Returns
/// the file and where it really is.
fn create_new(path: &Path) -> std::io::Result<(std::fs::File, PathBuf)> {
    let mut candidate = path.to_path_buf();
    for number in 1..=MAX_VARIANTS + 1 {
        match open_new(&candidate) {
            Ok(file) => return Ok((file, candidate)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                candidate = variant(path, number);
            }
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("{} and its numbered variants already exist", path.display()),
    ))
}

/// What a finished export wrote and where: the chosen path, or the numbered variant used
/// because that name was taken.
#[derive(Debug)]
pub(crate) struct Written {
    pub(crate) path: PathBuf,
    pub(crate) rows: u64,
}

/// One chunk of an export, whatever the engine calls its fields: the text to append, the
/// entries it holds and where the next chunk continues (`None` after the last).
struct Piece<N> {
    content: String,
    rows: u64,
    next: Option<N>,
    /// The engine's id for this export: the first chunk's, passed back with every later one
    /// (only the first chunk is audited, so a continuation must name an audited export).
    export_id: Option<String>,
}

fn unexpected_reply() -> CommandError {
    CommandError::internal("the engine sent an unexpected reply")
}

/// Writes every piece `fetch` returns, in order, until one has no continuation; `fetch` gets
/// where to continue and the export id (both `None` for the first). A failure removes the partial
/// file (which this call created, exclusively), so a half export is never mistaken for a whole
/// one.
async fn write_pieces<N, F, Fut>(path: &Path, mut fetch: F) -> Result<Written, CommandError>
where
    F: FnMut(Option<N>, Option<String>) -> Fut,
    Fut: Future<Output = Result<Piece<N>, CommandError>>,
{
    let (mut file, path) = create_new(path).map_err(|error| {
        CommandError::internal(format!("cannot create {}: {error}", path.display()))
    })?;
    let io = |error: std::io::Error| {
        CommandError::internal(format!("cannot write {}: {error}", path.display()))
    };
    let mut rows = 0_u64;
    let mut cursor = None;
    let mut export_id = None;
    let outcome = loop {
        let piece = match fetch(cursor.take(), export_id.take()).await {
            Ok(piece) => piece,
            Err(error) => break Err(error),
        };
        if let Err(error) = file.write_all(piece.content.as_bytes()) {
            break Err(io(error));
        }
        rows += piece.rows;
        export_id = piece.export_id;
        match piece.next {
            Some(next) => cursor = Some(next),
            None => break Ok(rows),
        }
    };
    let outcome = match outcome {
        Ok(rows) => file.sync_all().map(|()| rows).map_err(io),
        Err(error) => Err(error),
    };
    if outcome.is_err() {
        drop(file);
        let _ = std::fs::remove_file(&path);
    }
    outcome.map(|rows| Written { path, rows })
}

/// Writes every audit chunk `fetch` returns, oldest first, until the engine says it is
/// complete.
pub(crate) async fn write_export<F, Fut>(
    path: &Path,
    request: AuditExportRequest,
    mut fetch: F,
) -> Result<Written, CommandError>
where
    F: FnMut(AuditExportRequest) -> Fut,
    Fut: Future<Output = Result<Response, CommandError>>,
{
    write_pieces(path, |after_seq, export_id| {
        let reply = fetch(AuditExportRequest {
            after_seq,
            export_id,
            ..request.clone()
        });
        async move {
            match reply.await? {
                Response::AuditExport(chunk) => Ok(Piece {
                    content: chunk.content,
                    rows: u64::from(chunk.rows),
                    next: chunk.next_after_seq,
                    export_id: chunk.export_id,
                }),
                _ => Err(unexpected_reply()),
            }
        }
    })
    .await
}

/// Writes every chunk of a message export `fetch` returns, oldest message first, until the
/// engine has no more. Counts the messages written.
pub(crate) async fn write_message_export<F, Fut>(
    path: &Path,
    request: ExportRequest,
    mut fetch: F,
) -> Result<Written, CommandError>
where
    F: FnMut(ExportRequest) -> Fut,
    Fut: Future<Output = Result<Response, CommandError>>,
{
    write_pieces(path, |offset, export_id| {
        let reply = fetch(ExportRequest {
            offset: offset.unwrap_or(0),
            export_id,
            ..request.clone()
        });
        async move {
            match reply.await? {
                Response::MessageExport(chunk) => Ok(Piece {
                    content: chunk.content,
                    rows: u64::from(chunk.count),
                    next: chunk.next_offset,
                    export_id: chunk.export_id,
                }),
                _ => Err(unexpected_reply()),
            }
        }
    })
    .await
}

/// The extension the dialog suggests for a format.
fn extension(format: AuditExportFormat) -> &'static str {
    match format {
        AuditExportFormat::Csv => "csv",
        AuditExportFormat::JsonLines => "jsonl",
    }
}

/// The extension the dialog suggests for a message export.
fn message_extension(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Csv => "csv",
        ExportFormat::Jsonl => "jsonl",
    }
}

/// Asks where to save with the native dialog. `None` when the operator cancelled it.
async fn ask_where(
    app: &AppHandle,
    title: &str,
    suggested_name: String,
    kind: &str,
    extension: &str,
) -> Result<Option<PathBuf>, CommandError> {
    let (send, receive) = oneshot::channel();
    app.dialog()
        .file()
        .set_title(title)
        .set_file_name(suggested_name)
        .add_filter(kind, &[extension])
        .save_file(move |chosen| {
            let _ = send.send(chosen);
        });
    let Ok(Some(chosen)) = receive.await else {
        return Ok(None);
    };
    chosen
        .into_path()
        .map(Some)
        .map_err(|error| CommandError::internal(format!("cannot use the chosen file: {error}")))
}

/// Asks where to save, then exports the matching audit events there. `None` when the operator
/// cancelled the dialog.
#[tauri::command]
pub(crate) async fn save_audit_export(
    app: AppHandle,
    hub: State<'_, Hub>,
    suggested_name: String,
    format: AuditExportFormat,
    filter: AuditFilter,
) -> Result<Option<Exported>, CommandError> {
    let Some(path) = ask_where(
        &app,
        "Save audit export",
        suggested_name,
        "Audit export",
        extension(format),
    )
    .await?
    else {
        return Ok(None);
    };
    let request = AuditExportRequest {
        filter,
        format,
        after_seq: None,
        export_id: None,
    };
    let written = write_export(&path, request, |request| {
        hub.request(Request::ExportAudit(request))
    })
    .await?;
    Ok(Some(Exported::from(written)))
}

/// Asks where to save, then exports the messages the filter selects there, with their bodies
/// only when asked. `None` when the operator cancelled the dialog. The engine audits the
/// export when it serves the first chunk.
#[tauri::command]
pub(crate) async fn save_message_export(
    app: AppHandle,
    hub: State<'_, Hub>,
    suggested_name: String,
    format: ExportFormat,
    filter: MessageFilter,
    include_bodies: bool,
) -> Result<Option<Exported>, CommandError> {
    let Some(path) = ask_where(
        &app,
        "Save message export",
        suggested_name,
        "Message export",
        message_extension(format),
    )
    .await?
    else {
        return Ok(None);
    };
    let request = ExportRequest {
        filter,
        format,
        include_bodies,
        offset: 0,
        export_id: None,
    };
    let written = write_message_export(&path, request, |request| {
        hub.request(Request::ExportMessages(request))
    })
    .await?;
    Ok(Some(Exported::from(written)))
}

/// The formats of a report the window builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReportFormat {
    Csv,
    Json,
}

impl ReportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

/// Writes `content` to a new owner-only file at `path` (or its first free numbered variant) and
/// returns where it went; a failure leaves no file.
fn write_report(path: &Path, content: &str) -> Result<PathBuf, CommandError> {
    let (mut file, path) = create_new(path).map_err(|error| {
        CommandError::internal(format!("cannot create {}: {error}", path.display()))
    })?;
    let written = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(error) = written {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(CommandError::internal(format!(
            "cannot write {}: {error}",
            path.display()
        )));
    }
    Ok(path)
}

/// Asks where to save, then writes a report the window built (the change report, a
/// transmission CSV). `None` when the operator cancelled the dialog.
#[tauri::command]
pub(crate) async fn save_report_file(
    app: AppHandle,
    suggested_name: String,
    extension: ReportFormat,
    content: String,
) -> Result<Option<Saved>, CommandError> {
    let Some(path) = ask_where(
        &app,
        "Save report",
        suggested_name,
        "Report",
        extension.extension(),
    )
    .await?
    else {
        return Ok(None);
    };
    let path = write_report(&path, &content)?;
    Ok(Some(Saved {
        path: path.to_string_lossy().into_owned(),
    }))
}

#[cfg(test)]
mod tests {
    use packie_ipc::audit::AuditExportChunk;
    use packie_ipc::reports::ExportChunk;

    use super::*;

    fn chunk(content: &str, rows: u32, next: Option<u64>) -> Response {
        Response::AuditExport(AuditExportChunk {
            format: AuditExportFormat::Csv,
            content: content.into(),
            rows,
            next_after_seq: next,
            export_id: next.map(|_| "export-1".into()),
        })
    }

    #[tokio::test]
    async fn writes_every_chunk_in_order_and_counts_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.csv");
        let mut replies = vec![
            Ok(chunk("a\n", 3, Some(3))),
            Ok(chunk("b\n", 2, Some(7))),
            Ok(chunk("c\n", 1, None)),
        ];
        let mut seen = Vec::new();
        let mut ids = Vec::new();
        let rows = write_export(&path, AuditExportRequest::default(), |request| {
            seen.push(request.after_seq);
            ids.push(request.export_id);
            std::future::ready(replies.remove(0))
        })
        .await
        .unwrap();
        assert_eq!(rows.rows, 6);
        assert_eq!(rows.path, path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\nb\nc\n");
        assert_eq!(seen, vec![None, Some(3), Some(7)]);
        // Only the first chunk is audited: every later one names the export it belongs to.
        let named = Some("export-1".to_string());
        assert_eq!(ids, vec![None, named.clone(), named]);
    }

    #[tokio::test]
    async fn a_failed_export_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.csv");
        let mut replies = vec![
            Ok(chunk("a\n", 1, Some(1))),
            Err(CommandError::disconnected("lost")),
        ];
        let error = write_export(&path, AuditExportRequest::default(), |_| {
            std::future::ready(replies.remove(0))
        })
        .await
        .unwrap_err();
        assert!(matches!(error, CommandError::Disconnected { .. }));
        assert!(!path.exists());
    }

    fn message_chunk(content: &str, count: u32, next: Option<u32>) -> Response {
        Response::MessageExport(ExportChunk {
            content: content.into(),
            count,
            total: 7,
            next_offset: next,
            export_id: next.map(|_| "export-2".into()),
        })
    }

    fn default_request() -> ExportRequest {
        ExportRequest {
            filter: MessageFilter::default(),
            format: ExportFormat::Csv,
            include_bodies: false,
            offset: 0,
            export_id: None,
        }
    }

    #[tokio::test]
    async fn a_message_export_continues_from_each_chunks_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("messages.csv");
        let mut replies = vec![
            Ok(message_chunk("h\na\n", 3, Some(3))),
            Ok(message_chunk("b\n", 3, Some(6))),
            Ok(message_chunk("c\n", 1, None)),
        ];
        let mut offsets = Vec::new();
        let mut bodies = Vec::new();
        let mut ids = Vec::new();
        let request = ExportRequest {
            include_bodies: true,
            ..default_request()
        };
        let rows = write_message_export(&path, request, |request| {
            offsets.push(request.offset);
            bodies.push(request.include_bodies);
            ids.push(request.export_id);
            std::future::ready(replies.remove(0))
        })
        .await
        .unwrap();
        assert_eq!(rows.rows, 7);
        assert_eq!(rows.path, path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "h\na\nb\nc\n");
        assert_eq!(offsets, vec![0, 3, 6]);
        assert_eq!(bodies, vec![true, true, true]);
        let named = Some("export-2".to_string());
        assert_eq!(ids, vec![None, named.clone(), named]);
    }

    #[tokio::test]
    async fn a_failed_message_export_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("messages.csv");
        let mut replies = vec![
            Ok(message_chunk("h\n", 1, Some(1))),
            Err(CommandError::disconnected("lost")),
        ];
        let error = write_message_export(&path, default_request(), |_| {
            std::future::ready(replies.remove(0))
        })
        .await
        .unwrap_err();
        assert!(matches!(error, CommandError::Disconnected { .. }));
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn an_unexpected_reply_fails_the_export_and_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("messages.csv");
        let error = write_message_export(&path, default_request(), |_| {
            std::future::ready(Ok(Response::Done))
        })
        .await
        .unwrap_err();
        assert!(matches!(error, CommandError::Internal { .. }));
        assert!(!path.exists());
    }

    #[test]
    fn a_report_is_written_whole_and_unreadable_by_others() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.csv");
        assert_eq!(write_report(&path, "a,b\r\n1,2\r\n").unwrap(), path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a,b\r\n1,2\r\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o077, 0);
        }
    }

    #[test]
    fn a_report_into_a_missing_folder_fails_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("report.csv");
        assert!(matches!(
            write_report(&path, "x"),
            Err(CommandError::Internal { .. })
        ));
    }

    #[test]
    fn report_formats_come_from_the_window_as_lowercase_words() {
        let csv: ReportFormat = serde_json::from_value(serde_json::json!("csv")).unwrap();
        let json: ReportFormat = serde_json::from_value(serde_json::json!("json")).unwrap();
        assert_eq!((csv.extension(), json.extension()), ("csv", "json"));
        assert!(serde_json::from_value::<ReportFormat>(serde_json::json!("exe")).is_err());
    }

    /// A path that did not exist yet is used as it is.
    #[test]
    fn a_free_name_is_used_as_chosen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.csv");
        let (_, used) = create_new(&path).unwrap();
        assert_eq!(used, path);
    }

    /// The dialog lets an operator pick (and confirm replacing) an existing file: it is kept
    /// as it was and the export goes to the next free numbered name.
    #[test]
    fn an_existing_file_is_never_truncated_and_a_numbered_name_is_used() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.csv");
        std::fs::write(&path, "earlier export").unwrap();
        std::fs::write(dir.path().join("report-1.csv"), "another").unwrap();
        let written = write_report(&path, "new").unwrap();
        assert_eq!(written, dir.path().join("report-2.csv"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "earlier export");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("report-1.csv")).unwrap(),
            "another"
        );
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "new");
    }

    #[test]
    fn a_name_without_extension_gets_a_plain_suffix() {
        assert_eq!(
            variant(Path::new("/x/report"), 3),
            PathBuf::from("/x/report-3")
        );
        assert_eq!(
            variant(Path::new("/x/audit.2026.jsonl"), 1),
            PathBuf::from("/x/audit.2026-1.jsonl")
        );
    }

    #[test]
    fn giving_up_when_every_numbered_name_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.csv");
        std::fs::write(&path, "x").unwrap();
        for number in 1..=MAX_VARIANTS {
            std::fs::write(variant(&path, number), "x").unwrap();
        }
        let error = create_new(&path).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x");
    }

    /// An existing file with open permissions keeps them and its content; the new file is private.
    #[cfg(unix)]
    #[test]
    fn the_new_file_is_private_from_creation_whatever_the_old_one_was() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.csv");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let written = write_report(&path, "new").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o644);
        assert_eq!(mode(&written), 0o600);
        // Not even a permissive umask can open it up: the mode is set at creation.
        let (_, fresh) = create_new(&dir.path().join("fresh.csv")).unwrap();
        assert_eq!(mode(&fresh), 0o600);
    }

    /// A planted link at the destination is not followed: what it points at is untouched,
    /// whether the target exists or the link dangles, and the export lands beside it.
    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_at_the_destination_is_never_followed() {
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("victim.txt");
        std::fs::write(&victim, "precious").unwrap();
        let link = dir.path().join("report.csv");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        let written = write_report(&link, "export").unwrap();
        assert_eq!(written, dir.path().join("report-1.csv"));
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious");
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let dangling = dir.path().join("audit.csv");
        let missing = dir.path().join("not-yet.txt");
        std::os::unix::fs::symlink(&missing, &dangling).unwrap();
        let written = write_report(&dangling, "export").unwrap();
        assert_eq!(written, dir.path().join("audit-1.csv"));
        assert!(!missing.exists(), "the link's target must not be created");
    }

    /// A failed export takes away only the file it made: the earlier one stays, whole.
    #[tokio::test]
    async fn a_failed_export_over_an_existing_name_leaves_the_earlier_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.csv");
        std::fs::write(&path, "earlier export").unwrap();
        let mut replies = vec![
            Ok(chunk("a\n", 1, Some(1))),
            Err(CommandError::disconnected("lost")),
        ];
        write_export(&path, AuditExportRequest::default(), |_| {
            std::future::ready(replies.remove(0))
        })
        .await
        .unwrap_err();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "earlier export");
        assert!(!dir.path().join("audit-1.csv").exists());
    }

    /// The export reports the name it really used.
    #[tokio::test]
    async fn an_export_to_a_taken_name_reports_the_numbered_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.csv");
        std::fs::write(&path, "earlier export").unwrap();
        let mut replies = vec![Ok(chunk("a\n", 2, None))];
        let written = write_export(&path, AuditExportRequest::default(), |_| {
            std::future::ready(replies.remove(0))
        })
        .await
        .unwrap();
        assert_eq!(written.path, dir.path().join("audit-1.csv"));
        assert_eq!(written.rows, 2);
        assert_eq!(std::fs::read_to_string(&written.path).unwrap(), "a\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "earlier export");
    }
}
