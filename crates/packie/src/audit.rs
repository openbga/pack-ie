//! `packie audit`: review, export and verify the audit log, and the adapter that gives the
//! engine its persistent audit log.

use std::fs::File;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Args, Subcommand, ValueEnum};
use packie_core::Timestamp;
use packie_engine::{AuditLog, AuditRecord, AuditUnavailable};
use packie_ipc::audit::{
    AuditAction, AuditEvent, AuditExportChunk, AuditExportFormat, AuditExportRequest, AuditFilter,
    AuditObjectType, AuditOrigin, AuditOutcome, AuditPage, AuditQuery, AuditVerification,
    MAX_AUDIT_PAGE,
};
use packie_ipc::{Request, Response};

use crate::client::{Target, ask, unexpected};
use crate::output::{count, print_json, print_line, print_table, short_time};
use crate::{Failure, args};

/// The engine's persistent audit log: `packie-audit` behind the engine's [`AuditLog`] trait.
pub(crate) struct PersistentAudit(packie_audit::Audit);

fn unavailable(error: packie_audit::Error) -> AuditUnavailable {
    AuditUnavailable(error.to_string())
}

impl AuditLog for PersistentAudit {
    fn record(&self, record: &AuditRecord) -> Result<u64, AuditUnavailable> {
        self.0
            .record(&packie_audit::Entry {
                actor: record.actor.clone(),
                action: record.action,
                object: record.object.clone(),
                outcome: record.outcome,
                details: record.details.clone(),
            })
            .map_err(unavailable)
    }

    fn query(&self, query: &AuditQuery) -> Result<AuditPage, AuditUnavailable> {
        self.0.query(query).map_err(unavailable)
    }

    fn export(&self, request: &AuditExportRequest) -> Result<AuditExportChunk, AuditUnavailable> {
        self.0.export(request).map_err(unavailable)
    }

    fn verify(&self) -> Result<AuditVerification, AuditUnavailable> {
        self.0.verify().map_err(unavailable)
    }

    fn available(&self) -> bool {
        self.0.status().available
    }

    fn check_writable(&self) -> Result<(), AuditUnavailable> {
        self.0.check_writable().map_err(unavailable)
    }
}

/// Opens `<state>/audit` for the engine. It never fails: a log that cannot be opened
/// starts unavailable and is retried, and the engine refuses audited commands meanwhile.
pub(crate) fn open_persistent(
    bootstrap: &packie_config::Bootstrap,
) -> Result<Arc<dyn AuditLog>, AuditUnavailable> {
    let options = packie_audit::Options {
        retention_days: bootstrap.audit.retention_days,
        ..packie_audit::Options::default()
    };
    let audit = packie_audit::Audit::start(bootstrap.state_dir().join("audit"), options);
    Ok(Arc::new(PersistentAudit(audit)))
}

#[derive(Debug, Subcommand)]
pub(crate) enum AuditCommand {
    /// List audit events, newest first.
    List(ListArgs),
    /// Export audit events, oldest first, as CSV or JSON Lines.
    Export(ExportArgs),
    /// Check the audit log's hash chain for edited, removed or reordered events.
    Verify(VerifyArgs),
}

/// Which events.
#[derive(Debug, Args)]
pub(crate) struct FilterArgs {
    /// Only events recorded at or after this time: RFC 3339 (2026-11-06T08:00:00Z) or an
    /// age such as 15m, 2h or 7d.
    #[arg(long, value_parser = args::time)]
    since: Option<Timestamp>,
    /// Only events recorded before this time (same forms as --since).
    #[arg(long, value_parser = args::time)]
    until: Option<Timestamp>,
    /// Only events of this account, by name or by uid/SID.
    #[arg(long)]
    actor: Option<String>,
    /// Only this action, such as config_apply, message_body_view or backup_create.
    #[arg(long, value_parser = args::audit_action)]
    action: Option<AuditAction>,
    /// Only events from this origin: console, cli, engine or other.
    #[arg(long, value_parser = args::audit_origin)]
    origin: Option<AuditOrigin>,
    /// Only events with this outcome: requested, succeeded, rejected or failed.
    #[arg(long, value_parser = args::audit_outcome)]
    outcome: Option<AuditOutcome>,
    /// Only events on this kind of object, such as channel, message or config_version.
    #[arg(long, value_parser = args::audit_object_type)]
    object_type: Option<AuditObjectType>,
    /// Only events on the object with this id.
    #[arg(long = "object")]
    object_id: Option<String>,
}

