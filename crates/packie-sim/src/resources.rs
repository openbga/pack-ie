//! Resource allocation (scenarios.md §3) and host capabilities.
//!
//! Per run: free loopback ports, a fresh run directory (state, logs, folders, serial
//! links) and virtual serial pairs. Nothing is hard-coded, so scenarios run in parallel.

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tempfile::TempDir;

use crate::actors::serial::{SerialPair, serial_pair_supported};
use crate::error::SimError;
use crate::placeholders::Placeholders;
use crate::ports::PortLease;
use crate::scenario::{Capability, Scenario};

/// What the host provides.
#[derive(Debug, Clone, Default)]
pub struct Capabilities(BTreeSet<Capability>);

impl Capabilities {
    /// Probes the host and, when `probe_engine` is set, the engine binary (which alone
    /// knows whether it has test hooks; the probe starts it once).
    pub async fn detect(engine_bin: &Path, probe_engine: bool) -> Self {
        let mut set = BTreeSet::new();
        if serial_pair_supported() {
            set.insert(Capability::SerialPair);
        }
        let alias = {
            let _creating = crate::fdguard::creating();
            std::net::TcpListener::bind(("127.0.0.2", 0)).is_ok()
        };
        if alias {
            set.insert(Capability::LoopbackAlias);
        }
        if probe_engine
            && crate::engine::probe_capabilities(engine_bin)
                .await
                .iter()
                .any(|c| c == "test_hooks")
        {
            set.insert(Capability::TestHooks);
        }
        Self(set)
    }

    /// A fixed set, for tests.
    #[must_use]
    pub fn of(capabilities: impl IntoIterator<Item = Capability>) -> Self {
        Self(capabilities.into_iter().collect())
    }

    /// The capabilities in `required` that are missing.
    #[must_use]
    pub fn missing(&self, required: &BTreeSet<Capability>) -> Vec<Capability> {
        required
            .iter()
            .filter(|c| !self.0.contains(c))
            .copied()
            .collect()
    }
}

/// The per-run directory tree.
#[derive(Debug)]
pub struct RunDir {
    root: PathBuf,
    guard: Option<TempDir>,
}

impl RunDir {
    /// Creates a fresh directory under a path short enough for a Unix socket.
    pub fn create() -> Result<Self, SimError> {
        let preferred = std::env::temp_dir();
        // `sun_path` holds about 100 bytes: prefix + random part + `/state/packie.sock`.
        let base = if preferred.as_os_str().len() + 1 + 15 + 15 > 100 && Path::new("/tmp").is_dir()
        {
            PathBuf::from("/tmp")
        } else {
            preferred
        };
        let dir = tempfile::Builder::new()
            .prefix("packie-sim-")
            .rand_bytes(6)
            .tempdir_in(&base)
            .map_err(|e| {
                SimError::io(format!("create a run directory in {}", base.display()), e)
            })?;
        let root = dir.path().to_owned();
        for sub in ["state", "logs", "folders", "serial"] {
            std::fs::create_dir_all(root.join(sub))
                .map_err(|e| SimError::io(format!("create {sub} directory"), e))?;
        }
        Ok(Self {
            root,
            guard: Some(dir),
        })
    }

    /// The run directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The engine state directory (`${state_dir}`).
    #[must_use]
    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    /// The engine log directory.
    #[must_use]
    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Stops cleanup on drop; the directory stays for inspection.
    pub fn keep(&mut self) -> PathBuf {
        if let Some(guard) = self.guard.take() {
            let _ = guard.keep();
        }
        self.root.clone()
    }

    /// Removes the directory now.
    pub fn remove(&mut self) {
        if let Some(guard) = self.guard.take() {
            let _ = guard.close();
        }
    }
}

/// Everything allocated for one run.
#[derive(Debug)]
pub struct Allocation {
    /// The run directory.
    pub run_dir: RunDir,
    /// Placeholder values.
    pub placeholders: Placeholders,
    /// Virtual serial pairs by name.
    pub serial_pairs: BTreeMap<String, Arc<SerialPair>>,
    /// Folders by name.
    pub folders: BTreeMap<String, PathBuf>,
    /// Reservations of the ports of `tcp_server` actors, by actor name. The actor takes the
    /// bound socket; the lock stays until the allocation is dropped.
    pub server_leases: BTreeMap<String, PortLease>,
    /// Reservations of the `resources.ports`, which the engine binds.
    engine_leases: Vec<PortLease>,
}

impl Allocation {
    /// Closes the sockets held on the engine's ports so that the engine can bind them.
    /// The ports stay reserved against other runs until the allocation is dropped.
    pub fn release_engine_ports(&mut self) {
        self.engine_leases
            .iter_mut()
            .for_each(PortLease::release_socket);
    }
}

/// Allocates the resources `scenario` declares.
pub fn allocate(scenario: &Scenario) -> Result<Allocation, SimError> {
    let run_dir = RunDir::create()?;
    let mut placeholders = Placeholders::new();
    let mut engine_leases = Vec::new();
    for name in &scenario.resources.ports {
        let lease = PortLease::reserve_loopback()?;
        placeholders.set_number(format!("port.{name}"), u64::from(lease.port()));
        engine_leases.push(lease);
    }
    let mut folders = BTreeMap::new();
    for name in &scenario.resources.folders {
        let dir = run_dir.root().join("folders").join(name);
        std::fs::create_dir_all(&dir)
            .map_err(|e| SimError::io(format!("create folder {name}"), e))?;
        placeholders.set_text(format!("folder.{name}"), dir.to_string_lossy());
        folders.insert(name.clone(), dir);
    }
    let mut serial_pairs = BTreeMap::new();
    for name in &scenario.resources.serial_pairs {
        let link = run_dir.root().join("serial").join(format!("{name}.tty"));
        let pair = SerialPair::create(link)?;
        placeholders.set_text(
            format!("serial.{name}.engine"),
            pair.engine_path().to_string_lossy(),
        );
        placeholders.set_text(format!("serial.{name}.peer"), format!("pty-master:{name}"));
        serial_pairs.insert(name.clone(), Arc::new(pair));
    }
    let mut server_leases = BTreeMap::new();
    for (name, actor) in &scenario.actors {
        if actor.get("kind").and_then(Value::as_str) == Some("tcp_server") {
            let host = actor
                .get("bind")
                .and_then(Value::as_str)
                .unwrap_or("127.0.0.1");
            // An address that is not an IP fails later, with the actor's own message.
            let bind: IpAddr = host
                .parse()
                .unwrap_or_else(|_| IpAddr::from([127, 0, 0, 1]));
            let recv_buffer = actor
                .get("recv_buffer_bytes")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok());
            let lease = PortLease::reserve(bind, recv_buffer)?;
            placeholders.set_number(format!("actor.{name}.port"), u64::from(lease.port()));
            placeholders.set_text(format!("actor.{name}.host"), host);
            server_leases.insert(name.clone(), lease);
        }
    }
    placeholders.set_text("state_dir", run_dir.state().to_string_lossy());
    placeholders.set_text("scenario.name", scenario.name.clone());
    Ok(Allocation {
        run_dir,
        placeholders,
        serial_pairs,
        folders,
        server_leases,
        engine_leases,
    })
}
