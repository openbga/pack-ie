# Configuration field reference

Use this reference to look up fields while editing PACK-IE configuration. The
[configuration specification](../spec/configuration.md) defines validation, apply behavior,
path syntax and transformation rules in full.

`bootstrap.json` is read at engine start; restart after editing it. `runtime.json` is
versioned and applied through the console or CLI. Unknown fields are rejected. Both files
are UTF-8 JSON without comments. Durations use the unit in their name; sizes are bytes
unless named `_megabytes`. Relative bootstrap paths resolve beside `bootstrap.json`.

In the tables, **required** means no default, **assigned** means the engine supplies it,
and `null` is allowed only where shown. IDs match `^[a-z0-9][a-z0-9._-]{0,63}$`; labels
contain 1–100 characters. Keep IDs stable. Settings fields are relative to their named
object, rather than complete JSON paths.

## Bootstrap

| Field | Default | Value or purpose |
| --- | --- | --- |
| `schema_version` | `1` | Integer; supported schema version. |
| `node.node_id` | Required | ID; also used in the Windows pipe name. |
| `node.display_name` | Required | Display label. |
| `node.environment` | `prod` | `dev`, `test`, `prod`. |
| `node.site_id` | `null` | Optional string, at most 100 characters. |
| `paths.state_directory` | Required | Creatable, writable directory for stored messages, versions and IPC socket. |
| `paths.log_directory` | `<state_directory>/logs` | Creatable, writable directory. |
| `ipc.path` | `null` | Default: `<state_directory>/packie.sock` on Unix or `\\.\pipe\packie-<node_id>` on Windows. Custom Unix socket path: at most 100 bytes. |
| `ipc.group` | `packie` | Management access group on every platform. Unix: socket mode `0660` for the group. Windows: `packie` means local `PACK-IE Operators`; another name selects a local/domain group besides SYSTEM and Administrators. `null` or a missing group leaves service/administrator access only (Unix `0600`); a missing group logs `ipc.group_unavailable`. Empty string is invalid. |
| `hosting.shutdown_drain_seconds` | `15` | Integer, 1–300; host stop timeout must allow at least this long. |
| `runtime.file` | `<state_directory>/runtime.json` | Runtime document; parent directory must be writable. |
| `runtime.versions_kept` | `null` | Keep all versions; otherwise a positive integer limits the count, preserving versions younger than `audit.retention_days`. Includes change records/sign-offs. |
| `logging.level` | `info` | `error`, `warn`, `info`, `debug`, `trace`. |
| `logging.retention_days` | `30` | Integer, 1–3650. |
| `logging.rotate_at_megabytes` | `64` | Integer, 1–1024; logs also rotate at local midnight. |
| `logging.ring_buffer_events` | `20000` | Integer, 1000–1,000,000; live in-memory log capacity. |
| `diagnostics.snapshot_interval_seconds` | `1` | Integer, 1–60. |
| `diagnostics.retain_recent_errors` | `100` | Integer, 10–10,000 per channel. |
| `secrets.provider` | `keystore` (macOS, Windows); `file` (Linux) | `keystore` or `file`. On Linux `keystore` needs a Secret Service (desktop session); without one the engine refuses to start and names `file`. The Linux package sets `file`. |
| `secrets.file` | `<state_directory>/secrets.json` with `provider=file` | Secret-store file path; created owner-only on the first secret write; on Unix, group and others must have no read access. |
| `audit.retention_days` | `2190` | Integer, 365–36,500. Removes whole sealed yearly audit files only after their newest event ages out; records the removal. Keep at least as long as the longest message retention and the lab's required calendar interval. |
| `backup.directory` | Not set | Backup root; required with a schedule. Each default/scheduled backup uses a new `packie-backup-<UTC time>` directory. Use an encrypted volume writable by the service account. |
| `backup.schedule` | `null` | No automatic backup; otherwise an object with `daily_at` and optional `keep` below. |
| `backup.schedule.daily_at` | Required with schedule | Local time `HH:MM`, daily. A time skipped by daylight saving runs at the next valid instant. |
| `backup.schedule.keep` | `7` | Integer, 1–365. After a scheduled backup, retain this many newest complete `packie-backup-*` directories; other names and pre-restore copies are untouched. |
| `backup.stale_after_hours` | `26` with schedule, otherwise `null` | Integer, 1–8760; `backup_stale` warns when the newest successful backup exceeds this age, or no backup succeeds within this interval after start. `null` without a schedule turns it off. |
| `notifications.rate_limit_minutes` | `15` | Integer, 1–1440; one notification per alert code/channel/endpoint/destination in this interval, shared by targets and by raise/clear events. |
| `notifications.ca_file` | `null` | Optional path to a PEM CA certificate bundle, read at startup; a relative path resolves against the bootstrap file's directory. SMTP adds these roots to bundled Mozilla roots; HTTPS webhooks use only this bundle when set. Unreadable/invalid bundles fail TLS sends. |
| `notifications.email` | `null` | SMTP target object; `null` disables e-mail. |
| `notifications.email.host` | Required with e-mail | Non-empty relay host name or address. |
| `notifications.email.port` | Required with e-mail | Integer, 1–65535. |
| `notifications.email.security` | `starttls` | `starttls` (usually 587), `tls` (usually 465) or `none`. `none` is allowed only for `localhost`, `127.0.0.1` or `::1`. TLS verifies the relay certificate against bundled Mozilla roots plus `notifications.ca_file` when set. |
| `notifications.email.username`, `notifications.email.password` | Not set | Optional login; set both together. Password must be a secret reference, such as `{"secret":"smtp-password"}`. |
| `notifications.email.from`, `notifications.email.to` | Required with e-mail | Sender address and non-empty array of recipient addresses; each address needs one `@` with text on both sides, without spaces or angle brackets. |
| `notifications.email.severities` | `["warning","critical"]` | Non-empty subset of `warning` and `critical`. |
| `notifications.webhook` | `null` | Webhook target object; `null` disables it. |
| `notifications.webhook.url` | Required with webhook | HTTPS URL without a user name; HTTP allowed only for `localhost`, `127.0.0.1` or `[::1]`. Bundled Mozilla roots verify HTTPS certificates unless `notifications.ca_file` is set (then only that bundle is trusted); redirects are not followed. |
| `notifications.webhook.bearer` | `null` | Optional secret reference sent as `Authorization: Bearer <secret>`. |
| `notifications.webhook.severities` | `["warning","critical"]` | Non-empty subset of `warning` and `critical`. |
| `notifications.webhook.timeout_seconds` | `10` | Integer, 1–60; time to connect and receive the response. |
| `test_hooks` | `false` | Boolean; scenario hooks. Never enable in production. |