impl FilterArgs {
    fn filter(&self) -> AuditFilter {
        AuditFilter {
            since: self.since,
            until: self.until,
            actor: self.actor.clone(),
            action: self.action,
            origin: self.origin,
            outcome: self.outcome,
            object_type: self.object_type,
            object_id: self.object_id.clone(),
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    filter: FilterArgs,
    /// Events to print, 1 to 1000.
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_AUDIT_PAGE)))]
    limit: u32,
    /// Only events before this seq (the next page, as the previous page's footer says).
    #[arg(long)]
    before_seq: Option<u64>,
    /// Print the page as JSON.
    #[arg(long)]
    json: bool,
}

impl ListArgs {
    fn query(&self) -> AuditQuery {
        AuditQuery {
            filter: self.filter.filter(),
            before_seq: self.before_seq,
            limit: self.limit,
        }
    }
}

/// The export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    /// RFC 4180 CSV with a header row.
    Csv,
    /// One JSON event per line.
    Jsonl,
}

impl Format {
    const fn wire(self) -> AuditExportFormat {
        match self {
            Self::Csv => AuditExportFormat::Csv,
            Self::Jsonl => AuditExportFormat::JsonLines,
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    #[command(flatten)]
    filter: FilterArgs,
    /// The file format.
    #[arg(long, value_enum, default_value_t = Format::Csv)]
    format: Format,
    /// Write to this new file (owner-only; an existing file is never overwritten) instead
    /// of stdout.
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct VerifyArgs {
    /// Check this audit directory (a backup's `audit` folder, or a stopped engine's
    /// `<state>/audit`) instead of asking the running engine.
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Print the report as JSON.
    #[arg(long)]
    json: bool,
}

pub(crate) async fn run(target: &Target, command: AuditCommand) -> Result<(), Failure> {
    match command {
        AuditCommand::List(args) => list(target, &args).await,
        AuditCommand::Export(args) => export(target, &args).await,
        AuditCommand::Verify(args) => verify(target, &args).await,
    }
}

async fn list(target: &Target, args: &ListArgs) -> Result<(), Failure> {
    let query = args.query();
    let filtered = query.filter != AuditFilter::default() || query.before_seq.is_some();
    let client = target.connect().await?;
    let Response::AuditEvents(page) = ask(&client, Request::GetAuditEvents(query)).await? else {
        return Err(unexpected());
    };
    if args.json {
        return print_json(&page);
    }
    if page.events.is_empty() {
        print_line(empty_text(filtered));
        return Ok(());
    }
    let rows: Vec<Vec<String>> = page.events.iter().map(row).collect();
    print_table(&HEADERS, &rows);
    print_line("");
    print_line(&footer(&page));
    Ok(())
}

const HEADERS: [&str; 7] = [
    "SEQ", "TIME", "ACCOUNT", "ORIGIN", "ACTION", "OBJECT", "OUTCOME",
];

fn row(event: &AuditEvent) -> Vec<String> {
    let object = match (&event.object_type, &event.object_id) {
        (Some(kind), Some(id)) => format!("{kind} {id}"),
        (None, Some(id)) => id.clone(),
        (Some(kind), None) => kind.to_string(),
        (None, None) => "-".to_owned(),
    };
    vec![
        event.seq.to_string(),
        short_time(event.at),
        event.actor_name.clone(),
        event.origin.to_string(),
        event.action.to_string(),
        object,
        event.outcome.to_string(),
    ]
}

const fn empty_text(filtered: bool) -> &'static str {
    if filtered {
        "No audit events match these filters. Widen them, or run without filters."
    } else {
        "No audit events recorded yet. Operator actions and content views appear here."
    }
}

/// `Showing 50 events. Next page: --before-seq 123.`
fn footer(page: &AuditPage) -> String {
    let shown = u64::try_from(page.events.len()).unwrap_or(u64::MAX);
    let mut text = format!("Showing {}.", count(shown, "event", "events"));
    if let Some(seq) = page.next_before_seq {
        text.push_str(&format!(" Next page: --before-seq {seq}."));
    }
    text
}

/// Where an export goes.
enum Sink {
    Stdout,
    File(File, PathBuf),
}

impl Sink {
    /// Creates the file before the first request, so an existing file refuses the export
    /// before it is audited.
    fn open(output: Option<&Path>) -> Result<Self, Failure> {
        let Some(path) = output else {
            return Ok(Self::Stdout);
        };
        let file = crate::output::create_export_file(path)?;
        Ok(Self::File(file, path.to_owned()))
    }

    fn write(&mut self, content: &str) -> Result<(), Failure> {
        match self {
            Self::Stdout => {
                if let Err(e) = std::io::stdout().lock().write_all(content.as_bytes())
                    && e.kind() == ErrorKind::BrokenPipe
                {
                    std::process::exit(0);
                }
                Ok(())
            }
            Self::File(file, path) => file
                .write_all(content.as_bytes())
                .map_err(|e| Failure::new(format!("cannot write {}: {e}", path.display()))),
        }
    }

    /// Syncs the file; a failed export leaves no partial file behind.
    fn finish(self, ok: bool) -> Result<(), Failure> {
        let Self::File(file, path) = self else {
            return Ok(());
        };
        if !ok {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Ok(());
        }
        file.sync_all()
            .map_err(|e| Failure::new(format!("cannot write {}: {e}", path.display())))
    }
}

async fn export(target: &Target, args: &ExportArgs) -> Result<(), Failure> {
    let mut sink = Sink::open(args.output.as_deref())?;
    let result = export_chunks(target, args, &mut sink).await;
    sink.finish(result.is_ok())?;
    let rows = result?;
    if let Some(path) = &args.output {
        print_line(&exported_text(rows, path));
    }
    Ok(())
}

/// Requests chunks until the engine reports the end; returns the events exported.
async fn export_chunks(
    target: &Target,
    args: &ExportArgs,
    sink: &mut Sink,
) -> Result<u64, Failure> {
    let client = target.connect().await?;
    let mut request = AuditExportRequest {
        filter: args.filter.filter(),
        format: args.format.wire(),
        after_seq: None,
        export_id: None,
    };
    let mut rows = 0_u64;
    loop {
        let Response::AuditExport(chunk) =
            ask(&client, Request::ExportAudit(request.clone())).await?
        else {
            return Err(unexpected());
        };
        sink.write(&chunk.content)?;
        rows += u64::from(chunk.rows);
        match chunk.next_after_seq {
            Some(seq) if request.after_seq != Some(seq) => {
                request.after_seq = Some(seq);
                request.export_id = chunk.export_id;
            }
            _ => return Ok(rows),
        }
    }
}

fn exported_text(rows: u64, path: &Path) -> String {
    format!(
        "Exported {} to {}. The export can contain account names and operator notes; keep it with the lab's records.",
        count(rows, "audit event", "audit events"),
        path.display()
    )
}

async fn verify(target: &Target, args: &VerifyArgs) -> Result<(), Failure> {
    let report = match &args.dir {
        Some(dir) => {
            let report = packie_audit::verify_dir(dir)
                .map_err(|e| Failure::new(format!("cannot verify {}: {e}", dir.display())))?;
            // An empty or wrong directory proves nothing; do not report it as verified.
            if report.files == 0 {
                return Err(Failure::new(format!(
                    "{} holds no audit log files (audit.sqlite3, audit-<year>-….sqlite3); nothing was verified.",
                    dir.display()
                )));
            }
            report
        }
        None => {
            let client = target.connect().await?;
            match ask(&client, Request::VerifyAudit).await? {
                Response::AuditVerification(report) => *report,
                _ => return Err(unexpected()),
            }
        }
    };
    if args.json {
        print_json(&report)?;
    } else {
        for line in verification_lines(&report) {
            print_line(&line);
        }
    }
    if report.ok {
        Ok(())
    } else {
        Err(Failure::new(format!(
            "the audit log does not verify: {}.",
            count(
                u64::try_from(report.problems.len()).unwrap_or(u64::MAX),
                "problem",
                "problems"
            )
        )))
    }
}

fn verification_lines(report: &AuditVerification) -> Vec<String> {
    let seq = |seq: Option<u64>| seq.map_or_else(|| "-".to_owned(), |s| s.to_string());
    let mut lines = vec![
        if report.ok {
            "The audit chain verifies: no edited, removed or reordered events.".to_owned()
        } else {
            "The audit chain does NOT verify.".to_owned()
        },
        format!("  Files:     {}", report.files),
        format!("  Events:    {}", report.events),
        format!(
            "  Seq range: {} to {}",
            seq(report.first_seq),
            seq(report.last_seq)
        ),
        format!(
            "  Head hash: {}",
            report.head_hash.as_deref().unwrap_or("-")
        ),
    ];
    for problem in &report.problems {
        let at = problem
            .seq
            .map_or_else(String::new, |s| format!(" seq {s}"));
        lines.push(format!(
            "  Problem in {}{at}: {}",
            problem.file, problem.message
        ));
    }
    if report.ok {
        lines.push(
            "Compare the head hash with the last `audit.anchor` line in the engine log: removed newest events leave a valid, shorter chain."
                .to_owned(),
        );
    }
    lines
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use packie_ipc::audit::AuditProblem;

    use super::*;

    #[derive(Debug, Parser)]
    struct Cli {
        #[command(subcommand)]
        command: AuditCommand,
    }

    fn parse(arguments: &[&str]) -> AuditCommand {
        let mut all = vec!["audit"];
        all.extend(arguments);
        Cli::try_parse_from(all).unwrap().command
    }

    #[test]
    fn list_arguments_become_the_filter_and_page() {
        let AuditCommand::List(list) = parse(&[
            "list",
            "--since",
            "2026-11-06T08:00:00Z",
            "--actor",
            "jdoe",
            "--action",
            "config_apply",
            "--origin",
            "cli",
            "--outcome",
            "failed",
            "--object-type",
            "channel",
            "--object",
            "ch1",
            "--before-seq",
            "90",
            "--limit",
            "10",
        ]) else {
            panic!("not a list");
        };
        let query = list.query();
        assert_eq!(query.limit, 10);
        assert_eq!(query.before_seq, Some(90));
        assert_eq!(query.filter.actor.as_deref(), Some("jdoe"));
        assert_eq!(query.filter.action, Some(AuditAction::ConfigApply));
        assert_eq!(query.filter.origin, Some(AuditOrigin::Cli));
        assert_eq!(query.filter.outcome, Some(AuditOutcome::Failed));
        assert_eq!(query.filter.object_type, Some(AuditObjectType::Channel));
        assert_eq!(query.filter.object_id.as_deref(), Some("ch1"));
        assert_eq!(
            query.filter.since,
            Some(Timestamp::parse("2026-11-06T08:00:00Z").unwrap())
        );
        assert_eq!(query.filter.until, None);
    }

    #[test]
    fn list_defaults_and_bad_values() {
        let AuditCommand::List(list) = parse(&["list"]) else {
            panic!("not a list");
        };
        assert_eq!(
            list.query(),
            AuditQuery {
                limit: 50,
                ..AuditQuery::default()
            }
        );
        assert!(Cli::try_parse_from(["audit", "list", "--limit", "0"]).is_err());
        assert!(Cli::try_parse_from(["audit", "list", "--limit", "1001"]).is_err());
        let error = Cli::try_parse_from(["audit", "list", "--action", "nope"]).unwrap_err();
        assert!(error.to_string().contains("config_apply"));
    }

    #[test]
    fn export_formats_map_to_the_wire() {
        let AuditCommand::Export(export) = parse(&["export", "--format", "jsonl"]) else {
            panic!("not an export");
        };
        assert_eq!(export.format.wire(), AuditExportFormat::JsonLines);
        let AuditCommand::Export(export) = parse(&["export"]) else {
            panic!("not an export");
        };
        assert_eq!(export.format.wire(), AuditExportFormat::Csv);
    }

    #[test]
    fn export_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.csv");
        let mut sink = Sink::open(Some(&path)).unwrap();
        sink.write("a,b\n").unwrap();
        sink.finish(true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a,b\n");
        let error = Sink::open(Some(&path)).err().unwrap();
        assert!(error.message.contains("already exists"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn a_failed_export_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.csv");
        let mut sink = Sink::open(Some(&path)).unwrap();
        sink.write("partial").unwrap();
        sink.finish(false).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn pages_say_how_to_continue() {
        let page = AuditPage {
            events: Vec::new(),
            next_before_seq: Some(41),
        };
        assert_eq!(
            footer(&page),
            "Showing 0 events. Next page: --before-seq 41."
        );
        let last = AuditPage {
            next_before_seq: None,
            ..page
        };
        assert_eq!(footer(&last), "Showing 0 events.");
    }

    #[test]
    fn verification_report_lists_problems() {
        let report = AuditVerification {
            ok: false,
            files: 1,
            events: 3,
            first_seq: Some(1),
            last_seq: Some(3),
            head_hash: Some("abc".to_owned()),
            problems: vec![AuditProblem {
                file: "audit-2026.sqlite3".to_owned(),
                seq: Some(2),
                kind: packie_ipc::audit::AuditProblemKind::HashMismatch,
                message: "edited".to_owned(),
            }],
        };
        let lines = verification_lines(&report);
        assert_eq!(lines[0], "The audit chain does NOT verify.");
        assert!(lines.contains(&"  Seq range: 1 to 3".to_owned()));
        assert!(lines.contains(&"  Head hash: abc".to_owned()));
        assert!(lines.contains(&"  Problem in audit-2026.sqlite3 seq 2: edited".to_owned()));
    }
}
