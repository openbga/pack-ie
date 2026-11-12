# Audit log specification

The append-only, hash-chained record of who did what through the management interface, and
who looked at message content (decision #14; requirements
[P2 and P6](../requirements.md#compliance-and-hardening-phase-1-version-020)). It supports the lab's audit controls (HIPAA §164.312(b)), activity review
(§164.308(a)(1)(ii)(D)) and breach scoping (§164.402). Attribution comes from the OS account
of the IPC peer (decision #13); alerts are in [diagnostics.md](diagnostics.md#6-alert-catalog),
backups in [backup.md](backup.md).

## 1. Storage

| Rule | Statement |
| --- | --- |
| Files | `<state>/audit/audit.sqlite3` takes new events. Sealed yearly files `audit-<year>-<first seq, 12 digits>.sqlite3` sit beside it (section 6). On Unix the directory is `0700`, the current file `0600`, and sealed files `0400`. On Windows sealed files are marked read-only; access restrictions come from the state directory. |
| Separate | The audit log is its own SQLite database with its own writer, not part of the message store, so it keeps working when the store does not. |
| Writer | One connection behind a lock. Recording an event is synchronous. The crate's latency test checks p99 < 1 ms for its recording workload. |
| Durability | WAL with `synchronous=NORMAL`: when recording returns, the event is committed and survives an engine process crash. A maintenance thread attempts a passive checkpoint every second when events are pending. Checkpoint errors are logged and retried; this is not a fixed upper bound on events lost after power loss. Closing the last handle also attempts a checkpoint. |
| Append-only | No code path updates or deletes an event, and triggers on `events` refuse `UPDATE` and `DELETE`. Retention removes whole sealed files only (section 6). |
| Schema | Table `events` (STRICT) with the columns of section 2; indexes on `at`, `(action, seq)`, `(actor_name, seq)` and `(object_id, seq)`. Embedded forward-only migrations; a newer schema is refused untouched. |

## 2. Event record

| Column | Type | Notes |
| --- | --- | --- |
| `seq` | integer | 1, 2, 3, … without gaps across all files. |
| `at` | string | UTC, RFC 3339 with nine fractional digits. |
| `actor_uid` | string | Unix uid or Windows SID of the OS account behind the IPC connection; empty for the engine itself. Offline CLI backup and restore use the account of their own process, read from the OS (Unix uid; on Windows the SID of the process token, as for pipe clients, never environment variables); when Windows cannot say, they refuse to run. |
| `actor_name` | string | The account name; `system` for the engine, `unknown` when a refused connection had no identity. |
| `origin` | string | `console`, `cli`, `engine`, or `other` (the simulator, unidentified clients). The kind of client is what the client declared; the account is what the OS reported. |
| `action` | string | Section 3. |
| `object_type`, `object_id` | string or null | `channel`, `message`, `delivery`, `config_version`, `secret`, `backup`, `audit`, `engine`, with the object's id. |
| `outcome` | string | `requested` (a command is about to run), `succeeded`, `rejected` (a precondition or validation refused it; nothing changed), `failed`. |
| `details` | JSON object | At most 4 KiB, stored as compact JSON with sorted keys. Never message bodies. It can hold text an operator entered (apply notes, resend and close reasons, a searched identifier or text), which may identify a patient: the audit log is PHI-bearing like the store. |
| `prev_hash`, `hash` | string | Section 4. |

## 3. Actions

Every audited IPC request is recorded twice: `requested` before it runs and its outcome
after, including views, searches, exports and verification. Only the first export chunk
is audited (`offset == 0` for messages, no `after_seq` for audit events). Later chunks do
not write audit events, so each must carry the `export_id` the first chunk returned: it is
accepted only from the same OS account with the same filter, format and content choice,
while the export is open (until its last chunk, at most 30 minutes between chunks), and only
while the audit log can be written, checked at each continuation by writing and syncing a
small file in the audit directory, not taken from the last write (`unavailable` otherwise,
as for audited requests). A
continuation without a valid `export_id` is refused as `bad_request`. Plain message lists, message metadata, audit browsing, snapshots
and logs are not audited.

Successful responses produce `succeeded`, except a validation response with errors produces
`rejected`. Errors `bad_request`, `invalid_config`, `conflict`, `not_found` and `rejected`
produce `rejected`; `not_implemented`, `unavailable` and `internal` produce `failed`. An
error response adds `error_code` to the outcome's details; a validation response with
errors does not. Engine lifecycle, access refusal, anchor, seal and prune events are
single events. Backup events depend on how the backup runs, as listed below.

| Action | Recorded when | Object | `details` |
| --- | --- | --- | --- |
| `config_validate` | A document is validated | — | — |
| `config_apply`, `config_rollback` | A document is applied; a rollback (note starting `rollback of`) | — | `base_version`, `note` |
| `config_verify` | A version is signed off as verified | `config_version` | `note` |
| `channel_pause`, `channel_resume`, `channel_restart` | A channel command | `channel` | — |
| `message_resend` | A resend | `message` | `destinations`, `mode`, `confirm_duplicate_risk`, `confirm_superseded`, `reason` |
| `delivery_close` | A delivery is closed | `delivery` | `reason` |
| `secret_set`, `secret_delete` | A secret value is stored or removed (never the value) | `secret` | — |
| `message_body_view` | An original or transformed body is requested | `message` | `body` (`original`, `transformed`); `delivery_id` only for a transformed body |
| `message_fields_view` | A message's parsed fields are returned | `message` | — |
| `message_search` | A message query filters by identifier, text or id prefix (browsing by channel, state or time is not recorded) | — | `identifier`, `identifier_prefix`, `text`, `message_id_prefix`, `channels`, `since`, `until` |
| `message_export` | The first chunk of a message export | — | `filter`, `format`, `include_bodies`; total matching message `count` on success |
| `transmission_verify` | A transmission verification report is produced | `channel` | `since`, `until`; `messages` on the outcome |
| `endpoint_test` | An endpoint definition is probed | — | `kind`, `target` (host and port, bind address, port name or folder path from the settings; never secrets); `ok` on success |
| `notification_test` | A test message is sent to the notification targets | — | `results` (`target`, `ok` per target) on success |
| `traffic_view` | A live traffic transcript is opened (`subscribe`, topic `traffic`); refused while the audit log cannot record it | `channel` | — |
| `client_export` | A client writes an export it built itself (`record_client_export`), before writing it; the console's change report | — | `kind` (`change_report`), `rows` |
| `audit_export` | The first chunk of an audit export | `audit` | `format` |
| `audit_verify` | The chain is verified over IPC | `audit` | — |
| `backup_create` | An IPC backup request, with request and outcome events | `backup` (requested directory, or `backup.directory`) | `scheduled: false`; `error_code` on an error response |
| `backup_create` | A scheduled backup finishes, with one outcome event attributed to the engine | `backup` (`scheduled`) | `scheduled: true`; `files` on success, `error` on failure |
| `backup_create` | An offline CLI backup, with request and outcome events | `backup` (destination directory) | Empty on request; `files`, `bytes`, `messages` on success, `error_code: backup_failed` on failure |
| `backup_restore` | A backup is restored, with one success event in the restored log | `backup` (backup directory) | `created_at`, `node_id`, `config_version`, `messages`, `audit_head_seq`, `pre_restore` (pre-restore directory name) |
| `engine_start`, `engine_stop` | The engine starts and stops (`origin` `engine`) | `engine` | `version` |
| `access_refused` | A management connection is refused because its OS identity cannot be read, with outcome `rejected` and actor `unknown` | — | `reason` |
| `audit_anchor` | The chain head is published (section 5) | — | `reason` (`daily`, `shutdown`) |
| `audit_seal` | First event of a new current file (section 6) | `audit` (the sealed file) | `year`, `first_seq`, `last_seq` |
| `audit_prune` | Before a sealed file is removed by retention | `audit` (the removed file) | `first_seq`, `last_seq`, `last_hash`, `last_at`, `retention_days` |

## 4. Hash chain

`hash = hex(SHA-256(prev_hash ‖ encoding))`, lowercase. `prev_hash` is the previous event's
`hash` as 64 hex characters; the first event's is 64 zeros. `encoding` is the columns `seq`
(decimal), `at`, `actor_uid`, `actor_name`, `origin`, `action`, `object_type`, `object_id`,
`outcome`, `details`, in this order, each written as one byte `0x00` when null, or `0x01`
followed by its UTF-8 length as a 4-byte big-endian integer and the bytes. The stored text is
hashed as stored, so anyone can recompute every hash from the database with SQLite and a
SHA-256 tool.

## 5. Tamper evidence

| Check | What it finds |
| --- | --- |
| `hash` recomputed for every event | An edited event (`hash_mismatch`). |
| `prev_hash` equals the previous event's `hash` | Removed, inserted or reordered events (`broken_link`). |
| `seq` continues the previous event's | Removed or reordered events (`sequence_gap`). |
| The oldest kept event is `seq` 1 after 64 zeros, or follows the `last_seq`/`last_hash` of a recorded `audit_prune` | A sealed file removed outside retention (`unexplained_start`). |
| Every recognized audit file opens and its events can be read | Damaged or foreign content in those files (`unreadable`). Other filenames are ignored. |

Verification walks every file in seq order and reports at most 100 problems; an edited event
is reported once (the walk continues from its stored hash). Removing the newest events
leaves a valid, shorter chain. Anchors make that detectable: an `audit_anchor` event is
written by minute maintenance when events have arrived since the previous anchor and at
least 24 hours have elapsed, or when there is no anchor in the current file. The last
handle closing also writes an anchor if events have arrived since the last one. Its
`seq` and `hash` go to the engine log as `audit.anchor`. Compare the verified head with
the last `audit.anchor` line in a copy of the log. An administrator of the computer can
still destroy both; that is the OS trust boundary (SECURITY.md).

## 6. Retention

| Rule | Statement |
| --- | --- |
| Sealing | The first event of a new UTC year renames the current file to `audit-<year of its first event>-<its first seq>.sqlite3` (checkpointed, rollback-journal mode, read-only) and starts a new current file whose first event is `audit_seal`, linked to the sealed file's last event. When the rename is impossible (a reader holds the file on Windows), the event goes into the current file and sealing is retried at the next event. |
| Removal | Every minute, maintenance attempts to remove sealed files whose newest event is older than `audit.retention_days` (bootstrap, default 2190 days, approximately six years; allowed range 365–36,500), oldest first. An `audit_prune` event naming the file and its last seq and hash is recorded before each removal, so verification explains the chain's new start. A failure stops that pass and is logged as `audit.prune_failed`. |
| Never | Events are never deleted individually, and the current file is never removed. |

The `packie` host opens the persistent log from `<state>/audit` using the bootstrap's
`audit.retention_days`, read at startup. The same duration is the minimum age for pruning
saved configuration versions when a version-count limit is configured. Offline backup creation also
uses this retention setting, with background maintenance disabled for the short command.

## 7. Availability and fail-closed commands

| Rule | Statement |
| --- | --- |
| Fail closed | An audited IPC request whose `requested` event cannot be written is refused with `unavailable` ("the audit log cannot be written") and does nothing, including returning no content for a view. Offline backup creation also refuses to start when its request cannot be recorded. |
| Lost outcome | When the outcome event after an audited IPC request cannot be written, its result is still returned; the `requested` event shows that it was attempted, without a recorded outcome. `audit.write_failed` is logged. Offline backup creation likewise returns its result and prints a warning if its outcome cannot be recorded. |
| Alert | A failed write on an opened log marks it unavailable: `audit_unavailable` (critical) is raised and `audit.unavailable` logged. The next successful write clears it; maintenance retries every minute, and every audited command tries again. |
| Traffic | Message traffic never waits on the audit log. |
| Start | An audit log that cannot be opened at start does not stop the engine: it uses an unavailable fallback until restart; audited requests remain refused. |
| Scheduled backups | A scheduled backup still runs when the audit log is unavailable; writing its single outcome is best effort. Engine start and stop records are also best effort. |

## 8. Review

| Request (IPC) | CLI | Answer |
| --- | --- | --- |
| `get_audit_events` with `filter`, `before_seq`, `limit` (1–1000, default 100) | `packie audit list` with `--since`, `--until`, `--actor`, `--action`, `--origin`, `--outcome`, `--object-type`, `--object`, `--limit` (default 50), `--before-seq`, `--json` | Events newest first across all files; `next_before_seq` for the next page. |
| `export_audit` with `filter`, `format` (`csv`, `json_lines`), `after_seq` | `packie audit export --format csv\|jsonl [--output <file>]` | Chunks of at most 1000 events oldest first; `next_after_seq` until complete. CSV has a header row in the first chunk and the columns of section 2. The first chunk is audited. |
| `verify_audit` | `packie audit verify` (or `--dir <directory>` for a copy, without an engine) | Files, events, first and last seq, head hash, problems (section 5). IPC verification is audited; offline `--dir` verification is read-only and writes no event. |

Filters: `since` inclusive, `until` exclusive, `actor` matches the account name or uid/SID
exactly, `object_id` exactly. Audit events never appear in the rotating log files or the
log ring, because `details` may hold PHI.
