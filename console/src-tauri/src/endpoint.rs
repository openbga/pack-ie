//! Where the engine is: the endpoint resolution order and the stored override.
//!
//! Order: the in-memory override (set by `ipc_configure` or read from `console.json`), then
//! `PACKIE_IPC`, then a bootstrap file: `PACKIE_BOOTSTRAP`, else the one the installer puts in the
//! operating system's usual place, else `config/bootstrap.json` in the working directory or
//! the nearest parent that has one (a development checkout).

use std::path::{Path, PathBuf};

use packie_config::Bootstrap;

/// The relative path searched for in the working directory and its parents.
const DEFAULT_BOOTSTRAP: &str = "config/bootstrap.json";
/// The bootstrap files the installers write (`packaging/README.md`), for this operating system.
/// Windows: `%ProgramData%\Positive ACK`; macOS: `/Library/Application Support/Positive ACK`;
/// Linux: `/etc/packie`. A macOS user agent has a bootstrap its owner chose, so none is searched
/// for it: that one is named with `PACKIE_BOOTSTRAP`.
pub(crate) fn packaged_bootstraps() -> Vec<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var_os("ProgramData")
            .map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        vec![base.join("Positive ACK").join("bootstrap.json")]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from(
            "/Library/Application Support/Positive ACK/bootstrap.json",
        )]
    } else {
        vec![PathBuf::from("/etc/packie/bootstrap.json")]
    }
}

/// The key in `console.json`.
const OVERRIDE_KEY: &str = "ipc_endpoint";

/// Everything resolution depends on, so it can be tested without the process environment.
#[derive(Debug, Clone, Default)]
pub(crate) struct ResolveInput {
    pub(crate) override_endpoint: Option<String>,
    pub(crate) env_ipc: Option<String>,
    pub(crate) env_bootstrap: Option<PathBuf>,
    pub(crate) cwd: Option<PathBuf>,
    /// Where the installers put the bootstrap, tried before the working-directory search.
    pub(crate) packaged: Vec<PathBuf>,
}

impl ResolveInput {
    /// Reads `PACKIE_IPC`, `PACKIE_BOOTSTRAP` and the working directory of this process.
    pub(crate) fn from_process(override_endpoint: Option<String>) -> Self {
        Self {
            override_endpoint,
            env_ipc: std::env::var("PACKIE_IPC").ok(),
            env_bootstrap: std::env::var_os("PACKIE_BOOTSTRAP").map(PathBuf::from),
            cwd: std::env::current_dir().ok(),
            packaged: packaged_bootstraps(),
        }
    }
}

/// File access for resolution.
pub(crate) trait Files {
    fn is_file(&self, path: &Path) -> bool;
    fn load(&self, path: &Path) -> Result<Bootstrap, String>;
}

/// The real file system.
pub(crate) struct DiskFiles;

impl Files for DiskFiles {
    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn load(&self, path: &Path) -> Result<Bootstrap, String> {
        Bootstrap::load(path).map_err(|e| e.to_string())
    }
}

/// A bootstrap document the endpoint came from.
#[derive(Debug, Clone)]
pub(crate) struct LoadedBootstrap {
    pub(crate) path: PathBuf,
    pub(crate) document: Bootstrap,
}

/// A resolved engine location.
#[derive(Debug, Clone)]
pub(crate) struct Resolved {
    pub(crate) endpoint: String,
    /// Set only when the endpoint came from a bootstrap file.
    pub(crate) bootstrap: Option<LoadedBootstrap>,
}

/// Resolution found no usable location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ResolveError {
    /// Nothing names the engine.
    #[error(
        "no engine location is configured: the installed engine's bootstrap file was not found \
         in its usual place; set PACKIE_IPC or PACKIE_BOOTSTRAP, or start the console from a folder \
         that contains config/bootstrap.json"
    )]
    NotConfigured,
    /// A bootstrap file was named or found but could not be used.
    #[error("cannot use the bootstrap file {}: {message}", path.display())]
    Bootstrap { path: PathBuf, message: String },
}

