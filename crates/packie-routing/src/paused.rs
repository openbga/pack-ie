//! Operator pause flags, persisted so they survive an engine restart
//! (`docs/spec/bridge-channels.md` §12).
//!
//! Stored as `<state>/routing/paused.json`: `{"paused": ["<channel id>", ...]}`, written
//! atomically. A missing or unreadable file means nothing is paused (logged).

use std::collections::BTreeSet;
use std::path::PathBuf;

use packie_core::ChannelId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PausedFile {
    paused: BTreeSet<ChannelId>,
}

/// The paused channels and where they are saved.
#[derive(Debug)]
pub(crate) struct PausedSet {
    path: PathBuf,
    ids: BTreeSet<ChannelId>,
}

impl PausedSet {
    /// Reads the set at `path`.
    pub(crate) fn load(path: PathBuf) -> Self {
        let ids = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<PausedFile>(&bytes) {
                Ok(file) => file.paused,
                Err(error) => {
                    tracing::warn!(
                        event = "routing.paused_unreadable",
                        path = %path.display(),
                        %error,
                        "Could not read the paused-channel list; no channel starts paused"
                    );
                    BTreeSet::new()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(error) => {
                tracing::warn!(
                    event = "routing.paused_unreadable",
                    path = %path.display(),
                    %error,
                    "Could not read the paused-channel list; no channel starts paused"
                );
                BTreeSet::new()
            }
        };
        Self { path, ids }
    }

    /// Whether `id` is paused.
    pub(crate) fn contains(&self, id: ChannelId) -> bool {
        self.ids.contains(&id)
    }

    /// Pauses `id`.
    pub(crate) fn insert(&mut self, id: ChannelId) {
        if self.ids.insert(id) {
            self.save();
        }
    }

    /// Unpauses `id`.
    pub(crate) fn remove(&mut self, id: ChannelId) {
        if self.ids.remove(&id) {
            self.save();
        }
    }

    /// Forgets channels that are no longer configured.
    pub(crate) fn retain(&mut self, configured: &BTreeSet<ChannelId>) {
        let before = self.ids.len();
        self.ids.retain(|id| configured.contains(id));
        if self.ids.len() != before {
            self.save();
        }
    }

    fn save(&self) {
        let result = (|| {
            if let Some(dir) = self.path.parent() {
                packie_config::create_private_dir(dir)?;
            }
            let file = PausedFile {
                paused: self.ids.clone(),
            };
            let text = serde_json::to_vec_pretty(&file).unwrap_or_default();
            packie_config::write_atomic(&self.path, &text)
        })();
        if let Err(error) = result {
            tracing::warn!(
                event = "routing.paused_not_saved",
                path = %self.path.display(),
                %error,
                "Could not save the paused-channel list; the pause holds until the engine restarts"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn survives_a_reload_and_forgets_removed_channels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routing").join("paused.json");
        let (kept, removed) = (ChannelId::generate(), ChannelId::generate());
        let mut set = PausedSet::load(path.clone());
        set.insert(kept);
        set.insert(removed);
        let mut reloaded = PausedSet::load(path.clone());
        assert!(reloaded.contains(kept) && reloaded.contains(removed));
        reloaded.retain(&BTreeSet::from([kept]));
        reloaded.remove(ChannelId::generate());
        let again = PausedSet::load(path);
        assert!(again.contains(kept));
        assert!(!again.contains(removed));
    }

    #[test]
    fn unreadable_file_pauses_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paused.json");
        std::fs::write(&path, b"not json").unwrap();
        assert!(!PausedSet::load(path).contains(ChannelId::generate()));
    }
}
