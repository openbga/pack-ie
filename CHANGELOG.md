# Changelog

Notable changes are grouped by release and change type, following
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## 0.2.0 (unreleased)

### Added

- **Attribution and audit:** OS identity from each management connection; separate append-only,
  hash-chained audit databases with yearly sealing, retention, daily/shutdown anchors and
  `packie audit list`, `export` and `verify`. Records cover operator actions, message body/field
  views and identifier/text searches. Audited commands/content views fail closed when their
  initial record cannot be written; message traffic continues. `audit_unavailable` alerts
  report failures.
- **Change control:** attributed configuration records, required apply notes, verification
  sign-offs with account/time/reference and `packie config verify`; versions retained without
  limit by default, with audit-retention protection when count pruning is configured.
- **Backups:** `packie backup create`, `verify`, `restore` and `status`; consistent store/body,
  audit and configuration snapshots with SHA-256 manifests, optional daily scheduling,
  backup-count retention and `backup_stale` alerts. Restore requires a stopped engine and
  preserves replaced state in a pre-restore directory; secret values are excluded.
- **Notifications:** bootstrap SMTP and webhook alert targets with severity selection, shared
  rate limiting, PHI-free operational content, secret references and a private-CA PEM bundle;
  `packie notify test`, audited as `notification_test`, and `notifications_failing` for failed
  deliveries.
- **Endpoint setup:** `packie endpoints list-serial` shows OS ports, USB details and configured
  use; `packie endpoints test` checks saved or unsaved serial, TCP, folder and SFTP settings
  without sending application data, with per-step results and `endpoint_test` audit records.
- **Evidence tools:** `packie messages verify` produces per-destination transmission summaries
  and CSV/JSON reports with original/transformed hashes, ACK level/code and the ACK's own
  control id; `packie messages export` writes filtered CSV/JSON Lines with optional original
  bodies. Both are attributed and audited; outputs can contain PHI.
- **Instrument mappings:** exact-decimal calculations, text steps, appended records,
  `require` preconditions and `for_each` modes in mappings; six connector presets and seven
  mapping presets for common instrument layouts, with console editors and previews; ASTM
  `enq_attempts`, `busy_retry_ms`, `sample_id_path` and frames up to 64,000 characters; AU
  `test_code_digits`; HL7 `require_end_cr`, `ack_message_types` and `reply_type` for order
  responses. Specimen identifiers (ASTM O-4, HL7 SPM) reach receipt metadata and search.
- **Configuration comparison:** `packie config diff` compares saved versions, active configuration
  and runtime files field by field; `packie config export` aliases the active JSON export.
- **Documentation:** laboratory safeguards crosswalk, validation record and go-live checklist;
  audit review, backup/restore, change-control and incident/downtime operating procedures;
  notification setup/recovery, endpoint discovery/tests and transmission/export evidence.
- **Console privacy:** idle protection conceals message content and searched identifiers until
  the operator returns.
- **Endurance testing:** chaos soak harness covering crashes, blocked storage, destination
  outages and configuration changes. A one-hour run handled 64,870 messages and an
  eight-hour run 565,791; after the two defects the long run exposed were fixed, a four-hour
  run handled 284,941. No run lost or duplicated a message. Twelve fuzz targets each ran
  clean for at least ten minutes after an allocation defect was fixed, with weekly fuzz runs
  configured in CI.

### Changed

- Resending a superseded revision is refused by default in both modes. The explicit
  `--superseded-ok` override is audited and remains separate from duplicate-risk confirmation.
- Retention keeps an earlier revision while a retained revision references it.
- Bridge recordings use `tap.retention`, defaulting to 30 days with no count limit. The
  retention run purges observations beyond either limit; `retention_unbounded` warns only
  when recording is enabled and neither limit is set.
- **Performance:** measured durable throughput improved from 525 to about 2,450 messages/s
  across 50 channels, with commit p99 reduced from 130–293 ms to 43–47 ms. One-channel
  throughput improved from 467 to about 1,250 messages/s; idle CPU measured 1.8 % of one core.