fn non_empty(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// Finds the engine endpoint; see the module comment for the order.
pub(crate) fn resolve_endpoint(
    input: &ResolveInput,
    files: &dyn Files,
) -> Result<Resolved, ResolveError> {
    if let Some(endpoint) = non_empty(&input.override_endpoint).or(non_empty(&input.env_ipc)) {
        return Ok(Resolved {
            endpoint: endpoint.to_owned(),
            bootstrap: None,
        });
    }
    let path = match &input.env_bootstrap {
        Some(path) if !path.as_os_str().is_empty() => path.clone(),
        _ => input
            .packaged
            .iter()
            .find(|candidate| files.is_file(candidate))
            .cloned()
            .or_else(|| find_bootstrap(input.cwd.as_deref(), files))
            .ok_or(ResolveError::NotConfigured)?,
    };
    let document = files
        .load(&path)
        .map_err(|message| ResolveError::Bootstrap {
            path: path.clone(),
            message,
        })?;
    Ok(Resolved {
        endpoint: document.ipc_endpoint(),
        bootstrap: Some(LoadedBootstrap { path, document }),
    })
}

fn find_bootstrap(cwd: Option<&Path>, files: &dyn Files) -> Option<PathBuf> {
    cwd?.ancestors()
        .map(|dir| dir.join(DEFAULT_BOOTSTRAP))
        .find(|candidate| files.is_file(candidate))
}

/// The override stored in `console.json`, if any.
pub(crate) fn load_override(file: &Path) -> Option<String> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(file = %file.display(), %error, "cannot read the console settings");
            return None;
        }
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) => value
            .get(OVERRIDE_KEY)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .filter(|v| !v.trim().is_empty()),
        Err(error) => {
            tracing::warn!(file = %file.display(), %error, "the console settings are not JSON");
            None
        }
    }
}

