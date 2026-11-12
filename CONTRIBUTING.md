# Contributing

Start with the [developer guide](docs/development/README.md), [architecture](docs/architecture.md)
and the relevant [behavior specification](docs/spec/). Use synthetic data in issues, tests and
pull requests. Report vulnerabilities through the [security policy](SECURITY.md).

## Build

Install Rust through rustup; [rust-toolchain.toml](rust-toolchain.toml) pins Rust 1.97.1 with
rustfmt and Clippy. Use Node 24 and npm, as CI does. Native desktop builds need the
[Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/):

| OS | Prerequisites |
| --- | --- |
| Windows | Microsoft C++ Build Tools with Desktop development with C++, the MSVC Rust host toolchain, and WebView2 Runtime. NSIS 3 is needed for the release installer. |
| macOS | Xcode Command Line Tools (or Xcode); the desktop console uses the system WebView. |
| Linux | A C/C++ build toolchain and pkg-config; libudev for serial endpoints; GTK 3, WebKitGTK 4.1 and the other libraries installed by the [CI Tauri job](.github/workflows/ci.yml). Use the Tauri prerequisite guide for distribution-specific package names. |

From the repository root:

```sh
cargo build
```

In `console/`:

```sh
npm ci
npm run tauri dev
```

Start the engine separately as described in the developer guide. `npm run dev` runs the
browser UI with mock data. The Tauri backend is a separate Cargo workspace.

## Check a change

Follow [docs/testing.md](docs/testing.md): run scoped checks when a unit of work is complete;
CI runs the workspace gate on every pull request and a maintainer runs it before
a release. Do not repeat the full gate after each edit. For a change to `packie-core`, for example:

```sh
cargo fmt --all --check
cargo test -p packie-core
cargo clippy -p packie-core --all-targets -- -D warnings
```

Choose the affected crate and its regression tests. Protocol changes also need conformance
fixtures; channel behavior belongs in simulator scenarios. For console changes, in `console/`:

```sh
npm run typecheck
npm run lint
npm test
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

From the repository root, build the engine and simulator, validate scenarios, then run the
affected scenario (or the suite at the gate):

```sh
cargo build -p packie -p packie-sim
target/debug/packie-sim check tests/scenarios
target/debug/packie-sim run tests/scenarios/bridge-tcp-full-duplex.json
target/debug/packie-sim run-all tests/scenarios --jobs 4
```

On Windows the binaries end in `.exe`. Serial scenarios require Linux pseudo-terminals;
unsupported capabilities are reported as skipped. CI uses `--deny-skips` on Linux.
Performance checks run at the release gate, not on every edit:

```sh
target/debug/packie-sim bench --assert
target/debug/packie-sim bench --target prf02 --channels 50 --assert
```

## Code and documentation

Use the workspace lints and Rust 2024 conventions: errors are values with typed errors,
no `unwrap`, `expect` or panic shortcuts outside tests, and `tracing` for structured logs.
Keep clinical values in exact decimals. Never put PHI, message bodies or secrets in logs,
snapshots, alerts or error summaries. The console uses strict TypeScript, React Aria and
CSS Modules. Prefer existing dependencies; explain additions and their license compatibility.

Documentation describes the current state. Update the affected specification, operator guide
and console help with behavior changes; avoid plans, diaries and duplicate implementation
descriptions. Keep local investigation artifacts out of the repository.

## Propose a change

Open an issue for a substantial behavior or design change before implementing it. Keep PRs
small and focused. Include the problem and resulting behavior, affected specification or
requirement, tests/checks run with results, documentation updates, new dependencies, and any
remaining limits. Include a synthetic reproduction for a bug and a screenshot for a visible
UI change. Never include patient information, real laboratory exports or credentials.

## License and development disclaimer

Contributions are accepted under [AGPL-3.0-only](LICENSE), the same license as the project;
there is no CLA. Identify third-party material and preserve its notices.

PACK-IE is developed with substantial AI assistance. Contributors are responsible for reviewing
and verifying their changes, including AI-assisted work. As stated in the [README](README.md),
the software is provided as is, without warranty, and is not clinically validated.