- Windows management access now uses the local `PACK-IE Operators` group besides SYSTEM and
  Administrators. Default `ipc.group: "packie"` selects this group; service installation creates
  it and adds the installing account. Unix continues to use the `packie` group by default.
- `packie config apply` requires `--note`; history includes applied-by and verification records.
  Original parsed fields are requested explicitly with `packie messages show --fields` and the
  view is audited.

### Fixed

- **Recovery and delivery:** early shutdown during startup, consecutive ASTM transfers,
  unresponsive HL7 sessions, retention races and retry of startup reconciliation. Protocol
  settings and acknowledgement policies remain consistent through delivery and recovery.
- **Evidence integrity:** transactional backup restore, exclusion of secret values from
  backups, audited export continuations, configuration-history ordering and transmission
  report totals/truncation. Delimited-file output respects its configured size limit, and
  endpoint checks respect exclusive serial-port ownership.
- **Configuration:** verification sign-off no longer crashes; validation rejects trailing
  JSON text, and retry multipliers retain their precision when configuration is saved.

## 0.1.0 (unreleased)

### Added

- **Engine:** Rust engine that runs as an OS service (Windows service, launchd, systemd) or in
  a console, one instance per state directory. Local management over a Unix socket or Windows
  named pipe, validated and versioned configuration applied per channel, structured logs with
  a bounded in-memory ring, snapshots, and alerts for endpoint, delivery, storage, disk and
  configuration conditions.
- **Command line:** `packie run`, `status`, `channels` (pause, resume, restart), `messages`
  (list, show, resend, close), `logs` (with `--follow`), `config` (show, validate, apply,
  history), `secrets` and `service` (install, uninstall, start, stop, status), with `--json`
  output and exit codes for scripts.
- **Protocols:** ASTM framing and record codecs with receive and send drivers; AU400/AU640
  fixed-width messages; HL7 v2 parsing, MLLP and original/enhanced acknowledgements;
  templated delimited-file reading and writing, with synthetic conformance fixtures.
- **Channels:** Serial, virtual serial, TCP client/listener, folder and SFTP transports;
  bidirectional byte bridges with bounded buffers, pairing, reconnect, pause/resume and a live
  traffic transcript (one viewer, bounded, never stored); message channels with
  receipt-before-ACK, destination retries, duplicate detection, startup reconciliation, resend
  with a reason, declarative filters, mappings and code tables checked when a configuration is
  validated.
- **Store:** Bundled SQLite WAL store with one writer, original and transformed bodies,
  delivery attempts, message search, retention that preserves unresolved work, and migrations.
  If the store cannot be opened the engine starts degraded, keeps bridges running and retries.
- **Security:** Management interface limited to the local computer (socket group `packie`, pipe
  access control list); state and log directories private to the service account; secrets in
  the OS key store or an owner-only file and never in logs; listener peer allowlists.
- **Console:** Tauri 2 and React desktop application attached over local IPC; dashboard,
  channels, endpoints, messages, logs, alerts, settings, configuration editors, version history
  and page help; browser mock mode and generated TypeScript IPC types.
- **Packaging:** Windows NSIS installer, macOS package and Linux engine Debian package, each
  installing and starting the service; CI checks and a manually dispatched release-build
  workflow that includes a reproducible-build check of the engine on Linux and macOS.
- **Docs:** Requirements, architecture, decisions, behavior specifications, acceptance
  criteria, simulator scenarios and benchmarks, operator guides, contributor and developer
  guides, security policy and installer verification checklist.

### Known limitations

- Installers are unsigned and macOS packages are not notarized. Windows and Linux installer
  qualification remains pending; see [packaging/VERIFY.md](packaging/VERIFY.md).
- The Linux engine package does not include the console. Serial scenario execution requires
  Linux pseudo-terminals and is skipped on macOS.
