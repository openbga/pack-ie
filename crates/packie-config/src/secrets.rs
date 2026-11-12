//! Secret values behind [`SecretRef`]s (`docs/spec/configuration.md` §11): the OS key
//! store or an owner-only file. Values never reach documents, logs, errors or snapshots.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::bootstrap::{Bootstrap, SecretsProvider};
use crate::secret::SecretRef;
use crate::store::write_atomic;

/// A secret value. `Debug` never shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value, for handing to the protocol that needs it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// A secret could not be read or written. Messages name the secret, never its value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    /// No value is stored under this name.
    #[error("secret {0} is not set")]
    Missing(String),
    /// The name does not match `^[a-z0-9][a-z0-9_.-]{0,62}$`.
    #[error("{0:?} is not a valid secret name")]
    InvalidName(String),
    /// The provider failed.
    #[error("secret store: {0}")]
    Store(String),
}

impl SecretError {
    fn store(cause: impl std::fmt::Display) -> Self {
        Self::Store(cause.to_string())
    }
}

/// Reads and writes secret values by name. Blocking: OS key stores may make IPC calls, so
/// call it from a blocking context (`tokio::task::spawn_blocking`).
pub trait SecretResolver: Send + Sync + 'static {
    /// The value of `secret`.
    ///
    /// # Errors
    ///
    /// [`SecretError::Missing`] when it has no value; [`SecretError::Store`] when the
    /// provider fails.
    fn resolve(&self, secret: &SecretRef) -> Result<Secret, SecretError>;

    /// Stores `value` under `name`, replacing any previous value.
    ///
    /// # Errors
    ///
    /// [`SecretError::InvalidName`] for a malformed `name`; [`SecretError::Store`] when the
    /// provider fails.
    fn set(&self, name: &str, value: &Secret) -> Result<(), SecretError>;

    /// Removes `name`; removing a missing name is not an error.
    ///
    /// # Errors
    ///
    /// [`SecretError::InvalidName`] for a malformed `name`; [`SecretError::Store`] when the
    /// provider fails.
    fn delete(&self, name: &str) -> Result<(), SecretError>;

    /// The names that have values, sorted.
    ///
    /// # Errors
    ///
    /// [`SecretError::Store`] when the provider fails.
    fn names(&self) -> Result<Vec<String>, SecretError>;
}

fn check_name(name: &str) -> Result<(), SecretError> {
    if SecretRef::is_valid_name(name) {
        Ok(())
    } else {
        Err(SecretError::InvalidName(name.to_owned()))
    }
}

/// The key store service name.
pub const KEYRING_SERVICE: &str = "packie";

/// The OS key store (Windows Credential Manager, macOS Keychain, Secret Service), under
/// service `packie` with the secret name as account. Key stores cannot list entries, so the
/// names (never values) are kept in an owner-only index file.
#[derive(Debug, Clone)]
pub struct KeyringSecrets {
    index: PathBuf,
}

impl KeyringSecrets {
    /// Key store secrets whose names are listed in `index`.
    #[must_use]
    pub fn new(index: impl Into<PathBuf>) -> Self {
        Self {
            index: index.into(),
        }
    }

    fn entry(name: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(KEYRING_SERVICE, name).map_err(SecretError::store)
    }

    fn update_index(&self, change: impl FnOnce(&mut BTreeSet<String>)) -> Result<(), SecretError> {
        let mut names = read_names(&self.index)?;
        change(&mut names);
        let text = serde_json::to_vec_pretty(&names).map_err(SecretError::store)?;
        write_atomic(&self.index, &text).map_err(SecretError::store)
    }
}

