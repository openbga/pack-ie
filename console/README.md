# PACK-IE Console

Desktop console for the PACK-IE engine: Tauri 2 shell (`src-tauri/`) around a React 19 app
(`src/`). Inside Tauri it attaches to the engine over local IPC (`src/api/ipc/`); in a browser,
or with `?mock=1` in the URL, it runs against a mock engine (`src/api/mock/`) with synthetic
data. Navigation and pages follow [docs/architecture.md §8](../docs/architecture.md).

## Run

```sh
npm install
npm run tauri dev     # the desktop window, attached to the engine
npm run dev           # the same UI in a browser at http://localhost:5174, on mock data
```

The window finds the engine like the `packie` CLI does: an override saved by the shell, then
`PACKIE_IPC` (socket path or pipe name), then the bootstrap file named by `PACKIE_BOOTSTRAP` or the
nearest `config/bootstrap.json` above the working directory. Start an engine first, from the
repository root: `cargo run -p packie -- run --bootstrap config/bootstrap.json`. Without one the
console says so, keeps trying, and continues when the engine appears.

`?mock=1` forces the mock inside Tauri too; `?mock=offline` starts the mock with no engine, to
see the "engine is not running" screen.

## Check

```sh
npm run typecheck && npm run lint && npm test && npm run build
npm run knip                 # unused files, exports and dependencies
cargo check --manifest-path src-tauri/Cargo.toml
```

Browser journeys (Playwright, chromium only) drive the pages against the mock engine and prove
the console acceptance rows H-01..H-09, D-08, D-12 and the console half of I-03 (docs/spec/acceptance.md §9). One
spec file per area in `e2e/`, each test named by its row:

```sh
npx playwright install chromium   # once
npm run e2e                       # starts `npm run dev` on 5174, or reuses a running one
npx playwright test e2e/logs -g "H-06"
PACKIE_E2E_PORT=5190 npm run e2e     # another port, beside a suite already on 5174 (artifacts in test-results-5190/)
```

The journeys of the audit log (`auditGrid.spec.ts` too), backups, change control (`CC-nn`), configuration outcomes and the change report (`VO-nn`, `CR-nn` in `changeReport.spec.ts`), idle protection (`IP-nn`), dialog live regions (`A11-01`), the message export and superseded resend (`ME-nn`, `SS-nn`) the transmission report (`TR-nn`), the serial port picker (`SP-nn`), endpoint test connection (`TE-nn`) and Settings → Notifications (`NT-nn`) are in `audit.spec.ts`, `backup.spec.ts`, `changeControl.spec.ts`, `changeReport.spec.ts`, `idle.spec.ts`, `liveRegions.spec.ts`, `messages.export.spec.ts`, `messages.superseded.spec.ts`, `transmission.spec.ts`, `serialPorts.spec.ts`, `endpointTest.spec.ts` and `notifications.spec.ts`; they are not acceptance rows.

They run in the console job of `.github/workflows/ci.yml`; failures upload `test-results/`.
The mock takes switches in the URL, for tests and demos: `mock=1|offline`, `messages=<n>` (500,000
for the paging journey), `scenario=empty|errors|slow`, `steady=1` (no random changes) and `e2e=1`
(exposes `window.__packieMock` to simulate an engine outage or restart, an audit log that cannot be written or whose chain is broken or pruned, a stale or failing backup, a configuration apply that ends partial, a revision arriving for a message, content-read counters, failing serial listing, a failing notification target, notifications configured or unknown). What a browser cannot show, the
Tauri window itself, is a ten-minute manual checklist in [e2e/MANUAL.md](e2e/MANUAL.md).

To check the console against a running engine (opt-in; the tests drive the production API and
pages over the engine's socket, edit a configuration label and put it back, pause and resume a
channel and leave it as found). Any engine works. With a bridge `bridge-1` between a TCP
listener and a TCP client and two peers exchanging `ping <n>` / `pong <n>` lines (see
`docs/spec/` scenario `bridge-tcp-full-duplex`) the bridge tests run too; without it they are
skipped and the others act on the first running channel (for example the two channels of
`tests/scenarios/msg-hl7-in-ack.json`):

```sh
PACKIE_LIVE_IPC=/path/to/state/packie.sock npx vitest run src/api/ipc/live.test.tsx
```

Messages against a message channel with the SQLite store (opt-in; it feeds HL7 over MLLP to the
channel's listener, then lists, opens, resends and closes). The channel's HL7 source must have no
other peer connected while it runs:

```sh
PACKIE_LIVE_IPC=/path/to/state/packie.sock PACKIE_LIVE_HL7_PORT=5401 PACKIE_LIVE_HL7_CHANNEL=hl7-results \
  npx vitest run src/api/ipc/liveMessages.test.tsx
```

## Layout

| Path | Holds |
| --- | --- |
| `src/api/` | `EngineApi` (the only way pages reach the engine), the console's DTO types, query hooks. `ipc/` implements it over the Tauri commands and maps the engine's generated types (`src/ipc/generated/`) onto the console's; `mock/` implements it with synthetic data. `createEngineApi()` in `index.ts` picks one. |
| `src/components/` | UI kit: domain-free components with tests. |
| `src/features/` | One folder per area; features never import each other. |
| `src/app/` | Shell, navigation, routes, help drawer. |
| `src/lib/` | Formatting, error handling, state wording (`status.ts`), test helpers. |
| `help/` | One JSON file per page: the text of the help drawer. |
| `src-tauri/` | Tauri shell; its own Cargo workspace root, not part of the engine workspace. Holds the one engine connection (reconnects with backoff, resubscribes) and exposes it as `ipc_*` commands and `packie://event` / `packie://connection` events. It also shows the native save dialog and writes the file the operator chose: the engine's chunks of an audit export (`save_audit_export`) or a message export (`save_message_export`, with or without bodies), and a report the window built itself, such as the change report or a transmission CSV (`save_report_file`). Files are created exclusively and owner-only from the first byte: an existing file is never replaced or followed (the export goes to the first free `name-1.ext`, and the notice names it), and a failed export leaves no partial file. The window never names a path and has no file system permission. The change report is recorded in the engine's audit log (`record_client_export`) before it is written. |

Generated IPC types go to `src/ipc/generated/` (owned by the engine's `packie-ipc` export).
