//! Reports: one record per step, then a scenario summary (scenarios.md §8 "Reporting").

use std::fmt::Write as _;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::json;

/// Result of one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    /// The step held.
    Passed,
    /// The step failed; the scenario ended here.
    Failed,
    /// An earlier step failed.
    NotRun,
}

/// Result of one scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Every step and global assertion held.
    Passed,
    /// A step or a global assertion failed.
    Failed,
    /// The host lacks a required capability.
    Skipped,
}

/// One step's record.
#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    /// Scenario name.
    pub scenario: String,
    /// Position in the file.
    pub index: usize,
    /// The step's `id`, if it has one.
    pub id: Option<String>,
    /// The `do` value.
    #[serde(rename = "do")]
    pub action: String,
    /// Actor name or `engine`.
    pub actor: String,
    /// Result.
    pub status: StepStatus,
    /// Wall-clock milliseconds.
    pub duration_ms: u64,
    /// Expected versus observed for failures, a short note otherwise.
    pub detail: Option<String>,
}

/// A scenario's result.
#[derive(Debug, Clone)]
pub struct ScenarioReport {
    /// Scenario name.
    pub name: String,
    /// Result.
    pub outcome: Outcome,
    /// Why it was skipped.
    pub skip_reason: Option<String>,
    /// Every step, in order.
    pub steps: Vec<StepReport>,
    /// Whole-run milliseconds.
    pub duration_ms: u64,
    /// Failure of the run itself (engine start, scenario timeout) when no step is to blame.
    pub run_failure: Option<String>,
    /// Failed global assertions.
    pub global_failures: Vec<String>,
    /// The run directory, kept on failure.
    pub kept_dir: Option<PathBuf>,
    /// The engine's last output lines, for failures.
    pub engine_output: Vec<String>,
    /// The engine's last log events as JSON lines, for failures of `--verbose` runs.
    pub engine_log: Vec<String>,
}

impl ScenarioReport {
    /// A skipped scenario.
    #[must_use]
    pub fn skipped(name: &str, reason: String) -> Self {
        Self {
            name: name.to_owned(),
            outcome: Outcome::Skipped,
            skip_reason: Some(reason),
            steps: Vec::new(),
            duration_ms: 0,
            run_failure: None,
            global_failures: Vec::new(),
            kept_dir: None,
            engine_output: Vec::new(),
            engine_log: Vec::new(),
        }
    }

    /// The failed step, if any.
    #[must_use]
    pub fn failed_step(&self) -> Option<&StepReport> {
        self.steps.iter().find(|s| s.status == StepStatus::Failed)
    }

    /// One JSON object per step, then the summary object.
    #[must_use]
    pub fn json_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .steps
            .iter()
            .filter_map(|s| serde_json::to_string(s).ok())
            .collect();
        lines.push(
            json!({
                "scenario": self.name,
                "summary": self.outcome,
                "duration_ms": self.duration_ms,
                "skip_reason": self.skip_reason,
                "run_failure": self.run_failure,
                "global_failures": self.global_failures,
                "state_dir": self.kept_dir,
                "engine_output": self.engine_output,
                "engine_log": self.engine_log,
            })
            .to_string(),
        );
        lines
    }

    /// A human-readable report. `verbose` adds passed steps.
    #[must_use]
    pub fn human(&self, verbose: bool) -> String {
        let mut out = String::new();
        let label = match self.outcome {
            Outcome::Passed => "PASS",
            Outcome::Failed => "FAIL",
            Outcome::Skipped => "SKIP",
        };
        let _ = writeln!(out, "{label} {} ({} ms)", self.name, self.duration_ms);
        if let Some(reason) = &self.skip_reason {
            let _ = writeln!(out, "  skipped: {reason}");
        }
        if verbose || self.outcome == Outcome::Failed {
            for step in &self.steps {
                write_step(&mut out, step, verbose);
            }
        }
        if let Some(failure) = &self.run_failure {
            let _ = writeln!(out, "  {}", failure.replace('\n', "\n  "));
        }
        for failure in &self.global_failures {
            let _ = writeln!(out, "  global assertion failed: {failure}");
        }
        if self.outcome == Outcome::Failed && !self.engine_output.is_empty() {
            let _ = writeln!(
                out,
                "  engine output (last {} lines):",
                self.engine_output.len()
            );
            for line in &self.engine_output {
                let _ = writeln!(out, "    {line}");
            }
        }
        if self.outcome == Outcome::Failed && !self.engine_log.is_empty() {
            let _ = writeln!(out, "  engine log (last {} events):", self.engine_log.len());
            for line in &self.engine_log {
                let _ = writeln!(out, "    {line}");
            }
        }
        if let Some(dir) = &self.kept_dir {
            let _ = writeln!(out, "  state kept: {}", dir.display());
        }
        out
    }

    /// The reason shown in the summary table.
    #[must_use]
    pub fn reason(&self) -> String {
        match self.outcome {
            Outcome::Passed => String::new(),
            Outcome::Skipped => self.skip_reason.clone().unwrap_or_default(),
            Outcome::Failed => {
                if let Some(step) = self.failed_step() {
                    let first = step
                        .detail
                        .as_deref()
                        .and_then(|d| d.lines().next())
                        .unwrap_or("");
                    format!("step {} {}: {first}", step.index, step.action)
                } else if let Some(run) = &self.run_failure {
                    run.lines().next().unwrap_or_default().to_owned()
                } else {
                    self.global_failures.first().cloned().unwrap_or_default()
                }
            }
        }
    }
}