fn read_names(index: &Path) -> Result<BTreeSet<String>, SecretError> {
    match std::fs::read_to_string(index) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|_| SecretError::Store(format!("{} is not a list of names", index.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(e) => Err(SecretError::Store(format!("{}: {e}", index.display()))),
    }
}

impl SecretResolver for KeyringSecrets {
    fn resolve(&self, secret: &SecretRef) -> Result<Secret, SecretError> {
        match Self::entry(secret.name())?.get_password() {
            Ok(value) => Ok(Secret(value)),
            Err(keyring::Error::NoEntry) => Err(SecretError::Missing(secret.name().to_owned())),
            Err(e) => Err(SecretError::store(e)),
        }
    }

    fn set(&self, name: &str, value: &Secret) -> Result<(), SecretError> {
        check_name(name)?;
        Self::entry(name)?
            .set_password(value.expose())
            .map_err(SecretError::store)?;
        self.update_index(|names| {
            names.insert(name.to_owned());
        })
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        check_name(name)?;
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(SecretError::store(e)),
        }
        self.update_index(|names| {
            names.remove(name);
        })
    }

    fn names(&self) -> Result<Vec<String>, SecretError> {
        Ok(read_names(&self.index)?.into_iter().collect())
    }
}

/// A JSON file of name to value, readable only by its owner (`secrets.provider=file`).
/// Read on every resolve, so edits apply without a restart; written atomically.
#[derive(Debug)]
pub struct FileSecrets {
    path: PathBuf,
    write: Mutex<()>,
}

impl FileSecrets {
    /// Secrets from the file at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            write: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<BTreeMap<String, String>, SecretError> {
        let shown = self.path.display();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            match std::fs::metadata(&self.path) {
                Ok(meta) if meta.permissions().mode() & 0o077 != 0 => {
                    return Err(SecretError::Store(format!(
                        "{shown} is readable by other users; restrict it to the owner (chmod 600)"
                    )));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
                Err(e) => return Err(SecretError::Store(format!("{shown}: {e}"))),
            }
        }
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(SecretError::Store(format!("{shown}: {e}"))),
        };
        serde_json::from_str(&text)
            .map_err(|_| SecretError::Store(format!("{shown} is not a JSON object of strings")))
    }

    fn modify(
        &self,
        change: impl FnOnce(&mut BTreeMap<String, String>),
    ) -> Result<(), SecretError> {
        let _guard = self.write.lock().unwrap_or_else(PoisonError::into_inner);
        let mut values = self.read()?;
        change(&mut values);
        let text = serde_json::to_vec_pretty(&values).map_err(SecretError::store)?;
        // write_atomic writes a new owner-only temporary file and renames it.
        write_atomic(&self.path, &text).map_err(SecretError::store)
    }
}

impl SecretResolver for FileSecrets {
    fn resolve(&self, secret: &SecretRef) -> Result<Secret, SecretError> {
        self.read()?
            .remove(secret.name())
            .map(Secret)
            .ok_or_else(|| SecretError::Missing(secret.name().to_owned()))
    }

    fn set(&self, name: &str, value: &Secret) -> Result<(), SecretError> {
        check_name(name)?;
        self.modify(|values| {
            values.insert(name.to_owned(), value.expose().to_owned());
        })
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        check_name(name)?;
        self.modify(|values| {
            values.remove(name);
        })
    }

    fn names(&self) -> Result<Vec<String>, SecretError> {
        Ok(self.read()?.into_keys().collect())
    }
}

/// Secrets in memory, for tests and simulations.
#[derive(Debug, Default)]
pub struct MapSecrets {
    values: Mutex<HashMap<String, String>>,
}

