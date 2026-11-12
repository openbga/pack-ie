# Backup specification

Consistent backups of an engine's state, their verification and their restore (decision #17; requirement
[P5](../requirements.md#compliance-and-hardening-phase-1-version-020)). They support the lab's data backup
plan and disaster recovery (HIPAA §164.308(a)(7)(ii)(A)–(B)). Encryption at rest stays the
OS's: a backup holds message content (PHI), so it belongs on an encrypted volume. Settings
are in [configuration.md](configuration.md#3-bootstrapjson), the alert in
[diagnostics.md](diagnostics.md#6-alert-catalog), the audit log in [audit.md](audit.md).

## 1. Contents

A backup is one directory. On Unix, directories are `0700`, files `0600`, and sealed
audit copies `0400`. On Windows, permissions come from the destination's inherited ACL;
sealed audit copies are read-only.

| Path | Content | Restored |
| --- | --- | --- |
| `store/messages.sqlite3` | A consistent snapshot of the message store (SQLite online backup API, one step), as a standalone file in rollback-journal mode. | yes |
| `store/bodies/<xx>/<hash>` | Every spilled body file the snapshot references. | yes |
| `audit/` | Every audit file: sealed years as they are (read-only), the current file as a snapshot. | yes |
| `config/` | Every saved runtime version and its `.meta.json` change-control record (actor, note, outcomes and sign-offs). | yes |
| `runtime.json` | The active runtime document (`runtime.file`). | yes, to `runtime.file` |
| `bootstrap.json` | The bootstrap document. | no (reference) |
| `secrets.names.json` | The key-store provider's secret-name index, when present; never values. The file provider does not maintain this index. | no (reference) |
| `manifest.json` | Written last (synced temporary file, then rename); a directory without it is incomplete. | — |

Secret values are excluded from backups: the secrets file the bootstrap names
(`secrets.file`) is never copied, even when it is placed under `<state>/config` or is the
runtime document's path. On a replacement computer, configure the bootstrap
paths and secrets provider before starting the engine. For the file provider, create the
configured secrets file containing `{}` with access restricted to the service account
(`0600` on Unix). Start the engine, then enter the values again with `packie secrets set`
(use `secrets.names.json` when present, or the
secret references in the restored runtime document): secrets are written by the engine,
under its OS account, into the configured key store or secrets file and audited.
Endpoints that need a missing secret report it and connect once it is set.

### Manifest

| Field | Notes |
| --- | --- |
| `format` | `1`. |
| `engine_version`, `node_id` | Of the engine whose state was copied. |
| `created_at` | When the backup started (UTC). |
| `actor` | Who took it: `name`, `uid_or_sid`, `origin` (`cli`, `console`, `system` for the schedule), `pid`. |
| `store` | `schema_version`, `messages`, `body_files`; `null` when the state had no store. |
| `audit` | `files`, `head_seq`, `head_hash`: the newest event copied; `null` without an audit log. |
| `config_version` | The newest saved version copied. |
| `files` | Every other file: `path` (relative, `/`-separated), `bytes`, `sha256`, sorted by path. |

## 2. Taking a backup

| Rule | Statement |
| --- | --- |
| Target | An absolute directory that does not exist or is empty. Without one, a new `packie-backup-<UTC time>` directory under `backup.directory`. |
| While the engine runs | `packie backup create [<dir>]` asks the engine (`create_backup`), which writes the backup under its OS account. The store is read through its own read-only connection: one WAL snapshot, so commits continue. A body file that retention removes between the snapshot and its copy makes the snapshot repeat once. The audit current file is copied the same way; a seal or retention removal during the copy repeats it once. |
| Engine stopped | `packie backup create` takes the instance lock (so no engine can start meanwhile), opens the audit log itself and works offline. |
| Durability | Configuration/reference files and the manifest are explicitly synced. On Unix, every copied file is synced before the manifest names it. |
| Failure | After the target is accepted, a failed backup attempts to remove what it wrote (the whole directory when it created it) and records the failure in the status. Cleanup failures are logged. Target-validation failures do not update the status. |
| Audit | Manual creation records `backup_create`, `requested` before and the outcome after; refused when the request cannot be recorded ([audit.md §7](audit.md#7-availability-and-fail-closed-commands)). A lost outcome is logged without failing the backup. Scheduled creation records its outcome after the copy and proceeds even when the audit log cannot be written. |
| Status | Accepted backup attempts update `<state>/backup/status.json` (newest attempt and newest success, with the summary). A status-write failure is logged without failing the backup. |

## 3. Verifying

`packie backup verify <dir>` reads the backup only (databases opened read-only):

| Check | Problem reported |
| --- | --- |
| `manifest.json` reads and has format 1 | The backup is incomplete or not a backup. |
| The backup holds only directories and regular files | `<path>` is a symbolic link or special file; a backup holds only regular files (links, FIFOs, devices and sockets anywhere, including a linked directory, which is not entered). |
| Every listed file is reached without following a link and has its size and SHA-256 | `<path>` is a symbolic link or not a regular file; `<path>` is missing or unreadable; `<path>` changed since the backup was taken. |
| No file is present that the manifest does not list | `<path>` is not listed in the manifest. |
| `PRAGMA integrity_check` of `store/messages.sqlite3` is `ok` | The store copy fails its integrity check. |
| Every body file the store copy references is listed | `<n>` body files the store references are not in the backup. |
| The audit copy's chain verifies ([audit.md §5](audit.md#5-tamper-evidence)) and ends at `audit.head_seq` | The audit problems; the copy ends at a different event. |

The command exits with status 1 when any problem is found.

## 4. Restoring

`packie backup restore <dir> --yes` with the engine stopped:

1. Takes the instance lock; refuses when an engine holds the state directory ("stop it
   before restoring"), and keeps any engine from starting during the restore.
2. Verifies the backup completely (section 3); refuses on any problem.
3. Moves the current `store/`, `audit/` and `config/` into `<state>/pre-restore-<UTC time>/`,
   copies the current runtime document there, and checks that every moved directory is
   there and gone from the state directory.
4. Copies in only the regular files the manifest lists under `store/`, `audit/` and
   `config/`: each is opened without following links (every path component must be a real
   directory, the file a regular file, and the opened file the one inspected), hashed while
   it is copied, and refused when it no longer matches its manifest entry. Sealed audit
   files stay read-only. Then writes the backup's `runtime.json` to `runtime.file`
   (temporary file and rename), unless `runtime.file` is the secrets file.
   A secrets file the bootstrap keeps inside one of these directories
   (`secrets.file` under `<state>/config`) stays where it is: it is moved back to its path
   after the swap, and a file the backup has at that path (an older backup) is not
   copied, so the current secret values are kept and the engine starts.
5. Records `backup_restore` in the restored audit log, after the restored chain head.
6. When run as root for a service account, gives the restored files to the owner of the
   state directory.

A failure in steps 3 to 5 fails the restore and undoes exactly what it did: the directories
copied in from the backup are removed, the moved originals are moved back, a runtime
document it replaced is put back (one it created is removed), and directories it never moved
stay as they are. Rollback failures are logged.
The bootstrap document and secret values are not touched. The previous state stays in
`pre-restore-<time>/` until an operator removes it. At its next start the engine opens the
restored store (forward migrations apply when the backup came from an older version) and
reconciles it like after any restart.

## 5. Schedule and staleness

| Setting (bootstrap) | Effect |
| --- | --- |
| `backup.directory` | Where scheduled backups go. |
| `backup.schedule.daily_at` | While the engine runs, a backup every day at this local time (`HH:MM`). A time in a daylight-saving gap is shifted forward by the gap (02:30 becomes 03:30 for a one-hour gap); a repeated time uses its earlier occurrence. Missed runs while the engine is stopped are not caught up. |
| `backup.schedule.keep` | After each scheduled backup, `packie-backup-*` directories with a manifest beyond the newest `keep` names are removed. This includes manually created backups with matching names under `backup.directory`; other directories are never touched. |
| `backup.stale_after_hours` | `backup_stale` (warning) while the newest successful backup is older than this, or none succeeded within that time of the engine's start. Default 26 hours with a schedule, otherwise off. |

`packie backup status` (IPC `backup_status`, or read from the state directory when the engine is
stopped) shows the directory, the schedule with its next run, the stale limit, the newest
attempt with its error and the newest success. The engine refreshes the `backup_stale`
condition after a backup and on its one-minute scheduler checks. Offline status has no
engine-start time: without a saved success it starts the staleness interval at the status query, so it
does not report stale. The lab's own backup system takes the backup directory off the
computer.
