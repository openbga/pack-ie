//! `packie messages export` and `packie messages verify`: the selected-message export and the
//! transmission verification report (`docs/spec/operator-console.md` §12). Both are audited
//! by the engine.

use std::io::Write;
use std::path::PathBuf;

use clap::{Args, ValueEnum};
use packie_core::{ChannelId, Timestamp};
use packie_ipc::reports::{ExportFormat, ExportRequest, TransmissionQuery, TransmissionReport};
use packie_ipc::{IpcClient, Request, Response};

use crate::client::{Target, ask, unexpected};
use crate::messages::FilterArgs;
use crate::output::{eprint_line, print_line, print_table, short_time};
use crate::{Failure, args};

/// File format of `packie messages export`.
#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub(crate) enum Format {
    /// Comma-separated values with a header row.
    #[default]
    Csv,
    /// One JSON object per line.
    Jsonl,
}

#[derive(Debug, Args)]
pub(crate) struct ExportArgs {
    #[command(flatten)]
    filter: FilterArgs,
    /// csv or jsonl.
    #[arg(long, value_enum, default_value_t = Format::Csv)]
    format: Format,
    /// Also export each original body. Message content: audited as a content view.
    #[arg(long)]
    bodies: bool,
    /// Write to this file (created new; an existing file is not overwritten) instead of
    /// standard output.
    #[arg(long, short)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct VerifyArgs {
    /// The channel.
    #[arg(long)]
    channel: ChannelId,
    /// Messages received at or after this time: RFC 3339 or an age such as 24h or 7d.
    #[arg(long, value_parser = args::time)]
    since: Timestamp,
    /// Messages received before this time (default now).
    #[arg(long, value_parser = args::time)]
    until: Option<Timestamp>,
    /// Print one row per delivery as CSV (for the laboratory's records) instead of the
    /// summary.
    #[arg(long, conflicts_with = "json")]
    csv: bool,
    /// Print the whole report as JSON.
    #[arg(long)]
    json: bool,
    /// Write the CSV or JSON to this file (created new) instead of standard output.
    #[arg(long, short)]
    output: Option<PathBuf>,
}

/// Opens the destination: a new file, or standard output.
fn destination(path: Option<&PathBuf>) -> Result<Box<dyn Write>, Failure> {
    match path {
        None => Ok(Box::new(std::io::stdout().lock())),
        Some(path) => {
            crate::output::create_export_file(path).map(|file| Box::new(file) as Box<dyn Write>)
        }
    }
}

fn write_failed(error: &std::io::Error) -> Failure {
    Failure::new(format!("writing the export failed: {error}"))
}

pub(crate) async fn export(target: &Target, args: &ExportArgs) -> Result<(), Failure> {
    let client = target.connect().await?;
    let mut out = destination(args.output.as_ref())?;
    let mut request = ExportRequest {
        filter: args.filter.filter(),
        format: match args.format {
            Format::Csv => ExportFormat::Csv,
            Format::Jsonl => ExportFormat::Jsonl,
        },
        include_bodies: args.bodies,
        offset: 0,
        export_id: None,
    };
    let mut exported: u64 = 0;
    loop {
        let chunk = next_chunk(&client, &request).await?;
        out.write_all(chunk.content.as_bytes())
            .map_err(|e| write_failed(&e))?;
        exported += u64::from(chunk.count);
        match chunk.next_offset {
            Some(next) => {
                request.offset = next;
                request.export_id = chunk.export_id;
            }
            None => break,
        }
    }
    out.flush().map_err(|e| write_failed(&e))?;
    if let Some(path) = &args.output {
        print_line(&format!(
            "Exported {exported} messages to {}.",
            path.display()
        ));
    } else {
        // Standard output carries the data; the summary goes to standard error.
        eprint_line(&format!("Exported {exported} messages."));
    }
    Ok(())
}

async fn next_chunk(
    client: &IpcClient,
    request: &ExportRequest,
) -> Result<packie_ipc::reports::ExportChunk, Failure> {
    match ask(client, Request::ExportMessages(request.clone())).await? {
        Response::MessageExport(chunk) => Ok(chunk),
        _ => Err(unexpected()),
    }
}

/// Said when a report lists only the first deliveries of its period.
const TRUNCATED_WARNING: &str = "The period has more deliveries than one report lists: the \
    rows stop early, the totals cover the whole period. Report a shorter period for every row.";

pub(crate) async fn verify(target: &Target, args: &VerifyArgs) -> Result<(), Failure> {
    let client = target.connect().await?;
    let query = TransmissionQuery {
        channel_id: args.channel,
        since: args.since,
        until: args.until,
    };
    let Response::Transmission(report) = ask(&client, Request::VerifyTransmission(query)).await?
    else {
        return Err(unexpected());
    };
    if args.csv || args.json || args.output.is_some() {
        let text = if args.json {
            serde_json::to_string_pretty(&report).map_err(|e| Failure::new(e.to_string()))? + "\n"
        } else {
            report.rows_csv()
        };
        let mut out = destination(args.output.as_ref())?;
        out.write_all(text.as_bytes())
            .map_err(|e| write_failed(&e))?;
        out.flush().map_err(|e| write_failed(&e))?;
        if report.truncated {
            eprint_line(TRUNCATED_WARNING);
        }
        return Ok(());
    }
    print_summary(&report);
    Ok(())
}

fn print_summary(report: &TransmissionReport) {
    print_line(&format!(
        "Channel {}: {} messages received {} to {}.",
        report.channel_id,
        report.messages,
        short_time(report.since),
        short_time(report.until)
    ));
    if report.destinations.is_empty() {
        print_line("No deliveries in this period.");
        return;
    }
    let rows: Vec<Vec<String>> = report
        .destinations
        .iter()
        .map(|d| {
            vec![
                format!("{} ({})", d.label, d.destination_id),
                d.deliveries.to_string(),
                d.accepted.to_string(),
                d.received.to_string(),
                d.handed_off.to_string(),
                d.unresolved.to_string(),
                d.closed.to_string(),
                d.pending.to_string(),
            ]
        })
        .collect();
    print_line("");
    print_table(
        &[
            "DESTINATION",
            "DELIVERIES",
            "ACCEPTED",
            "RECEIVED",
            "HANDED OFF",
            "UNRESOLVED",
            "CLOSED",
            "PENDING",
        ],
        &rows,
    );
    print_line("");
    print_line(
        "Accepted: the destination's application accepted it. Received: the destination \
         confirmed receipt. Handed off: written, not confirmed by the destination.",
    );
    if report.truncated {
        print_line(TRUNCATED_WARNING);
    }
    print_line("`--csv` prints one row per delivery with content hashes for your records.");
}
