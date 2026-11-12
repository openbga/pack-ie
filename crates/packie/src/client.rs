//! The connection to a running engine, how its errors read, and the `config` commands
//! (`config validate` does not need an engine).

use std::path::{Path, PathBuf};

use packie_config::{ApplyOutcome, Bootstrap, ChannelOutcome, Problem, RuntimeConfig};
use packie_ipc::messages::RejectionCode;
use packie_ipc::{ClientError, ErrorCode, IpcClient, IpcError, Request, Response};

use crate::Failure;
use crate::output::{json_text, print_line, print_table, short_time};

/// Where the engine is.
pub(crate) struct Target {
    pub(crate) bootstrap: PathBuf,
    pub(crate) ipc: Option<String>,
}

impl Target {
    fn endpoint(&self) -> Result<String, Failure> {
        if let Some(ipc) = &self.ipc {
            return Ok(ipc.clone());
        }
        Bootstrap::load(&self.bootstrap)
            .map(|b| b.ipc_endpoint())
            .map_err(|e| Failure::new(format!("{e}\nPass --bootstrap <file> or --ipc <endpoint>.")))
    }

    pub(crate) async fn connect(&self) -> Result<IpcClient, Failure> {
        let endpoint = self.endpoint()?;
        let client = IpcClient::connect(&endpoint).await.map_err(|e| {
            Failure::new(format!(
                "the engine is not running or not reachable at {endpoint} ({e}). \
                 Start it with `packie run`."
            ))
        })?;
        // Identifies the CLI, so its commands are logged with origin `cli`.
        let hello = Request::Hello {
            client: format!("packie-cli {}", env!("CARGO_PKG_VERSION")),
        };
        client.request(hello).await.map_err(failed)?;
        Ok(client)
    }
}

/// An engine error as the operator reads it: the engine's message, the field problems of
/// a document, and what to do next where the rejection has a standard remedy.
pub(crate) fn failed(error: ClientError) -> Failure {
    match error {
        ClientError::Server(e) => Failure::new(server_text(&e)),
        other => Failure::new(other.to_string()),
    }
}

fn server_text(error: &IpcError) -> String {
    let mut text = error.message.clone();
    if !error.field_errors.is_empty() {
        text.push('\n');
        text.push_str(&field_errors(&error.field_errors));
    }
    if let Some(hint) = error.reason_code.and_then(hint) {
        text.push(' ');
        text.push_str(hint);
    }
    text
}

/// What to do about a rejection, when the answer is always the same.
pub(crate) fn hint(code: RejectionCode) -> Option<&'static str> {
    match code {
        RejectionCode::DuplicateRiskUnconfirmed => Some(
            "Pass --confirm-duplicate-risk to resend anyway; the destination may already have processed it.",
        ),
        RejectionCode::Superseded => Some(
            "Resend the newer revision instead, or pass --superseded-ok if the older version must go again.",
        ),
        RejectionCode::DeliveryInProgress => {
            Some("Wait for the attempt to finish, then try again.")
        }
        RejectionCode::InvalidState => {
            Some("Check the delivery's state with `packie messages show <message-id>`.")
        }
        RejectionCode::DestinationNotFound => Some(
            "`packie messages show <message-id>` lists its destinations; a destination must be enabled in the active configuration.",
        ),
        RejectionCode::OperatorPaused => {
            Some("Use `packie channels resume <channel-id>` to start a paused channel.")
        }
        RejectionCode::NotPaused => Some(
            "Only a paused channel can be resumed; `packie status` shows each channel's state.",
        ),
        _ => None,
    }
}

/// The reply to `request`, with engine errors turned into operator text.
pub(crate) async fn ask(client: &IpcClient, request: Request) -> Result<Response, Failure> {
    client.request(request).await.map_err(failed)
}

/// Sends `request`, which the engine answers with `Done` or an error.
pub(crate) async fn ask_done(client: &IpcClient, request: Request) -> Result<(), Failure> {
    match ask(client, request).await? {
        Response::Done => Ok(()),
        _ => Err(unexpected()),
    }
}

pub(crate) fn unexpected() -> Failure {
    Failure::new("unexpected response from the engine")
}

