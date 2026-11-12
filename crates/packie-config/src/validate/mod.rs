//! Validation of the runtime document (`docs/spec/configuration.md` §9). Every problem is
//! `{severity, code, path, message}`; all problems are reported together; any error
//! rejects the document, warnings do not.
//!
//! [`Validator`] collects the problems; its checks are grouped by what they check:
//! [`endpoints`], [`channels`] and [`protocols`].

mod channels;
mod endpoints;
mod problem;
mod protocols;

use std::collections::{BTreeMap, HashMap};
use std::ops::RangeInclusive;

use packie_core::EndpointId;

use crate::endpoint::{EndpointConfig, Transport};
use crate::runtime::{AlertThresholds, DiagnosticsOverrides, RuntimeConfig, SCHEMA_VERSION};

pub use problem::{Problem, ProblemCode, ProblemSeverity, ValidationErrors};

/// Checks the rules the schema alone cannot express. Returns the warnings of a valid
/// document, or every problem when there is an error.
///
/// # Errors
///
/// [`ValidationErrors`] with every error and warning when the document has an error.
pub fn validate(config: &RuntimeConfig) -> Result<Vec<Problem>, ValidationErrors> {
    let mut v = Validator::default();
    if config.schema_version != SCHEMA_VERSION {
        v.schema("schema_version", "must be 1");
    }
    v.diagnostics(&config.diagnostics);
    v.alerts(&config.alerts);
    let endpoints = v.endpoints(&config.endpoints);
    v.port_conflicts(&config.endpoints);
    v.channels(&config.channels, &endpoints);
    if v.problems.iter().any(Problem::is_error) {
        Err(ValidationErrors::from(v.problems))
    } else {
        Ok(v.problems)
    }
}

/// `secret_unresolved` warnings for every secret reference whose name is not among
/// `known` (the names the secret store holds).
#[must_use]
pub fn unresolved_secrets(config: &RuntimeConfig, known: &[String]) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (i, endpoint) in config.endpoints.iter().enumerate() {
        let Transport::Sftp(sftp) = &endpoint.transport else {
            continue;
        };
        for (field, secret) in sftp.auth.secret_refs() {
            if !secret.is_inline() && !known.iter().any(|k| k == secret.name()) {
                problems.push(Problem::warning(
                    ProblemCode::SecretUnresolved,
                    format!("endpoints[{i}].settings.auth.{field}"),
                    format!(
                        "secret {} is not in the secret store; the endpoint reports an error until it is set",
                        secret.name()
                    ),
                ));
            }
        }
    }
    problems
}

/// Largest `max_message_bytes` (64 MiB).
const MAX_MESSAGE: u64 = 64 * 1024 * 1024;

type Endpoints<'a> = HashMap<EndpointId, &'a EndpointConfig>;

/// Which end of a message channel a connector is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Source,
    Destination,
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Collects the problems of one document; the checks themselves are in the submodules.
#[derive(Default)]
struct Validator {
    problems: Vec<Problem>,
}

impl Validator {
    fn err(&mut self, code: ProblemCode, path: impl Into<String>, message: impl Into<String>) {
        self.problems.push(Problem::error(code, path, message));
    }

    fn warn(&mut self, code: ProblemCode, path: impl Into<String>, message: impl Into<String>) {
        self.problems.push(Problem::warning(code, path, message));
    }

    fn schema(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.err(ProblemCode::Schema, path, message);
    }

    /// A second definition of an id (or name) within its scope.
    fn duplicate(&mut self, path: String, what: &str) {
        self.err(ProblemCode::DuplicateId, path, format!("duplicate {what}"));
    }

    /// `path.field` must be within `range`.
    fn range(
        &mut self,
        path: &str,
        field: &str,
        value: impl Into<u64>,
        range: RangeInclusive<u64>,
    ) {
        let value = value.into();
        if !range.contains(&value) {
            self.schema(
                format!("{path}.{field}"),
                format!("must be from {} to {}", range.start(), range.end()),
            );
        }
    }

    /// Each `(field, value, range)` of `path`, as [`range`](Self::range).
    fn ranges<const N: usize>(
        &mut self,
        path: &str,
        checks: [(&str, u64, RangeInclusive<u64>); N],
    ) {
        for (field, value, range) in checks {
            self.range(path, field, value, range);
        }
    }

    fn positive(&mut self, path: &str, field: &str, value: impl Into<u64>) {
        if value.into() == 0 {
            self.schema(format!("{path}.{field}"), "must be greater than 0");
        }
    }

    fn label(&mut self, path: &str, label: &str) {
        if !(1..=100).contains(&label.trim().chars().count()) {
            self.schema(format!("{path}.label"), "must be 1 to 100 characters");
        }
    }

    fn tags(&mut self, path: &str, tags: &BTreeMap<String, String>) {
        if tags.len() > 20 {
            self.schema(format!("{path}.tags"), "at most 20 tags");
        }
    }

    fn non_empty(&mut self, path: &str, field: &str, value: &str) {
        if value.trim().is_empty() {
            self.schema(format!("{path}.{field}"), "must not be empty");
        }
    }

    fn diagnostics(&mut self, d: &DiagnosticsOverrides) {
        let path = "diagnostics";
        if let Some(n) = d.snapshot_interval_seconds {
            self.range(path, "snapshot_interval_seconds", n, 1..=60);
        }
        if let Some(n) = d.retain_recent_errors {
            self.range(path, "retain_recent_errors", n, 10..=10_000);
        }
    }

    fn alerts(&mut self, a: &AlertThresholds) {
        self.ranges(
            "alerts",
            [
                (
                    "endpoint_down_seconds",
                    a.endpoint_down_seconds.into(),
                    1..=86_400,
                ),
                ("no_peer_seconds", a.no_peer_seconds.into(), 0..=86_400),
                ("buffer_high_percent", a.buffer_high_percent.into(), 1..=100),
                (
                    "buffer_high_seconds",
                    a.buffer_high_seconds.into(),
                    1..=3600,
                ),
                (
                    "delivery_backlog_count",
                    a.delivery_backlog_count.into(),
                    0..=10_000_000,
                ),
                (
                    "delivery_backlog_age_seconds",
                    a.delivery_backlog_age_seconds.into(),
                    0..=604_800,
                ),
                (
                    "delivery_attention_critical_count",
                    a.delivery_attention_critical_count.into(),
                    1..=10_000_000,
                ),
                (
                    "invalid_messages_count",
                    a.invalid_messages_count.into(),
                    0..=10_000,
                ),
                (
                    "invalid_messages_window_seconds",
                    a.invalid_messages_window_seconds.into(),
                    1..=86_400,
                ),
                (
                    "disk_low_megabytes",
                    a.disk_low_megabytes.into(),
                    1..=10_000_000,
                ),
                (
                    "disk_critical_megabytes",
                    a.disk_critical_megabytes.into(),
                    1..=u64::from(a.disk_low_megabytes.max(1)),
                ),
            ],
        );
        if a.endpoint_down_critical_seconds < a.endpoint_down_seconds {
            self.schema(
                "alerts.endpoint_down_critical_seconds",
                "must be at least endpoint_down_seconds",
            );
        }
    }
}
