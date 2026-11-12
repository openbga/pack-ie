# Architecture

How Positive ACK is built. Behavior details per area live in `docs/spec/`.

## 1. Process model

```
┌──────────────────────────── one lab computer ────────────────────────────┐
│  packie (service / foreground process)                                     │
│  ┌──────────────┐ ┌──────────────┐ ┌───────────┐ ┌──────────┐           │
│  │ transports   │ │ routing      │ │ store     │ │ ipc      │◄──┐       │
│  │ serial tcp   │►│ bridge chans │►│ SQLite    │ │ UDS/pipe │   │ local │
│  │ files sftp   │ │ message chans│ │ messages  │ │ JSON     │   │ only  │
│  └──────────────┘ └──────────────┘ └───────────┘ └──────────┘   │       │
│         diagnostics: snapshots · alerts · log ring · tracing files        │
└──────────────────────────────────────────────────────────────────────────┘
        ▲ serial/TCP/files to instruments, LIS, EHR        │
                                        PACK-IE Console (Tauri) ┘  packie CLI ┘
```

- **`packie-engine`** is a library: it loads configuration, starts diagnostics, audit, backup
  scheduling and IPC, and applies configuration through the channel registry supplied by its
  host. `packie-routing` supplies the production registry, transports and message store. It has no UI.