Backup paths resolve relative to the bootstrap directory like other bootstrap paths. See
[backup setup and restore](install.md#backup-and-restore) and [audit behavior](../spec/audit.md).
Notifications are off without targets. See
[notification relay setup](install.md#notification-relay-setup) for private-CA installation and
service-account file access.
Keep node/channel/endpoint labels free of patient information because notifications include them.
See [notification behavior and tests](running-and-monitoring.md#alert-notifications).
Apply notes and verification references belong to commands, not fields in `runtime.json`:
`packie config apply <file> --note "Reason"` and
`packie config verify <version> --note "Validation record reference"` require 1–500 characters.

## Runtime envelope and diagnostics

| Field | Default | Value or purpose |
| --- | --- | --- |
| `schema_version` | `1` | Integer; supported schema version. |
| `config_version` | Assigned | Successful apply increments the version. CLI apply uses an exported value to detect conflicting edits; it is not the new version number. |
| `generated_at` | Assigned | ISO-8601 UTC save time; informational. |
| `diagnostics` | `{}` | Overrides below. |
| `alerts` | `{}` | Thresholds below. |
| `endpoints` | `[]` | Endpoint definitions. |
| `channels` | `[]` | Channel definitions. |
| `diagnostics.snapshot_interval_seconds` | Bootstrap value | Integer, 1–60. |
| `diagnostics.retain_recent_errors` | Bootstrap value | Integer, 10–10,000 per channel. |

## Alert thresholds

All fields belong to runtime `alerts` and take integers. Zero disables only the thresholds
marked “off”.

| Field | Default | Range and alert |
| --- | --- | --- |
| `endpoint_down_seconds` | `60` | 1–86,400; endpoint-down warning. |
| `endpoint_down_critical_seconds` | `900` | At least `endpoint_down_seconds`; endpoint-down critical. |
| `no_peer_seconds` | `0` (off) | 0–86,400; no peer connected. |
| `buffer_high_percent` | `80` | 1–100; bridge buffer pressure level. |
| `buffer_high_seconds` | `10` | 1–3600; bridge buffer pressure duration. |
| `delivery_backlog_count` | `1000` | 0–10,000,000 (0 off); queued delivery count. |
| `delivery_backlog_age_seconds` | `300` | 0–604,800 (0 off); oldest queued delivery age. |
| `delivery_attention_critical_count` | `10` | 1–10,000,000; deliveries needing attention become critical. |
| `invalid_messages_count` | `5` | 0–10,000 (0 off); invalid messages in the window. |
| `invalid_messages_window_seconds` | `600` | 1–86,400; invalid-message counting window. |
| `disk_low_megabytes` | `1024` | 1–10,000,000; low-disk warning. |
| `disk_critical_megabytes` | `256` | 1–`disk_low_megabytes`; low-disk critical. |

## Endpoints

Fields below belong to each entry in runtime `endpoints`. Kind-specific fields belong to
that endpoint's `settings`.

| Common field | Default | Value or purpose |
| --- | --- | --- |
| `endpoint_id` | Required | ID unique among endpoints. |
| `label` | Required | Display label. |
| `enabled` | `true` | Boolean; `false` prevents opening the endpoint. |
| `kind` | Required | `serial`, `tcp_client`, `tcp_listener`, `folder`, `sftp`. |
| `settings` | Required | Object for the chosen kind. |
| `tags` | `{}` | Up to 20 string-to-string metadata entries. |

### Serial settings

| Field | Default | Value or purpose |
| --- | --- | --- |
| `port_name` | Required | Device path or COM port; unique among enabled endpoints, ignoring case. |
| `baud_rate` | Required | Integer greater than 0. |
| `data_bits` | `8` | `5`, `6`, `7`, `8`. |
| `parity` | `none` | `none`, `even`, `odd`. |
| `stop_bits` | `one` | `one`, `two`. |
| `flow_control` | `none` | `none`, `rts_cts`, `xon_xoff`. |
| `read_timeout_ms` | `1000` | Integer, 1–60,000; read polling window, not a disconnect deadline. |
| `write_timeout_ms` | `1000` | Integer, 1–60,000; expiry fails the write attempt. |
| `virtual_link` | `null` | Unix pseudo-terminal path. When set, `port_name` must be `virtual`. |

### TCP settings

| Field | Kind | Default | Value or purpose |
| --- | --- | --- | --- |
| `host` | `tcp_client` | Required | Host name, IPv4 or IPv6 literal. |
| `bind_address` | `tcp_listener` | Required | IPv4 or IPv6 literal. |
| `port` | Both | Required | Integer, 1–65535. Enabled listener binds must not overlap on the same port. |
| `connect_timeout_ms` | `tcp_client` | `5000` | Integer, 1–120,000. |
| `accept_backlog` | `tcp_listener` | `16` | Integer, 1–1024. |
| `allowed_peer_cidrs` | `tcp_listener` | `[]` | IPv4/IPv6 CIDR strings; restrict accepted peers. |
| `allow_any_peer` | `tcp_listener` | `false` | Boolean; explicitly set `true` to allow a non-loopback listener with no allowlist. Ignored with a non-empty allowlist. |
| `idle_timeout_seconds` | Both | `0` | Integer, 0–604,800; 0 disables. Closes a connection idle in both directions. |
| `keep_alive` | Both | `true` | Boolean; listener applies it to accepted sockets. |
| `keep_alive_time_sec` | Both | `30` | Integer, 1–7200; idle time before first probe. |
| `keep_alive_interval_sec` | Both | `10` | Integer, 1–3600; interval between probes. |
| `keep_alive_retry_count` | Both | `3` | Integer, 1–100; unanswered probes before failure. |

Probe settings apply when `keep_alive=true`. Older Windows hosts without probe tuning use
OS timing and log a warning; see the [TCP settings specification](../spec/configuration.md#52-tcp_client).

### Folder and SFTP settings

| Field | Kind | Default | Value or purpose |
| --- | --- | --- | --- |
| `path` | `folder` | Required | Existing readable directory; writable when used as a destination. |
| `host` | `sftp` | Required | Host name, IPv4 or IPv6 literal. |
| `port` | `sftp` | `22` | Integer, 1–65535. |
| `username` | `sftp` | Required | String, 1–255 characters. |
| `auth.method` | `sftp` | Required | `password` or `key`. |
| `auth.password` | `sftp` | Required for password auth | Secret reference. |
| `auth.private_key` | `sftp` | Required for key auth | Secret reference. |
| `auth.passphrase` | `sftp` | Optional for key auth | Secret reference or `null`. |
| `host_key_fingerprint` | `sftp` | Required | `SHA256:` plus base64; pinned server key. |
| `remote_directory` | `sftp` | Required | Absolute remote path. |
| `connect_timeout_ms` | `sftp` | `10000` | Integer, 1–120,000. |
| `operation_timeout_ms` | `sftp` | `30000` | Integer, 1–600,000 per list/read/write operation. |
| `mode` | `folder` | `watch` | `watch` or `poll`. |
| `poll_interval_seconds` | Both | `30` folder; `60` SFTP | Integer, 1–86,400; also the safety poll in folder watch mode. |
| `file_pattern` | Both | `*` | File-name glob using `?` and `*`. |
| `recursive` | Both | `false` | Boolean; include subdirectories except `processed_directory`. |
| `ignore_suffixes` | Both | `[".tmp", ".part", ".partial", ".crdownload"]` | Array of file-name suffixes never read. |
| `stable_seconds` | Both | `2` | Integer, 0–3600; unchanged size and modification time before reading. |
| `exclusive_open_check` | `folder` | `true` | Boolean; also require exclusive read access before reading. |
| `on_consumed` | Both | `move` | `move`, `delete`, `leave`; after durable receipt. |
| `processed_directory` | Both | `processed` | Relative to source directory; created when needed. |
| `write_mode` | Both | `atomic_rename` | `atomic_rename` or `direct`. |

Secret references are objects with a `secret` key naming the stored secret. Names match
`^[a-z0-9][a-z0-9_.-]{0,62}$`. Store values through Settings → Secrets in the console; never put values in
configuration. See [secrets](../spec/configuration.md#11-secrets).

## Channels

Fields below belong to each runtime `channels` entry. Every referenced endpoint must exist;
enabled channels require enabled endpoints. Sharing a serial or listener endpoint between
enabled channels produces a warning because only one channel can hold it at a time.

| Common field | Default | Value or purpose |
| --- | --- | --- |
| `channel_id` | Required | ID unique among channels. |
| `label` | Required | Display label. |
| `kind` | Required | `bridge` or `message`. |
| `enabled` | `true` | Boolean; `false` keeps the channel stopped. |
| `tags` | `{}` | Up to 20 string-to-string metadata entries. |

### Bridge fields

| Field | Default | Value or purpose |
| --- | --- | --- |
| `endpoint_a`, `endpoint_b` | Required | Different endpoint IDs; kinds `serial`, `tcp_client`, `tcp_listener`. |
| `connection.connect_strategy` | `parallel` | `parallel`, `wait_for_listener_then_pair`, `receiver_then_sender`. The listener strategy requires a listener and behaves as `parallel`. |
| `connection.receiver` | Required for `receiver_then_sender` | `a` or `b`; must be absent for other strategies. |
| `connection.pairing_timeout_ms` | `5000` | Integer greater than 0. |
| `buffer.max_buffered_segments` | `1000` | Integer greater than 0, per direction. |
| `buffer.max_bytes` | `10485760` | Integer greater than 0, per direction; both buffer limits apply. |
| `buffer.overflow_action` | `block_then_degrade` | Only `block_then_degrade`. |
| `buffer.block_timeout_ms` | `5000` | Integer greater than 0. |
| `retry` | Retry defaults below | Endpoint reconnect policy. |
| `tap.framer` | `none` | `none`, `astm`, `hl7_mllp`, `au`. |
| `tap.record_messages` | `false` | Boolean; storing observations requires a framer other than `none`. |
| `tap.max_message_bytes` | `1048576` | Integer, 1024–67,108,864. |
| `tap.retention.max_age_days` | `30` | Integer 1–36,500, or `null` for no age limit. |
| `tap.retention.max_messages` | `null` | Integer 1,000–1,000,000,000, or `null` for no count limit. |

Tap retention applies to stored bridge observations, which have no deliveries. After startup
reconciliation and every ten minutes, the retention run purges the oldest recordings beyond
either limit. Setting both limits to `null` produces a `retention_unbounded` warning only
when `tap.record_messages` is `true`. See [bridge tap behavior](../spec/bridge-channels.md#11-framing-and-tap).

### Retry policy fields

Used in channel `retry` and delivery `retry` objects. Delay grows by `multiplier`, capped
at `max_delay_ms`, with random variation of ±`jitter_percent`.

| Field | Default | Value or purpose |
| --- | --- | --- |
| `initial_delay_ms` | `1000` | Integer greater than 0. |
| `max_delay_ms` | `60000` | Integer at least `initial_delay_ms`. |
| `multiplier` | `2.0` | Number at least 1.0. |
| `jitter_percent` | `20` | Integer, 0–100. |
| `max_consecutive_failures_before_alert` | `3` | Integer greater than 0; bridge channels only, ignored for delivery retries. |

### Message fields and retention

| Field | Default | Value or purpose |
| --- | --- | --- |
| `source` | Required | Source connector below. |
| `filters` | `[]` | Ordered expressions; all must pass. |
| `destinations` | Required | 1–20 destination connectors. |
| `retry` | Retry defaults above | Endpoint reconnect/reopen policy. |
| `delivery` | Delivery defaults below | Default policy for destinations. |
| `retention.max_age_days` | `90` | Integer, 1–36,500, or `null` for no age limit. |
| `retention.max_messages` | `null` | Integer, 1000–1,000,000,000, or `null` for no count limit. |

Retention never purges undelivered or unresolved messages. Setting both limits to `null`
produces an unbounded-retention warning.

### Connector fields

| Field | Applies to | Default | Value or purpose |
| --- | --- | --- | --- |
| `destination_id` | Destination | Required | ID unique within the channel. |
| `label` | Destination | Required | Display label. |
| `enabled` | Destination | `true` | Boolean; disabled destinations get no new deliveries and queued deliveries wait. |
| `endpoint` | Both | Required | Endpoint ID. |
| `protocol` | Both | Required | Protocol compatible with the endpoint, below. |
| `settings` | Both | Per protocol | Protocol settings below. |
| `duplicate_detection` | Source | `true` | Boolean. |
| `filters` | Destination | `[]` | Expressions evaluated on the source message; all must pass. |
| `mapping` | Destination | `null` | Mapping object below; `null` passes through and requires the source protocol. |
| `delivery` | Destination | Channel policy | Override individual delivery fields. |

| Protocol | Allowed endpoint kinds | Default / highest `ack_required` |
| --- | --- | --- |
| `astm`, `au` | `serial`, `tcp_client`, `tcp_listener` | `received` / `received` |
| `hl7_mllp` | `tcp_client`, `tcp_listener` | `accepted` / `accepted` |
| `delimited_file` | `folder`, `sftp` | `handed_off` / `handed_off` |

### Delivery policy fields

| Field | Default | Value or purpose |
| --- | --- | --- |
| `max_attempts` | `5` | Integer, 1–100 per attempt cycle; operator resend starts another cycle. |
| `retry` | Retry defaults above | Retry delays; bridge failure-alert threshold is ignored. |
| `send_timeout_seconds` | `30` | Integer, 1–3600 for the whole write or upload. |
| `ack_timeout_seconds` | `30` | Integer, 1–3600 after the last byte for the required acknowledgement. |
| `ack_required` | Per protocol above | `handed_off`, `received`, `accepted`; cannot exceed the protocol's highest level. |
| `retry_unacknowledged` | `false` | Boolean; enable only when the destination tolerates duplicate messages. |

## Protocol settings

Fields belong to source or destination connector `settings`.

### ASTM (`astm`)

| Field | Default | Value or purpose |
| --- | --- | --- |
| `charset` | `latin1` | `ascii`, `latin1`, `utf8`. |
| `role` | `computer` | `computer` yields to instrument contention; `instrument` has priority. |
| `ack_timeout_ms` | `15000` | Integer, 1000–120,000. |
| `receive_timeout_ms` | `30000` | Integer, 1000–300,000. |
| `max_frame_retries` | `6` | Integer, 0–20. |
| `max_frame_chars` | `240` | Integer, 64–240. |
| `contention_wait_ms` | `20000` | Integer, 1000–60,000. |
| `max_message_bytes` | `1048576` | Integer, 1024–67,108,864. |

### AU (`au`)

Use the instrument's online parameter sheet. All fields are required except
`block_timeout_ms`.

| Field | Default | Value or purpose |
| --- | --- | --- |
| `profile` | Required | `au400`, `au640`. |
| `framing.start`, `framing.end` | Required | Arrays of 1–2 control bytes, 1–31, excluding 6 and 21. |
| `framing.bcc` | Required | Boolean; XOR check byte. |
| `framing.use_etb` | Required | Boolean; only with `framing.end` set to `[3]`. |
| `framing.max_wire_len` | Required | `256`, `512`, `1024`. |
| `framing.class` | Required | `a` (no receive ACK/NAK) or `b` (ACK after durable commit). |
| `format.unit_no` | Required | Boolean; incoming two-digit unit number. |
| `format.rack_digits` | Required | `0`, `4`, `5`. |
| `format.sample_id_digits` | Required | Integer, 1–20. |
| `format.sex`, `format.age`, `format.month` | Required | Boolean patient-header options. |
| `format.patient_info_digits` | Required | Array of six integers, each 0–20. |
| `format.result_digits` | Required | `6`, `9`. |
| `frame_timeout_ms` | Required | Integer, 100–9900 in steps of 100. |
| `block_timeout_ms` | `9900` | Integer, 100–9900 in steps of 100; wait between blocks of an unfinished sample. |

### HL7 over MLLP (`hl7_mllp`)

| Field | Default | Value or purpose |
| --- | --- | --- |
| `ack_mode` | `original` | Source: `original`, `enhanced`. Destination also allows `none`, requiring `ack_required=handed_off`. |
| `send_application_ack` | `true` | Boolean; enhanced-mode source sends the requested application ACK after commit ACK. |
| `version` | `2.5.1` | `2.x` or `2.x.y`; generated MSH-12. |
| `sending_application`, `sending_facility`, `receiving_application`, `receiving_facility` | Empty string | Each at most 180 characters; generated MSH-3 through MSH-6. |
| `processing_id` | `P` | `P`, `D`, `T`. |
| `charset` | `utf8` | `utf8`, `latin1`, `ascii`. |
| `max_message_bytes` | `1048576` | Integer, 1024–67,108,864. |
| `frame_timeout_ms` | `30000` | Integer, 1000–300,000. |

### Delimited files (`delimited_file`)

| Field | Default | Value or purpose |
| --- | --- | --- |
| `delimiter` | `,` | One character: comma, semicolon, vertical bar or tab. |
| `quote` | Double quote | One character or `null` to disable quoting. |
| `header_row` | `true` | Boolean; read or write column headers. |
| `skip_lines` | `0` | Integer, 0–1000; leading lines ignored on read. |
| `charset` | `utf8` | `utf8`, `latin1`, `ascii`. |
| `line_ending` | `crlf` | Destination: `crlf`, `lf`. |
| `write_bom` | `false` | Boolean; destination only. |
| `columns` | Required | Non-empty array of columns below. |
| `record_mode` | `one_per_line` | `one_per_line`, `grouped`. |
| `group_by` | Required for `grouped` | Column name; consecutive equal values form one message. |
| `max_file_bytes` | `20971520` | Integer, 1024–1,073,741,824. Source: larger files are recorded as aborted, not read. Destination: a mapped file over the limit is not written (delivery failed, `transform_error`). |
| `file_name` | Required for destination | Template with no path separators. Variables: `{message_id}`, `{control_id}`, `{sample_id}`, `{date}`, `{time}`, `{seq}`; date is `yyyyMMdd`, time is `HHmmss`. |
| `on_exists` | `suffix` | Destination: `suffix`, `overwrite`, `append`, `fail`. |

| Column field | Default | Value or purpose |
| --- | --- | --- |
| `name` | Required | Unique name matching `^[a-z][a-z0-9_]{0,40}$`. |
| `header` | `name` | Case-insensitive source header match when `header_row=true`. |
| `position` | Next position | Integer at least 1; source without headers and destination order. |
| `type` | `string` | `string`, `integer`, `decimal`, `date`, `datetime`. |
| `format` | Required for date/datetime | Date/time format tokens listed below. |
| `required` | `false` | Boolean; source empty values make a message invalid. |

The names `control_id`, `sample_id`, `patient_id` also populate message identifiers.
`raw` has no settings and is carried only by bridges, never message connectors.

## Filters and mappings

This is a field lookup; use the [full transformation reference](../spec/configuration.md#8-filters-and-mappings)
for structure, escaping and error semantics. Occurrences and repetitions start at 0;
fields, components and subcomponents start at 1. AU fields are read-only.

| Protocol | Path form |
| --- | --- |
| HL7 | `SEG[occ].field[rep].component.subcomponent` |
| ASTM | `R[occ].field[rep].component` (record letters `H P O R C Q M L`) |
| AU | Header name or `test[occ].name` |
| Delimited file | `col[occ].name` |
| Current loop occurrence | `$.` plus field/component or field name; only inside `for_each`. |

### Expression fields

| Field or form | Default | Value or purpose |
| --- | --- | --- |
| `path`, `op` | Required for comparison | Protocol path and operator below. |
| `value` | Per operator | String or number for text comparison; numeric operators require an exact decimal. |
| `ignore_case` | `false` | Boolean; applies to text comparisons and regular expressions. |
| `all`, `any` | Alternative expression form | Non-empty array of expressions. |
| `not` | Alternative expression form | One expression. |

| Operator | Required value |
| --- | --- |
| `equals`, `not_equals`, `starts_with`, `contains` | Text. |
| `in`, `not_in` | Non-empty array of text. |
| `matches` | Valid regular expression, at most 200 characters; anchor for a whole-value match. |
| `exists`, `not_exists` | No value. |
| `gt`, `gte`, `lt`, `lte` | Exact decimal; no exponent or thousands separator. |

Use one comparison or one Boolean form per expression; nesting is limited to 32. Missing
fields read as empty text. Non-decimal field text fails every numeric comparison.

### Mapping fields and rule operations

| Field | Default | Value or purpose |
| --- | --- | --- |
| `tables` | `{}` | Named code tables, each mapping strings to strings. |
| `rules` | Required | Ordered rule array; output begins with generated protocol headers. |

| Rule `op` | Required fields | Optional fields and defaults |
| --- | --- | --- |
| `copy` | `from`, `to` | `default`; `required=false`. |
| `constant` | `value`, `to` | None. |
| `lookup` | `from`, `table`, `to` | `on_missing=error` (`error`, `keep`, `default`, `skip`); `default` required only for `on_missing=default`; `ignore_case=false`. |
| `concat` | `parts`, `to` | `parts`: non-empty array of objects, each containing exactly one `from` path or `text` string. |
| `datetime` | `from`, `to`, `input`, `output` | `from_timezone`, `to_timezone`: IANA names; conversion requires both. |
| `when` | `condition`, `then` | `else`; branches contain rule arrays. |
| `for_each` | `over`, `into`, `rules` | Record names identify input/output occurrences; loops cannot nest. |
| `sequence` | `to` | `start=1`, integer 0–999,999,999; only inside `for_each`. |

Date/time tokens are `yyyy`, `yy` (1970–2069), `MM`, `dd`, `HH`, `mm`, `ss`. Tokens are
fixed-width and may appear only once per format; quote literal letters with single quotes.
At least one token is required. See [date and time formats](../spec/configuration.md#date-and-time-formats).
