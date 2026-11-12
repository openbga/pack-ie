//! The `folder` actor: writes and expects files in a temporary directory.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::time::Instant;

use crate::payload::{PayloadSpec, preview};

/// Interval at which a folder is re-read while waiting for a file.
const POLL: Duration = Duration::from_millis(25);
/// A file counts as complete once its size and modification time stay unchanged this long
/// (three polls): the engine may still be writing it when it first appears.
const STABLE_FOR: Duration = Duration::from_millis(75);

/// What `expect_file` requires of the content.
#[derive(Debug, Clone)]
pub enum ContentRule {
    /// Any content, once the file stops changing.
    Any,
    /// Exactly these bytes.
    Equals(Vec<u8>),
    /// Each string occurs in the text.
    Contains(Vec<String>),
}

/// The folder owner.
#[derive(Debug, Clone)]
pub struct FolderActor {
    dir: PathBuf,
}

impl FolderActor {
    /// An actor for `dir`.
    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Writes `name`; with `atomic` under a temporary name first, then renames.
    pub fn write_file(&self, name: &str, bytes: &[u8], atomic: bool) -> Result<(), String> {
        let target = self.dir.join(name);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        if atomic {
            let temp = target.with_file_name(format!(
                ".{}.packie-sim-tmp",
                target
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("file")
            ));
            std::fs::write(&temp, bytes).map_err(|e| format!("write {}: {e}", temp.display()))?;
            std::fs::rename(&temp, &target)
                .map_err(|e| format!("rename to {}: {e}", target.display()))
        } else {
            std::fs::write(&target, bytes).map_err(|e| format!("write {}: {e}", target.display()))
        }
    }

    /// Removes `name`.
    pub fn delete_file(&self, name: &str) -> Result<(), String> {
        let target = self.dir.join(name);
        std::fs::remove_file(&target).map_err(|e| format!("delete {}: {e}", target.display()))
    }

    /// Files (relative, `/` separated) matching `pattern`, sorted.
    #[must_use]
    pub fn matching(&self, pattern: &str) -> Vec<String> {
        let depth = pattern.split('/').count();
        let mut found = Vec::new();
        collect(&self.dir, "", depth, &mut found);
        found.retain(|f| glob_match(pattern, f));
        found.sort();
        found
    }

    /// Waits for a complete matching file whose content satisfies `rule`.
    pub async fn expect_file(
        &self,
        selector: &Selector,
        rule: &ContentRule,
        timeout: Duration,
    ) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        let mut last = String::from("no matching file");
        loop {
            let mut observations = Vec::new();
            for name in &self.candidates(selector) {
                match self.check_content(name, rule).await {
                    Ok(()) => return Ok(name.clone()),
                    Err(why) => observations.push(format!("{name}: {why}")),
                }
            }
            if !observations.is_empty() {
                last = observations.join("\n    ");
            }
            if Instant::now() >= deadline {
                return Err(format!(
                    "timed out after {} ms; last observation:\n    {last}",
                    timeout.as_millis()
                ));
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Files the selector names now.
    fn candidates(&self, selector: &Selector) -> Vec<String> {
        match selector {
            Selector::Name(name) => Some(name.clone())
                .filter(|n| self.dir.join(n).is_file())
                .into_iter()
                .collect(),
            Selector::Pattern(pattern) => self.matching(pattern),
        }
    }

    async fn check_content(&self, name: &str, rule: &ContentRule) -> Result<(), String> {
        let path = self.dir.join(name);
        let content = std::fs::read(&path).map_err(|e| format!("cannot read: {e}"))?;
        match rule {
            ContentRule::Any => {
                tokio::time::sleep(STABLE_FOR).await;
                let again = std::fs::read(&path).map_err(|e| format!("cannot read: {e}"))?;
                if again == content {
                    Ok(())
                } else {
                    Err("still being written".into())
                }
            }
            ContentRule::Equals(want) => {
                if &content == want {
                    Ok(())
                } else {
                    Err(format!(
                        "content differs: expected {} bytes {}, observed {} bytes {}",
                        want.len(),
                        preview(want, 120),
                        content.len(),
                        preview(&content, 120)
                    ))
                }
            }
            ContentRule::Contains(parts) => {
                let text = String::from_utf8_lossy(&content);
                match parts.iter().find(|p| !text.contains(p.as_str())) {
                    None => Ok(()),
                    Some(missing) => Err(format!(
                        "does not contain {missing:?}; content: {}",
                        preview(&content, 120)
                    )),
                }
            }
        }
    }

    /// Passes when no file matches `pattern` during `window`.
    pub async fn expect_no_file(&self, pattern: &str, window: Duration) -> Result<(), String> {
        let deadline = Instant::now() + window;
        loop {
            let found = self.matching(pattern);
            if let Some(first) = found.first() {
                return Err(format!("file `{first}` matches `{pattern}`"));
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            tokio::time::sleep(POLL).await;
        }
    }
}

/// How `expect_file` finds files.
#[derive(Debug, Clone)]
pub enum Selector {
    /// An exact relative name.
    Name(String),
    /// A glob.
    Pattern(String),
}

/// Builds the content rule of an `expect_file` step.
pub fn content_rule(
    equals: Option<&PayloadSpec>,
    contains: Option<&Vec<String>>,
) -> Result<ContentRule, String> {
    match (equals, contains) {
        (Some(p), _) => p
            .bytes()
            .map(ContentRule::Equals)
            .map_err(|e| e.to_string()),
        (None, Some(c)) => Ok(ContentRule::Contains(c.clone())),
        (None, None) => Ok(ContentRule::Any),
    }
}

fn collect(root: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth > 1 {
                collect(&entry.path(), &rel, depth - 1, out);
            }
        } else if kind.is_file() {
            out.push(rel);
        }
    }
}

/// Glob with `*` (any run inside one path segment) and `?` (one character).
#[must_use]
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    let parts: Vec<&str> = path.split('/').collect();
    pat.len() == parts.len() && pat.iter().zip(&parts).all(|(p, t)| segment_match(p, t))
}

fn segment_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) && p[pi] != '*' {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_do_not_cross_directories() {
        assert!(glob_match("orig-*.csv", "orig-1.csv"));
        assert!(glob_match("*.csv", ".csv"));
        assert!(!glob_match("*.csv", "a/b.csv"));
        assert!(glob_match("a/*.txt", "a/x.txt"));
        assert!(glob_match("f?le", "file"));
        assert!(!glob_match("f?le", "fle"));
    }
}
