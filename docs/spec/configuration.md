# Configuration specification

Fields, defaults and validation for the two configuration documents in
[architecture §5](../architecture.md#5-configuration-packie-config). Behavior of what the
settings control is in [bridge-channels.md](bridge-channels.md) and
[message-channels.md](message-channels.md); alert thresholds feed
[diagnostics.md](diagnostics.md). Requirements: ENG-03, ENG-05, EPT-01..05, BRG-01..04,
MSG-01, MSG-07, MSG-08, PRO-01..05, CON-06, LOG-01, LOG-03, SEC-01, SEC-03.

## 1. Documents

| Document | Format | Owns | Lifecycle |
| --- | --- | --- | --- |
| `bootstrap.json` | JSON | Node identity, paths, IPC, hosting, logging, secrets provider, audit retention, backup settings | Read once at start. Edits take effect after restart. Invalid bootstrap: the engine refuses to start and prints the errors with their paths (exit code 2); a schema error (wrong type, unknown or missing field) stops at the first one, as in §9. |
| `runtime.json` | JSON | Endpoints, channels, alert thresholds, diagnostics overrides | Versioned. Edited through IPC (console or CLI), validated, written atomically, applied per channel. |

Where `bootstrap.json` lives is a hosting decision (architecture §1); this document defines
only its content. `runtime.json` is the active document; every applied version is also kept
as a file under `<state_directory>/config` (`runtime.v<N>.json`, N zero-padded to ten digits,
with its change-control record beside it as `runtime.v<N>.meta.json`).

## 2. Conventions

| Rule | Meaning |
| --- | --- |
| Encoding | UTF-8 JSON, no comments. Unknown fields are rejected (typo protection), except inside `tags`. |
| Names | JSON keys are `snake_case`. Enumerations are lowercase `snake_case` strings, except HL7 `processing_id` (`P`, `D`, `T`). |
| Ids | `node_id`, `endpoint_id`, `channel_id`, `destination_id`: `^[a-z0-9][a-z0-9._-]{0,63}$`, stable, never reused for a different thing. Labels are free text, 1–100 characters after trimming surrounding whitespace. Ids the engine creates (messages, deliveries, attempts, alerts) are UUIDv7. |
| Units | Durations carry their unit in the field name (`_ms`, `_seconds`, or TCP keepalive `_sec`). Sizes are bytes unless the name says `_megabytes` (MiB, 1,048,576 bytes). |
| Paths | Relative `paths.state_directory`, `paths.log_directory`, `runtime.file`, `secrets.file` and `backup.directory` resolve against the directory of `bootstrap.json`. `ipc.path` is used as written. |
| Required | A field with no default in a table is required. `null` is accepted only where a table says so. |
| Secrets | Never stored in either document; referenced as `{"secret": "<name>"}` (section 11). |
| Payloads | Neither document may contain message bodies. |

## 3. `bootstrap.json`

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `schema_version` | integer | `1` | Must be `1`. |
| `node.node_id` | string | none | Id pattern. Never generated at runtime; used in the Windows pipe name. |
| `node.display_name` | string | none | 1–100 characters after trimming surrounding whitespace. |
| `node.environment` | string | `prod` | `dev`, `test` or `prod`. |
| `node.site_id` | string or null | `null` | Optional grouping label, ≤ 100 characters. |
| `paths.state_directory` | path | none | Must be creatable and writable. Holds the store, bodies, versions and the IPC socket. |
| `paths.log_directory` | path or null | `<state_directory>/logs` | Creatable and writable. |
| `ipc.path` | string or null | `null` | `null` selects `<state_directory>/packie.sock` or `\\.\pipe\packie-<node_id>`. A socket path must be ≤ 100 bytes. |
| `ipc.group` | string or null | `"packie"` | The OS group whose members may use the management interface besides the service account; every request is attributed to the member's OS account. Unix: socket `0660` owned by the group. Windows: the pipe's DACL grants the group read and write besides SYSTEM and Administrators (`D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;<group SID>)`); `packie` means the local group `PACK-IE Operators`, any other value names a local or domain group. `null`, or a group that cannot be resolved or assigned, leaves the Unix socket owner-only (`0600`) and the Windows pipe limited to SYSTEM and Administrators; a group failure logs `ipc.group_unavailable`; an empty string is invalid. `packie service install` creates the default management group and adds the installing user (`$SUDO_USER` on Unix, the elevated account on Windows). Remote pipe clients are rejected. A member can also create instances of the pipe name and so could intercept another member's connection; this is accepted (decision 22). |
| `hosting.shutdown_drain_seconds` | integer | `15` | 1–300. The host's own stop timeout must be at least this long. |
| `runtime.file` | path or null | `<state_directory>/runtime.json` | Parent directory writable. |
| `runtime.versions_kept` | integer or null | `null` | `null` keeps every version (change-control records, decision 16); otherwise at least 1, and older versions beyond it are removed, except versions younger than `audit.retention_days`. |
| `logging.level` | string | `info` | `error`, `warn`, `info`, `debug`, `trace`. `PACKIE_LOG` overrides it with a tracing filter. |
| `logging.retention_days` | integer | `30` | 1–3650. Older rotated files are deleted. |
| `logging.rotate_at_megabytes` | integer | `64` | 1–1024. A file rotates before it would grow past this size and at local midnight: `packie.YYYY-MM-DD.log`, then `packie.YYYY-MM-DD.1.log`, `.2.log` …; writing continues in the newest file after a restart. |
| `logging.ring_buffer_events` | integer | `20000` | 1000–1,000,000. In-memory events served live over IPC. |
| `diagnostics.snapshot_interval_seconds` | integer | `1` | 1–60. |
| `diagnostics.retain_recent_errors` | integer | `100` | 10–10,000. Recent errors kept per channel for display. |
| `secrets.provider` | string | `keystore` on macOS and Windows; `file` on Linux and other Unix systems | `keystore` (OS key store: macOS Keychain, Windows Credential Manager, Linux Secret Service) or `file`. Linux's key store is a Secret Service over D-Bus, which a server or VM without a desktop session does not have; with `keystore` and no reachable key store the engine refuses to start (exit code 2), naming the `file` provider as the alternative. |
| `secrets.file` | path or null | `<state_directory>/secrets.json` with `provider=file` | The secrets file. A missing file holds no secrets and is created owner-only on the first secret write. On Unix an existing file must have no group or other permission bits, and the engine's account must be able to reach it; otherwise the engine refuses to start. A client that loads the bootstrap only to find the management interface (the CLI or console of a user outside the service's group) skips the check when it cannot reach the file, and the connection then reports the permission problem. On Windows startup does not check its ACL. An existing file must contain a JSON object of strings (`{}` when empty). |
| `audit.retention_days` | integer | `2190` | 365–36,500. Sealed yearly audit files whose newest event is older than this are removed whole, and the removal is itself audited ([audit.md](audit.md#6-retention)). Keep it at least as long as the longest message retention. |
| `backup.directory` | path or null | `null` | Where scheduled backups, and backups requested without a directory, are written, each in its own `packie-backup-<UTC time>` directory ([backup.md](backup.md)). Required with `backup.schedule`. |
| `backup.schedule` | object or null | `null` | `{"daily_at": "HH:MM", "keep": N}`: a backup every day at that local time. Validation requires two-character hour and minute components parsed as integers in 0–23 and 0–59; a leading sign is accepted within those two characters. The newest `keep` (1–365, default 7) `packie-backup-*` directories with a manifest under `backup.directory` are kept, including manually created backups with matching names. |
| `backup.stale_after_hours` | integer or null | `26` with a schedule, else `null` | 1–8760. `backup_stale` fires when the newest successful backup is older than this. An omitted value or `null` uses 26 with a schedule and turns the alert off without one. |
| `notifications.rate_limit_minutes` | integer | `15` | 1–1440. At most one message per alert (code, channel, endpoint and destination) in this many minutes; a raise and the clear that follows share the window ([diagnostics.md](diagnostics.md#61-notifications)). |
| `notifications.ca_file` | path or null | `null` | A PEM file holding the certificates of one or more private certificate authorities, for an e-mail relay or webhook whose certificate does not chain to a public root. A relative path is resolved against the bootstrap file's directory, like the other bootstrap paths. Read once when the engine starts; changing the file takes effect at the next restart. E-mail trusts these certificates in addition to the bundled Mozilla roots; the webhook trusts only these (its HTTP client cannot combine the two), so a webhook behind a public CA and one behind a private CA cannot share a bootstrap. Only used by targets that connect with TLS (`starttls`, `tls`, `https://`). A file that cannot be read or holds no usable certificate does not stop the engine: every TLS send fails naming the file and `notifications_failing` is raised ([diagnostics.md](diagnostics.md#61-notifications)). |
| `notifications.email` | object or null | `null` | An SMTP relay that receives alert e-mail; `null` sends none. Fields below. |
| `notifications.email.host`, `.port` | string, integer | none | Relay host name or address, not empty; port 1–65535. |
| `notifications.email.security` | string | `starttls` | `starttls` (connection upgraded with STARTTLS, usually port 587), `tls` (TLS from the first byte, usually 465) or `none`. `none` is only valid with host `localhost`, `127.0.0.1` or `::1`. The relay's certificate is verified against the bundled Mozilla root certificates and, when `notifications.ca_file` is set, also against the certificates in that file. |
| `notifications.email.username`, `.password` | string, secret reference | none | Optional login, set together; `password` is `{"secret": "<name>"}` (a literal is rejected). |
| `notifications.email.from`, `.to` | string, list of strings | none | Sender address and at least one recipient; each must contain one `@` with text on both sides. |
| `notifications.email.severities` | list | `["warning","critical"]` | Non-empty subset of `warning` and `critical`; alerts of other severities are not mailed. |
| `notifications.webhook` | object or null | `null` | An HTTP endpoint that receives a JSON POST for each alert; `null` sends none. Fields below. |
| `notifications.webhook.url` | string | none | `https://…`; `http://` only for `localhost`, `127.0.0.1` or `[::1]`. No user name in the URL. Redirects are not followed. The certificate is verified against the bundled Mozilla root certificates, or, when `notifications.ca_file` is set, against the certificates in that file only. |
| `notifications.webhook.bearer` | secret reference or null | `null` | Sent as `Authorization: Bearer <secret>`. |
| `notifications.webhook.severities` | list | `["warning","critical"]` | As for e-mail. |
| `notifications.webhook.timeout_seconds` | integer | `10` | 1–60. Time to connect and receive the answer. |
| `test_hooks` | boolean | `false` | Enables the scenario test hooks over the management interface ([scenarios.md](scenarios.md#72-engine-steps-actor-engine)). Never set in production. |

Fixed, not configurable: the IPC is local only (no TCP management port), the store is
SQLite under `state_directory` (architecture §4). Private state files and subdirectories are
created owner-only on Unix; service state/log roots allow management-group traversal, and
the IPC socket uses the permissions above. Windows access follows the containing directory's
ACL; service installation restricts state/log roots to SYSTEM and Administrators. Files
written to a folder endpoint follow the process umask, so the receiving software can read them.

## 4. `runtime.json`

### 4.1 Envelope

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `schema_version` | integer | `1` | Must be `1`. |
| `config_version` | integer | assigned | Assigned by the engine at every successful apply (previous + 1, starting at 1). A value in an incoming document is ignored. |
| `generated_at` | string | assigned | ISO-8601 UTC time of the save. Informational only. |
| `diagnostics` | object | `{}` | Section 4.2. |
| `alerts` | object | `{}` | Section 4.3. |
| `endpoints` | array | `[]` | Section 5. |
| `channels` | array | `[]` | Section 6. |

An empty `endpoints` and `channels` document is valid: the engine runs with nothing to do.

### 4.2 `diagnostics` (runtime overrides)

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `snapshot_interval_seconds` | integer or null | inherited from bootstrap | 1–60. |
| `retain_recent_errors` | integer or null | inherited from bootstrap | 10–10,000. |

An omitted field or `null` inherits the bootstrap value. An invalid value rejects the whole document.

### 4.3 `alerts` (thresholds)

Each threshold belongs to an alert in the catalog in [diagnostics.md](diagnostics.md#6-alert-catalog).
Every field is an integer; `0` disables the alert where the table says so.

| Field | Default | Range | Used by |
| --- | --- | --- | --- |
| `endpoint_down_seconds` | `60` | 1–86,400 | `endpoint_down` (warning) |
| `endpoint_down_critical_seconds` | `900` | ≥ `endpoint_down_seconds` | `endpoint_down` (critical) |
| `no_peer_seconds` | `0` (off) | 0–86,400 | `no_peer_connected` |
| `buffer_high_percent` | `80` | 1–100 | `bridge_buffer_pressure` |
| `buffer_high_seconds` | `10` | 1–3600 | `bridge_buffer_pressure` |
| `delivery_backlog_count` | `1000` | 0–10,000,000 (0 off) | `delivery_backlog` |
| `delivery_backlog_age_seconds` | `300` | 0–604,800 (0 off) | `delivery_backlog` |
| `delivery_attention_critical_count` | `10` | 1–10,000,000 | `delivery_needs_attention` (critical) |
| `invalid_messages_count` | `5` | 0–10,000 (0 off) | `invalid_messages` |
| `invalid_messages_window_seconds` | `600` | 1–86,400 | `invalid_messages` |
| `disk_low_megabytes` | `1024` | 1–10,000,000 | `disk_low` (warning) |
| `disk_critical_megabytes` | `256` | 1–`disk_low_megabytes` | `disk_low` (critical) |

## 5. Endpoints

An endpoint is a transport and its settings; it knows nothing about protocols or messages.

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `endpoint_id` | string | none | Id pattern; unique among endpoints. |
| `label` | string | none | 1–100 characters. |
| `enabled` | boolean | `true` | A disabled endpoint is never opened; state `disabled`. |
| `kind` | string | none | `serial`, `tcp_client`, `tcp_listener`, `folder`, `sftp`. |
| `settings` | object | `{}` | Fields of the kind (below); required fields within it still apply. |
| `tags` | object | `{}` | String to string, ≤ 20 entries. Free operator metadata. |

An endpoint of kind `serial` or `tcp_listener` is an **exclusive resource**: one open handle
or one bound socket exists per endpoint (see [bridge-channels.md](bridge-channels.md#9-serial-endpoints)).

### 5.1 `serial`

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `port_name` | string | none | Non-empty after trimming. Example `COM3`, `/dev/ttyUSB0`, `/dev/tty.usbserial-0001`. Unique among enabled endpoints, compared case-insensitively (a virtual endpoint is identified by its `virtual_link` path instead). |
| `baud_rate` | integer | none | > 0. |
| `data_bits` | integer | `8` | 5, 6, 7 or 8. |
| `parity` | string | `none` | `none`, `even`, `odd`. |
| `stop_bits` | string | `one` | `one`, `two`. |
| `flow_control` | string | `none` | `none`, `rts_cts`, `xon_xoff`. |
| `read_timeout_ms` | integer | `1000` | 1–60,000. Poll window of one read. Expiry never means disconnect. |
| `write_timeout_ms` | integer | `1000` | 1–60,000. A write that exceeds it fails the attempt; it is never retried in place. |
| `virtual_link` | string or null | `null` | Unix only (an error on Windows). When set, the path must be non-empty; the engine creates a pseudo-terminal pair, serves the endpoint on one end and exposes the other end at this path (a symlink) for software that only opens a serial port. `port_name` must then be `"virtual"`, and `"virtual"` without a link is an error. |

### 5.2 `tcp_client`

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `host` | string | none | Non-empty after trimming; host name, IPv4 or IPv6 literal. |
| `port` | integer | none | 1–65535. |
| `connect_timeout_ms` | integer | `5000` | 1–120,000. Expiry fails the attempt as a `connect` failure and counts toward the failure streak. |
| `idle_timeout_seconds` | integer | `0` | 0 disables; otherwise 1–604,800. The connection is dropped after this long without bytes in either direction. |
| `keep_alive` | boolean | `true` | Per-connection TCP keepalive. |
| `keep_alive_time_sec` | integer | `30` | 1–7200. Idle time before the first probe. |
| `keep_alive_interval_sec` | integer | `10` | 1–3600. Interval between probes. |
| `keep_alive_retry_count` | integer | `3` | 1–100. Unanswered probes before the connection is declared dead. |

The probe fields are range-checked and applied only when `keep_alive` is `true`. With defaults a silently dropped
connection is detected in about 60 seconds. Probe tuning needs Windows 10 1709+ (always
available on Linux and macOS); on older hosts base keepalive is enabled, a warning names the
peer, and OS default timing applies. A message-channel source waiting for the rest of a message probes
after 1 s regardless of these settings (`message-channels.md` §2.2).

### 5.3 `tcp_listener`

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `bind_address` | string | none | IPv4 or IPv6 literal. No default, so a listener never binds a wildcard by accident. |
| `port` | integer | none | 1–65535. Unique per `bind_address` among enabled endpoints; a wildcard address (`0.0.0.0`, `::`) conflicts with every address on that port. |
| `accept_backlog` | integer | `16` | 1–1024. |
| `allowed_peer_cidrs` | array of string | `[]` | Each entry valid IPv4 or IPv6 CIDR (`10.0.0.0/8`; a bare address is a single host). Empty means any peer, which a non-loopback bind allows only with `allow_any_peer`. |
| `allow_any_peer` | boolean | `false` | Must be `true` when `bind_address` is not loopback and `allowed_peer_cidrs` is empty (SEC-01: listeners facing other hosts have an allowlist unless the operator opts out). Ignored when the list is non-empty. |
| `idle_timeout_seconds` | integer | `0` | As for `tcp_client`. The listener keeps listening. |
| `keep_alive` | boolean | `true` | Applies to the accepted peer socket. |
| `keep_alive_time_sec`, `keep_alive_interval_sec`, `keep_alive_retry_count` | integer | `30`, `10`, `3` | As for `tcp_client`. |

Keepalive defaults to on here because an accepted socket otherwise inherits the OS default
(off, or about two hours) and an instrument that goes quiet can die silently. Keepalive
detects a peer that vanishes without closing; a peer that resets is seen immediately.

### 5.4 `folder` and `sftp` (shared file settings)

Both kinds read files as a source and write files as a destination. A folder endpoint names
one directory; use two endpoints for separate in and out directories.

| Field | Kinds | Type | Default | Validation |
| --- | --- | --- | --- | --- |
| `path` | folder | string | none | Non-empty path. When used: existing directory on the engine host or a share (UNC path on Windows); must be readable and, when used as a destination, writable. |
| `host` | sftp | string | none | As `tcp_client.host`. |
| `port` | sftp | integer | `22` | 1–65535. |
| `username` | sftp | string | none | 1–255 characters. |
| `auth` | sftp | object | none | `{"method":"password","password":<secret>}` or `{"method":"key","private_key":<secret>,"passphrase":<secret or null>}`. Secrets by reference only. |
| `host_key_fingerprint` | sftp | string | none | Required server-key pin, normally `SHA256:` followed by base64. Configuration validation checks only the `SHA256:` prefix and a minimum total length of 10 bytes. A mismatch with the server's fingerprint puts the endpoint in `error` (`host_key_mismatch`). |
| `remote_directory` | sftp | string | none | Absolute remote path. |
| `connect_timeout_ms` | sftp | integer | `10000` | 1–120,000. |
| `operation_timeout_ms` | sftp | integer | `30000` | 1–600,000. One list, read or write operation. |
| `mode` | folder | string | `watch` | `watch` (change notification plus a safety poll) or `poll`. |
| `poll_interval_seconds` | both | integer | `30` (folder), `60` (sftp) | 1–86,400. Always used by sftp and by folder in `poll` mode; the safety poll in `watch` mode. |
| `file_pattern` | both | string | `*` | Non-empty after trimming, without `/` or `\`. Glob matched against the file name; `?` and `*` are wildcards. |
| `recursive` | both | boolean | `false` | Include sub-directories (except `processed_directory`). |
| `ignore_suffixes` | both | array of string | `[".tmp",".part",".partial",".crdownload"]` | Each suffix is non-empty after trimming. Files ending in any suffix are never read. Neither are files whose name starts with `.` (the engine's own temporary files), links and special files. |
| `stable_seconds` | both | integer | `2` | 0–3600. A file is complete when its size and modified time have not changed for this long (sftp: across two polls at least this far apart). |
| `exclusive_open_check` | folder | boolean | `true` | Also require that the file can be opened for exclusive read, which detects a writer holding it. |
| `on_consumed` | both | string | `move` | What happens to a source file after its content is committed: `move` (to `processed_directory`), `delete`, `leave`. With `leave`, duplicate detection prevents reprocessing. |
| `processed_directory` | both | string | `processed` | Non-empty after trimming. Relative path inside the source directory, with `/` or `\` separators; an absolute path or a `..` part is rejected. Created when needed. A moved file keeps its relative path below it; a name that already exists there gets a `-1`, `-2`… suffix. |
| `write_mode` | both | string | `atomic_rename` | Destination writes: `atomic_rename` (write a uniquely named hidden `.<random>.part` file created exclusively in the same directory, then move it into place; on Windows a replacing move refused because another program briefly holds the name (another writer replacing it, a virus scanner) is retried for up to about 1.4 s; `on_exists` `fail`/`suffix` never replace a file) or `direct` (write the final name; links are never written through). `on_exists` `append` always appends to the existing file in place, in either mode. |

## 6. Channels

### 6.1 Common fields

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `channel_id` | string | none | Id pattern; unique among channels. |
| `label` | string | none | 1–100 characters. |
| `kind` | string | none | `bridge` or `message`. |
| `enabled` | boolean | `true` | A disabled channel exists but is not started (`stopped`, `disabled_by_config`). |
| `tags` | object | `{}` | As for endpoints. |

Common rules: every referenced endpoint must exist; an enabled channel (or enabled
destination of an enabled channel) may not reference a disabled endpoint. At most one enabled channel should reference an
exclusive endpoint (serial, `tcp_listener`); more than one is a validation **warning**
 and at run time only one holds the resource at a time ([bridge-channels.md](bridge-channels.md#9-serial-endpoints)).

### 6.2 Bridge channel

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `endpoint_a`, `endpoint_b` | string | none | Endpoint ids of kind `serial`, `tcp_client` or `tcp_listener`. Must differ. |
| `connection.connect_strategy` | string | `parallel` | `parallel`, `wait_for_listener_then_pair`, `receiver_then_sender`. `wait_for_listener_then_pair` requires at least one endpoint of kind `tcp_listener` and behaves exactly like `parallel`. |
| `connection.receiver` | string or null | `null` | `a` or `b`. Required when `connect_strategy=receiver_then_sender`, otherwise absent or `null`. |
| `connection.pairing_timeout_ms` | integer | `5000` | > 0. Time for the second side to attach once the first has; the first wait is unbounded. |
| `buffer.max_buffered_segments` | integer | `1000` | > 0. Per direction. |
| `buffer.max_bytes` | integer | `10485760` | > 0. Per direction, enforced together with the segment limit. |
| `buffer.overflow_action` | string | `block_then_degrade` | Only `block_then_degrade`. |
| `buffer.block_timeout_ms` | integer | `5000` | > 0. How long a write into a full buffer blocks before the channel reads `degraded`; it keeps waiting afterwards and nothing is dropped. |
| `retry` | object | defaults | [Retry policy](#63-retry-policy). Applies to both endpoints. |
| `tap.framer` | string | `none` | `none`, `astm`, `hl7_mllp`, `au`. |
| `tap.record_messages` | boolean | `false` | Store each observed message (state `observed`). Requires a framer other than `none`. |
| `tap.max_message_bytes` | integer | `1048576` | 1024–67,108,864. A longer unframed run is counted as a framing error and the framer resynchronizes. |
| `tap.retention.max_age_days` | integer or `null` | `30` | 1–36,500. Recorded messages older than this are purged; `null` means no age limit. |
| `tap.retention.max_messages` | integer or `null` | `null` | 1,000–1,000,000,000. The oldest recorded messages beyond this count are purged; `null` means no count limit. |

Fixed, not configurable: one live session per channel instance at a time (replacement
instances may overlap during make-before-break apply), bidirectional close propagation,
overflow never drops bytes, bytes are never altered.

### 6.3 Retry policy

Used by bridge channels (endpoint reconnects) and as the default for delivery retries.

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `initial_delay_ms` | integer | `1000` | > 0. |
| `max_delay_ms` | integer | `60000` | ≥ `initial_delay_ms`. |
| `multiplier` | number | `2.0` | ≥ 1.0, at most 4 decimal places (`retry_invalid`). |
| `jitter_percent` | integer | `20` | 0–100. `0` is allowed so tests are deterministic. |
| `max_consecutive_failures_before_alert` | integer | `3` | > 0. Bridge channels only. |

Delay before retry *n* (n starts at 1): `min(max_delay_ms, initial_delay_ms × multiplier^(n−1))`,
then randomly scaled by ±`jitter_percent`.

### 6.4 Message channel

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `source` | object | none | [Source connector](#connectors). |
| `filters` | array | `[]` | Ordered [filter expressions](#8-filters-and-mappings); a message must satisfy all. |
| `destinations` | array | none | 1–20 [destination connectors](#connectors); `destination_id` unique within the channel. |
| `retry` | object | defaults | [Retry policy](#63-retry-policy) for reconnecting and reopening the channel's endpoints, as for bridges. |
| `delivery` | object | defaults | Channel default for destination delivery policy (below). |
| `retention.max_age_days` | integer or null | `90` | 1–36,500; `null` means no age limit. |
| `retention.max_messages` | integer or null | `null` | 1000–1,000,000,000; `null` means no count limit. |

#### Connectors

A connector binds an endpoint to a protocol.

| Field | Source | Destination | Type | Default | Validation |
| --- | --- | --- | --- | --- | --- |
| `destination_id` | no | yes | string | none | Id pattern, unique in the channel. |
| `label` | no | yes | string | none | 1–100 characters. |
| `enabled` | no | yes | boolean | `true` | A disabled destination gets no new deliveries; its queued deliveries wait. |
| `endpoint` | yes | yes | string | none | Existing endpoint; must be enabled for an enabled source channel or an enabled destination of an enabled channel (section 5). |
| `protocol` | yes | yes | string | none | `astm`, `au`, `hl7_mllp`, `delimited_file` (`raw` is only for bridge taps). Must suit the endpoint kind (table below). |
| `settings` | yes | yes | object | per protocol | Section 7. |
| `duplicate_detection` | yes | no | boolean | `true` | Source only. See [message-channels.md](message-channels.md#7-duplicate-detection). |
| `filters` | no | yes | array | `[]` | Destination filters; all must pass for a delivery to be created. |
| `mapping` | no | yes | object or null | `null` | Section 8. `null` passes the source message through unchanged and requires the same `protocol` as the source. |
| `delivery` | no | yes | object | channel `delivery` | Overrides, field by field. An omitted override field or a field set to `null` inherits; a supplied `retry` object replaces the channel retry policy and fills omitted retry fields with retry defaults. |

| Protocol | Endpoint kinds |
| --- | --- |
| `astm`, `au` | `serial`, `tcp_client`, `tcp_listener` |
| `hl7_mllp` | `tcp_client`, `tcp_listener` |
| `delimited_file` | `folder`, `sftp` |

The source connector receives, the destination connector sends, whatever the endpoint kind.

#### Delivery policy

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `max_attempts` | integer | `5` | 1–100. Attempts per attempt cycle (the first send, and again after each operator resend). |
| `retry` | object | [retry policy](#63-retry-policy) defaults | `max_consecutive_failures_before_alert` is ignored (it must still be > 0). |
| `send_timeout_seconds` | integer | `30` | 1–3600. Time allowed to write the whole message (file and SFTP: one write or upload). |
| `ack_timeout_seconds` | integer | `30` | 1–3600. Time allowed after the last byte for the required acknowledgement. |
| `ack_required` | string or null | `null` (per protocol) | `handed_off`, `received` or `accepted`; must not exceed what the protocol can prove (table below). |
| `retry_unacknowledged` | boolean | `false` | When `true`, an attempt that sent the whole message but got no acknowledgement is retried like a failed send. Set only for destinations that tolerate duplicates. |

| Protocol | Highest level | Default `ack_required` |
| --- | --- | --- |
| `delimited_file` | `handed_off` | `handed_off` |
| `astm`, `au` class B | `received` | `received` |
| `au` class A | `handed_off` (class A sends no acknowledgement) | `handed_off` |
| `hl7_mllp` | `accepted`; `handed_off` with `ack_mode: none` | `accepted`; `handed_off` with `ack_mode: none` |

## 7. Protocol settings

All protocol settings are validated when the document is validated.

### 7.1 `astm`

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `charset` | string | `latin1` | `ascii`, `latin1`, `utf8`. `latin1` never fails to decode. |
| `role` | string | `computer` | `computer` or `instrument`: the side PACK-IE plays in LIS01-A2 link contention. `computer` yields to a simultaneous instrument ENQ and sends after receiving; `instrument` has priority. |
| `ack_timeout_ms` | integer | `15000` | 1000–120,000. Sender waits this long for ACK/NAK. |
| `receive_timeout_ms` | integer | `30000` | 1000–300,000. Receiver waits this long for the next frame before aborting. |
| `max_frame_retries` | integer | `6` | 0–20. Retransmissions of one frame before the transfer aborts. |
| `max_frame_chars` | integer | `240` | 64–64,000. Text characters per frame, for sending and receiving. 240 is the E1381 limit; larger values are long frames, which instruments document for ASTM over TCP (up to 64,000). A sender splits records longer than this with ETB; a receiver NAKs a frame with more text than this, so both sides must agree. |
| `contention_wait_ms` | integer | `20000` | 1000–60,000. Wait before sending ENQ again after losing contention. |
| `enq_attempts` | integer | `3` | 1–10. Destination: ENQ transmissions per message before the send is abandoned (`EstablishmentFailed`, a send timeout that the delivery policy retries). Some receivers document up to 7. |
| `busy_retry_ms` | integer | `10000` | 1000–60,000. Destination: wait after a NAK or silence in answer to ENQ before the next ENQ. |
| `max_message_bytes` | integer | `1048576` | 1024–67,108,864. Larger receives abort (`message_too_large`). |
| `sample_id_path` | string | `O.3` | Source: where the specimen id sits in the first order record: `O.<field>` or `O.<field>.<component>` (field 1 is the record type; field 3 to 99, component 1 to 99; the first repetition). It is the message's `sample_id` — shown and searched on the Messages page, part of the duplicate identity — and is read before the message is stored. Other layouts than `O.3` exist, for example the instrument specimen id `O.4.3` (`^^123456^B`). A path of another form is a validation error; a path that finds nothing leaves `sample_id` empty. |

### 7.2 `au`

No defaults except `block_timeout_ms` (9900, the codec's): the laboratory supplies the
settings of the instrument's online parameter sheet.

| Field | Type | Validation |
| --- | --- | --- |
| `profile` | string | `au400` or `au640`. |
| `framing.start`, `framing.end` | array of integer | 1–2 control bytes (1–31), never ACK (6) or NAK (21). |
| `framing.bcc` | boolean | One XOR check byte per frame. |
| `framing.use_etb` | boolean | ETB for intermediate blocks; only when `end` is `[3]`. |
| `framing.max_wire_len` | integer | 256, 512 or 1024. |
| `framing.class` | string | `a` (no ACK/NAK on receive) or `b` (ACK after durable commit). |
| `format.unit_no` | boolean | Two-digit unit number in incoming messages. |
| `format.rack_digits` | integer | 0, 4 or 5. |
| `format.sample_id_digits` | integer | 1–20. |
| `format.sex`, `format.age`, `format.month` | boolean | Patient header options. |
| `format.patient_info_digits` | array of 6 integers | Each 0–20. |
| `format.result_digits` | integer | 6 or 9. |
| `format.test_code_digits` | integer | 2 or 3, default 2. Width of a test code in result items and request test lists, as the instrument's parameter sheet says. A code of only zeros is invalid. |
| `frame_timeout_ms` | integer | 100–9900 in steps of 100. Time allowed for one frame from its first byte; a frame that has not ended by then is discarded without a response (the instrument sends it again), and an unfinished sample goes back to waiting for its next block (`block_timeout_ms`). Also the limit for writing one response. |
| `block_timeout_ms` | integer | 100–9900 in steps of 100, default 9900. Wait for the next block of an unfinished sample; does not time out an idle instrument between samples. |

A receipt keeps the settings it was received with; later config changes never reinterpret it.

### 7.3 `hl7_mllp`

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `ack_mode` | string | `original` | Source: `original` or `enhanced`. Destination: `original`, `enhanced` or `none` (no acknowledgement expected; level reached is `handed_off`). |
| `send_application_ack` | boolean | `true` | Source in `enhanced` mode: send the application acknowledgement (second message) after the commit acknowledgement when the sender asks for it. |
| `version` | string | `2.5.1` | `2.x` or `2.x.y`. Version of generated messages (MSH-12); received messages keep their own. |
| `sending_application`, `sending_facility`, `receiving_application`, `receiving_facility` | string | `""` | ≤ 180 characters. Used for generated MSH-3..6 on sent messages and ACKs. |
| `processing_id` | string | `P` | `P`, `D`, `T`. |
| `charset` | string | `utf8` | `utf8`, `latin1`, `ascii`. Written to MSH-18 and used to decode. |
| `max_message_bytes` | integer | `1048576` | 1024–67,108,864. |
| `require_end_cr` | boolean | `true` | `false`: a frame ends at the end block (0x1C) and a carriage return right after it is accepted but not required, for senders that omit it. Frames the engine writes always end 0x1C 0x0D. |
| `ack_message_types` | array of string | `["ACK"]` | Destination: 1–8 message codes (MSH-9.1, three capitals or digits) accepted as the acknowledgement of a sent message. An order message is answered by an order response (`ORL`, `ORR`), which carries the same MSA segment; list its code or the answer is ignored (`delivery.ack_unmatched`) and the delivery times out. Correlation by MSA-2 and the acknowledgement levels do not change; per-order status in ORC-1 (for example `OK` or `UA`) is not evaluated, so `acknowledged` reflects the MSA code only. |
| `reply_type` | string | `ack` | Source: `ack` answers every message with an `ACK`; `order_response` answers `OML^O33` with `ORL^O34`, `OML^O35` with `ORL^O36`, `OML^O21` with `ORL^O22` and `ORM^O01` with `ORR^O02` (MSH-9.3 `ORL_O34` and so on from v2.3.1), with the codes and MSA-2 an `ACK` would carry; other messages still get an `ACK`. The response is the base HL7 form, MSH and MSA only (the response group is optional in these structures): `AA` means PACK-IE stored the order, it states no per-order acceptance (no ORC) and the reply is not an IHE LAW LAB-28 response. In enhanced mode only the application stage changes; the commit acknowledgement stays an `ACK`. |
| `frame_timeout_ms` | integer | `30000` | 1000–300,000. A frame that has started (0x0B) and receives no further bytes for this long is discarded and the connection closed. The time counts while the engine waits for the frame's bytes, from when the frames completed by the same read are committed and acknowledged. |

Fixed: framing is MLLP (0x0B … 0x1C 0x0D, with the trailing CR optional on input by `require_end_cr`); no TLS in version 1.

### 7.4 `delimited_file`

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `delimiter` | string | `,` | One character: `,` `;` `\|` or tab. |
| `quote` | string or null | `"` | One character, or `null` for no quoting. A doubled quote inside a quoted field is a literal quote. |
| `header_row` | boolean | `true` | Source: first non-skipped line names the columns. Destination: write a header line first. |
| `skip_lines` | integer | `0` | 0–1000. Leading lines ignored on read. |
| `charset` | string | `utf8` | `utf8`, `latin1`, `ascii`. A UTF-8 BOM is accepted on read. |
| `line_ending` | string | `crlf` | Destination only: `crlf` or `lf`. |
| `write_bom` | boolean | `false` | Destination only. |
| `columns` | array | none | At least one [column](#columns). |
| `record_mode` | string | `one_per_line` | `one_per_line` (each row is one message) or `grouped`. |
| `group_by` | string or null | `null` | Required for `grouped` (a column `name`); otherwise absent or `null`. Consecutive rows with equal values form one message; the same value later in the file starts a new group. |
| `max_file_bytes` | integer | `20971520` | 1024–1,073,741,824. Source: a larger file is not read into memory; an `aborted` message records it and the file is consumed as usual (`on_consumed`). Destination: a mapped file that would be larger is not written; the delivery fails with `transform_error`, so a reader with the same template accepts every file the engine writes. |
| `file_name` | string or null | `null` | Destination only, required. Template; variables `{message_id}`, `{control_id}`, `{sample_id}`, `{date}` (`yyyyMMdd`), `{time}` (`HHmmss`), `{seq}`. Must be non-blank and must not contain path separators or start with `.` (hidden names are never read). |
| `on_exists` | string | `suffix` | Destination only: `suffix` (append `-1`, `-2`…), `overwrite`, `append`, `fail`. |

#### Columns

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `name` | string | none | `^[a-z][a-z0-9_]{0,40}$`, unique. Addressed as `col.<name>`. |
| `header` | string or null | `null` (uses `name`) | Source with `header_row`: header text to match (case-insensitive). |
| `position` | integer or null | `null` (next) | ≥ 1. Source without `header_row`, and destination column order. |
| `type` | string | `string` | `string`, `integer`, `decimal`, `date`, `datetime`. Decimals keep their text and are never converted to floating point. |
| `format` | string or null | `null` | Required and non-blank for `date`/`datetime`: tokens `yyyy`, `yy`, `MM`, `dd`, `HH`, `mm`, `ss` and literal separators. |
| `required` | boolean | `false` | Source: an empty value makes the message `invalid`. |

Columns named `control_id`, `sample_id` and `patient_id` fill the message identifiers of the
same names ([message-channels.md](message-channels.md#3-message-states)); a source message from
a file has `message_type` `delimited`.

### 7.5 `raw`

No settings. Raw is what a bridge carries when `tap.framer` is `none`; it is not a valid
message channel protocol.

### 7.6 Instrument profiles

`crates/packie-config/profiles/profiles.json` (`packie_config::profiles::catalog()`) ships ready
values for the variants above and message mappings with the field positions that instrument
interface documents publish. It is data: applying a profile copies explicit values into a
connector or mapping, which the operator then edits; the engine does not know a profile by
name and a stored configuration never refers to one. The file's `qualification` text says
what the profiles are: built from published interface documentation and synthetic fixtures,
documented and synthetically tested, not qualified on the instrument: validate in the laboratory
before clinical use. A laboratory validates a profile with its own
instrument before use ([compliance guide](../operations/compliance.md) validation records).

| Part | Content |
| --- | --- |
| `connectors[]` | `id`, `label`, `protocol`, `roles` (`source`, `destination`), `summary`, `settings` (a patch: only the listed settings change), `evidence`. |
| `mappings[]` | `id`, `label`, `from` and `to` protocols, `summary`, `notes`, `mapping` (a destination `mapping` object, section 8), optional `destination_settings` (a patch for the destination connector), `sample` and `evidence`. |
| `sample` | `input` and `output`: a synthetic source message and the destination message the mapping produces from it (records and segments end with CR; the AU input is the framed wire text). `header` holds the MSH fields an HL7 destination generates (`MSH.3` and so on) that `output` was produced with, and `source_settings` the AU settings needed to read `input`. |

Code tables named `tests` ship empty and their lookups keep or skip an unlisted code, so an
unedited preset does not fail a message for an unknown test. Result presets set
`require_single_group`: a batch with several patients or orders is refused with a transform
error instead of mixing specimens. Download presets state what one order record can carry as
mapping preconditions (`require`): at most one `PID`, for OML exactly one `SPM`, 1 to 18 `OBR`
(the 18-test limit of the Roche cobas e 411 order record), and one specimen id, priority and
order control (and for Roche collection time and specimen type) across the `OBR`. Every `OBR`
becomes one repetition of the universal test id (`for_each` with `mode: "repeat"`), so a
message that satisfies them is carried completely; any other message is refused with a
transform error and the delivery is `failed`, nothing is dropped or filtered. Tests apply every
sample through its mapping, check every patch inside a complete document, and run the presets
through message channels.

| Mapping preset | Positions read | Written |
| --- | --- | --- |
| `astm_results_to_hl7_oru` | Patient `P.3` `P.6` `P.8` `P.9`; specimen `O.3`, panel `O.5.4`, time `O.8`; per `R`: test code `R.3.4`, value `R.4.1`, units `R.5`, range `R.6.1`, flag `R.7`, status `R.9`, operator `R.11`, completion time `R.13`, instrument `R.14` | `ORU^R01`: `PID`, `OBR`, one `OBX` per result (NM for a plain number, ST otherwise). Documented by the Beckman Coulter Access/DxI interface and the Roche Elecsys-type upload |
| `astm_results_to_hl7_oru_roche_cobas` | As above without the range; test code `R.3.4` before the first `/` (`^^^10/2/pre-diluted`) | The same `ORU^R01` |
| `astm_results_to_hl7_oru_instrument_specimen_id` | Specimen `O.4.3`, panel `O.5.5`, test code `R.3.5` | The same `ORU^R01` (driver evidence only; the vendor specification was not read) |
| `au_results_to_hl7_oru` | `sample_id`, per item `test[n].code` `.value` (leading zeros removed) `.flags` | `ORU^R01` with `OBR-3` and one `OBX` per item |
| `hl7_orm_to_astm_download`, `hl7_oml_o33_to_astm_download` | `PID-3.1` `PID-5` `PID-7` `PID-8`; specimen `OBR-3` (ORM) or `SPM-2.1.1` (OML); tests `OBR-4.1` of every OBR; priority `OBR-5`; action `ORC-1`; specimen type `OBR-15.1` or `SPM-4.1` | ASTM `P` (`P.3` `P.6` `P.8` `P.9`) and one `O` with `O.3`, the tests as repeated `^^^code` in `O.5`, `O.6` S or R, `O.12` A or C, `O.16` specimen type text (Beckman Coulter Access/DxI layout) |
| `hl7_orm_to_astm_download_roche_cobas` | As above for the order | A bare `P|1` and one `O` with `O.3`, `O.5`, `O.6`, `O.8` (14 digits), `O.12` `A`, `O.16` 1 serum or 2 urine, `O.26` `O` (Roche cobas e 411 cobas-type layout) |

The connector profiles are `hl7_mllp_end_without_cr`, `hl7_order_response_source`,
`hl7_order_response_destination`, `astm_specimen_id_in_o4_component_3` (the source setting
`sample_id_path: O.4.3` that `astm_results_to_hl7_oru_instrument_specimen_id` needs so the
stored message carries the specimen id), `astm_long_frames` and `astm_repeated_enq`.

## 8. Filters and mappings

Declarative only in version 1 (decisions #6). Paths address the parsed message model. A path
that stops above the leaf yields the text with its separators (`MSH.9` is `ORU^R01`); a path to
a component yields that component. Filters and mappings never see raw bytes.

**Path syntax.** Occurrence (`[occ]`) and repetition (`[rep]`) count from 0 and default to 0, the
first. Fields, components and subcomponents count from 1. Every numeric index is at most
65,535. Anything else is a syntax error that names the position.

| Protocol | Form | Examples |
| --- | --- | --- |
| HL7 | `SEG[occ].field[rep].component.subcomponent`; `SEG` matches `[A-Z][A-Z0-9]{2}` | `MSH.9`, `PID.3[0].1`, `OBX[2].5`, `OBX[2].5[1].2.1` |
| ASTM | `R[occ].field[rep].component`; record letter `H P O R C Q M L` | `R.4`, `R[2].3.4`, `P.3`, `O.3` |
| AU | `name` for a header field, `test[occ].name` for a result item | `sample_id`, `test[0].value` |
| Delimited file | `col[occ].name`; `name` is a column `name` of the connector | `col.sample_id`, `col[1].result` |

Only these forms are accepted. The HL7 codec's own `OBX(2)-5.1` and the form `R(3).4` are not
paths in configuration.

| Protocol | Rules |
| --- | --- |
| HL7 | `MSH` has one occurrence. `MSH.1` (field separator) and `MSH.2` (encoding characters) are numbered as in the standard and cannot be written. A path to a non-UTF-8 value reads as empty. |
| ASTM | Fields are numbered as in ASTM E1394, where field 1 is the record type: `R.1` is `R`, `R.2` the sequence number, `R.3` the universal test id (`^^^GLU`, so `R.3.4` is `GLU`), `R.4` the value, `R.7` the abnormal flag, `R.13` the completion time; `P.3` and `P.4` the patient ids, `P.6` the name; `O.3` the specimen id. The message is one flat list of records: `R[2]` is the third result of the message wherever it sits under its patient and order. Field 1 and `H.2` (the delimiters) cannot be written. A new record is inserted before `L`. |
| AU | Header fields: `kind` (`normal`, `rerun`, `stat`, `stat_rerun`, `reagent_blank`, `calibration`, `quality_control`), `unit_no`, `rack`, `cup`, `sample_type`, `sample_no`, `sample_id`, `reserved`, `sex`, `age`, `month`, `info1` to `info6`. Items: `test[n].code`, `.value`, `.flags`. Padding spaces are trimmed, leading zeroes kept; a field the instrument profile does not carry is empty. AU messages are read-only: no `to` path or `into` record can be AU. |
| Delimited file | `col.<name>` is the column of the first row. A `grouped` message has several rows: `col[1].<name>` is the second, and `for_each` can run `over` or `into` `col`. Values are text; decimals are never converted. |
| `$.` | Inside `for_each` a path may begin with `$.` and omit the record: `$.3.1` (HL7, ASTM), `$.value` (AU), `$.sample_id` (delimited). In `from` and in conditions it addresses the current input occurrence; in `to` the output occurrence (the one the loop created, with `mode: "update"` the target's copy of the input occurrence, with `mode: "repeat"` repetition *i* of the `into` field). Inside `append` a `to` is always a `$.` path of the new record; inside a `calculate` `find`, `$.` addresses the candidate occurrence. Elsewhere a `$.` path is invalid. |

**Reading and writing structure (HL7 and ASTM).** Reading a field, repetition or component
returns decoded text with the separators of the levels below it in place. Writing is symmetric:
text written to a field, repetition or component is split on the separators below that level
and stored as structure. The separators are those of the message (HL7 `~` repetition, `^`
component, `&` subcomponent by default; ASTM `\` repeat and `^` component by default). So
`constant "ORU^R01"` to `MSH.9` stores two components, and an ASTM `^^^GLU` copied to `OBX.3`
stores three empty components and `GLU`. Writing a subcomponent path (HL7 `PID.5.1.1`) or an ASTM
component path (`R.3.4`) stores the text literally, delimiters included, escaped on the wire. A
value copied to a field-level target is therefore read as structure when its text contains a
separator, even if it was an escaped delimiter in the source; copy at the leaf path to keep it
literal. ASTM text is escaped when the message is encoded; characters outside printable ASCII
use hexadecimal escapes.

### 8.1 Expressions (filters and conditions)

| Form | Meaning |
| --- | --- |
| `{"path":P, "op":O, "value":V}` | Compare the field at `P`. Optional `"ignore_case": true`. |
| `{"all":[E…]}`, `{"any":[E…]}`, `{"not":E}` | Boolean combination; an object has exactly one of these keys. |

A text `value` is a JSON string or number (a number is compared as its text). Operators:

| Operator | True when the field text … | `value` |
| --- | --- | --- |
| `equals`, `not_equals` | is / is not the value | text |
| `in`, `not_in` | is / is not one of the values | non-empty array of text |
| `starts_with`, `contains` | starts with / contains the value | text |
| `matches` | contains a match of the regular expression (anchor with `^` and `$`); linear-time engine | pattern, ≤ 200 characters; compiled regex ≤ 1 MiB |
| `exists`, `not_exists` | is not empty / is empty | none |
| `gt`, `gte`, `lt`, `lte` | compares to the value as exact decimals | decimal number |

- **Missing field.** A missing field is empty text; every operator evaluates on the empty
  string. It satisfies `not_exists` and `equals ""`; it satisfies `not_equals` for a non-empty
  value and `not_in` when the list does not contain `""`. It satisfies `in` when the list
  contains `""`, and `starts_with` or `contains` with an empty value. It does not satisfy
  `exists` or any numeric operator. A path of another protocol than the message reads as empty.
- **Decimals.** The accepted form is an optional sign, digits and an optional fraction (`5`,
  `-0.25`, `.5`, `7.`); surrounding whitespace is ignored. There is no rounding and no length
  limit. Text that is not a decimal (`<5`, `1e3`, `1,000`, empty) compares false for every
  numeric operator. Timestamps of one format compare in time order because they are digit strings of equal length.
- **`ignore_case`** lowercases both sides of the text operators; `matches` uses case-insensitive
  regular expression matching. It has no effect on `exists`, `not_exists` and the numeric operators.
- **Evaluation** never fails. Expressions nest at most 32 deep; empty `all`, `any`, `in` and
  `not_in` lists are validation errors, because they would pass or drop every message.

### 8.2 Mapping

| Field | Type | Default | Validation |
| --- | --- | --- | --- |
| `tables` | object | `{}` | Name to object of string to string (code tables). |
| `require_single_group` | boolean | `false` | `true`: before any rule runs, a source message holding more than one patient, order or specimen group (HL7: more than one `PID`, `OBR` or `SPM`; ASTM: more than one `P` or `O`) fails the mapping with a transform error at the first repeated record (`rule require_single_group (mapping) at path P: the message holds more than one patient, order or specimen group`). For mappings that copy one sample's identifiers to every result, such as presets that assume one sample per message: the delivery fails visibly instead of attributing results to the wrong sample. Nothing is filtered. |
| `require` | array | `[]` | Preconditions on the source, checked in order after `require_single_group` and before any rule. `{"count": R, "min": n, "max": n}`: the number of occurrences of record or segment `R` is within `min` to `max` (integers 0–65,535, at least one, `min` ≤ `max`). `{"same": P}`: every occurrence of the record of path `P` (written without `$.` and without an occurrence) holds the same text at `P`, missing counting as empty; zero or one occurrence passes. A failed condition is a transform error naming `require[i]`, op `require` and the record or path, with the reason `the message has 19 occurrences; required 1 to 18` (or `at most …`, `at least …`) or `the occurrences hold different values`; nothing is filtered. For example a one-specimen order download: `[{"count": "PID", "max": 1}, {"count": "SPM", "max": 1}, {"count": "OBR", "min": 1, "max": 18}, {"same": "OBR.3.1"}, {"same": "ORC.1"}]`. |
| `rules` | array | none | Ordered [rules](#rules), executed top to bottom. An HL7 or ASTM target of the same protocol starts as a copy of the source. A cross-protocol target starts empty except for generated header fields (HL7 MSH from the connector settings, a new MSH-10 and MSH-7; ASTM H and L records). Delimited output starts with empty rows and writes the configured file header. |

#### Rules

| `op` | Fields | Behavior |
| --- | --- | --- |
| `copy` | `from`, `steps` (optional), `to`, `default` (optional), `required` (default `false`) | Copy the text. Empty or missing source uses `default`; still empty and `required` is a transform error. |
| `constant` | `value`, `to` | Write the text. |
| `lookup` | `from`, `steps` (optional), `table`, `to`, `on_missing` (default `error`), `default`, `ignore_case` (default `false`) | Map through `tables[table]` by exact match, or with case folded on both sides when `ignore_case` is true. `on_missing`: `error` (transform error), `keep` (copy unchanged), `default` (use `default`), `skip` (write nothing). |
| `concat` | `parts`, `to` | `parts` is a non-empty array of `{"from":P}` (optionally with `"steps"`) or `{"text":S}` (exactly one of `from` and `text`); results joined in order. |
| `datetime` | `from`, `steps` (optional), `to`, `input`, `output`, `from_timezone` (optional), `to_timezone` (optional) | Reformat using the [date and time formats](#date-and-time-formats) below. Timezones are IANA names; both present converts, otherwise no conversion. Input missing components that the output needs is a transform error. |
| `calculate` | `formula`, `to`, `scale`, `rounding`, `comparator`, `notation`, `on_invalid`, `default`, `required` | Exact decimal arithmetic over fields and constants: see [Calculations](#calculations). |
| `when` | `condition`, `then`, `else` (optional) | Run the `then` rules when the expression holds, otherwise `else`. |
| `for_each` | `over`, `into`, `mode` (default `create`), `rules` | For every occurrence of the record or segment `over` (for example `R`), run `rules` where a path beginning `$.` addresses the current input occurrence (in `from`) or the output occurrence (in `to`). `over` and `into` are record names (`OBX`, `R`, `test`, `col`). `mode: "create"`: each input occurrence creates a new `into` occurrence, the output. `mode: "update"`: the output is the target's copy of the same occurrence, so a same-protocol mapping rewrites every OBX or R in place instead of adding new ones; valid only when `into` equals `over` and source and target are both HL7 or both ASTM. `mode: "repeat"`: `into` is a field path of an HL7 or ASTM target (`O.5`, `O[1].5`; no repetition or component) and input occurrence *i* writes repetition *i* of that field: a `$.` `to` addresses that field without a repetition (`$.5.4` is `O.5[i].4`), absolute `to` paths are allowed, `sequence` writes `start` + *i*, and a repetition whose iteration wrote nothing stays empty. For example every OBR test as one ASTM order record's universal test ids: `{"op": "for_each", "over": "OBR", "into": "O.5", "mode": "repeat", "rules": [{"op": "copy", "from": "$.4.1", "to": "$.5.4", "required": true}]}`. |
| `append` | `into`, `rules` | Add one `into` record or segment (for example a calculated result): see [Appended records](#appended-records). |
| `sequence` | `to`, `start` (default `1`) | Inside `for_each`: write `start` plus the 0-based index of the current input occurrence, which is the 1-based loop counter by default (for example OBX-1). Inside `append`: `start` plus the 0-based occurrence number of the new record among records of its name. `start` is an integer from 0 to 999,999,999. |

Text fields `copy.default`, `constant.value`, `lookup.default` and `concat.parts[].text`
accept JSON strings or numbers; a number is used as its text. `calculate` constants and
`calculate.default` accept strings and safe JSON integers only ([Calculations](#calculations)).

Rule semantics:

| Case | Behavior |
| --- | --- |
| Empty result | Nothing is written when the result is empty: `copy` with an empty source and no `default`, `concat` of only empty parts, `datetime` of an empty source, `lookup` with `skip` (or with `keep` or `default` that yield empty text), `calculate` with an empty operand or with `on_invalid: "skip"`. `constant` and `sequence` always write. |
| Empty lookup source | Counts as a table miss: `on_missing` applies. |
| `datetime` failure | An input that does not match its format, names a date that does not exist, or lacks a component the output needs (year, month, day, hour, minute, second) is a transform error. With both time zones, the input needs year to minute; absent seconds convert as `:00` and stay absent. A local time that does not exist or occurs twice at a clock change resolves with the offset in force before the change. |
| Transform error | The first failing rule stops the mapping and no output is produced. The error names the rule (for example `rules[2].rules[1]`), its `op` and path and the reason, never a value. |
| Writes | Rules add or overwrite what they address; a target that starts as a copy of the source keeps everything else. A write to an HL7 segment or ASTM record occurrence that does not exist yet creates it. New HL7 segments are appended in the order of first write. |
| Text steps | Optional `steps` after a `from` (in `copy`, `lookup`, `datetime`, `concat` parts and `calculate` operands) change the text read before the rule uses it: see [Text steps](#text-steps). An empty result of the steps is an empty source (`copy.default` applies, `lookup` misses). |

#### Calculations

```json
{ "op": "calculate", "formula": { "mul": [ { "from": "$.4" }, "0.0555" ] },
  "to": "$.5", "scale": 2 }
```

| Field | Default | Meaning |
| --- | --- | --- |
| `formula` | required | What to compute (below). |
| `to` | required | Where the result is written. |
| `scale` | none | Integer 0–12: decimal places of the result, which is rounded to it and written with exactly that many places (`5.00`, `100`). Required when the formula contains `div`. Without `scale` the exact result is written in its shortest form: trailing zeros are dropped (`5.50` read and written is `5.5`, `5.50 × 2` is `11`), so set `scale` wherever the number of places is part of the reported result. |
| `rounding` | `half_up` | `half_up` (ties away from zero: 2.345 → 2.35, −2.345 → −2.35), `half_even` (ties to the even digit: 2.345 → 2.34) or `truncate` (toward zero). Only with `scale`. Not used for a result that carries a comparator. |
| `comparator` | `carry` | For operand text with a leading `<`, `>`, `<=` or `>=` (spaces after it allowed). `carry`: compute with the number and put the comparator before the result (`<5` × 0.0555 at scale 2 → `<0.28`). It is reversed when the direction reverses (multiplying or dividing by a negative, being subtracted); multiplying by an exact zero gives `0` without comparator. With `scale`, the number of a carried comparator is rounded so the bound stays true: up for `<` and `<=`, down for `>` and `>=` (`<0.2725` → `<0.28`, `>17.68` → `>17.6` at scale 1, whatever `rounding` says). Two comparator operands, or one inside `min`, `max`, `abs` or as a divisor, cannot be carried and are invalid. `reject`: text with a comparator is not a number. |
| `notation` | `text` | `sn`: HL7 structured numeric values (below). |
| `on_invalid` | `error` | When an operand is not a number, a divisor is zero, a comparator cannot be carried or a value is out of range: `error` (transform error), `skip` (write nothing), `keep` (write the operand's text as read; only for a formula with exactly one `from` or `find` operand), `default` (write `default`, which is then required; a string, or a JSON integer as for constants). |
| `required` | `false` | An empty or missing operand, or a `find` without a match, makes the result empty and nothing is written; with `required` that is a transform error. |

A formula is one of:

| Form | Value |
| --- | --- |
| `"5.5"`, `"18.016"`, `-2` | A constant decimal: a JSON string, or a JSON integer from −9,007,199,254,740,991 to 9,007,199,254,740,991. A fractional or larger JSON number is a validation error (write it as a string): JSON numbers are read as binary floating point before the engine sees them, so `1.0000000000000001` would arrive as `1`. |
| `{"from": P, "steps": [S…]}` | The field at `P` after optional [steps](#text-steps). Accepted text: surrounding spaces, an optional sign, digits and an optional fraction (`5`, `-0.25`, `.5`, `7.`). `NEG`, `1e3`, `1,000` and `5 mmol` are not numbers. |
| `{"find": R, "where": E, "from": P, "steps": [S…]}` | The field `P` of the first occurrence of record or segment `R` for which [expression](#81-expressions-filters-and-conditions) `E` holds; inside `find`, `$.` paths in `where` and `from` address the candidate, and `from` is a `$.` path. For example the sodium result: `{"find": "OBX", "where": {"path": "$.3.1", "op": "equals", "value": "NA"}, "from": "$.5"}`. No match is an empty operand. |
| `{"add": [F…]}`, `{"sub": [F…]}`, `{"mul": [F…]}`, `{"div": [F…]}` | Sum, difference (first minus each following), product, quotient (first divided by each following) of 2–16 formulas. |
| `{"min": [F…]}`, `{"max": [F…]}` | Smallest, largest of 2–16 formulas (`{"max": [{"from": "$.4"}, 0]}` clamps negatives to 0). |
| `{"abs": F}` | Absolute value. |

Formulas nest at most 8 levels and hold at most 64 constants, operands and operators. There is
no floating point: values are exact fractions of 128-bit integers (about 38 digits in total,
integer and fraction digits together), computed exactly and rounded once at the end; an
operand, intermediate value or result beyond that range is out of range. Results have no `+`,
no exponent, never `-0`, and a `0` before the point (`0.28`).

`find` addresses the whole message, so it only runs on a source with at most one group: more
than one HL7 `PID`, `OBR` or `SPM`, or more than one ASTM `P` or `O`, is a transform error (the
message holds more than one patient, order or specimen group) whatever `on_invalid` says,
because the first match could belong to another sample than the result.

**Structured numeric (`notation: "sn"`).** For HL7 values such as `OBX-5 = <^5` (`OBX-2 = SN`):
source and target are HL7, and every operand and `to` path addresses a field or repetition
(`$.5`), not a component. Component 1 is the comparator (empty or `=` for none; `<`, `>`, `<=`,
`>=`; anything else, including `<>`, is not a number), component 2 the number (steps apply to
it); a value with component 3 or 4 (range `^10^-^20`, ratio `^1^:^128`) is not a number. The
result replaces the field at `to` with the comparator and the number (`<^17.7`, `^5.00`);
`on_invalid: "keep"` writes the operand field unchanged. Each OBX in place:

```json
{ "op": "for_each", "over": "OBX", "into": "OBX", "mode": "update", "rules": [
  { "op": "when", "condition": {"path": "$.3.1", "op": "equals", "value": "CREA"}, "then": [
    { "op": "calculate", "notation": "sn", "formula": {"mul": [{"from": "$.5"}, "88.4"]},
      "to": "$.5", "scale": 1, "on_invalid": "keep" },
    { "op": "constant", "value": "umol/L", "to": "$.6" } ] } ] }
```

Runtime reasons (never with the value): `value is not a decimal number` and `the text steps
would make the value longer than allowed` (path: the operand),
`division by zero`, `the value is outside the supported range`, `a comparator value cannot be
used here` (path: `to`), `required value is missing` (path: the operand), `the message holds
more than one patient, order or specimen group`.

#### Appended records

`append` adds one `into` record or segment, typically a result calculated from others:

```json
{ "op": "append", "into": "OBX", "rules": [
  { "op": "calculate", "to": "$.5", "on_invalid": "skip", "formula": { "sub": [
      { "find": "OBX", "where": {"path": "$.3.1", "op": "equals", "value": "NA"}, "from": "$.5" },
      { "find": "OBX", "where": {"path": "$.3.1", "op": "equals", "value": "CL"}, "from": "$.5" },
      { "find": "OBX", "where": {"path": "$.3.1", "op": "equals", "value": "HCO3"}, "from": "$.5" } ] } },
  { "op": "sequence", "to": "$.1" },
  { "op": "constant", "value": "NM", "to": "$.2" },
  { "op": "constant", "value": "AG^Anion gap", "to": "$.3" } ] }
```

- `rules` is non-empty and holds only `copy`, `constant`, `lookup`, `concat`, `datetime`,
  `calculate` and `sequence`. Every `to` is a `$.` path, which addresses the new record; `from`
  paths are absolute.
- All or nothing: the record is added only when every rule writes. If one writes nothing (an
  empty source, a missing operand, `skip`), nothing is added and the mapping continues.
- `into` follows the `for_each.into` rules. HL7 segments are added at the end of the message,
  ASTM records before `L`. A target holding more than one group (as for `find`) is a transform
  error at `into`, because the record would land under the last group whatever group its values
  came from.
- `append` may be at the top level or inside `when`, not inside `for_each` or `append`.

#### Text steps

`"steps": [S…]` after a `from` runs in order on the text read (at most 16 steps). No step may
produce more than 65,535 bytes or the field's length before the steps, whichever is larger:
`replace`, `upper` and `lower`, the steps that can lengthen text, compute their result's length
first and refuse it before building it, so cleanup may lengthen an ordinary value modestly and
a large field (an embedded document) may still be processed as long as it does not grow. A
refused step is a transform error at the `from` path (`the text steps would make the value
longer than allowed`, never with the value) in every rule, whatever `default`, `on_missing` or
`required` say; in `calculate` it is an invalid operand (`on_invalid` `error`, `skip` and
`default` apply; `keep` is then a transform error, because the operand text is what could not be
produced).

| Step | Fields | Result |
| --- | --- | --- |
| `{"step": "trim"}` | | Leading and trailing whitespace removed. |
| `{"step": "strip_leading", "chars": "0"}` | `chars`: 1–16 characters | Leading characters in `chars` removed, never the last character (`000120` → `120`, `000` → `0`). |
| `{"step": "split", "separator": "/", "take": 0}` | `separator`: 1–16 characters; `take`: −100 to 100 | Part `take` of the text split on `separator` (0 the first, −1 the last); a part that does not exist is empty. |
| `{"step": "slice", "start": 0, "length": 20}` | `start`: 0–65,535; `length` (optional): 1–65,535 | Up to `length` characters from `start` (0-based); empty past the end. |
| `{"step": "replace", "find": ",", "with": "."}` | `find`: 1–64 characters; `with`: 0–64 characters | Every occurrence of `find` replaced, literally and case-sensitively. |
| `{"step": "upper"}`, `{"step": "lower"}` | | Case changed. |

#### Date and time formats

Tokens are `yyyy` (year), `yy` (year, 1970 to 2069), `MM`, `dd`, `HH` (00–23), `mm`, `ss`; every
token is fixed-width and zero-padded. Any other punctuation or space is a literal. Any other
letter must be quoted: `'T'` is a literal `T`, and `''` inside quotes is a quote, so
`yyyy-MM-dd'T'HH:mm:ss` is ISO 8601 local time. A token appears at most once, and a format has at
least one token. The input must match its format completely.

#### Semantic checks

Mapping and filter paths are checked for syntax when the document is validated and against
the protocol models. HL7 accepts segment names matching `[A-Z][A-Z0-9]{2}`; ASTM accepts its
record letters. Their field numbers are checked against the syntax limit, without a
segment/record-specific field dictionary. AU field names and delimited column names must
exist in their models. `from` paths that
are valid but absent in a given message are empty at run time. Every problem is returned,
each with its JSON path. Besides syntax and unknown names, these are errors:

| Rule |
| --- |
| An unknown field in a rule or expression, or a field of the wrong type. |
| A `lookup` of a table that is not defined; `on_missing` other than `error`, `keep`, `default`, `skip`. |
| `default` on a `lookup` unless `on_missing` is `default`, where it is required. |
| `ignore_case` on a `lookup` whose table has two codes that differ only by case and map to different texts. |
| `for_each` or `append` inside `for_each` or `append`; `when` inside `append`; `sequence` or a `$.` path outside `for_each` and `append`; a `to` inside `append` that is not a `$.` path, or a `from` inside it that is. |
| `for_each.mode` other than `create`, `update` and `repeat`; `update` with `into` different from `over` or without an HL7 or ASTM target of the source protocol; `repeat` without an HL7 or ASTM target, with an `into` that is not a field path, or with a `$.` `to` of another field or with a repetition. |
| A `require` entry that is not `{"count": …}` or `{"same": …}`, an unknown record, `count` without `min` and `max`, a bound outside 0–65,535 or `min` > `max`, a `same` path that starts with `$.` or names an occurrence. |
| `require_single_group` that is not a boolean. A `calculate` formula that is not one of the forms, a constant that is not a decimal or is a fractional or larger-than-2^53 JSON number (also for `calculate.default`), an operator list of fewer than 2 or more than 16 formulas, more than 8 levels or 64 parts; `scale` outside 0–12 or missing with `div`; `rounding` without `scale`; an unknown `rounding`, `comparator`, `notation` or `on_invalid`; `default` without `on_invalid: "default"` or missing with it; `keep` without exactly one operand; a `find` record the model does not have or a `find.from` that is not a `$.` path; `notation: "sn"` without an HL7 source and target or with a component path. |
| An unknown step, a step field out of its range, more than 16 steps, or `steps` on a `concat` text part. |
| A `to` path or `into` record that cannot be written (AU, `MSH.1`, `MSH.2`, ASTM field 1 and `H.2`, a second `MSH`, an ASTM `H` or `L`). |
| An invalid date format, or a time zone the system does not know. |
| A `matches` pattern over 200 characters, not a valid regular expression, or exceeding the 1 MiB compiled-size limit; a numeric operator with a non-decimal `value`. |

The version of a mapping is the `config_version` in force when the transformation ran; it is
stored with each transformed body ([message-channels.md](message-channels.md#6-pipeline)).

## 9. Validation

The engine validates a document completely before applying and returns every problem, not
the first. Each problem is `{severity, code, path, message}`; `path` is a JSON path such as
`channels[2].connection.receiver`. Any `error` rejects the document; `warning` does not.

| Code | Severity | Rule |
| --- | --- | --- |
| `schema` | error | Type, range, enumeration, unknown field, missing required field. |
| `duplicate_id` | error | `endpoint_id`, `channel_id`, `destination_id` or column `name` repeated within its scope. |
| `unknown_endpoint` | error | A channel references an endpoint that does not exist. |
| `endpoint_disabled` | error | An enabled channel (or enabled destination) references a disabled endpoint. |
| `endpoint_kind_not_allowed` | error | Bridge endpoint is not `serial`/`tcp_client`/`tcp_listener`, or a protocol does not suit the endpoint kind. |
| `same_endpoint` | error | A bridge's `endpoint_a` equals `endpoint_b`. |
| `port_conflict` | error | Two enabled endpoints use the same serial `port_name` (case-insensitive) or the same listener port on overlapping bind addresses. |
| `listener_strategy` | error | `wait_for_listener_then_pair` without a `tcp_listener` endpoint. |
| `receiver_required` | error | `receiver_then_sender` without `receiver`, or `receiver` set with another strategy. |
| `allowlist_required` | error | Non-loopback `tcp_listener` with empty `allowed_peer_cidrs` and `allow_any_peer=false`. |
| `invalid_cidr` | error | An `allowed_peer_cidrs` entry is not valid CIDR notation. |
| `retry_invalid` | error | A retry number out of range, a multiplier with more than 4 decimal places, or `max_delay_ms < initial_delay_ms`. |
| `tap_invalid` | error | `record_messages=true` with framer `none`. |
| `ack_level_unreachable` | error | `ack_required` above what the protocol can prove, or `ack_mode=none` with `ack_required` above `handed_off`. |
| `passthrough_protocol` | error | A destination with `mapping=null` has a protocol different from the source. |
| `mapping_invalid` | error | Rule syntax, unknown op, unknown table, or a path not valid for the protocol model. |
| `filter_invalid` | error | Expression syntax or a path not valid for the protocol model. |
| `secret_inline` | error | A secret field holds a literal instead of `{"secret": name}`. |
| `secret_unresolved` | warning | A referenced secret does not resolve now; the endpoint will be in `error` until it does. |
| `shared_exclusive_endpoint` | warning | More than one enabled channel references the same serial or listener endpoint. |
| `retention_unbounded` | warning | A message channel's `retention`, or a recording tap's `tap.retention`, has neither limit. |

Documents read from disk at startup are external input and go through the same validation.

A deserialization error (wrong type, unknown field, missing required field, invalid
enumeration or CIDR, malformed JSON, or text after the document) stops parsing: the engine
reports that one problem with its path and the hint "Fix this and validate again", and later
problems are reported once it is fixed. Its code is `invalid_cidr` for a malformed CIDR and
`schema` otherwise. Checks on a parsed document, including range checks reported as `schema`,
return all their problems together. `secret_unresolved` compares secret
references with the names held by the secret store when the document is validated or
applied. `mapping_invalid` and `filter_invalid` come from compiling every filter and mapping
with the transformation engine when the document is validated (so `packie config validate`
reports them offline): channel and destination filters against the source protocol's model,
mapping `from` paths against the source's and `to` paths against the destination's (delimited
models know their configured columns); the errors are those of
[Semantic checks](#semantic-checks). Each problem carries the path of the offending rule or
field.

## 10. Versioning and apply

| Rule | Behavior |
| --- | --- |
| Entry | A client sends a complete document, the `base_version` it started from and a note saying why (1–500 characters after trimming surrounding whitespace; required, else `reason_required`). If `base_version` is not the active `config_version` the apply is rejected (`version_conflict`) so concurrent edits do not overwrite each other. |
| Dry run | `validate` returns the problems and the diff against the active one (what each channel and endpoint would undergo: `added`, `changed`, `updated`, `removed`, `unchanged`, and `stopped` for a channel the document disables that would otherwise be added or changed) without changing anything. The diff is always present; it is empty when the document has errors. |
| Order | Validate → on success save the change-control record, then the version document, then mirror it to `runtime.file` → make the new version active → apply per channel. Each file is written atomically (temp file, flush, rename); a record whose document cannot be written is removed, so no version exists without its record. At startup the newest valid version that has its record is active; a version file without one is skipped with `config.version_skipped` (a store that keeps no records at all, from before records were kept, loads as before). A document with errors changes nothing; the previous configuration stays active. |
| Version | Each successful apply creates version `previous + 1`: `runtime.v<N>.json` (N zero-padded to ten digits) with its change-control record `runtime.v<N>.meta.json` (who applied it, the OS account behind the connection; when; the note). The engine's own saves (startup import) are attributed to `system`. The per-channel outcomes of the apply are added to the record once the apply finishes (empty for the startup import). Versions are kept without limit by default (`runtime.versions_kept=null`); a configured limit removes older versions beyond it only after the `audit.retention_days` protection period. |
| Verification | `verify_config_version { config_version, note }` signs a saved version off as verified after the change: the account, the time and the note (1–500 characters after trimming surrounding whitespace, such as a validation record number; otherwise `reason_required`) are added to its record; a version can be signed off more than once. What verified means is the laboratory's procedure. The history lists, per version: number, time, applied by, endpoint and channel counts, the per-channel outcomes, note and sign-offs; a version without one shows "not verified since change". |
| Rollback | Console only: applies an older stored document as a new version with the note `rollback of <N>: <reason>` using the operator's trimmed reason. |
| Unchanged | A channel is unchanged when its definition and the definitions of the endpoints it references are equal after normalization (defaults filled in, key order and spelling of defaults irrelevant). Unchanged channels are not restarted. |
| Endpoint change | An endpoint definition change restarts every channel that references it; an unreferenced endpoint is reconfigured on its own. |
| In place | A change confined to `label` or `tags` of a channel, or, for message channels, to filters, destination filters, mappings, `delivery`, `retention`, is applied without restarting the channel (outcome `updated`). |
| Result | Every apply returns a per-channel outcome: `unchanged`, `updated`, `started`, `restarted`, `stopped`, `removed`, `failed`. |
| Failure | If any channel change fails, the apply is reported `partial`: the new version is saved and becomes active (one active version keeps history, diff and the console consistent), changes to other channels are in effect, and the failed channel keeps its previous definition and instance when it was working, reports `failed` with its reason and raises `config_apply_failed`. The next apply retries the failed channel even when its definition is unchanged. |
| Pending | The snapshot reports `pending_version` while an apply runs and `active_version` throughout. |
| Operator pause | A channel stopped by an operator keeps its stopped state through an apply; its stored definition updates. |
| Startup | A valid `runtime.json` is the configuration; when it differs from the newest stored version it is saved as the next version (note `imported from the runtime file`). If `runtime.json` is unreadable or invalid, the engine starts from the newest valid stored version and raises `config_rejected`; if none exists it runs with zero channels and a degraded node state. With no `runtime.json` the newest valid stored version applies; with neither, zero channels (and the same degraded state). |

How an apply replaces a running channel (break-before-make or make-before-break) is in
[bridge-channels.md](bridge-channels.md#13-configuration-apply) and
[message-channels.md](message-channels.md#12-configuration-apply).

## 11. Secrets

A secret is referenced as `{"secret": "<name>"}` wherever a secret is accepted (SFTP
`password`, `private_key`, `passphrase`). Names are `^[a-z0-9][a-z0-9_.-]{0,62}$`. The value is
resolved when the endpoint connects, from the OS key store under service `packie` or, with
`secrets.provider=file`, from the secrets file (JSON object of name to string, read on every
resolve, so edits apply without a restart). A key store cannot list its entries, so the names
(never values) are also kept in `<state_directory>/secrets.names.json`. Secrets never
appear in `runtime.json`, logs, snapshots, alerts, exported configuration or error messages
(SEC-03). Writing secrets is a console/CLI function that goes to the provider, not to a
document: Settings → Secrets, or `packie secrets set <name>` (the value is asked for without echo
or read from standard input, never taken as an argument), `packie secrets delete <name>` and
`packie secrets list` (names only).

## 12. Examples

Bridge: serial instrument to a TCP listener, receiver first.

```json
{
  "schema_version": 1,
  "endpoints": [
    { "endpoint_id": "hema-serial", "label": "Analyzer serial port", "kind": "serial",
      "settings": { "port_name": "COM3", "baud_rate": 9600 } },
    { "endpoint_id": "lis-listener", "label": "LIS connects here", "kind": "tcp_listener",
      "settings": { "bind_address": "10.0.0.5", "port": 5000, "allowed_peer_cidrs": ["10.0.0.20/32"] } }
  ],
  "channels": [
    { "channel_id": "hema-bridge", "label": "Hematology to LIS", "kind": "bridge",
      "endpoint_a": "hema-serial", "endpoint_b": "lis-listener",
      "connection": { "connect_strategy": "receiver_then_sender", "receiver": "a" } }
  ]
}
```

Message channel: ASTM results in, HL7 out.

```json
{
  "schema_version": 1,
  "endpoints": [
    { "endpoint_id": "chem-tcp", "label": "Chemistry analyzer", "kind": "tcp_listener",
      "settings": { "bind_address": "10.0.0.5", "port": 6000, "allowed_peer_cidrs": ["10.0.0.31/32"] } },
    { "endpoint_id": "lis-hl7", "label": "LIS HL7 interface", "kind": "tcp_client",
      "settings": { "host": "10.0.0.20", "port": 3001 } }
  ],
  "channels": [
    { "channel_id": "chem-results", "label": "Chemistry results to LIS", "kind": "message",
      "source": { "endpoint": "chem-tcp", "protocol": "astm", "settings": {} },
      "destinations": [
        { "destination_id": "lis", "label": "LIS", "endpoint": "lis-hl7", "protocol": "hl7_mllp",
          "settings": { "ack_mode": "enhanced", "sending_application": "PACKIE", "receiving_application": "LIS" },
          "mapping": { "tables": { "flags": { "H": "H", "L": "L", "N": "N" } },
            "rules": [
              { "op": "constant", "value": "ORU^R01", "to": "MSH.9" },
              { "op": "copy", "from": "P.3", "to": "PID.3" },
              { "op": "copy", "from": "O.3", "to": "OBR.3" },
              { "op": "for_each", "over": "R", "into": "OBX", "rules": [
                { "op": "sequence", "to": "$.1" },
                { "op": "copy", "from": "$.3", "to": "$.3" },
                { "op": "copy", "from": "$.4", "to": "$.5" },
                { "op": "lookup", "from": "$.7", "table": "flags", "to": "$.8", "on_missing": "skip" } ] } ] } }
      ] }
  ]
}
```