impl MapSecrets {
    /// Secrets with these values.
    #[must_use]
    pub fn new(values: HashMap<String, String>) -> Self {
        Self {
            values: Mutex::new(values),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.values.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl SecretResolver for MapSecrets {
    fn resolve(&self, secret: &SecretRef) -> Result<Secret, SecretError> {
        self.lock()
            .get(secret.name())
            .map(|v| Secret(v.clone()))
            .ok_or_else(|| SecretError::Missing(secret.name().to_owned()))
    }

    fn set(&self, name: &str, value: &Secret) -> Result<(), SecretError> {
        check_name(name)?;
        self.lock()
            .insert(name.to_owned(), value.expose().to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretError> {
        self.lock().remove(name);
        Ok(())
    }

    fn names(&self) -> Result<Vec<String>, SecretError> {
        let mut names: Vec<String> = self.lock().keys().cloned().collect();
        names.sort();
        Ok(names)
    }
}

/// What is wrong with the secrets file at `path`, if anything: on Unix an existing file must
/// be owner-only. A missing file holds no secrets (the first write creates it owner-only).
/// A file that cannot be reached at all (permission denied on the file or its directory) is
/// a problem only when `reachable` is required: the engine needs it, a client that only
/// loads the bootstrap to find the management interface does not.
pub(crate) fn secrets_file_problem(path: &Path, reachable: bool) -> Option<String> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && !reachable => return None,
        Err(e) => return Some(format!("{}: {e}", path.display())),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Some(format!(
                "{} is readable by other users; restrict it to the owner (chmod 600)",
                path.display()
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = meta;
    None
}

/// Why the secret store the bootstrap selects cannot be used on this computer, if it cannot:
/// the secrets file must be reachable by the engine's account, and the OS key store must be
/// reachable (on Linux, a Secret Service over D-Bus, which a server without a desktop
/// session does not have). The engine refuses to start then.
#[must_use]
pub fn secret_store_problem(bootstrap: &Bootstrap) -> Option<String> {
    if bootstrap.secrets.provider == SecretsProvider::File {
        let file = bootstrap.secrets.file.as_ref()?;
        return secrets_file_problem(file, true).map(|problem| format!("secrets.file: {problem}"));
    }
    keyring::Entry::store_status().as_ref().err().map(|error| {
        format!(
            "secrets.provider is keystore, but the OS key store cannot be used ({error}). On a \
             computer without a Secret Service (a server or VM without a desktop session) use \
             the secrets file instead: \"secrets\": {{\"provider\": \"file\"}} keeps them in \
             <state_directory>/secrets.json, readable by the service account only"
        )
    })
}

/// The secret store the bootstrap selects: the key store (names indexed in
/// `<state>/secrets.names.json`) or the secrets file.
#[must_use]
pub fn secrets_for(bootstrap: &Bootstrap) -> Box<dyn SecretResolver> {
    match (&bootstrap.secrets.provider, &bootstrap.secrets.file) {
        (SecretsProvider::File, Some(file)) => Box::new(FileSecrets::new(file)),
        _ => Box::new(KeyringSecrets::new(
            bootstrap.state_dir().join("secrets.names.json"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(n: &str) -> SecretRef {
        SecretRef::new(n).unwrap()
    }

    #[test]
    fn debug_hides_the_value() {
        assert_eq!(
            format!("{:?}", Secret::new("hunter2")),
            "Secret(<redacted>)"
        );
    }

    #[test]
    fn file_secrets_write_list_and_require_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        let secrets = FileSecrets::new(&path);
        assert!(secrets.names().unwrap().is_empty());
        secrets.set("lis-sftp", &Secret::new("pw")).unwrap();
        secrets.set("partner.key", &Secret::new("k")).unwrap();
        assert_eq!(secrets.names().unwrap(), ["lis-sftp", "partner.key"]);
        assert_eq!(secrets.resolve(&name("lis-sftp")).unwrap().expose(), "pw");
        secrets.delete("partner.key").unwrap();
        assert_eq!(
            secrets.resolve(&name("partner.key")),
            Err(SecretError::Missing("partner.key".into()))
        );
        assert_eq!(
            secrets.set("Bad Name", &Secret::new("x")),
            Err(SecretError::InvalidName("Bad Name".into()))
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            let err = secrets.resolve(&name("lis-sftp")).unwrap_err();
            assert!(err.to_string().contains("readable by other users"), "{err}");
        }
    }

    #[test]
    fn map_secrets_write_and_list() {
        let secrets = MapSecrets::default();
        secrets.set("b", &Secret::new("2")).unwrap();
        secrets.set("a", &Secret::new("1")).unwrap();
        assert_eq!(secrets.names().unwrap(), ["a", "b"]);
    }
}
