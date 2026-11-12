# Developing PACK-IE

Start with [architecture](../architecture.md), [requirements](../requirements.md) and the
relevant [behavior specification](../spec/). [Decisions](../decisions.md) records design
constraints; [STATUS.md](../../STATUS.md) records current completion and known issues.
For installed systems, use the [operator guides](../operations/README.md).

## Repository layout

All engine workspace crates live under `crates/`.

| Crate | Owns |
| --- | --- |
| `packie-core` | IDs, timestamps, state and kind enums, error and byte helpers. |
| `packie-config` | Bootstrap/runtime models, validation, versioning, persistence and secret references/providers. |
| `packie-transport` | Serial, TCP, folder and SFTP adapters, stream/file contracts and serial port leases. |
| `packie-astm` | ASTM framing, records and receive/send state machines. |
| `packie-au` | AU framing, fixed-width models, codecs and session driver. |
| `packie-hl7` | HL7 v2 model, paths, escaping, ACKs and MLLP framing. |
| `packie-files` | Delimited-file templates, parsing and encoding. |
| `packie-transform` | Protocol model adapters, paths, filters, mapping rules and code tables. |
| `packie-store` | SQLite single-writer store, message bodies, deliveries and migrations. |
| `packie-diagnostics` | Structured logging, snapshots, counters, alerts and traffic tap. |
| `packie-ipc` | Local IPC DTOs, framing, server/client and TypeScript export. |
| `packie-engine` | Engine composition, configuration apply, diagnostics and IPC lifecycle; channel registry contract. |
| `packie-routing` | Registry implementation, channel supervisors, bridges, protocol connectors and message pipeline. |
| `packie-host` | OS service control, Windows service entry point and single-instance lock. |
| `packie` | Foreground/service host and operator CLI. |
| `packie-sim` | Scripted peers, scenario validation/runner and performance benchmarks. |

| Path | Owns |
| --- | --- |
| `console/` | React 19 UI, mock/live API adapters, help and component tests; [console guide](../../console/README.md). |
| `console/src-tauri/` | Tauri shell and local engine connection; a separate Cargo workspace. |
| `config/` | Development bootstrap. |
| `tests/scenarios/` | Synthetic JSON integration cases executed by `packie-sim`. |
| `docs/spec/` | Configuration, channel, diagnostics, console, scenario and acceptance contracts. |
| `packaging/` | Platform installer scripts and [qualification checklist](../../packaging/VERIFY.md). |
| `.github/workflows/` | CI, scenario suite and manual release builds. |

## Local development

