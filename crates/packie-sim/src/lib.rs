//! The Positive ACK simulator: scripted peers and the scenario runner.
//!
//! A scenario (`tests/scenarios/*.json`, format in `docs/spec/scenarios.md`) loads an engine
//! configuration, starts scripted peers (actors) on loopback TCP, virtual serial ports and
//! folders, runs ordered steps against a real `packie` engine process and asserts on bytes,
//! snapshots, logs, alerts and the store. The `packie-sim` binary wraps this library:
//! `check` validates files, `run` and `run-all` execute them.
//!
//! # How a run goes
//!
//! | Stage | Module | What happens |
//! | --- | --- | --- |
//! | Load | [`scenario`], `validate` | Parse the file, then resolve placeholders and type every actor and step. All static rules (unknown fields, actors, steps, placeholders; actor and step compatibility; required capabilities; payload forms; async names; engine start and stop order) fail here, before anything starts. `check` is exactly this stage with dummy resource values. |
//! | Skip | [`resources`] | A scenario whose `requires` the host cannot provide is reported `skipped` with the capability name. |
//! | Allocate | [`resources`], [`ports`] | Locked loopback port leases below the ephemeral range, a run directory (state, logs, folders, serial links), pseudo-terminal pairs, `tcp_server` sockets. |
//! | Start | [`runner`], [`engine`] | Actors with `listen: auto` bind; the bootstrap and the runtime file are written; `packie run --bootstrap` starts; the runner waits for the management socket, `node.started` and a first snapshot. |
//! | Steps | `exec` | Steps run strictly in order against actors ([`actors`]) or the engine ([`engine::EngineHost`]); the first failure ends the run and later steps are `not_run`. |
//! | Finish | [`runner`] | Actors close, the engine stops (SIGTERM, killed after the drain budget), global assertions run, the run directory is removed on success and kept (path reported) on failure. |
//!
//! # Where things live
//!
//! - [`steps`]: every step as a typed value ([`steps::Action`]), tagged by `do`.
//! - [`payload`]: `text`, `hex`, `base64` and `generate` payload forms.
//! - [`matcher`]: matchers and dotted paths of assertion steps.
//! - [`actors`]: `tcp_server`, `tcp_client`, `serial_peer`, `folder`; every connection has
//!   a read buffer, a state and a reading flag ([`actors::conn::Conn`]).
//! - [`protocol`]: `send_message` and `expect_message` for HL7 v2 over MLLP and ASTM.
//! - [`snapshot_view`]: translates the engine's snapshot into the field names scenarios
//!   use. The only place that knows the snapshot DTO.
//! - [`store`]: rows for `assert_store`, read through the management interface.
//! - [`engine`]: bootstrap generation, process launch, management session, log files, global
//!   checks, capability probe.
//! - [`report`]: per-step records, summaries, JSON lines and the failure text.
//!
//! # Adding an actor
//!
//! 1. Add a variant and a definition struct (with `#[serde(deny_unknown_fields)]`) to
//!    [`scenario::Actor`]; unknown fields must stay errors.
//! 2. Write the runtime type under `actors/` and add a variant to [`actors::RtActor`]
//!    (`conn` and `shutdown` say what it exposes and how it is cleaned up).
//! 3. Create it in `runner::build_actor`; allocate what it needs in [`resources::allocate`]
//!    and name its placeholders in [`placeholders::Placeholders::dry`] and there.
//! 4. Say which steps it accepts: `validate::check_actor_rule` and `check_actors`.
//! 5. Give it a test in `tests/actors.rs`.
//!
//! # Adding a step
//!
//! 1. Add an argument struct and a variant to [`steps::Action`], and its names in
//!    [`steps::Action::name`] and [`steps::Action::actor_rule`] (the compiler lists the
//!    matches to update).
//! 2. Add static rules to `validate::check_action` (required fields, exclusive fields, names).
//! 3. Execute it in `exec`: actor steps in `actor_step` or `connection_step`, engine steps in
//!    `engine_step`, assertions in `assertion_step`. Return `Ok(note)` or an error phrased
//!    as expected versus observed; waiting steps take their limit from `timeout_ms` or
//!    `defaults.step_timeout_ms`, assertions poll with [`matcher::Matcher`] until `within_ms`.
//! 4. Document the step in `docs/spec/scenarios.md` and
//!    cover it in a test under `tests/` with a fixture scenario when it needs the engine.
//!
//! # What the engine must provide
//!
//! TCP peers rejected by a listener may be reset before the client's `connect` completes.
//! The client records this as an established connection already closed by the peer, just
//! as when the reset arrives after `connect`. Scenarios verify rejection with
//! `expect_closed` and the engine's rejection event. A TCP connection refusal remains the
//! separate `connect` outcome `refused`.
//!
//! Startup, including management requests, counts toward the scenario's deadline. Cleanup
//! has a separate budget: the configured shutdown drain plus process-exit grace. Built-in
//! polling and timeout constants describe their purpose next to their definitions.
//!
//! The runner speaks only `packie-ipc` and reads the log files in the logs directory.
//! Scenarios address channels, endpoints, destinations and messages by the ids written in
//! the file (for example `bridge-1`); the engine's snapshot and message DTOs must carry
//! those same ids. Test hooks are used when the bootstrap sets `test_hooks` and `hello`
//! lists the `test_hooks` capability. Log events carry an `event` field and are written as
//! JSON lines to `<logs>/packie.*.log`.

pub mod actors;
pub mod bench;
pub mod engine;
pub mod error;
mod exec;
mod fdguard;
pub mod glob;
pub mod matcher;
pub mod payload;
pub mod placeholders;
pub mod ports;
pub mod protocol;
pub mod report;
pub mod resources;
pub mod runner;
pub mod scenario;
pub mod snapshot_view;
pub mod soak;
pub mod steps;
pub mod store;
mod validate;