fn field_errors(errors: &[Problem]) -> String {
    errors
        .iter()
        .map(|e| format!("  {e}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) async fn config_show(target: &Target) -> Result<(), Failure> {
    let client = target.connect().await?;
    let Response::Config { document } = ask(&client, Request::GetConfig).await? else {
        return Err(unexpected());
    };
    let text = document
        .to_pretty_json()
        .map_err(|e| Failure::new(e.to_string()))?;
    print_line(&text);
    Ok(())
}

fn read(file: &Path) -> Result<String, Failure> {
    std::fs::read_to_string(file)
        .map_err(|e| Failure::new(format!("cannot read {}: {e}", file.display())))
}

pub(crate) fn config_validate(file: &Path) -> Result<(), Failure> {
    match RuntimeConfig::parse(&read(file)?) {
        Ok(config) => {
            print_line(&format!(
                "{} is valid: {} endpoints, {} channels.",
                file.display(),
                config.endpoints.len(),
                config.channels.len()
            ));
            Ok(())
        }
        Err(errors) => Err(Failure::new(format!(
            "{} is invalid:\n{}",
            file.display(),
            field_errors(errors.problems())
        ))),
    }
}

pub(crate) async fn config_apply(
    target: &Target,
    file: &Path,
    force: bool,
    note: String,
) -> Result<(), Failure> {
    let document: serde_json::Value = serde_json::from_str(&read(file)?)
        .map_err(|e| Failure::new(format!("{} is not JSON: {e}", file.display())))?;
    let client = target.connect().await?;
    // A document saved by `config show` carries the version it was edited from; a
    // hand-written one (or --force) applies on top of the active version.
    let edited_from = document
        .get("config_version")
        .and_then(serde_json::Value::as_u64)
        .filter(|v| *v > 0);
    let base_version = match edited_from.filter(|_| !force) {
        Some(version) => version,
        None => active_version(&client).await?,
    };
    let request = Request::PutConfig {
        document,
        base_version,
        note: Some(note),
    };
    match client.request(request).await {
        Ok(Response::ConfigApplied {
            config_version,
            result,
            outcomes,
            warnings,
            ..
        }) => {
            let partial = json_text(&result) == "partial";
            print_line(&format!(
                "Applied configuration version {config_version}{}.",
                if partial {
                    " partially; some channels failed"
                } else {
                    ""
                }
            ));
            print_outcomes(&outcomes);
            for warning in &warnings {
                print_line(&format!("  {warning}"));
            }
            Ok(())
        }
        Ok(_) => Err(unexpected()),
        Err(ClientError::Server(e)) if e.code == ErrorCode::Conflict => {
            Err(Failure::new(format!("{} Or pass --force.", e.message)))
        }
        Err(error) => Err(failed(error)),
    }
}

async fn active_version(client: &IpcClient) -> Result<u64, Failure> {
    match ask(client, Request::GetConfig).await? {
        Response::Config { document } => Ok(document.config_version),
        _ => Err(unexpected()),
    }
}

fn print_outcomes(outcomes: &[ChannelOutcome]) {
    let rows: Vec<Vec<String>> = outcomes
        .iter()
        .filter(|c| c.outcome != ApplyOutcome::Unchanged)
        .map(|c| {
            let detail = match &c.outcome {
                ApplyOutcome::Failed { reason } => reason.clone(),
                _ => String::new(),
            };
            vec![
                c.channel_id.to_string(),
                outcome_name(&c.outcome).to_owned(),
                detail,
            ]
        })
        .collect();
    if rows.is_empty() {
        print_line("No channel changed.");
    } else {
        print_table(&["CHANNEL", "OUTCOME", "DETAIL"], &rows);
    }
}

fn outcome_name(outcome: &ApplyOutcome) -> &'static str {
    match outcome {
        ApplyOutcome::Unchanged => "unchanged",
        ApplyOutcome::Updated => "updated",
        ApplyOutcome::Started => "started",
        ApplyOutcome::Restarted => "restarted",
        ApplyOutcome::Stopped => "stopped",
        ApplyOutcome::Removed => "removed",
        ApplyOutcome::Failed { .. } => "failed",
    }
}

pub(crate) async fn config_history(target: &Target) -> Result<(), Failure> {
    let client = target.connect().await?;
    let Response::ConfigHistory { versions } = ask(&client, Request::GetConfigHistory).await?
    else {
        return Err(unexpected());
    };
    let rows: Vec<Vec<String>> = versions.iter().map(history_row).collect();
    print_table(
        &[
            "VERSION",
            "APPLIED",
            "BY",
            "ENDPOINTS",
            "CHANNELS",
            "CHANGES",
            "VERIFIED",
            "NOTE",
        ],
        &rows,
    );
    Ok(())
}

fn history_row(v: &packie_config::VersionInfo) -> Vec<String> {
    let verified = v.verifications.last().map_or_else(
        || "not verified since change".to_owned(),
        |s| format!("{} by {}", short_time(s.verified_at), s.verified_by.name),
    );
    vec![
        v.config_version.to_string(),
        short_time(v.applied_at.unwrap_or(v.generated_at)),
        v.applied_by
            .as_ref()
            .map_or_else(|| "-".to_owned(), |a| a.name.clone()),
        v.endpoints.to_string(),
        v.channels.to_string(),
        changes(&v.outcomes),
        verified,
        v.note.clone().unwrap_or_default(),
    ]
}

/// `2 restarted, 1 failed`: what applying a version did, without the unchanged channels.
fn changes(outcomes: &[packie_config::ChannelOutcome]) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for outcome in outcomes {
        let name = match &outcome.outcome {
            packie_config::ApplyOutcome::Unchanged => continue,
            packie_config::ApplyOutcome::Updated => "updated",
            packie_config::ApplyOutcome::Started => "started",
            packie_config::ApplyOutcome::Restarted => "restarted",
            packie_config::ApplyOutcome::Stopped => "stopped",
            packie_config::ApplyOutcome::Removed => "removed",
            packie_config::ApplyOutcome::Failed { .. } => "failed",
        };
        match counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    if counts.is_empty() {
        return "-".to_owned();
    }
    counts
        .iter()
        .map(|(name, count)| format!("{count} {name}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `packie config verify`: signs off a version as verified.
pub(crate) async fn config_verify(
    target: &Target,
    version: u64,
    note: String,
) -> Result<(), Failure> {
    let client = target.connect().await?;
    let request = Request::VerifyConfigVersion {
        config_version: version,
        note,
    };
    let Response::ConfigVersionVerified { version: info } = ask(&client, request).await? else {
        return Err(unexpected());
    };
    let by = info
        .verifications
        .last()
        .map_or_else(String::new, |s| format!(" by {}", s.verified_by.name));
    print_line(&format!(
        "Version {} is signed off as verified{by}. `packie config history` lists the sign-offs.",
        info.config_version
    ));
    Ok(())
}