Use the Rust toolchain pinned in `rust-toolchain.toml` (1.97.1, edition 2024), a native C
toolchain and Node 24, as CI does. Linux engine builds need `libudev-dev` and `pkg-config`;
the Tauri Linux libraries are listed in the `tauri` job of
[ci.yml](../../.github/workflows/ci.yml). See [Contributing](../../CONTRIBUTING.md#build)
for prerequisites on each OS. Build into the checkout's own `target/`. Do not point a second checkout (a `git worktree`) at the same `target/`: Cargo names a workspace member's artifacts from its path relative to the workspace root, so two checkouts with the same layout overwrite each other's `.rmeta` files and the other tree then reports missing items until `cargo clean -p` of the affected crates. Verify a clean checkout from `git archive` in its own target directory or on another machine.

From the repository root, start a foreground engine:

```sh
cargo run -p packie -- run --bootstrap config/bootstrap.json
```

The development bootstrap resolves state and logs to `state/` and `logs/` in the checkout.
It does not seed channels; configure them through the console or CLI using the
[first-channel examples](../operations/first-channel.md). Stop the foreground engine with
Ctrl-C. Service installation is unnecessary for this loop.

In another terminal:

```sh
cd console
npm ci
npm run tauri dev
```

The desktop console connects to that engine over local IPC. The shell checks its saved
override, `PACKIE_IPC`, then the bootstrap selected by `PACKIE_BOOTSTRAP` or the nearest
`config/bootstrap.json`. `npm run dev` instead opens the browser UI on mock data at
`http://localhost:5174`; `?mock=1` also forces mock data inside Tauri.

The simulator starts its own engine process with a fresh bootstrap, state, logs and allocated
peers for each scenario. It does not drive the engine above. From the repository root:

```sh
cargo build -p packie -p packie-sim
target/debug/packie-sim check tests/scenarios
target/debug/packie-sim run tests/scenarios/bridge-tcp-full-duplex.json --verbose
target/debug/packie-sim run-all tests/scenarios --filter 'msg-*' --jobs 4
```

On Windows use `target/debug/packie-sim.exe`. The engine is found beside the simulator, or via
`--engine <path>` / `PACKIE_BIN`. Failures retain their run directory; `--keep-state` keeps
successful runs too. `run-all` supports repeated `--filter`, `--junit <path>` and
`--deny-skips`. Serial-pair scenarios run on Linux and are skipped on macOS when the host
cannot provide them; a skipped case is not a pass. See the
[scenario contract](../spec/scenarios.md) and [simulator module guide](../../crates/packie-sim/src/lib.rs).

## Extending behavior

Keep codecs pure and I/O in transports/routing. Preserve durable receipt before ACK,
bounded buffers, channel isolation and payload-free diagnostics. Rust uses typed errors and
`tracing`, with no production `unwrap`, `expect` or `panic`; clinical values use exact
decimals. Console code uses strict TypeScript, React Aria and CSS Modules.

| Addition | Implementation and coverage |
| --- | --- |
| Protocol crate | Create `crates/packie-<protocol>/Cargo.toml` and `src/lib.rs` together; `crates/*` joins the workspace automatically. Implement codecs/session state with malformed-input and conformance fixtures. Add the kind in `packie-core`, settings/schema/ACK limits and endpoint compatibility in `packie-config`, and source/destination integration under `packie-routing/src/message/`. Add a transform model adapter and tap framer where supported. Cover durable ACK and delivery outcomes in scenarios; update the configuration/message specifications and console configuration support. |
| Endpoint kind | Extend `EndpointKind` in `packie-core`, `Transport`/settings and validation in `packie-config`, then implement the stream opener or file source/sink in `packie-transport` and dispatch it from `SystemTransports`. Routing owns reconnects and sessions. Cover failure classification, cancellation, state/counters and recovery in transport tests and a scenario; update the endpoint specification and console editor. |
| Mapping rule | Add the compiled kind in `packie-transform/src/rules.rs`, strict JSON compilation in `mapping.rs`, and evaluation in `apply.rs`. Cover invalid fields/paths, normal output and runtime failure in transform tests; errors name the rule/path, never its data. Document it in [configuration §8](../spec/configuration.md#8-filters-and-mappings), update console mapping support, and add a pipeline scenario when delivery behavior changes. |
| Scenario | Add `tests/scenarios/<name>.json`, with matching `name`, requirement IDs, allocated resources, synthetic payloads and public assertions. Declare required capabilities and body-leak sentinels. Run `check`, then the case; connect its coverage to [acceptance](../spec/acceptance.md). New actor/step types also need simulator validation, execution, tests and a scenario-spec entry; the simulator module guide lists these touchpoints. |
| Alert | Extend `AlertKind` and the snapshot-derived rule in `packie-diagnostics/src/alert.rs`; add thresholds/validation in `packie-config` only when needed and expose missing observations in snapshots. Supply severity, scope, a payload-free summary and a useful next action. Test raise/update/clear, threshold boundaries and pause/disable suppression in `packie-diagnostics/tests/alerts.rs`. Update the [alert catalog](../spec/diagnostics.md#6-alert-catalog), console presentation/help and a scenario for the observable condition. |

Change the relevant current-state specification/help with behavior. Reuse existing
dependencies; any new dependency must be maintained, AGPL-3.0-compatible and telemetry-free.

## IPC and generated types

Rust is the wire-contract source: `packie-ipc/src/protocol.rs` and `messages.rs`, with shared
configuration/core/diagnostics DTOs reached by `ClientFrame` and `ServerFrame`. After changing
a DTO, regenerate from the repository root:

```sh
cargo test -p packie-ipc --test typescript export_typescript_bindings
```

This test removes the existing generated `.ts` files, exports the reachable DTOs with
`ts-rs`, and rebuilds `console/src/ipc/generated/index.ts`. Do not edit generated files by
hand. Update the console's `src/api/ipc/` mapping and `src-tauri/` handling as needed, then
run console typecheck and affected tests.

## Branches and the presentation website

`develop` is the default branch and the integration branch: pull requests target it. `main`
is the release branch: it receives `develop` by pull request, and releases are cut from it.
CI and the scenario suite run on every pull request and on every push to either branch.

The [presentation website](https://openbga.github.io/pack-ie/) is the static content of
`website/`, with no build step. [pages.yml](../../.github/workflows/pages.yml) publishes it
with GitHub Pages when `website/` changes on `main`, or when run by hand, so a change made on
`develop` goes live when it reaches `main`. Preview it with
`python3 -m http.server -d website`.

The screens in `website/media/screens/` are captured from the console's browser mock
(`npm run dev`, then `/?mock=1&steady=1#/messages` and the other pages) at 1440×900 and show
demo data only; recapture them when the pages change. `og-image.png` is `og-image.svg`
rendered at 1200×630. Keep the page's status and license statements in step with
[STATUS.md](../../STATUS.md) and the [README](../../README.md).

## Verification and release

For a changed crate, run its tests and all-target Clippy, plus affected scenarios; for
example, `cargo test -p packie-core` and
`cargo clippy -p packie-core --all-targets -- -D warnings`. For console
changes, run `npm run typecheck`, `npm run lint`, `npm test` and `npm run build` in `console/`;
shell changes also need `cargo check --manifest-path console/src-tauri/Cargo.toml` from the
root. [Testing](../testing.md) defines the integration gate.

| Workflow/job | Checks |
| --- | --- |
| [CI](../../.github/workflows/ci.yml): `rust` | Ubuntu workspace formatting, all-target clippy with warnings denied, locked tests. |
| CI: `rust-cross` | Locked workspace all-target clippy and tests on macOS and Windows. |
| CI: `console` | Node 24, `npm ci`, typecheck, lint, tests and production frontend build. |
| CI: `tauri` | Separate console Rust workspace: locked check and all-target clippy on Ubuntu. |
| CI: `deny` | Dependency licenses and advisories through cargo-deny. |
| [Scenarios](../../.github/workflows/scenarios.yml): `scenarios` | Ubuntu builds both binaries, runs all cases with `--jobs 4 --deny-skips --verbose`, publishes JUnit and retains failed state/log artifacts. |
| [Publish presentation](../../.github/workflows/pages.yml): `deploy` | Uploads `website/` and deploys it to GitHub Pages; runs for website changes on `main`. |

CI and Scenarios run for pushes to `main` and pull requests; Scenarios also supports manual
dispatch. Performance targets are measured with `packie-sim bench` before a release; see
[acceptance](../spec/acceptance.md#10-performance-security-packaging).

[Release build](../../.github/workflows/release.yml) runs only through **Actions → Release
build → Run workflow**. It builds the engine, Tauri bundle and installer for Linux x86_64,
Windows x86_64, and macOS arm64/x86_64, then uploads workflow artifacts. The optional version
input labels artifacts; installer versions still come from the root Cargo manifest. It
does not publish a GitHub release. Packages are unsigned and not notarized.

Local packaging uses `cargo build --release -p packie`, the optional console bundle from
`npm run tauri build` in `console/`, then the platform script:

| Host | Script | Installer |
| --- | --- | --- |
| macOS | `packaging/macos/build-pkg.sh` | `.pkg`, with console `.app` when present. |
| Linux | `packaging/linux/build-deb.sh` | Engine `.deb`; console is separate. |
| Windows (PowerShell) | `.\packaging\windows\build.ps1` | NSIS setup, with console installer when present. |

[Packaging](../../packaging/README.md) covers prerequisites, binary/bundle overrides and
output paths. Before publishing, follow [VERIFY.md](../../packaging/VERIFY.md) on Windows and
Debian/Ubuntu with systemd: installation, ordinary-user IPC access, service permissions and
restart/reboot behavior, single-instance protection, upgrade preservation and removal. It
also includes the macOS system-daemon checks. Build success alone does not establish those
installer/service outcomes.
