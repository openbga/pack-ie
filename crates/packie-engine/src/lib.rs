//! The Positive ACK engine: the composition root that `packie run` and the OS service call
//! (`docs/architecture.md` §1).
//!
//! [`run`] loads `bootstrap.json`, starts logging, loads the newest valid saved runtime
//! version (importing the runtime file, `runtime.file`, as the next version when it differs),
//! applies it to the [`ChannelRegistry`], serves the management interface and publishes an
//! `EngineSnapshot` every `diagnostics.snapshot_interval_seconds` and on change, evaluating
//! alerts each time. It stops when the `shutdown` future completes.
//!
//! Channels and the message store are supplied through [`ChannelRegistry`], passed in by the
//! caller so that `packie-routing` can implement it without a dependency cycle.
//! [`NoopRegistry`] runs nothing; its message methods answer `not_implemented`.
//!
//! Configuration changes arrive as `put_config`: the document is parsed and validated
//! (filters and mappings included; all problems returned, nothing changes on failure), its
//! `config_version` must equal the active version, it is saved atomically as the next
//! version, and the per-channel diff is applied. Applies, channel commands, resends and
//! closes are serialized, and each operator command is logged as `operator.command`.
//! `GetMessage` adds the original's parsed fields (HL7 and ASTM). With `test_hooks` in the
//! bootstrap, [`EngineContext::test_hooks`] carries the scenario hooks (hold store commits)
//! and `crash` ends the process.

mod audit;
mod backups;
mod checks;
mod engine;
mod exports;
mod fields;
mod handler;
mod registry;
mod reports;

pub use audit::{AuditLog, AuditOpener, AuditRecord, AuditUnavailable, MemoryAudit};
pub use engine::{ENGINE_VERSION, EngineError, RunOptions, run};
pub use registry::{
    BoxFuture, ChannelRegistry, EngineContext, NoopRegistry, RegistryError, RegistrySnapshot,
    TestHooks,
};
