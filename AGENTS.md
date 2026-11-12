# Agent instructions — Positive ACK Interface Engine

Required reading: [docs/architecture.md](docs/architecture.md), [docs/requirements.md](docs/requirements.md)
and the `docs/spec/` file for your area. [STATUS.md](STATUS.md) is the current state of the
project and lists what remains. That is all; do not go looking for more process.

## What this is

A free, open-source (AGPL-3.0-only) laboratory interface engine: a Rust engine (`packie`) that
runs as an OS service on one lab computer, and a desktop console (Tauri 2 + React) that
attaches to it. Endpoints are transports (serial, TCP, files, SFTP). Bridge channels pass
raw bytes between two endpoints. Message channels speak protocols (ASTM, AU, HL7 v2, files),
store every message durably, transform it and deliver it to one or more destinations.

## Rules

1. **Build what the task needs.** Requirements are high level on purpose; use your
   judgement, research when the domain or an API is uncertain, and prefer the simplest design
   that meets them. Speculative abstractions and duplicated rules are defects.
2. **Rust:** edition 2024, workspace lints apply (no `unwrap`/`expect`/`panic` outside tests,
   `tracing` instead of `println!`, typed errors with `thiserror`, exact decimals never floats
   for clinical values). **Console:** strict TypeScript, React 19, React Aria components,
   CSS Modules; no CSS-in-JS frameworks.
3. **Dependencies:** prefer crates and packages already in the workspace. Add others
   deliberately, AGPL-3.0-compatible, maintained, no telemetry; name every new one in the
   pull request. Do not vendor code.
4. **Documentation describes the current state only.** When you change behavior, change the
   doc that describes it in the same change (`docs/`, `docs/spec/`, console help text). No
   plans, diaries, reports or screenshots in the repository; `.local/` is ignored for those.
   `STATUS.md` is one page that is replaced, not appended to.
5. **Testing is designed with the code, not sprinkled after every edit.** Each crate carries
   its tests; protocol crates carry a conformance corpus; integration goes through the
   simulator scenarios. Run scoped checks (`cargo test -p <crate>`,
   `cargo clippy -p <crate> --all-targets -- -D warnings`, console `tsc`/lint for your
   package) when a unit of work is done; test targets are linted too. CI runs the
   workspace-wide gate. See [docs/testing.md](docs/testing.md).
6. **Security is proportionate.** The product runs inside a private lab network. Bind to the
   configured address, keep peer allowlists, keep the local IPC local, keep secrets out of
   files and logs, never log message bodies by default. Do not add authentication frameworks,
   encryption layers or hardening that no requirement asks for.
7. **Usability is a requirement.** Screens are complete (loading, empty, error, success
   states), grids have sensible columns and usable filters, navigation is organized, actions
   explain their outcome. Reuse the component kit in `console/src/components`.
8. **Observability is a requirement.** Every channel and endpoint reports state, counters,
   last activity and last error; every significant event is a structured log line with the
   channel/endpoint fields set.
9. **Synthetic data only.** No patient data, real instrument exports, partner messages or
   credentials in code, fixtures, issues or pull requests. Cite a vendor manual or standard
   by title; do not copy its text or example messages.
10. **Generated and shared files.** `console/src/ipc/generated/` is regenerated with
    `cargo test -p packie-ipc --test typescript`, never hand-edited. When you create a crate,
    write `src/lib.rs` together with its `Cargo.toml` (the workspace glob picks up every
    `crates/*` manifest, and a manifest without a target breaks cargo for everyone).
11. **Names.** Keep other products' and organizations' names out of code and documentation,
    except where a name identifies a protocol, an instrument family, a standard, an operating
    system or a dependency.
12. **Report briefly:** what changed, which checks ran with results, new dependencies and
    open issues. Link files instead of pasting them. When delegating, give each child owned
    paths and a verification scope, and pass these rules on.
