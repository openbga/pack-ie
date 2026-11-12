# Testing

The aim is complete, efficient verification: tests are designed with each crate, protocols
are proven against corpora, behavior is proven through scenarios, and the whole workspace is
checked at integration points rather than after every edit.

## Layers

| Layer | What it proves | Where | Who runs it, and when |
| --- | --- | --- | --- |
| Crate tests | Each crate's own behavior, including error paths | `crates/<crate>/src` and `crates/<crate>/tests` | The crate's author, scoped (`cargo test -p`) |
| Protocol conformance | Codecs accept the corpus, reject malformed input with typed errors, never panic (property tests on framing and escaping) | `crates/packie-astm`, `packie-au`, `packie-hl7`, `packie-files` fixtures | The crate's author |
| Scenarios | Channels behave as specified under connect, send, disconnect, reconnect, config apply, pause/resume, overflow, duplicate, crash-and-restart; driven by the simulator over real loopback TCP and virtual serial ports | `tests/scenarios/*.json` + `crates/packie-sim` runner | Routing/transport authors for their scenarios; the gate for all |
| Console | Type-check and lint; component tests for grids and filters; smoke of each page against the simulator-backed engine | `console/` | Console author; the gate for the smoke journey |
| Gate | `cargo fmt --check`, `cargo clippy --workspace -D warnings`, `cargo test --workspace`, console `tsc` + lint + tests, scenario suite | CI on every pull request and every push to `main`; a maintainer before a release |

## Rules

- Write the tests when you write the code; a crate without tests is not done. Prefer a few
  scenario-shaped tests over many tests of internals (state machine transitions and retry math
  are proven by scenarios, not by unit tests of private functions).
- Do not rebuild and re-test the whole workspace after each edit. Run the crate's tests and
  clippy once a unit of work is complete and fix what fails; CI runs the gate.
- Fixed resources (ports, virtual serial pairs) are allocated by the test harness, never
  hard-coded, so scenarios can run in parallel.
- Synthetic data only. No real patient data, instrument exports or partner messages.
- Performance targets (PRF-*) are measured by a benchmark scenario in `packie-sim`, run at the
  gate before a release, not in every test run.