/// Stores (`Some`) or clears (`None`) the override in `console.json`, keeping other keys.
/// Failures are traced and otherwise ignored.
pub(crate) fn save_override(file: &Path, endpoint: Option<&str>) {
    let mut document = std::fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    match endpoint {
        Some(endpoint) => {
            document.insert(OVERRIDE_KEY.into(), endpoint.into());
        }
        None => {
            document.remove(OVERRIDE_KEY);
        }
    }
    let result = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| {
            let text = serde_json::to_string_pretty(&document).map_err(std::io::Error::other)?;
            std::fs::write(file, text)
        });
    if let Err(error) = result {
        tracing::warn!(file = %file.display(), %error, "cannot save the console settings");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Files held in memory: path -> endpoint written in the fake bootstrap.
    struct Fake(HashMap<PathBuf, Result<String, String>>);

    impl Files for Fake {
        fn is_file(&self, path: &Path) -> bool {
            self.0.contains_key(path)
        }

        fn load(&self, path: &Path) -> Result<Bootstrap, String> {
            match self.0.get(path) {
                Some(Ok(endpoint)) => Ok(serde_json::from_value(serde_json::json!({
                    "node": {"node_id": "n1", "display_name": "N"},
                    "paths": {"state_directory": "/state", "log_directory": "/logs"},
                    "ipc": {"path": endpoint},
                }))
                .unwrap()),
                Some(Err(message)) => Err(message.clone()),
                None => Err("missing".into()),
            }
        }
    }

    fn files(entries: &[(&str, Result<&str, &str>)]) -> Fake {
        Fake(
            entries
                .iter()
                .map(|(path, result)| {
                    (
                        PathBuf::from(path),
                        result.map(str::to_owned).map_err(str::to_owned),
                    )
                })
                .collect(),
        )
    }

    fn input() -> ResolveInput {
        ResolveInput {
            cwd: Some(PathBuf::from("/repo/console/src-tauri")),
            ..ResolveInput::default()
        }
    }

    #[test]
    fn override_beats_env_beats_bootstrap() {
        let fake = files(&[("/repo/config/bootstrap.json", Ok("/from/file"))]);
        let mut input = input();
        input.env_ipc = Some("/from/env".into());
        input.env_bootstrap = Some("/repo/config/bootstrap.json".into());
        input.override_endpoint = Some("/from/override".into());

        let found = resolve_endpoint(&input, &fake).unwrap();
        assert_eq!(found.endpoint, "/from/override");
        assert!(found.bootstrap.is_none());

        input.override_endpoint = Some("  ".into());
        let found = resolve_endpoint(&input, &fake).unwrap();
        assert_eq!(found.endpoint, "/from/env");
        assert!(found.bootstrap.is_none());

        input.env_ipc = None;
        let found = resolve_endpoint(&input, &fake).unwrap();
        assert_eq!(found.endpoint, "/from/file");
        assert!(found.bootstrap.is_some());
    }

    #[test]
    fn an_installed_bootstrap_comes_before_the_working_directory_search() {
        let fake = files(&[
            ("/etc/packie/bootstrap.json", Ok("/installed")),
            ("/repo/config/bootstrap.json", Ok("/checkout")),
        ]);
        let mut input = input();
        input.packaged = vec![PathBuf::from("/etc/packie/bootstrap.json")];
        assert_eq!(
            resolve_endpoint(&input, &fake).unwrap().endpoint,
            "/installed"
        );
        // The environment still wins over the installed location.
        input.env_bootstrap = Some("/repo/config/bootstrap.json".into());
        assert_eq!(
            resolve_endpoint(&input, &fake).unwrap().endpoint,
            "/checkout"
        );
    }

    #[test]
    fn falls_back_to_the_working_directory_when_nothing_is_installed() {
        let fake = files(&[("/repo/config/bootstrap.json", Ok("/checkout"))]);
        let mut input = input();
        input.packaged = vec![PathBuf::from("/etc/packie/bootstrap.json")];
        assert_eq!(
            resolve_endpoint(&input, &fake).unwrap().endpoint,
            "/checkout"
        );
    }

    #[test]
    fn the_installed_locations_are_those_the_installers_write() {
        let paths = packaged_bootstraps();
        assert_eq!(paths.len(), 1);
        let text = paths[0].to_string_lossy().replace('\\', "/");
        assert!(
            text.ends_with("Positive ACK/bootstrap.json") || text == "/etc/packie/bootstrap.json",
            "{text}"
        );
    }

    #[test]
    fn searches_parent_directories_for_the_bootstrap_file() {
        let fake = files(&[("/repo/config/bootstrap.json", Ok("/sock"))]);
        let found = resolve_endpoint(&input(), &fake).unwrap();
        assert_eq!(found.endpoint, "/sock");
        let loaded = found.bootstrap.unwrap();
        assert_eq!(loaded.path, PathBuf::from("/repo/config/bootstrap.json"));
        assert_eq!(loaded.document.node.node_id, "n1");

        // The nearest directory wins.
        let fake = files(&[
            ("/repo/config/bootstrap.json", Ok("/outer")),
            ("/repo/console/config/bootstrap.json", Ok("/inner")),
        ]);
        assert_eq!(
            resolve_endpoint(&input(), &fake).unwrap().endpoint,
            "/inner"
        );
    }

    #[test]
    fn env_bootstrap_is_used_without_searching() {
        let fake = files(&[
            ("/repo/config/bootstrap.json", Ok("/searched")),
            ("/elsewhere/b.json", Ok("/named")),
        ]);
        let mut input = input();
        input.env_bootstrap = Some("/elsewhere/b.json".into());
        assert_eq!(resolve_endpoint(&input, &fake).unwrap().endpoint, "/named");

        input.env_bootstrap = Some("/elsewhere/missing.json".into());
        assert!(matches!(
            resolve_endpoint(&input, &fake),
            Err(ResolveError::Bootstrap { .. })
        ));
    }

    #[test]
    fn nothing_configured_and_unusable_files_are_distinct_errors() {
        let empty = files(&[]);
        assert_eq!(
            resolve_endpoint(&input(), &empty).unwrap_err(),
            ResolveError::NotConfigured
        );
        let no_cwd = ResolveInput::default();
        assert_eq!(
            resolve_endpoint(&no_cwd, &empty).unwrap_err(),
            ResolveError::NotConfigured
        );
        let broken = files(&[("/repo/config/bootstrap.json", Err("node.id is invalid"))]);
        let error = resolve_endpoint(&input(), &broken).unwrap_err();
        assert!(error.to_string().contains("node.id is invalid"));
        assert!(error.to_string().contains("bootstrap.json"));
    }

    #[test]
    fn the_override_file_round_trips_and_keeps_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested").join("console.json");
        assert_eq!(load_override(&file), None);

        save_override(&file, Some("/tmp/packie.sock"));
        assert_eq!(load_override(&file).as_deref(), Some("/tmp/packie.sock"));

        let mut doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        doc["theme"] = "dark".into();
        std::fs::write(&file, doc.to_string()).unwrap();
        save_override(&file, Some("/other"));
        assert_eq!(load_override(&file).as_deref(), Some("/other"));

        save_override(&file, None);
        assert_eq!(load_override(&file), None);
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(doc["theme"], "dark");

        std::fs::write(&file, "not json").unwrap();
        assert_eq!(load_override(&file), None);
    }
}