/// One step line, and its detail for failures (and for every step when `verbose`).
fn write_step(out: &mut String, step: &StepReport, verbose: bool) {
    if !verbose && step.status != StepStatus::Failed {
        return;
    }
    let mark = match step.status {
        StepStatus::Passed => "ok",
        StepStatus::Failed => "FAILED",
        StepStatus::NotRun => "not run",
    };
    let id = step
        .id
        .as_deref()
        .map(|i| format!(" [{i}]"))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "  step {} {} ({}){id}: {mark} ({} ms)",
        step.index, step.action, step.actor, step.duration_ms
    );
    if let Some(detail) = &step.detail {
        let _ = writeln!(out, "    {}", detail.replace('\n', "\n    "));
    }
}

/// Characters XML 1.0 allows in text.
fn xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..)
}

/// `text` escaped for an XML attribute or element, without characters XML cannot carry.
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(|c| xml_char(*c)) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

fn seconds(duration_ms: u64) -> String {
    format!("{}.{:03}", duration_ms / 1000, duration_ms % 1000)
}

/// A JUnit XML report of `reports`: one test case per scenario. A failure carries the
/// human report (failed step with expected and observed values, engine output, the
/// kept state directory); a skip carries its reason. Suitable for CI test-report
/// publishers.
#[must_use]
pub fn junit(reports: &[ScenarioReport]) -> String {
    let count = |o: Outcome| reports.iter().filter(|r| r.outcome == o).count();
    let total_ms: u64 = reports.iter().map(|r| r.duration_ms).sum();
    let attrs = format!(
        r#"tests="{}" failures="{}" errors="0" skipped="{}" time="{}""#,
        reports.len(),
        count(Outcome::Failed),
        count(Outcome::Skipped),
        seconds(total_ms)
    );
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(out, r#"<testsuites name="packie-sim" {attrs}>"#);
    let _ = writeln!(out, r#"  <testsuite name="scenarios" {attrs}>"#);
    for r in reports {
        let name = xml_escape(&r.name);
        let time = seconds(r.duration_ms);
        let head = format!(r#"    <testcase classname="scenarios" name="{name}" time="{time}""#);
        match r.outcome {
            Outcome::Passed => {
                let _ = writeln!(out, "{head}/>");
            }
            Outcome::Skipped => {
                let reason = xml_escape(&r.reason());
                let _ = writeln!(
                    out,
                    "{head}>\n      <skipped message=\"{reason}\"/>\n    </testcase>"
                );
            }
            Outcome::Failed => {
                let message = xml_escape(&r.reason());
                let body = xml_escape(r.human(false).trim_end());
                let _ = writeln!(
                    out,
                    "{head}>\n      <failure message=\"{message}\" type=\"ScenarioFailed\">{body}</failure>\n    </testcase>"
                );
            }
        }
    }
    out.push_str("  </testsuite>\n</testsuites>\n");
    out
}
