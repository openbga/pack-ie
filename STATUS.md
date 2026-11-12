# Status

Current state of the project: what is built, what is verified and what remains. One page,
replaced when the state changes. Last update: 2026-11-12.

Version 0.2.0. No release has been published; installers are unsigned; the software is
tested on synthetic data only and is not clinically validated.

## Built

Version 1 ([requirements](docs/requirements.md)) and the compliance and hardening scope
([P1–P12](docs/requirements.md#compliance-and-hardening-phase-1-version-020)) are complete.
The [changelog](CHANGELOG.md) lists the features; the [specifications](docs/spec/) define the
behavior.

| Area | State |
| --- | --- |
| Engine and CLI | OS service on Windows, macOS and Linux; local management interface; complete operator CLI |
| Protocols | ASTM (receive and send), AU400/AU640, HL7 v2 over MLLP, delimited files |
| Channels | Bridge channels (serial, virtual serial, TCP); message channels with store, deliveries, resend, filters, mappings, calculations and presets |
| Records | Operator attribution, hash-chained audit log, change control, backup and restore, transmission reports, exports, alerts by e-mail and webhook |
| Console | Every page live against the engine, configuration editors, in-app help |
| Packaging | Windows NSIS installer, macOS package (engine and console), Debian package (engine) |

## Verified

| Check | Result |
| --- | --- |
| Rust workspace (format, Clippy with `-D warnings`, tests) | 1,120 tests pass on macOS; CI passes on Linux, Windows and macOS |
| Simulator scenarios | 30 of 30 on Linux; 29 on macOS, where the serial scenario needs Linux pseudo-terminals |
| Console | 2,371 unit and component tests, 444 browser journeys |
| Acceptance ([criteria](docs/spec/acceptance.md)) | 135 pass, 0 fail, 1 partial (H-01, the desktop window at an unlocked session) |
| Fuzzing | 12 targets; weekly CI run ([fuzz/README.md](fuzz/README.md)) |
| Endurance | 8-hour chaos soak: 565,791 messages, none lost or duplicated, two defects found and fixed. 4-hour soak on the fixed engine: 284,941 messages, none lost or duplicated, memory flat |
| Performance | 50 channels at about 2,450 messages/s durable with commit p99 under 50 ms; one channel about 1,250 messages/s |
| Dependencies | `cargo deny` (licenses, advisories, sources) passes for the engine and the console backend |

| Platform | Verified | Not verified |
| --- | --- | --- |
| macOS (arm64) | Package installs; system daemon runs; console attaches | Restart after kill, stop on a configuration error, start at boot, uninstall ([VERIFY.md](packaging/VERIFY.md) M5, M5a, M6); the desktop-window checklist ([MANUAL.md](console/e2e/MANUAL.md)) |
| Linux (Ubuntu 24.04 arm64) | Debian package install, upgrade, remove and purge; service at boot; serial over pseudo-terminals; reproducible engine build | x86_64; physical serial hardware |
| Windows | Builds, lints and passes its tests in CI | The installer and every installed-service check ([VERIFY.md](packaging/VERIFY.md) Windows section): service, pipe permissions, operators group, attribution |

## Remaining

### Before a binary release

- Ship `LICENSE` and third-party license notices with every artifact. Only the Debian package
  carries `LICENSE`; no artifact carries the notices that the MIT, Apache-2.0, ISC, BSD and
  CDLA-Permissive dependencies require.
- Sign the installers and notarize the macOS package.
- Run the unverified platform checks in the table above.
- Package the console for Linux.

### Known limitations

- Instrument presets are built from published interface documentation and synthetic tests;
  none is qualified on an instrument ([configuration](docs/spec/configuration.md) §7.6).
- The console's service-start button does not elevate; restore is CLI only.
- Benchmark and soak probes are unavailable on Windows.
- Audit log details carry operator notes and searched identifiers by design; treat the audit
  log as PHI. `packie secrets set` needs a running engine.
- The CAP checklist citations in the [compliance guide](docs/operations/compliance.md) use the
  2016 edition; [compliance-sources.md](docs/operations/compliance-sources.md) marks the
  sources that were not read in their current edition.

### Considered and not built

| Item | Today |
| --- | --- |
| Re-hash stored bodies in the transmission report and raise an alert on a mismatch | Bodies are hash-checked when read; the report does not re-check them |
| Side-by-side original and transformed view | The console shows one body at a time |
| Audit entries linked from a message's detail | The audit list filters by object type and object id |
| Audit chain anchor in the Windows event log | The anchor is an engine log event (`audit.anchor`) |
| Audit record for accepted management connections | Refused connections are audited (`access_refused`) |
| Export of selected messages by id, with a manifest | Export is by filter, as CSV or JSON Lines |
| A separate OS group for viewing message content | One management group (`ipc.group`) |

Deferred features (scripting, FHIR, TLS on MLLP/TCP, terminal UI, remote management, HTTP
endpoints, fan-in) are listed in [decisions](docs/decisions.md#deferred).