- **`packie`** (binary) hosts the engine. `packie run` runs it in the foreground; `packie service
  install|uninstall|start|stop|status` manages it as a Windows service, launchd daemon or
  systemd unit; `packie status`, `packie channels`, `packie messages`, `packie logs`, `packie config` and
  `packie secrets`, `packie audit` and `packie backup` provide operator commands. They use IPC except
  for configuration validation, offline audit verification, backup verification and offline
  backup creation, status and restore
  ([operator-console.md §12](spec/operator-console.md#12-command-line)). One binary, no
  runtime to install.
- **Hosting** (`packie-host`): the service manager starts `packie run --bootstrap <absolute path>
  --service`, which logs to the log directory only. macOS: launchd daemon
  `/Library/LaunchDaemons/org.openbga.positiveack.plist` (run at load, restarted when it
  exits with failure); without sudo, or with `--user`, a user agent in
  `~/Library/LaunchAgents` for development and evaluation. Linux: systemd unit
  `/etc/systemd/system/packie.service` running as the `packie` user and group, with `dialout` as a
  supplementary group for serial ports when that group exists, `Restart=on-failure`, and a read-only system apart
  from the state and log directories. Windows: service `packie` ("Positive ACK"), automatic
  start, LocalSystem, restart on failure, Application event-log entries on start and stop;
  install makes the state and log directories private to SYSTEM and Administrators
  (`icacls`, inheritance from `C:\ProgramData` removed). Unix state and log directories allow
  group traversal for IPC access; private files are owner-only.
  The stop timeout is `shutdown_drain_seconds` + 30 s. On Unix, SIGTERM and SIGINT are blocked
  before CLI parsing until their handlers exist, so a stop requested during startup ends the
  engine cleanly (exit 0) once it has started. Foreground Windows runs handle Ctrl-C; Windows
  services receive SCM stop requests. One engine per state directory:
  `<state>/packie.lock` is held while it runs and `<state>/packie.instance.json` records pid,
  start time and bootstrap for `packie service status`. Installing as a system service needs
  sudo or an administrator terminal.
- **PACK-IE Console** is a Tauri 2 desktop app. It attaches to the engine over IPC. If no engine is
  running it says so and offers to start the installed service. The desktop shell maintains
  an IPC connection and reconnects when it is lost.
- **IPC**: Unix domain socket at `<state>/packie.sock`, Windows named pipe `\\.\pipe\packie-<node>`.
  The socket is mode `0660` for its owner and the `ipc.group` (default `packie`; installers add
  the installing user), or `0600` when `ipc.group` is `null` or the group does not exist
  (logged as `ipc.group_unavailable`); the pipe rejects remote clients and its DACL grants
  SYSTEM, Administrators and the `ipc.group` (`packie` maps to `PACK-IE Operators` on Windows).
  A null or unresolved Windows group leaves access to SYSTEM and Administrators. The engine reads
  each connection's OS account from the connection (Unix peer credentials, the pipe client's
  token), refuses a connection it cannot identify, and attributes every request to that
  account (decision 13).
  Length-prefixed JSON frames; requests with ids and responses; subscriptions that stream
  snapshots, log events, alerts and traffic taps. DTOs are Rust types in `packie-ipc`, exported
  to TypeScript with `ts-rs` into `console/src/ipc/generated/`.

## 2. Domain model

| Term | Meaning |
| --- | --- |
| **Endpoint** | One side of a connection: its transport and settings. Kinds: `serial`, `tcp_client`, `tcp_listener`, `folder`, `sftp`. An endpoint has a stable id, a label, state (`disabled`, `connecting`, `listening`, `connected`, `retrying`, `disconnected`, `error`), counters and last error. |
| **Channel** | A unit of work the engine supervises. Two kinds: **bridge** and **message**. A channel has an id, a label, `enabled`, a state (`starting`, `running`, `retrying`, `degraded`, `stopped` with a stop reason), and per-channel retry and buffer policy. |
| **Bridge channel** | Two endpoints, A and B. Bytes flow A→B and B→A unchanged through bounded buffers. One live session per channel instance; client-only replacement can overlap old and new instances until takeover. Pairing strategy `parallel`, `wait_for_listener_then_pair` or `receiver_then_sender`; close on one side propagates to the other; reconnection with exponential backoff and jitter. Optional **tap**: a protocol framer that observes the stream to count messages and record them in the store without touching the bytes. |
| **Message channel** | One **source connector** (endpoint + protocol), ordered **filters** and **transformations**, one or more **destination connectors** (endpoint + protocol). Store-and-forward: a message is committed to the store before the protocol acknowledgement goes back to the sender; each destination has its own delivery with retries and acknowledgement state. |
| **Connector** | An endpoint bound to a protocol with protocol settings (e.g. ASTM receive on `serial COM3`; HL7 MLLP client to `tcp_client lis:3001` with enhanced ACK mode; delimited file writer to `folder \\lis\in` with template `orders.csv`). |
| **Message** | One protocol unit received or produced by a message channel: original bytes, parsed model, identifiers (control id, sample id, patient id as the protocol provides), received time, source, state. |
| **Delivery** | One message to one destination: attempts, acknowledgement received, state (`queued`, `sending`, `acknowledged`, `rejected`, `timed_out`, `outcome_unknown`, `failed`, `cancelled`). |
| **Alert** | An operator-relevant condition with severity, channel/endpoint, first and last seen, and a next action. Cleared when the condition clears. |
| **Snapshot** | A point-in-time read model of node, channels, endpoints and delivery queues that the console and CLI display. Published at the configured interval (default one second) and on change, with a 100 ms change debounce. |

### Invariants

1. A channel failure never stops another channel or the engine.
2. In a message channel the store commit precedes the protocol ACK; a transport ACK is never
   displayed as processing success; every delivery ends in an explicit state.
3. Buffers are bounded per direction and per channel; overflow is `block_then_degrade`, never
   silent drop.
4. Bytes within a direction keep their order; bridges never alter bytes.
5. Configuration is validated before it is applied; an invalid document leaves the previous
   configuration active; apply is per channel and unchanged channels are not restarted.
6. Message bodies never appear in logs, snapshots, alerts or error summaries. Local clients
   request them explicitly for message detail, export or the live traffic transcript; content
   requests are audited against the IPC peer's OS identity.
7. Duplicate detection is by source identity plus content hash; a duplicate is recorded and
   shown, not processed twice.

## 3. Protocols in version 1

| Protocol | Crate | Roles | Notes |
| --- | --- | --- | --- |
| ASTM E1381 / E1394 (LIS01-A2 / LIS02-A2) | `packie-astm` | receive results, send orders/queries | Frame layer with checksum, ACK/NAK/EOT, retransmission, contention; record layer H/P/O/R/C/Q/L with field accessors. |
| AU400 / AU640 | `packie-au` | receive results, send worklists | Fixed-width records. |
| HL7 v2.x | `packie-hl7` | MLLP client and listener; ORU, ORM/OML, ORL, ACK, ADT as generic messages | Generic segment/field/component model with escaping; typed helpers; ACK generation in original and enhanced modes; MLLP streaming framer. Message version from MSH-12. |
| Delimited files | `packie-files` | read and write | Templates: delimiter, quoting, header row, column→field mapping, one record per line or grouped; completion detection (stable size + rename/lock heuristics). |
| Raw | `packie-routing` | bridges, taps | No parsing. |

Transformation model (`packie-transform`): a message's parsed model is addressed by paths
(`PID.3[0].1`, `R.3`, `col.sample_id`); a mapping is an ordered list of rules (copy, constant,
code-table lookup, concatenate, date/time reformat, conditional, exact-decimal arithmetic);
a filter is a boolean expression over paths. Mappings are versioned with the channel configuration.

## 4. Store (`packie-store`)

SQLite via `rusqlite` (bundled), WAL, `synchronous=FULL`, STRICT tables, one writer thread
with `BEGIN IMMEDIATE` that commits queued operations together (group commit, each under its
own savepoint); readers on separate connections; WAL checkpoints on a thread of their own
(the writer checkpoints itself only past 10,000 WAL pages). The database is
`<state>/store/messages.sqlite3`. Tables: `messages`, `deliveries`, `delivery_attempts`,
`message_bodies` and `body_contents` (original and transformed bodies by content hash; bodies
above the inline cap are files in `<state>/store/bodies/<first two hash digits>/<hash>`),
`alerts`, `channel_counters`, `channel_sequences`, `channel_flags` and `resend_targets`.
Indexes serve the console's filters (channel+time, state, identifiers). Retention per channel
purges messages outside the age or count limit only after receipt and processing finish and
all deliveries are acknowledged or cancelled. An original remains while a retained revision
references it. Bridges' tap recordings are purged by their `tap.retention`. Retention
runs every ten minutes. Migrations
are embedded and forward-only. `Store::backup_into` and `backup_closed` copy a consistent snapshot
(online backup API) and the body files it references for backups
([spec/backup.md](spec/backup.md)). The audit log is a separate database under
`<state>/audit` ([spec/audit.md](spec/audit.md)). If the message store cannot open, bridges
continue, message channels stop with `store_unavailable`, and opening retries after 10 s,
doubling to 5 min. Successful opening reconciles stored work before message channels resume.

## 5. Configuration (`packie-config`)

| Document | Owns | Lifecycle |
| --- | --- | --- |
| `bootstrap.json` | node id and name, state/log directories, IPC path and group, hosting, log retention, audit retention, backup directory and schedule, secrets backend | Read at start; edits require restart |
| `runtime.json` (`runtime.file`, default `<state>/runtime.json`) | endpoints, channels (with connectors, mappings, filters, policies), alert thresholds, diagnostics overrides | Versioned; edited through IPC by the console or CLI; validated, saved as `<state>/config/runtime.vNNNNNNNNNN.json` (ten digits) and mirrored to `runtime.file` through atomic writes. The adjacent `.meta.json` records actor, apply time, required note, channel outcomes and verification sign-offs. Applied per channel. `runtime.versions_kept` defaults to `null` (unlimited); a finite limit also protects versions within the audit-retention window. At start a runtime file that differs from the newest saved version is imported as the next version; an invalid one is ignored in favor of the newest valid saved version |

Secrets are referenced by name and resolved from the OS key store (`keyring`) or a
restricted secrets file; never stored in the documents.

## 6. Observability (`packie-diagnostics`)

- `tracing` everywhere, with `channel_id`, `endpoint_id`, `message_id` fields. Subscriber
  writes JSON lines to `<logs>/packie.YYYY-MM-DD[.N].log` (local date), rotating at midnight or
  the configured size limit, with retention, and into an
  in-memory ring buffer (configurable, default 20k events) served over IPC with filters.
- Snapshots per node/channel/endpoint/delivery queue, published at the configured interval
  (default one second) and on change, with a 100 ms change debounce.
- Alerts derived from channel and endpoint snapshots and node conditions, including store
  and audit availability and backup staleness; thresholds come from `runtime.json` and bootstrap.
- Traffic tap: opt-in per channel, bounded ring of timestamped segments per direction,
  streamed to one local subscriber; never persisted.
- Counters: bytes and messages per direction, queue depth, retry count, last activity, last
  error summary with next action.

## 7. Crates

| Crate | Purpose | Depends on |
| --- | --- | --- |
| `packie-core` | Ids (operator-chosen names for configured things, UUIDv7 for engine-created ones), time (`jiff`), common state enums, error conventions, byte helpers | — |
| `packie-config` | Bootstrap and runtime models, validation (filters and mappings are compiled with `packie-transform` to check them), versioning, atomic persistence, secret references and the secret store (key store or file) | core, transform |
| `packie-transport` | Endpoint adapters: serial (physical + virtual), TCP client/listener, folder watcher/poller, SFTP; a common async byte-stream/connection trait; port sharing registry | core, config |
| `packie-astm`, `packie-au`, `packie-hl7`, `packie-files` | Pure protocol codecs (ASTM and AU also carry receive/send drivers; HL7 the MLLP framer and acknowledgement policy; files a complete-file reader and writer); no I/O, no internal dependencies so they build and test on their own | — |
| `packie-transform` | Paths, mappings, code tables, filters over parsed models; pure, no configuration types | astm, au, hl7 |
| `packie-store` | SQLite message and delivery store, body files, retention and migrations | core, ipc |
| `packie-audit` | Append-only, hash-chained audit log in `<state>/audit` (its own SQLite files and writer), review queries, export, chain verification, yearly sealing and retention ([spec/audit.md](spec/audit.md)) | core, ipc |
| `packie-backup` | Backup (store snapshot, bodies, audit, configuration, SHA-256 manifest), verification, restore with the engine stopped, schedule and status ([spec/backup.md](spec/backup.md)) | audit, config, core, host, ipc, store |
| `packie-diagnostics` | Tracing setup, ring buffer, snapshots, alerts, counters, traffic tap | core, config (alert thresholds) |
| `packie-routing` | Channel supervisors, bridge sessions and buffers, message pipeline, retry policy, connectors, store opening with retry; implements `packie_engine::ChannelRegistry` | astm, au, config, core, diagnostics, engine (registry contract), files, hl7, ipc, store, transform, transport |
| `packie-ipc` | IPC protocol DTOs (ts-rs), framing, server and client | core, config, diagnostics |
| `packie-engine` | Composition: loads config, starts diagnostics and IPC, opens audit through the host-supplied opener, schedules backups, publishes snapshots, applies config to the `ChannelRegistry` its host supplies | astm, backup, config, core, diagnostics, hl7, ipc, transform |
| `packie-sim` | Simulator: scripted TCP/serial/folder peers, scenario runner, benchmarks and soak runner with seeded faults, resource monitoring and message reconciliation; drives a real `packie` process over IPC | core, diagnostics, ipc |
| `packie-host` | OS service definitions and control (launchd, systemd, Windows SCM) behind a `ServiceManager` trait, the Windows service entry point, the single-instance lock | core |
| `packie` (bin) | Hosting (console or service) and CLI; chooses the channel registry | audit, backup, config, core, diagnostics, engine, host, ipc, routing |
| `console/` | Tauri 2 + React 19 app; `console/src-tauri` is its own Cargo workspace root (empty `[workspace]` table) and reaches engine crates by path | `packie-config`, `packie-core`, `packie-ipc` |
| `fuzz/` | Separate cargo-fuzz workspace with 12 targets for protocol parsers/drivers, delimited files, transforms, IPC framing and configuration; seed corpora and regression inputs | astm, au, config, files, hl7, ipc, transform |

Runtime: `tokio` multi-thread. Serial I/O runs on blocking threads behind async channels
(`serialport`); the message store has a dedicated writer thread. Blocking backup and
filesystem work is dispatched to blocking tasks. Lints and toolchain are set in the workspace
`Cargo.toml` and `rust-toolchain.toml`.

## 8. Console information architecture

Navigation: **Dashboard** · **Channels** (list → detail: state, counters, endpoints, live
traffic, message destinations and recent messages, edit) · **Endpoints** (list → detail → edit) · **Messages** (grid with filters →
message detail with original/transformed bodies and deliveries → resend) · **Logs** (live
viewer with filters) · **Alerts** · **Audit** (operator actions and content views, export,
verification) · **Transmission report** (per-destination delivery evidence for a channel and
period, with CSV export) · **Settings** (engine, retention, secrets, versions, backup,
notifications, privacy (idle protection, a console-side setting), about, help). The header names the OS account the engine
attributes actions to ("Signed in as"). Pages handle loading, empty and error states. Message
and audit queries use server-side paging and filtering; logs use sequence cursors. Grids use
virtualized rows and task-specific column defaults. The component kit in `console/src/components/`
is built on React Aria, TanStack table/virtual and CSS Modules.
