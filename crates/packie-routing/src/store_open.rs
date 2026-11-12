//! Opening the message store, and opening it again later when it fails at start
//! (`docs/spec/message-channels.md` §13): bridges run meanwhile, message channels report
//! `stopped` (`store_unavailable`), and the registry retries with backoff.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use packie_engine::EngineContext;

use crate::message::{MessageStore, SqliteStore};
use crate::{TapObservation, TapSink};

/// A store that opened, with the sink that records bridge-tap observations in it.
pub struct OpenedStore {
    /// The message store.
    pub store: Arc<dyn MessageStore>,
    /// Records tap observations, when the store supports it.
    pub tap_sink: Option<Arc<dyn TapSink>>,
}

/// Opens the message store; `Err` names why (for the log). Called again with backoff
/// until it succeeds.
pub type StoreOpener = Arc<dyn Fn() -> Result<OpenedStore, String> + Send + Sync>;

/// How long the registry waits between attempts to open the store: `initial`, doubling up
/// to `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreRetry {
    /// Before the second attempt.
    pub initial: Duration,
    /// The longest wait.
    pub max: Duration,
}

impl Default for StoreRetry {
    /// 10 seconds, doubling to 5 minutes.
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(10),
            max: Duration::from_secs(300),
        }
    }
}

/// The SQLite store under `<state>/store`, with the store's test-hook commit gate.
#[must_use]
pub fn sqlite_store_opener(context: &EngineContext) -> StoreOpener {
    let directory = context.state_dir.join("store");
    let open = packie_store::OpenOptions {
        commits_held: context
            .test_hooks
            .as_ref()
            .map(packie_engine::TestHooks::commits_held),
        ..packie_store::OpenOptions::default()
    };
    Arc::new(move || {
        let store =
            packie_store::Store::open(&directory, open.clone()).map_err(|e| e.to_string())?;
        let adapter = SqliteStore::new(store);
        Ok(OpenedStore {
            tap_sink: adapter.tap_sink().map(|s| Arc::new(s) as Arc<dyn TapSink>),
            store: Arc::new(adapter),
        })
    })
}

/// The tap sink bridges hold while the store is not open yet: observations are dropped
/// until the store opens, then recorded, without restarting the bridges.
#[derive(Default)]
pub(crate) struct DeferredTapSink {
    sink: OnceLock<Arc<dyn TapSink>>,
}

impl DeferredTapSink {
    /// Starts recording into `sink`.
    pub(crate) fn connect(&self, sink: Arc<dyn TapSink>) {
        // Set once; the store opens once.
        drop(self.sink.set(sink));
    }
}

impl TapSink for DeferredTapSink {
    fn observed(&self, observation: TapObservation) {
        if let Some(sink) = self.sink.get() {
            sink.observed(observation);
        }
    }
}
