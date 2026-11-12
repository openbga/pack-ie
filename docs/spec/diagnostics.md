# Diagnostics specification

The data the engine publishes about itself: snapshots, alerts, log events and counters. One
snapshot and alert contract feeds the console and command line; tracing feeds the log files
and in-memory log ring (LOG-01..04, EPT-06, CON-02). Behavior that produces these values is in
[bridge-channels.md](bridge-channels.md) and [message-channels.md](message-channels.md);
thresholds are in [configuration.md](configuration.md#43-alerts-thresholds); presentation is in
[operator-console.md](operator-console.md).

## 1. Principles

- The console and CLI consume the engine's snapshots and alerts. Log files and the log ring
  receive the same filtered tracing events.
- Field names are `snake_case`. Times are ISO-8601 UTC strings (nanosecond precision) named
  `*_at`, `*_since` or `last_activity`; durations are integers with the unit in the name.
- Nullable fields are always present and `null` when they do not apply.
- Message bodies never appear ([section 8](#8-what-never-appears)).

## 2. Publication

| Rule | Statement |
| --- | --- |
| Loop | One publication loop owns publishing: every `snapshot_interval_seconds` (default 1 s) and, debounced by 100 ms, after a channel, session or endpoint state change, a channel start, stop or in-place update, `config.applied` and `config.rejected`. |
| Consistency | Each publication reads the channel and endpoint snapshots, evaluates alerts against that set and the node conditions, then derives the node snapshot from those same channel and endpoint snapshots. Store figures and disk space are read separately for alerts and the node snapshot. |
| Replacement | Each publication replaces the published set wholesale: a channel removed by configuration disappears at the next publication; a disabled channel is published `stopped` with `disabled_by_config`; an operator-paused channel stays visible as `stopped` with `operator_paused`. |
| Delivery | Subscribers receive the newest snapshot (latest wins); a slow reader gets the newest, not a backlog. |
| Cost | Publication reads the registry's current view without waiting for configuration or channel control operations to finish. |
| Shutdown | After every channel has stopped, the final stopped channel snapshots and the `stopped` node snapshot are published. |

## 3. Engine and node snapshot

The published object is `{ node, channels, endpoints, alerts }`: the node snapshot below, one
snapshot per configured channel (section 4) and endpoint (section 5) in configuration order,
and the active alerts, newest first ([section 6](#6-alert-catalog)).

| Field | Type | Notes |
| --- | --- | --- |
| `node_id`, `display_name` | string | From bootstrap. |
| `version` | string | Engine version. |
| `hosting_mode` | string | `console` (`packie run`) or `service`. |
| `captured_at`, `started_at` | string | |
| `uptime_seconds` | integer | |
| `node_state` | string | `healthy`, `degraded` or `stopped`. |
| `config` | object | `active_version` (integer or null), `pending_version` (integer or null, set during an apply), `last_apply` (null, or `at` and `result`: `applied`, `rejected` or `partial`). |
| `channel_counts` | object | Keys `starting`, `running`, `retrying`, `degraded`, `stopped`, all always present with 0 when empty. |
| `endpoint_counts` | object | Keys for each endpoint state (`disabled`, `connecting`, `listening`, `connected`, `retrying`, `disconnected`, `error`), all present. |
| `totals` | object | `bytes_in`, `bytes_out` (all endpoints since start), `messages_stored` (current store row count), `deliveries_unresolved` (rejected, failed, timed out, outcome unknown), `deliveries_queued` (the three store figures are null until the store reports), `alerts_active` (`warning`, `critical` counts). |
| `store` | object | `state` (`ok`, `unavailable`, `not_configured`), `size_bytes`, `free_disk_bytes`, `total_disk_bytes` (integers or null when unknown). |

### Node state rules

`node_state` is derived in one place from the channel snapshots, the store state and the
active configuration; the published value and the engine's internal value never disagree.

- `healthy`: the process runs, a runtime configuration is active, the store is not
  `unavailable`, free disk is not below `disk_critical_megabytes`, and every enabled channel
  is `running` or `starting` (during normal startup or an apply).
- `degraded`: no runtime configuration is active (zero-channel start after a rejected
  configuration), the store is `unavailable`, free disk is below `disk_critical_megabytes`,
  or any channel is `retrying`, `degraded` or unexpectedly `stopped` (`store_unavailable`,
  `not_implemented` or a missing stop reason).
- `stopped`: after shutdown completes and the final snapshot is published.
- Channels that are intentionally disabled or stopped by an operator, and instances stopped
  because a configuration change replaced them (`replaced_by_config`), do not degrade the node.
- A channel in `starting` does not degrade the node. `starting` is only the initial state of
  an instance; reconnect attempts after a failure report `retrying`, so a channel that is down
  always degrades the node.
- Audit availability and backup age raise alerts but do not independently change `node_state`.

## 4. Channel snapshots

### 4.1 Common fields

| Field | Type | Notes |
| --- | --- | --- |
| `channel_id`, `label` | string | |
| `kind` | string | `bridge` or `message`. |
| `enabled` | boolean | |
| `state` | string | `starting`, `running`, `retrying`, `degraded`, `stopped`. |
| `stop_reason` | string or null | When `stopped`: `operator_paused`, `disabled_by_config`, `removed_by_config`, `replaced_by_config`, `shutdown`, `not_implemented` (this build cannot run the channel) or `store_unavailable` (a message channel waiting for the message store to open). The last two are not intentional, so the node is `degraded`. |
| `state_since` | string | When `state` last changed. |
| `config_version` | integer | Version of the definition the running instance was started from. |
| `instance_started_at` | string or null | Start of the running instance. |
| `endpoints` | array | Ids of the endpoints the channel uses. |
| `traffic` | object | `bytes_in`, `bytes_out`, `messages_in`, `messages_out`, `errors` (integers) and `last_activity` (string or null). Bridges: in is A to B, out is B to A. |
| `retry_count` | integer | Bridge: section 7 of bridge-channels. Message: retry count of the source endpoint cycle. |
| `next_retry_at` | string or null | Set while a retry waits; null during an attempt and while running. |
| `last_error` | object or null | Operator-facing last error: `at`, `summary`, `next_action` (string or null); see [section 7.4](#74-error-summaries). |
| `recent_errors` | array | The last 100 errors in the same shape, oldest first. |
| `allowed_actions` | array | Operator commands whose preconditions hold now: `pause`, `resume`, `restart`. |
| `bridge`, `message` | object or null | Detail for the channel's kind (sections 4.2 and 4.3); the other is null. |

### 4.2 Bridge channel

The `bridge` object:

| Field | Type | Notes |
| --- | --- | --- |
| `session` | string | `waiting_for_peer`, `bridged`, `closing`, `closed`. |
| `session_since` | string | When `session` last changed. |
| `connect_strategy` | string | `parallel`, `wait_for_listener_then_pair`, `receiver_then_sender`. |
| `receiver` | string or null | `a`, `b` or null. |
| `endpoint_a`, `endpoint_b` | object | [Endpoint reference](#endpoint-reference). |
| `a_to_b`, `b_to_a` | object | One direction: `bytes` (accepted into the buffer), `messages` (tap count; null without a framer), `buffered_segments`, `buffered_bytes`, `max_segments`, `max_bytes`, `overflowing` (boolean), `overflow_count` (episodes since the instance started). |
| `tap` | object | `framer` (`none`, `astm`, `hl7_mllp`, `au`), `record_messages`, `framing_errors` and `tap_dropped` (integers; null without a framer), `transcript_viewers` (0 or 1). |

#### Endpoint reference

Embedded in channel snapshots; the full object is in [section 5](#5-endpoint-snapshots).

| Field | Type | Notes |
| --- | --- | --- |
| `endpoint_id`, `label` | string | |
| `kind` | string | `serial`, `tcp_client`, `tcp_listener`, `folder`, `sftp`. |
| `role` | string | `a`, `b`, `source` or a `destination_id`. |
| `state` | string | State of this channel's use of the endpoint. |
| `consecutive_failures` | integer | Failure streak. |
| `active_peer` | string or null | Peer address for listeners and connected clients. |
| `last_activity` | string or null | Last successful I/O of this side. |

### 4.3 Message channel

The `message` object:

| Field | Type | Notes |
| --- | --- | --- |
| `source` | object | Endpoint reference with role `source`. |
| `protocol` | string | Source protocol: `astm`, `au`, `hl7_mllp`, `delimited_file`, `raw`. |
| `session` | string | Source session: `idle`, `receiving`, `waiting_for_peer`, `closed`. |
| `counters` | object | Since the instance started: `received` (every message committed, including duplicates and invalid ones), `duplicates`, `invalid`, `filtered`, `aborted`, `bytes_in`. |
| `last_received_at` | string or null | Last committed message. |
| `destinations` | array | One [destination snapshot](#44-delivery-queue-destination-snapshot) each. |
| `unresolved` | object | Current store counts across destinations: `rejected`, `timed_out`, `outcome_unknown`, `failed`, and `transform_errors` (of `failed`, the deliveries whose mapping failed). |

State rules for a message channel:

| State | Condition |
| --- | --- |
| `starting` | Instance created, reconciliation done, source not yet ready for the first time. |
| `running` | The source is ready (`connected`, or `listening` for a listener) and every enabled destination endpoint is ready. |
| `retrying` | The source endpoint is not ready after a failure (connect, open, bind). |
| `degraded` | The source is ready but an enabled destination endpoint is not ready, or the store is unavailable. |
| `stopped` | As for bridges. |

Deliveries that need attention do not by themselves change the channel state; they are
counted in `unresolved`, raise `delivery_needs_attention` and rank the channel on the
dashboard.

### 4.4 Delivery queue (destination snapshot)

| Field | Type | Notes |
| --- | --- | --- |
| `destination_id`, `label` | string | |
| `enabled` | boolean | |
| `endpoint` | object | Endpoint reference with role `destination_id`. |
| `protocol` | string | As for the source protocol. |
| `queue.queued`, `queue.sending` | integer | Current store counts. |
| `queue.oldest_queued_at` | string or null | Receipt time of the oldest queued delivery. |
| `queue.next_attempt_at` | string or null | Earliest scheduled attempt. |
| `unresolved` | object | `rejected`, `timed_out`, `outcome_unknown`, `failed`, `transform_errors` (current store counts). |
| `counters` | object | Store-backed values: `attempts`, `acknowledged` and `bytes_out` are persistent totals; `rejected`, `timed_out`, `outcome_unknown` and `failed` are current unresolved counts. |
| `last_attempt_at`, `last_ack_at` | string or null | Store-backed last occurrences, retained across instance and engine restarts. |
| `last_error` | object or null | Persisted latest destination failure in the section 4.1 shape; `next_action` is null. Its `at` is the last attempt time, or snapshot time if no attempt time is stored. |

## 5. Endpoint snapshots

| Field | Type | Notes |
| --- | --- | --- |
| `endpoint_id`, `label` | string | |
| `kind` | string | As for the endpoint reference. |
| `enabled` | boolean | |
| `state` | string | `disabled`, `connecting`, `listening`, `connected`, `retrying`, `disconnected`, `error`. When several channel instances use one shareable endpoint, the least healthy of their states in the order `error`, `retrying`, `disconnected`, `connecting`, `listening`, `connected`. An enabled endpoint no channel runs is `disconnected`. |
| `state_since` | string | State timestamp of the channel use chosen for `state`; the registry's endpoint timestamp when no running channel uses it. |
| `peer` | string or null | Connected peer (address, port name or server) when known. |
| `channels` | array | Ids of channels referencing the endpoint. |
| `held_by` | string or null | First channel whose exclusive endpoint (serial port, listener socket) is connected or listening. |
| `traffic` | object | Since the engine started, all channels: `bytes_in`, `bytes_out`, `messages_in`, `messages_out` (message-channel connectors only), `errors`, and `last_activity` (last successful I/O; string or null). |
| `consecutive_failures` | integer | Failure streak. |
| `retry_count`, `next_retry_at` | integer, string or null | As seen by the holding channel, or the first running channel using the endpoint when there is no holder. |
| `last_error` | object or null | As in section 4.1. |
| `rejections` | object or null | `tcp_listener` endpoints only: `count` (refused since the listener was bound), `last_at`, `last_peer`, `last_reason` (`not_allowed` or `session_active`; null until the first refusal). |
| `settings_summary` | string | Non-secret one-line description (for example `COM3 9600 8N1`, `0.0.0.0:3001`). Never contains secrets. |

## 6. Alert catalog

An alert is one record per code and scope: while its condition holds, repeated evaluations
update that record instead of creating more. Alerts are derived from snapshots by rules,
raised with the thresholds of [configuration.md](configuration.md#43-alerts-thresholds), and
cleared automatically when the condition clears (invariant: cleared when the condition
clears).

| Field | Type | Notes |
| --- | --- | --- |
| `alert_id` | string | UUID, stable while the condition holds; a condition that returns after clearing is a new record. |
| `code` | string | Catalog code below. |
| `severity` | string | `warning` or `critical`. |
| `state` | string | `active` or `cleared`. |
| `channel_id`, `endpoint_id`, `destination_id` | string or null | Scope. |
| `summary` | string | One operator-facing sentence, no payload. |
| `next_action` | string | What to do, or what the engine is doing next. |
| `first_seen_at`, `last_seen_at` | string | First and most recent evaluations that found the condition. |
| `cleared_at` | string or null | Set when cleared. |
| `occurrences` | integer | Times the condition was raised since the engine started, this one included. |
| `details` | object | Numbers and codes only (counts, ages, peer address, state); the keys per code are in the table. |

Alert history is in memory: cleared alerts stay visible for 30 days (at most 1000), and a
restart starts a new history. Stopped or disabled channels are excluded from traffic-condition
rules (backlog, endpoint down, no peer, buffer pressure and invalid messages). Within an
eligible message channel, all configured destination snapshots are checked, including disabled
destinations. Endpoint peer rejections and node-reported configuration apply failures are
evaluated separately.

The engine supplies these `NodeConditions` alongside the snapshots; they are alert inputs,
not additional fields in the published object:

| Input | Meaning |
| --- | --- |
| `free_disk_bytes` | Free state-volume bytes, or null when unknown. |
| `store_unavailable` | Store opening is failing, or a message channel's most recent source commit failed. |
| `config_rejected` | The last apply or startup document failed validation. |
| `apply_failures` | Channel ids and reasons from the last accepted apply that failed for those channels. |
| `audit_unavailable` | The audit writer reports unavailable. |
| `backup_stale` | Null, or `newest_success_at` (nullable) and `limit_hours` when the backup age limit is exceeded. |
| `notifications_failing` | The notification targets whose latest send failed: `target`, `since` (first failure after the last success), newest `error`. |

| Code | Scope | Condition | Severity | Clears when | Next action (shown verbatim in the console and CLI) |
| --- | --- | --- | --- | --- | --- |
| `endpoint_down` | channel, endpoint | An endpoint a running channel needs is `retrying`, `error` or `disconnected` for `endpoint_down_seconds`, or its failure streak reaches the channel's `retry.max_consecutive_failures_before_alert`. `details`: `state`, `seconds`, `failures`. | `warning`; `critical` after `endpoint_down_critical_seconds`, or when the state is `error` | The endpoint is no longer `retrying`, `error` or `disconnected` | "Check that the device or remote system is on and reachable (cable or network). The engine keeps retrying; the alert clears once the connection is stable." For `error`: "Fix the cause in the last error (permissions, credentials or host key). The engine retries at its longest interval and recovers on its own once it is fixed." |
| `no_peer_connected` | channel | A bridge session or message source is `waiting_for_peer` and its age reaches `no_peer_seconds` (0 = never raised). Age starts at `bridge.session_since` for a bridge and `channel.state_since` for a message source. `details`: `seconds`. | `warning` | The source/session is no longer waiting | "Check that the instrument is on, cabled or networked, and set to connect to this channel's listener." |
| `peer_rejected` | endpoint | A listener rejected a peer (not allowlisted, or a second peer during a session). `details`: `count`, `peer` (IP without the connection's source port), `reason` (`not_allowed`, `session_active`). | `warning` | 10 minutes without a rejection | Not allowlisted: "If {peer} should connect, add it to the endpoint's allowed peers and apply. If not, find out which device it is." Second peer: "Only one peer may be connected at a time. Find out why {peer} connects while another peer is connected." |
| `bridge_buffer_pressure` | channel | A bridge buffer stays at or above `buffer_high_percent` of either limit (bytes or segments) for `buffer_high_seconds`, or an overflow episode is active. `details`: `a_to_b_bytes`, `b_to_a_bytes`, `overflowing`. | `warning`; `critical` during an overflow episode | No overflow episode and occupancy below the threshold | "The receiving side is slower than the sender, so the engine holds the sender back; no data is lost. Check that the receiving system is processing." |
| `message_store_unavailable` | node | The store cannot be opened or a source commit failed. | `critical` | The store opens and no message channel still reports a failed source commit; a channel clears its commit failure on its next successful commit | "Check free disk space and the permissions of the state directory. Until storage works, incoming messages are not acknowledged, so senders keep them and retry." |
| `disk_low` | node | Free space on the state volume below `disk_low_megabytes`. `details`: `free_bytes`. | `warning`; `critical` below `disk_critical_megabytes` | Free space recovers above the warning level | "Free disk space on the state volume or shorten retention. Messages with unresolved deliveries are never purged automatically." |
| `delivery_needs_attention` | channel, destination | At least one delivery is `rejected`, `failed`, `timed_out` or `outcome_unknown`. `details`: `count`, `rejected`, `failed`, `timed_out`, `outcome_unknown`. | `warning`; `critical` at `delivery_attention_critical_count` | No such delivery remains | "Open Messages filtered to this destination. Rejected or failed: fix the cause, then resend. Timed out or outcome unknown: first check whether the destination received the message, then resend or close with a reason." |
| `transform_failed` | channel, destination | An unresolved delivery is `failed` with reason `transform_error`, from initial mapping or preparation for a resend. | `warning` | No unresolved delivery with that reason remains | "Correct the mapping (each affected delivery names the failing rule and path), apply the configuration, then resend the affected messages." Details: `count`. |
| `delivery_backlog` | channel, destination | Queued deliveries reach `delivery_backlog_count`, or the oldest queued delivery is `delivery_backlog_age_seconds` old (each 0 = off). `details`: `queued`, `oldest_seconds`. | `warning`; `critical` at three times either threshold | Below both thresholds | "Check the destination endpoint and the receiving system. Messages are safe in the store and are sent in order once the destination accepts them again." |
| `invalid_messages` | channel | `invalid_messages_count` or more `invalid` or `aborted` messages within `invalid_messages_window_seconds` (count 0 = off), measured from sampled counter changes. | `warning` | Fewer in the window | "The sender produces messages the engine cannot accept. Check its protocol settings (framing, character set, version). The originals are kept in Messages with the reason." Details: `count` (window total), `window_seconds`, `invalid`, `aborted` (instance totals). |
| `config_rejected` | node | The last apply or startup configuration failed validation. | `warning` | The next accepted apply, including a partial apply | "Validate the document (Settings → Versions, or `packie config validate <file>`), fix the errors and apply again. The previous configuration stays active." |
| `config_apply_failed` | channel | One channel's change failed during an apply. | `warning` | The next apply in which the channel does not fail | "The channel's previous definition is still running. Fix the error in the summary and apply again." |
| `audit_unavailable` | node | The audit log cannot be written (opening it or the last write failed). Audited commands are refused while it holds; message traffic is not affected. | `critical` | An audit write succeeds (the engine retries every minute) | "Check free disk space and the permissions of the audit directory (<state>/audit). Until the audit log can be written, commands that are audited (configuration changes, channel commands, resends, content views) are refused; message traffic continues." |
| `backup_stale` | node | `backup.stale_after_hours` is set and the newest successful backup is older than that, or none succeeded that long after the engine started. `details`: `newest_success_at`, `limit_hours`. | `warning` | A backup succeeds | "Check the backup directory (free space, permissions, mounted) and the last backup error (Settings → Backup or `packie backup status`), then run `packie backup create`. The alert clears after the next successful backup." |
| `notifications_failing` | node | The latest send to a notification target (section 6.1) failed. One alert names every failing target with its newest error in `summary`. `details`: `targets`, `failing_since`. | `warning` | Every target's latest send, a test message included, succeeded | "Check the notification settings (host, port, security, credentials secret, webhook address) and that this computer can reach the target, then send a test message (`packie notify test`). The alert clears after the next successful delivery to every target." |

### 6.1 Notifications

When the bootstrap's `notifications` section ([configuration.md](configuration.md#3-bootstrapjson))
configures an e-mail relay or a webhook, the engine sends a message each time an alert is
raised, changes severity or clears. Each target has its own `severities`; an alert whose
severity no target lists sends nothing. Sending happens in the background and never delays
the engine.

| Rule | Behavior |
| --- | --- |
| Rate limit | One message per alert (code, channel, endpoint and destination) per `notifications.rate_limit_minutes`, shared by all targets. A raise and the clear that follows count against the same window, so a condition that clears within the window sends no clear message; events inside the window are logged at debug as `notification.suppressed`. An event that no target wants does not start the window. |
| Content | Only the node's display name, alert code, severity, state (`raised` or `cleared`), channel, endpoint and destination ids, the channel's and endpoint's labels from the active configuration, the alert's `summary` and `next_action`, `first_seen_at`, `last_seen_at` and `occurrences`. Never `details`, and never anything from messages. `next_action` of `peer_rejected` names the peer's IP address. |
| E-mail | Plain text. Subject `[PACK-IE <node>] <SEVERITY> <code> raised` or `cleared`, then `: <channel, else endpoint, else destination id>` (the scope is left out for node alerts). |
| Webhook | `POST` with `Content-Type: application/json` and, when configured, `Authorization: Bearer <secret>`. Body keys: `node`, `code`, `severity`, `state`, `channel_id`, `channel_label`, `endpoint_id`, `endpoint_label`, `destination_id` (null when not in scope), `summary`, `next_action`, `first_seen_at`, `last_seen_at`, `occurrences`. A 2xx answer is success; anything else, including a redirect, is a failure. |
| Test | A test message (code `test`, severity `info`, state `test`) goes to every target on request (`test_notification`, `packie notify test`, audited as `notification_test`); the answer names each target and whether it accepted the message or why not. |
| Failures | A failed send is logged as `notification.failed` (`target`, `error_summary`; no content, no secret values) and is not retried. Failure texts for a peer's answer are fixed categories with only the numeric status: SMTP `SMTP relay refused the message (550)`, `refused the credentials (535)` or `deferred the message (451)`; webhook `the receiver answered HTTP 500`; connection, timeout, TLS and unreadable-reply failures are fixed sentences. The relay's or receiver's own text never appears, in the log, test results or the alert. The text does not repeat the target name (the alert shows `target: error`). A target is failing from its first failed send until a later send to it, including a test message, succeeds; while any target is failing the `notifications_failing` alert is raised naming each failing target and its newest error. A send that succeeds logs `notification.sent` at debug. |
| Private CA | `notifications.ca_file` (PEM bundle of private certificate authorities) is read once when the notifier starts. If it cannot be read, is not valid PEM or holds no usable certificate, one `notification.ca_invalid` warning is logged (`ca_file`, `error_summary`; never certificate contents) and every send to a target that uses TLS fails with `CA file <path> <reason>` until the file is fixed and the engine restarted; targets without TLS (`security: none`, `http://` to localhost) are unaffected. The failure appears in test results and in the `notifications_failing` alert like any other. |

Notifications are off without targets. Secrets (SMTP password, bearer token) are read from
the secret store when a message is sent; a missing secret fails that send, naming the
secret.

## 7. Logs

### 7.1 Files, ring and access

- Structured JSON lines, one event per line, written by `tracing` to
  `<logs>/packie.YYYY-MM-DD.log` (then `.1.log`, `.2.log` … within a day), rotated at local
  midnight and before a file would grow past `rotate_at_megabytes` (a single oversized line is
  still written), appended to across restarts, owner-only. Files of days older than
  `logging.retention_days` are deleted when a new day starts and at open (LOG-01).
- The same events enter an in-memory ring (`logging.ring_buffer_events`) served live through
  the management interface. A ring event has `seq` (increases by one per event), `at`,
  `level`, `target`, `message`, `event`, the lifted ids `channel_id`, `endpoint_id` and
  `message_id`, and every other field as text in `fields`. Filters: minimum level, time range
  (`since`, `until`), `channel_id`, `endpoint_id`, `event`, `after_seq`/`before_seq` for
  paging, `limit` (default 500, the newest), and text contained (case-insensitive) in the
  message, target or any field name or value. Results contain the newest matching events in
  oldest-first order; `since` is inclusive, `until` exclusive, and sequence bounds exclusive.
  `packie logs` and the console query and follow the ring only; it holds the most recent events
  since the engine started. Older history is in the files, which
  are read with ordinary tools; there is no query over the files (LOG-04).
- Work for a channel runs inside a `channel` span carrying `channel_id` and `channel_kind`.
  File lines list the enclosing spans in `spans`; the ring lifts their ids into the event, so
  events inherit the channel. Endpoint log entries carry `endpoint_id`, `endpoint_role` and
  `endpoint_kind`, and most start their text with
  `Channel {channel_id} {role} endpoint {endpoint_id}:`.
- Every state transition is logged exactly once per transition.

### 7.2 Fields

A file line is flat: `timestamp`, `level` (upper case: `ERROR`, `WARN`, `INFO`, `DEBUG`,
`TRACE`), `target`, `message`, the event's own fields and `spans`. The ring's `level` is
lower case. Not every event carries every field below.

| Field | Type | Present |
| --- | --- | --- |
| `event` | string | Catalog type from section 7.3 when the emitter sets it; store lines named only by their message text are listed below. |
| `message` | string | always; human text without payload |
| `node_id` | string | `node.started` |
| `channel_id`, `channel_kind` | string | when the event concerns a channel (from the span or the event) |
| `endpoint_id`, `endpoint_role`, `endpoint_kind` | string | when it concerns an endpoint (`endpoint_role`: `a`, `b`, `source` or a destination id) |
| `destination_id` | string | message channel destination |
| `message_id`, `delivery_id` | string | UUIDs; never protocol identifiers |
| `attempt_no` | integer | `delivery.attempt_started` |
| `direction` | string | `a_to_b`, `b_to_a` (buffer and tap events) |
| `channel_state`, `session_state`, `stop_reason` | string | state changes |
| `endpoint_state` | string | `endpoint.state_changed` of a bridge or source endpoint |
| `retry_count` | integer | retry events |
| `next_retry_at` | string | scheduled retries |
| `bytes`, `segment_count` | integer | traffic and buffer events |
| `ack_code`, `ack_level` | string | delivery acknowledgement events |
| `peer` | string | peer address (listener and endpoint events) |
| `config_version` | integer | config and channel start events |
| `error_code`, `error_summary` | string | failures; summary follows [section 7.4](#74-error-summaries) |

### 7.3 Event types

Levels are as emitted; a row with two levels names both.

| Event | Level | When | Extra fields |
| --- | --- | --- | --- |
| `node.started` | info | Engine start | `node_id`, `config_version`, `ipc` |
| `node.stopping`, `node.stopped` | info | Shutdown begins, shutdown completes | `config_version` (stopped) |
| `node.start_failed` | error | The engine could not start | `error` |
| `test_hooks.enabled`, `test_hook` | warn | Test hooks are enabled; a hook was invoked | `hook` |
| `config.applied` | info | Apply result | `config_version`, `result`, `added`, `restarted`, `updated`, `removed`, `stopped` |
| `config.rejected` | warn | An apply was rejected; the runtime file was invalid at startup | `errors` (count); `file`, `error` at startup |
| `config.imported`, `config.empty` | info, warn | Startup used the runtime file as a new version; there is no configuration | `file`, `config_version` |
| `config.version_skipped`, `config.prune_failed`, `config.save_failed` | warn, warn, error | A saved version was unusable; an old version could not be removed; a version could not be saved | `config_version` or `path`, `error` |
| `config.replacement_not_ready` | warn | A replacement instance did not pair in time and takes over anyway | `channel_id` |
| `secrets.unavailable` | warn | Secret names could not be listed to check references | `error` |
| `channel.started` | info | Instance created | `config_version` |
| `channel.stopped` | info | Instance stopped | `stop_reason` |
| `channel.state_changed` | info | Any channel state change | `channel_state`, `session_state` |
| `channel.updated` | info | A configuration change applied in place | |
| `channel.failed` | error | A channel runner crashed; it restarts | `error_summary` |
| `channel.compile_failed` | error | Filters or mappings did not compile; the previous form stays | `problems` |
| `bridge.session_opened`, `bridge.session_closed` | info | Session established, ended | `session_state`; `peer_a`, `peer_b` on open |
| `bridge.close_propagated` | info | A side's end closed the other | `endpoint_role`, `error_summary` |
| `endpoint.connected` | info | Endpoint ready | `peer` |
| `endpoint.retry_scheduled` | warn; error once the streak reaches `max_consecutive_failures_before_alert` (bridges) | Retry scheduled | `retry_count`, `next_retry_at`, `error_summary` |
| `endpoint.retry_succeeded` | info | Failure streak cleared | |
| `endpoint.write_failed` | warn | A write failed | `error_summary` |
| `endpoint.state_changed` | info | Any endpoint state change (a destination connection ending logs it with `destination_id`) | `endpoint_state` |
| `endpoint.keepalive_untuned` | warn | TCP keepalive tuning unsupported; OS defaults used | `error` |
| `listener.peer_rejected` | warn | Peer refused | `peer`, `error_code` (`not_allowed`, `session_active`) |
| `listener.accept_failed` | warn | Accept failed; the listener retries | `error` |
| `folder.watch_unavailable` | warn | No change notifications; polling | `error` |
| `sftp.file_written`, `sftp.file_read`, `sftp.file_consumed` | debug | An SFTP file operation succeeded (never the file name) | `bytes`, `on_exists`, `too_large`, `action` |
| `sftp.directory_skipped` | warn | A subdirectory could not be listed | `cause` |
| `buffer.overflow_detected`, `buffer.overflow_cleared` | warn, info | Overflow episode start, end | `direction`, `bytes`, `segment_count` |
| `tap.dropped` | warn | A tap lost observations (the first drop of an instance and every 1000th; the store falling behind logs each) | `direction` |
| `source.response_stalled` | warn | A message-channel source peer did not read a response (acknowledgement) in time; the connection was closed | `endpoint_id`, `endpoint_role` (`source`), `error_summary` |
| `source.framing_error` | warn | Bytes outside a valid frame were discarded | |
| `source.frame_abandoned` | warn | An HL7 source connection ended inside a frame (peer closed or reset, or `frame_timeout_ms`); the partial frame was discarded | `error_summary` |
| `source.unsupported`, `destination.unsupported` | error | The configured protocol cannot run here | `destination_id` |
| `message.frame_committed` | debug | A frame was committed | `message_id`, `byte_length`, `complete` |
| `message.committed` | debug | A message was committed; the source worker also reports its acknowledgement code (`none` for files) | `message_id`, `bytes`, `ack_code` (source worker); `sequence` (store) |
| `message.duplicate` | info | Duplicate recorded | `message_id` |
| `message.identity_reused` | info | Same identity, different content | `message_id` |
| `message.invalid` | warn | Invalid message | `message_id`, `error_code` |
| `message.aborted` | warn (source), info (store) | Aborted transfer | `message_id`, `error_code` (source), `reason_code` (store) |
| `delivery.attempt_started` | info | Attempt recorded | `delivery_id`, `attempt_no` |
| `delivery.attempt_refused` | warn | The store refused to start an attempt | `delivery_id` |
| `delivery.ack_progress` | info | A lower acknowledgement level arrived during an attempt | `delivery_id`, `ack_level` |
| `delivery.state_changed` | info; warn for `rejected`, `failed`, `timed_out`, `outcome_unknown` | Delivery state change, except `sending` | `delivery_id`, `ack_code`, `ack_level` |
| `delivery.transform_failed` | warn | Mapping error at delivery creation or before a resend attempt | `message_id`, `destination_id`, `error_summary` (routing); `delivery_id`, `state` (store) |
| `delivery.ack_unmatched` | warn | An acknowledgement matched nothing in flight | `destination_id` |
| `delivery.resend_requested` | info | Operator resend | `message_id`, `mode`, `accepted` |
| `delivery.closed`, `delivery.configuration_removed` | info | An operator closed a delivery; deliveries were cancelled because the destination left the configuration | `delivery_id`; `cancelled` |
| `file.written` | debug | A file destination wrote a delivery (the file name is never logged: it may carry identifiers) | `destination_id`, `delivery_id`, `bytes`, `suffixed` |
| `file.consume_failed` | warn | A committed source file could not be moved or deleted (named by its first message, never by file name) | `message_id`, `messages`, `operation` |
| `retention.purged` | info | A purge removed messages | `channel_id`, counts (`messages`, `deliveries`, `attempts`, `bodies`) |
| `reconciliation.completed` | info | Startup reconciliation | `recovered`, `aborted`, `requeued`, `unknown` |
| `store.error` | error; warn while open retries continue | A store operation failed | `error_code`: `open_failed`, `commit_failed`, `write_failed`, `queue_read_failed`, `finish_failed`, `pipeline_commit_failed`, `reconciliation_failed`, `retention_failed`, `cancel_failed`, `tap_record_failed`; `next_attempt_seconds` on open retries |
| `store.opened` | info | The store opened after failing at start; message channels start | |
| `store.checkpoint_failed` | warn | A background WAL checkpoint failed; the writer checkpoints itself as the WAL grows | |
| `audit.anchor` | info | The audit chain head was recorded by maintenance when new events exist and the anchor interval is due, or at shutdown when events are pending ([audit.md](audit.md#5-tamper-evidence)) | `seq`, `hash`, `reason` (`daily`, `shutdown`) |
| `audit.unavailable`, `audit.available` | error, info | The audit log cannot be written (audited commands are refused); it can again | `error` or `error_summary` (unavailable); none (available) |
| `audit.sealed`, `audit.pruned`, `audit.seal_deferred`, `audit.prune_failed` | info, info, warn, warn | A year's audit file was sealed; a sealed file past retention was removed; sealing waits for the next event; retention could not run | `file`, `last_seq`; `error` |
| `audit.write_failed` | error | An audit record could not be written; a command's requested record failure refuses the command, while an outcome-record failure is logged after the action | `error_summary`, `action` (requested record) |
| `audit.checkpoint_failed`, `audit.anchor_failed` | warn | A buffered audit checkpoint failed and remains pending; the closing audit anchor could not be written | `error` |
| `audit.maintenance_unavailable` | error | The audit maintenance thread could not start | `error` |
| `backup.created` | info | A backup completed | `directory`, `files`, `bytes` |
| `backup.failed`, `backup.cleanup_failed` | error, warn | A backup failed and cleanup was attempted; cleanup could not remove its partial output | `directory`, `error` |
| `backup.restored` | info | A backup was restored | `backup`, `pre_restore` |
| `notification.sent`, `notification.suppressed` | debug | A notification was delivered; one was skipped inside the rate-limit window | `target` (sent), `alert_code`, `channel`, `endpoint` |
| `notification.failed` | warn | A notification could not be delivered | `target`, `alert_code`, `channel`, `endpoint`, `error_summary` |
| `notification.secret_unresolved` | warn | At start, a notification secret is not in the secret store | `field`, `secret` |
| `notification.ca_invalid` | warn | At start, `notifications.ca_file` cannot be read or holds no usable certificate; TLS sends fail naming the file | `ca_file`, `error_summary` |
| `backup.pruned` | info | An old scheduled backup was removed | `directory` |
| `backup.status_unwritten` | warn | The attempt status could not be saved; this does not fail the backup | `error` |
| `backup.undo_failed` | error | A failed restore could not remove partial state or put previous state back | `path` (directory operations), `error` |
| `routing.paused_unreadable`, `routing.paused_not_saved` | warn | The paused-channel list could not be read or saved | `path`, `error` |
| `ipc.group_unavailable` | warn | The socket could not be given to `ipc.group` and stays owner-only | `group`, `error_summary` |
| `ipc.connected` | info | A management connection was accepted; its OS account is read from the connection (Unix peer credentials, the Windows pipe client's token) | `operator_account`, `uid_or_sid`, `pid` |
| `ipc.refused` | warn | A management connection was closed because its OS account could not be read | `error_summary` |
| `ipc.hello`, `ipc.client_disconnected`, `ipc.read_failed`, `ipc.write_failed`, `ipc.pipe_busy` | debug | Management-interface client lifecycle and I/O | `client`, `operator_account`, `origin` (hello), `error` |
| `host.eventlog_failed`, `host.service_failed`, `host.engine_failed`, `host.status_failed` | warn, error, error, warn | Windows service hosting failures | `error_summary` |
| `alert.raised`, `alert.cleared` | warn, info | Alert change | `alert_id`, `code`, `severity` (raised), `channel_id`, `endpoint_id` |
| `operator.command` | info | An operator command was answered | `command` (`apply_config`, `validate_config`, `pause_channel`, `resume_channel`, `restart_channel`, `resend`, `close_delivery`, `set_secret`, `delete_secret`), `result` (`accepted`, `rejected`, `failed`), `operator_account` and `uid_or_sid` (the OS account behind the connection), `origin` (the client kind named in its hello: `console`, `cli`, `sim`; `unknown` without one; a label, not an identity); free-text reasons are logged by length only (`reason_length`) |

The message store logs its own lines for `message.frame_committed`, `message.committed`,
`message.identity_reused`, `message.aborted` and the `delivery.*` events `attempt_started`,
`ack_progress`, `closed`, `configuration_removed`, `transform_failed`, `resend_requested` and
`state_changed` with the event name as the message text and no `event` field, so the ring's
`event` filter does not match those lines. Routing emits separate lines with an `event` field
for `message.committed`, `message.identity_reused`, `message.aborted`,
`delivery.transform_failed` and `delivery.state_changed`.

### 7.4 Error summaries

One pattern for every operator-facing endpoint error (`last_error.summary`, `error_summary`):

```text
Channel {channel_id} {endpoint_role} endpoint "{endpoint_label}" {operation} failed: {cause}. {next action}.
```

`endpoint_role` is `A`, `B`, `source` or `destination {destination_id}`. Operations: `connect`,
`listen`, `accept`, `open`, `read`, `write`, `send` (a destination delivery). A clean close by
the peer is a `read` that failed with "peer closed the connection". The next action is added
when the retry is scheduled, so it names the actual delay. A crashed channel runner reports
`Channel {channel_id} failed unexpectedly: internal error.` and the restart delay. Examples:

```text
Channel chem-results destination lis endpoint "LIS HL7 interface" send failed: connection reset by peer. Retrying in 5s.
Channel chem-results source endpoint "Chemistry analyzer" read failed: peer closed the connection. Retrying in 10s.
```

Causes name a condition or a field, never a field value.

## 8. What never appears

| Item | Logs, ring, snapshots, alerts, errors | Allowed only in |
| --- | --- | --- |
| Message bodies (original, transformed, file contents) | never | The Messages detail query and the traffic transcript, both served to a local client that asked, over the local management interface |
| Parsed field values and protocol identifiers (patient, sample, order, control ids) | never | The Messages page (store) |
| File names of messages | never (the channel and `message_id` identify them) | The Messages detail |
| Operator free text (`closed_reason`) | never (only its length is logged) | The store, Messages detail |
| Secrets, key material | never | The secret provider |
| Raw bytes in any error text | never | none |

Accordingly: log, snapshot and alert code takes only counts, codes, state, endpoint
addresses, ids generated by the engine and byte lengths. A scenario can plant a sentinel
string in a message and assert that it appears in no log, snapshot or alert
([scenarios.md](scenarios.md#9-global-assertions)). The traffic transcript is bounded, opt-in,
local and never persisted ([operator-console.md](operator-console.md#7-live-traffic-transcript)).

## 9. Counters

| Kind | Rule |
| --- | --- |
| Channel traffic counters (`bytes_*`, `messages_*`, `received`, `duplicates`) | Monotonic since the channel instance started; they restart at 0 with a new instance. |
| Destination `counters` | `attempts`, `acknowledged` and `bytes_out` persist in the store; the other fields are current unresolved counts, as in section 4.4. |
| Queue and unresolved counts | Current values from the store; they survive restarts. |
| Gauges (`buffered_*`, `retry_count`, `consecutive_failures`) | Current values. |
| Timestamps (`last_*_at`) | Last occurrence; channel timestamps start empty with an instance, destination timestamps are store-backed. |
| Endpoint counters | Since the engine started, summed over channels. |
| Accuracy | A byte counter counts a segment only after the channel accepted it; a message counter counts after commit. |

Counters required per channel and endpoint are those in sections 4 and 5; there is no
separate metrics endpoint (no network port is opened for management).

## 10. Setup checks

Read-only checks for setting up endpoints, over IPC and the CLI
([operator-console.md](operator-console.md#5-operator-commands)). They never send data to an
instrument or a destination system.

| Check | Behavior |
| --- | --- |
| `list_serial_ports` | The ports the operating system reports, sorted by name. Linux lists from `/sys/class/tty` metadata (ttys with a backing device and a `/dev` node, RFCOMM names, USB ids read from the parent USB device, legacy 8250 slots without a UART skipped by their sysfs `type`); macOS and Windows use the `serialport` enumeration. Fields: `name`, `kind` (`usb`, `pci`, `bluetooth`, `unknown`), `vid`, `pid`, `serial_number`, `manufacturer`, `product` where known. `used_by` lists the active configuration's serial endpoints naming the port (compared without case on Windows), with the channels that use each and `held_by`, the channel holding it open now. Listing opens no port. |
| `test_endpoint` | Takes `kind` and `settings` written as in the runtime document (secrets by reference) and an overall limit (`timeout_seconds` 1–60, default 10). The definition is checked by the document's endpoint rules first; problems come back as `invalid_config` with paths relative to the endpoint. Then each step runs within what is left of the limit; a step after a failed one is omitted. Audited as `endpoint_test`. |
| `test_notification` | Section 6.1. |

| Kind | Steps (nothing is sent or left behind) |
| --- | --- |
| `serial` | Open the port with the configured baud rate, framing and flow control, then close it (a `virtual_link` is checked instead). A port this engine holds, or keeps open between attempts, is reported as in use without opening it. |
| `tcp_client` | Resolve the host, connect within the limit and report the connection time; close without sending. |
| `tcp_listener` | Bind the configured address and port, then release them. A port a running listener holds fails as in use. |
| `folder` | Find the folder, read it and count the files matching the pattern, then create and delete one hidden empty file to prove it is writable (sources move or delete consumed files, destinations write). Counts only, never file names. |
| `sftp` | Look up the referenced credentials, resolve, connect and verify the host key, authenticate, open SFTP and list the remote directory (a count), then disconnect. Writes nothing. |

Each step reports `name`, `ok`, `detail` (what was found or why it failed: counts and
causes, never file names or secret values) and `millis`; `summary` is one sentence for the
operator.

Sending a test message to a destination (an HL7 ACK probe, an ASTM ENQ, raw bytes) is not
provided: whether a live laboratory system may receive test traffic is the laboratory's
decision, made outside the engine. Use the simulator or the destination's own test
environment.
