# Operator console specification

The UI-independent rules for how operators see and control the engine: emphasis, severity,
required information, error wording, commands and the live traffic transcript. They apply to
the console and, where a command or listing exists, to the CLI. Layout, components and
navigation patterns belong to the console implementation; the pages are those of
[architecture §8](../architecture.md#8-console-information-architecture). Data comes only
from snapshots, queries and subscriptions defined in [diagnostics.md](diagnostics.md) and
[message-channels.md](message-channels.md). Requirements: CON-01..08, BRG-05, MSG-03..06,
LOG-03, LOG-04, EPT-06.

| Page | Rules in |
| --- | --- |
| Dashboard | sections 2, 3 |
| Channels (list, detail) | sections 2, 4, 5, 7 |
| Endpoints (list, detail) | sections 2, 4, 5, 7 |
| Messages | section 6 (with message-channels.md) |
| Transmission report | section 6 |
| Logs | section 8 |
| Alerts | section 9 |
| Audit | section 11 |
| Settings, configuration editors | sections 10, 11 |
| Command line | section 12 |

## 1. Principles

- The first screen answers three questions: which channels are healthy, which are failing,
  retrying or waiting for a peer, and what the engine is doing about it. A failing channel is
  obvious within seconds of opening the app (CON-02).
- The console is a client. It never talks to a transport and never shows data that is not in
  a snapshot, a store query or an explicitly opened transcript.
- Every page has loading, empty and error states. Every error names the channel or endpoint,
  the operation and the next action; "failed" alone is never shown (CON-07).
- Destructive or session-ending actions ask for confirmation and say what will happen.
  Every command reports its outcome in words (section 5).
- Color is never the only signal: each state has a text label and a distinct shape.
- Nothing is sent off the computer (SEC-04).

## 2. Severity and color

Console tones are `neutral`, `info`, `warning`, `danger` and `success`. Routine running,
connected, listening and acknowledged states use `neutral`; color emphasizes conditions needing attention.

### Channel state

| State | Token | Shape and label |
| --- | --- | --- |
| `running` | `neutral` | filled circle, "Running" |
| `starting` | `info` | spinner, "Starting" |
| `retrying` | `warning` | circular arrow, "Retrying" with next retry time |
| `degraded` | `warning` | triangle, "Degraded" |
| `stopped`, `operator_paused` | `neutral` | pause bars, "Paused by operator" |
| `stopped`, `disabled_by_config` | `neutral` | dash, "Disabled" |
| `stopped`, other reasons | `neutral` | square, "Stopped" with the reason |

An intentionally stopped channel is never styled as a failure. A channel with an active
`critical` alert shows `danger` emphasis on its row regardless of its state token.

### Endpoint state

| State | Token | Shape and label |
| --- | --- | --- |
| `connected` | `neutral` | check, "Connected" |
| `listening` | `neutral` | circle with a dot, "Listening" (with "waiting for peer" when no peer) |
| `connecting` | `info` | spinner, "Connecting" |
| `retrying` | `warning` | circular arrow, "Retrying" |
| `disconnected` | `neutral` | hollow circle, "Disconnected" |
| `error` | `danger` | cross, "Error" |
| `disabled` | `neutral` | dash, "Disabled" |

Under `receiver_then_sender` the endpoint indicators carry a direction arrow: inbound on the
receiver side, outbound on the sender side; a disconnected side shows a cross with its arrow.

### Delivery state

| State | Token | Label |
| --- | --- | --- |
| `acknowledged` | `neutral` | "Acknowledged" plus the level (section 6) |
| `queued`, `sending` | `info` | "Queued", "Sending" |
| `rejected` | `danger` | "Rejected by destination" |
| `failed` | `danger` | "Failed, not delivered" |
| `timed_out` | `warning` | "No acknowledgement in time, may have been delivered" |
| `outcome_unknown` | `warning` | "Outcome unknown, may have been delivered" |
| `cancelled` | `neutral` | "Closed" with the reason |

### Alert severity

`warning` is `warning`, `critical` is `danger`; both carry a text label.

## 3. Dashboard

Shows every configured channel, health first. Selection stays on the same channel across
refreshes while it exists. Data refreshes from the snapshot stream (1 s by default); only
changed rows update, and input stays responsive.

### Header strip

Engine state (`node_state`), version, uptime, active configuration version (and the pending
one during an apply), counts of channels by state, active alerts by severity, unresolved
deliveries, store and free-disk state, and the connection status to the engine
([section 11](#11-settings-help-and-engine-connection)). The console's top bar also shows
"Signed in as `<account>`" when the engine's hello supplies the OS account, so operators see
whom the audit log and the change history will name.

### Sorting and emphasis

Rows sort by rank, then by the tie-break rules:

| Rank | Channels |
| --- | --- |
| 1 | With an active `critical` alert |
| 2 | `retrying` |
| 3 | `degraded` |
| 4 | `running` with a `warning` alert or unresolved deliveries |
| 5 | `running` with traffic in the last 60 s |
| 6 | `running`, idle |
| 7 | `starting` |
| 8 | `stopped` (paused, disabled) |

Tie-breaks inside a rank: channels with an error in the last 5 minutes above quiet ones, then
the most recent error first, then label A to Z. A retrying channel never sorts below a
degraded one; a paused channel never sorts above an active one.

### Columns

| Column | Content | Always shown |
| --- | --- | --- |
| Channel | Label, kind (bridge or message) | yes |
| State | State chip (section 2) with stop reason | yes |
| Endpoints | Bridge: indicators for A and B with labels. Message: source indicator, destination indicators | yes |
| Traffic | Bridge: bytes A→B and B→A. Message: received count, acknowledged count | no |
| Last activity | Per side (bridge) or last received / last acknowledged (message), as "8 s ago" | no |
| Queue | Bridge: highest buffer fill in percent, overflow marker. Message: queued, unresolved | no |
| Session | Bridge session state | no |
| Retry | Retry count and "in 5 s" | no |
| Last error | Concise summary | no |

On narrow windows columns drop right to left in this order: Retry, Last error, Session,
Last activity, Traffic, Queue; Channel, State and Endpoints always remain. A row opens its
channel page.

An endpoints list shows the same state chips with kind, address summary, bytes in and out,
last activity, failure streak and last error (EPT-06).

## 4. Channel and endpoint pages

### Bridge channel page

Required: label and id; channel state, session state and stop reason; endpoints A and B each
with state, label, kind, active peer, failure streak, last activity and bytes; bytes in each
direction; framed message counts when a tap is configured; buffer segments, bytes and limits
per direction with overflow count; retry count and next retry time; last error summary with
full structured context; active and pending configuration version; recent state changes and
recent errors; active alerts; the live transcript pane ([section 7](#7-live-traffic-transcript));
commands (section 5).

### Message channel page

Required: label and id; channel state and stop reason; the source connector (endpoint
state, protocol, session, last received, counters: received, duplicates, invalid, filtered,
aborted); every destination (endpoint state, protocol, queued, sending, unresolved by
state, oldest queued age, next attempt, last acknowledgement, last error); recent messages
and a link to Messages filtered to this channel and to its unresolved deliveries; recent
errors; active alerts; commands.

### Endpoint page

Required: label, id, kind, settings summary (never secrets), state and state-since time,
channels using it and which one holds it, bytes in and out, messages in and out where
applicable, last activity, failure streak, next retry, last error, alerts. Commands: restart
the channels that use it (a shortcut for `restart_channel` on each).

### Error presentation

| Where | Rule |
| --- | --- |
| Lists | One concise summary (the `last_error_summary` text, truncated with the full text available). |
| Detail | Full structured context: operation, endpoint role and label, cause, failure streak, time, retry state, next action. |
| Non-running channels | Retry timing, "waiting for peer" and the stop reason are visible without opening anything else. |
| Paused | Reads as intentionally paused, with who/what: "Paused by operator". No error styling. |
| Wording | The summary format of [diagnostics.md](diagnostics.md#74-error-summaries). Never only "failed"; never raw bytes. |
| Unknown outcome | Always says "may have been delivered" and which acknowledgement was or was not received. |

## 5. Operator commands

IPC requests return a typed response on success or an error with an operator-facing message;
precondition failures carry a stable reason code where defined. A resend can succeed for
some destinations and reject others in the same response. Configuration, channel, resend,
close and secret mutations also emit `operator.command` with an `accepted`, `rejected` or
`failed` outcome. Audited requests are refused before execution when the audit request
cannot be recorded ([audit.md](audit.md)).

The operation names below are those used by operators and command logs; the IPC request is
shown where its name differs. Confirmations describe the console; the CLI uses explicit
flags for content export, duplicate risk, superseded messages and restore.

| Operation | Scope | Preconditions | Console confirmation | Outcome |
| --- | --- | --- | --- | --- |
| `pause_channel` (`channel_command: pause`) | channel | Exists; enabled; an instance is running and its state is not `stopped` | Yes: explains connection closure/refusal and that accepted messages stay queued | `stopped` / `operator_paused`; "Channel paused. No retry is scheduled." |
| `resume_channel` (`channel_command: resume`) | channel | Exists; enabled; `stopped` with `operator_paused`; engine not shutting down | No | Starts a new instance from the active configuration |
| `restart_channel` (`channel_command: restart`) | channel | Exists; enabled; not operator-paused; runner available; engine not shutting down | Yes: the session closes and counters of the instance reset | New instance from the active definition |
| `resend` | message, destinations, mode, optional reason | Readable store; message belongs to a configured message channel; destinations and states satisfy [message-channels.md §9](message-channels.md#9-resend-and-close); optional reason is 1–500 trimmed characters; a superseded message needs `confirm_superseded` | The dialog explains both modes; duplicate risk requires a checkbox | Per-destination result list; `new_message_id` when a new message was created |
| `close_delivery` | delivery, reason | Readable store; delivery exists in `queued`, `rejected`, `failed`, `timed_out` or `outcome_unknown`; reason is 1–500 trimmed characters | Yes: "Closing does not contact the destination" | Delivery `cancelled` with the reason |
| `export_messages` | filter, format, optional original bodies | Readable store; valid message filter | CLI/IPC operation | Export chunks and matching count (section 6) |
| `verify_transmission` | channel id, period | Readable store; valid period; the channel id filters stored messages and need not remain configured | CLI/IPC operation | Per-destination report and up to 10,000 delivery rows |
| `create_backup` | directory, or configured backup directory | New or empty absolute directory, or `backup.directory` set; the engine's backup service available | Yes for Back up now; uses `backup.directory` | Directory, files, size and audit head |
| `validate_config` | document | None beyond a writable audit log over IPC; CLI validation is offline | No | Problems and the per-channel plan (`added`, `changed`, `updated`, `removed`, `unchanged`, `stopped` for channels the document disables); the plan is always present and empty when the document has errors; nothing changed |
| `apply_config` (`put_config`) | document, `base_version`, note | Valid document; base equals active version; note is 1–500 trimmed characters | Yes: shows changed and untouched channels | New active version and per-channel outcomes, including partial failures; validation/conflict/save failures leave the previous version active |
| `verify_config_version` | stored version, note | Version exists; note is 1–500 trimmed characters | No | Stored version with its sign-offs; who verified it and when |
| Rollback (`put_config`) | stored version, active base version | Selected version exists; normal apply preconditions | Yes: as apply | New version with note `rollback of <N>` |
| `set_secret`, `delete_secret` | secret name and, for set, value | Valid secret name; secret provider available | Delete confirms affected references; set is write-only | No value returned; changes are used on the next connection |
| `get_audit_events` | filter, cursor, limit | Audit log available; limit 1–1000 | No | Newest-first page and next cursor |
| `export_audit`, `verify_audit` | filter/format, or full chain | Audit log available and able to record the request | Export dialog explains its contents; verification runs immediately | Export chunks, or chain verification report |
| `list_serial_ports` | — | None | No | Ports with `used_by` (endpoint, its channels, `held_by`) |
| `test_endpoint` | `kind`, `settings`, optional `timeout_seconds` | A definition that passes the document's endpoint rules; a writable audit log | No (read-only; nothing is sent) | Probe report: `ok`, `steps` (`name`, `ok`, `detail`, `millis`), `summary` |
| `test_notification` | — | A writable audit log | Yes: says a message goes to every configured target | One result per target (`target`, `ok`, `detail`); empty when none is configured |
| Transcript (`subscribe`, topic `traffic`) | channel id | Local client; no other viewer of this channel; a writable audit log (`traffic_view`) | No | Section 7 |
| `record_client_export` | `kind` (`change_report`), `rows` | A writable audit log; the client writes the file only after `done` | No (the export dialog warns that notes and references may hold identifiers) | Audited `client_export`; nothing else changes |

Rejection reason codes (stable; used by the CLI and scenarios):

| Code | Meaning |
| --- | --- |
| `not_found` | The channel, message, delivery or version does not exist. |
| `already_stopped` | Pause of a channel that is already stopped. |
| `not_paused` | Resume of a channel that is not operator-paused. |
| `disabled_in_config` | The channel is disabled in the active configuration. |
| `operator_paused` | Restart of an operator-paused channel (use resume). |
| `delivery_in_progress` | `same_message` resend of a `queued` or `sending` delivery, or close of a `sending` one. |
| `invalid_state` | The delivery state does not allow the command. |
| `duplicate_risk_unconfirmed` | The resend needs `confirm_duplicate_risk`. |
| `destination_not_found` | The destination is not in the active configuration or is disabled. |
| `reason_required` | Missing or invalid required close reason/configuration note, or an invalid optional resend reason (1–500 trimmed characters). |
| `version_conflict` | `base_version` is not the active version. |
| `invalid_config` | The document has validation errors. |
| `transcript_in_use` | Another client already views the transcript. |
| `superseded` | A newer revision supersedes the message; resending it needs `confirm_superseded`. |

Rules:

- A rejected operation or destination is not executed; its response explains the state
  ("Channel is already paused"), never a generic error. A resend can still queue other
  destinations whose preconditions pass.
- Commands from the console and the CLI are the only way an operator stops a channel; the
  paused flag survives engine restart.
- No command exists to stop or restart the engine from the console; the service manager
  does that. The console only offers to start an installed service when no engine runs.

## 6. Messages

A data grid over the store (MSG-05, CON-04). Paging is server-side by offset and limit
(100 rows by default; the console offers 50, 100 and 200) with the total count of matching
messages. Filters and sorting are applied by the engine; the IPC limit is 1–1000.

### Default columns

Received (time), Channel, Direction, Message state, Delivery summary (up to two destination
chips with state and label, plus a count and tooltip for the rest), Message type, Identifier
(control id, else sample id),
Patient id, Size. Optional columns: message id, source peer, configuration version,
`resent_from`. Default sort: received time, newest first. The default time range is the last
24 hours.

### Filters

Time range; channel (multiple); direction; message state; delivery state; identifier
(control, sample or patient id, exact or prefix); text (substring of the control, sample or
patient id or the message type, minimum 3 trimmed characters, narrowed by the selected time
range; message bodies are never searched or indexed);
message id prefix (8–32 hex digits, dashes optional). Quick filters with counts: Needs attention, Queued, Duplicates, Invalid or aborted. Filter state is kept while
navigating and cleared by one control. Identifier, text and message id filters are held in
memory for this console session, never in its URL or persistent storage.

### Detail

Shown: ids, state, times, source peer, configuration version; the response sent to the
sender (code, time); the original body (as text with control characters made visible, with a
hex toggle; HL7 and ASTM one segment or record per line); the parsed fields (HL7 and ASTM
only, after an explicit **Show content** that is audited: `get_message_fields` returns
non-empty leaves of the original, at most 500, by the
paths filters and mappings use, `MSH.9.1`, `PID.3[1].1`, `R.3.4`; when none can be listed
the reason is given instead); for each
destination: the delivery state, acknowledgement level and code, the last error (`last_error`:
the attempt's error summary, or the failing mapping rule and path), the attempts with times and
results, and the transformed body; links to `duplicate_of`, `resent_from` and `revision_of`.
Opening the detail shows facts, states and deliveries only. A body or the parsed fields are
fetched only when the operator asks, served to this local client only, and each view is
recorded in the audit log with the operator's OS account (as are identifier, text and message
id prefix searches).

### Acknowledgement wording

| `ack_level` | Label |
| --- | --- |
| `handed_off` | "Written to the destination. Not confirmed by it." |
| `received` | "Received by the destination. Processing not confirmed." |
| `accepted` | "Accepted by the destination's application." |

A transport acknowledgement is never displayed as success of processing. The words
"delivered" and "success" are not used for any level below `accepted`, and no label implies
that a person has read a result.

### Resend flow

Row action and detail action "Resend…": destinations (default: those needing attention),
mode with a one-sentence explanation each (`same_message`: "Try again with the same output";
`new_message`: "Process the original again with the current configuration"), optional reason,
and the duplicate-risk checkbox when required. Invalid, aborted, filtered and duplicate
messages, and messages without deliveries, offer `new_message` only. The result lists each
destination and what happened. A superseded message is refused by the engine with the newer
message's id and time; overriding that refusal uses the CLI's `--superseded-ok` or IPC's
`confirm_superseded`.

### Export

`packie messages export` and `export_messages` write the messages a filter selects, oldest
first, as CSV (header row; formula-like values prefixed with `'`) or JSON Lines: id, received
time, channel, direction, state, protocol, type, control, sample and patient id, size and
each delivery's destination, state and acknowledgement level. CSV deliveries use
`destination:state:ack_level`, separated by semicolons. Original bodies are included only
when requested, as UTF-8 text with invalid sequences replaced; an original that cannot be
read in full stops the export with an error naming the message. The first chunk is audited
as `message_export` with the filter, content choice and matching count, and returns an
`export_id` that every later chunk must carry ([audit.md §3](audit.md#3-actions)). IPC chunks
contain up to 1000 messages without bodies, 100 with bodies. The console's Messages grid provides
browsing and resend, and its **Export…** action saves the messages the current filter selects
(CSV or JSON Lines, original content only when ticked) through the same IPC operation as the CLI.

### Transmission verification

`packie messages verify` and `verify_transmission` report, for messages received in a channel
and period, what each destination confirmed: deliveries acknowledged at `accepted`,
`received` and `handed_off`, unresolved, closed and pending. Delivery rows include what
produced them, as recorded with the message and delivery (the source endpoint, the source
peer, the configuration version active at receipt and the `mapping_version` whose mapping
produced the bytes sent), the control id, original SHA-256, stored transformed-body SHA-256 when present, acknowledgement
code, the acknowledgement's own control id (`ack_control_id`, HL7 `MSH-10` of the ACK), total
attempts, first-attempt time and acknowledgement time. A missing sent hash does
not prove that nothing was sent. The report is audited as `transmission_verify`; rows are
capped at 10,000 and `truncated` means a shorter period is needed; the report also carries
`rows_limit` (10,000) and `period_rows_total` (the deliveries a complete report would list),
so the console and the JSON output state how complete the rows are. The message count and the
destination totals always cover the whole period (the totals come from one aggregate over
the store), so only the rows stop early. A truncated report's CSV ends with a line starting
`# truncated:`, and the CLI also warns on standard error. Use the CLI's CSV output for the
laboratory's records. A channel that is neither configured nor has stored messages is
`not_found`; a removed channel's stored history stays reportable. The console shows this report
on its **Transmission report** page and exports the rows shown as CSV; the CLI and IPC provide
the same report.

## 7. Live traffic transcript

For troubleshooting a channel (BRG-05, LOG-02). It is separate from snapshots, metrics, logs
and alerts.

| Rule | Statement |
| --- | --- |
| Opt-in | Capture starts when a local client subscribes for a channel and stops when it unsubscribes or disconnects. With no viewer, forwarding pays one lookup per segment and copies no payload. |
| One viewer | One subscriber per channel; a second request is rejected ("Transcript in use by another client"). |
| Audited | Opening a transcript is a content view, audited as `traffic_view` with the channel and the subscriber's OS account before it opens; while the audit log cannot record it, the subscription is refused (`unavailable`). |
| Bounded | A ring per direction of at most 1000 segments or 1 MiB, whichever comes first, oldest dropped. |
| Never persisted | The ring lives in memory. When the last viewer leaves it is dropped; a channel removed by configuration loses its transcript. The console does not write transcripts to disk by itself; copying is the operator's action. |
| Local | Served only over the local management interface to the subscribing client; never sent to other clients, logs, metrics or alerts. |
| Timestamps | Every captured segment has the time it was captured at the bridge boundary (when the channel accepted it). |
| Panes | Bridge: two panes, `A → B` and `B → A`. Message channel: `Source` (received and engine responses, marked by direction) and `Destinations` (sent and received, tagged by destination). |
| Rendering | Text with control characters made visible, and a hex toggle. |
| Freeze | Freezing stops updating the display; capture continues; unfreezing shows the newest segments. |
| Survives | A transcript stays open across reconnects of the channel's endpoints. |

## 8. Logs

Structured viewer over the engine's in-memory log ring ([diagnostics.md](diagnostics.md#71-files-ring-and-access)):
the most recent `logging.ring_buffer_events` events since the engine started. Older history
is in the log files under the log directory, which the viewer does not read.
Filters: level (at or above), time range, channel, endpoint, event, text. Follows live by
default; scrolling up pauses following and shows "Jump to live". A row expands to its fields;
channel and endpoint fields link to their pages. The viewer keeps a bounded window (10,000
rows) and queries older ring entries with `before_seq`; reconnect backfill uses
`after_seq` (decision 20). Message bodies are never in logs, so none
are shown (CON-05).

## 9. Alerts

Lists active alerts first (critical, then warning, then first seen), then recently cleared
ones. Each row shows severity label, summary, next action, scope links, first and last seen
and occurrences. Alerts cannot be dismissed: they clear when the condition clears
([diagnostics.md](diagnostics.md#6-alert-catalog)). The header strip and the channel rows
show the active counts.

## 10. Configuration

| Rule | Statement |
| --- | --- |
| Editors | Endpoint editor by kind; channel editor by kind (bridge: endpoints A and B, strategy, receiver, buffers, retry, tap; message: source, filters, destinations with protocol settings, mappings and delivery policy, retention). The mapping editor offers every rule including `calculate` and `append`, optional text steps on `from`, and the shipped presets as starting points. |
| Validation | The document is validated by the engine (`validate_config`) before apply; problems are shown at their field with the `code` and message. Apply is disabled while errors exist. |
| Plan | Before apply the editor shows which channels restart, update in place, start, stop or stay untouched. |
| Versions | A version history (number, time, applied by, endpoint and channel counts, per-channel outcomes, verification, note) with a read-only view of each version, a comparison with the active one, and rollback (saved as a new version with the note `rollback of <N>`). |
| Conflicts | If the active version changed since the editor loaded, apply is rejected with "Configuration changed elsewhere; reload". |
| Deleting | Deleting an endpoint or channel confirms and explains: unresolved deliveries of a removed destination or channel are closed. |
| Secrets | Secret fields are write-only (stored in the secret provider); the editor shows only whether a value exists. |
| Unsaved | Leaving an editor with unsaved changes asks first. |

## 11. Settings, help and engine connection

- **Settings** shows: engine version and node identity, state and log directories, store
  size and free disk, logging settings (read-only; bootstrap edits need a restart and are
  made in the file), editable runtime alert thresholds and per-message-channel retention,
  secrets management, configuration versions, About and Help. Its tabs are **Engine**,
  **Retention**, **Secrets**, **Versions**, **Backup**, **Notifications**, **Privacy**, **About** and **Help**:
  - **Backup**: the schedule, the last attempt and last success, staleness, and "Back up
    now" (`create_backup`, into a new directory under `backup.directory`, after confirmation).
    Restore instructions use the CLI with the engine stopped.
  - **Notifications**: the configured e-mail and webhook targets, read-only (a webhook shows
    its origin only), with their severities and timeouts, and "Send test" (`test_notification`)
    after confirmation. Changing targets needs the bootstrap file and a restart; a failing target
    raises `notifications_failing`.
  - **Privacy**: idle protection, a setting of this console on this computer (not of the
    engine): after 1–240 minutes without activity (default 15) bodies, parsed fields and
    audit details are hidden behind "Show again". There is no login; it does not lock the
    computer.
- **Audit**: the audit log ([audit.md](audit.md)) as a grid, newest first, filtered by time,
  account, action, origin, outcome and object; a row opens the event's details; export (CSV,
  JSON Lines) and chain verification. Account (name or uid/SID) and object id filters match
  exactly, including case. Older events load by a sequence cursor; reading events is not
  audited, while exporting and verifying are. Event details can include entered notes and
  searched identifiers and are concealed after the idle timeout.
- **Help** is available on every page in a short panel; its text is maintained with the page
  and updated whenever the behavior it describes changes (CON-08).
- **Engine connection**: statuses `connected`, `reconnecting` (last known state shown dimmed)
  and `disconnected`. If no engine is running, the console says so and offers to start the
  installed service, or explains how when none is installed (CON-01). Reconnection retries
  start at 500 ms and double up to 5 s, with ±20% jitter. Closing the console never stops the engine.

## 12. Command line

`packie` hosts the engine and is a client of the same local management interface. Requests use
the OS account of the CLI process. These options apply at every command level:

| Option | Meaning |
| --- | --- |
| `--bootstrap <file>` | Engine bootstrap file; `PACKIE_BOOTSTRAP`; default `config/bootstrap.json` in the current directory. Hosting and offline state operations use this file. |
| `--ipc <socket-or-pipe>` | IPC endpoint override; `PACKIE_IPC`. Overrides bootstrap endpoint discovery for client connections; does not redirect hosting or offline file operations. Backup create/status still load the bootstrap to locate the state and determine whether an engine holds it. |
| `-h`, `--help` | Show help; `packie help [command…]` also shows command help. |
| `-V`, `--version` | Show the binary version; propagated to subcommands. |

Exit codes: 0 success (including a configuration applied with partial channel failures);
1 a command failed or was rejected; 2 invalid command-line usage, `packie run` with an invalid
bootstrap, a backup create without either a directory or `backup.directory`, or restore
without `--yes`. A stopped or uninstalled service is a successful `service status` result.

| Command | Shows or does |
| --- | --- |
| `packie status [--json]` | Node, state, engine version/uptime/configuration, store and alert lines, then channel and endpoint tables. Channel traffic is bridge bytes each way, or message received/acknowledged/unresolved/queued counts, plus invalid and duplicate counts when nonzero. `--json` is the complete snapshot. |
| `packie channels pause\|resume\|restart <channel-id>` | Section 5 channel commands. Rejections print the engine's message and a next step where available, exiting 1. Success explains what happened; `packie status` shows the current state. |
| `packie messages list [message-filters] [--oldest-first] [--offset N] [--limit N] [--json]` | One page, newest first by default: received time, channel, state, type, identifier, per-delivery state roll-up and full message id; then the position and next offset. Offset defaults to 0; limit defaults to 20, range 1–1000. No default time filter. `--json` is the page with total count. |
| `packie messages show <id> [--body original\|<destination>] [--as text\|hex] [--fields] [--json]` | Facts, deliveries, acknowledgement levels, failure causes and attempt history. `--body` explicitly fetches a body; `--as` requires it and defaults to `text` (visible control characters). `--fields` separately fetches the original's parsed fields (HL7/ASTM), at most 500, or a reason none can be listed. Body and fields can be requested together. JSON contains `body`/`fields` only when requested. Both content requests are audited. |
| `packie messages resend <id> [--destination <id>…] [--new-message] [--confirm-duplicate-risk] [--superseded-ok] [--reason "…"] [--json]` | Section 5 resend. With no destination flag, targets every enabled destination in the current channel definition; repeat the flag or comma-separate ids to select several. Defaults to `same_message`; `--new-message` reprocesses the original with current configuration. Risk confirmation is needed for acknowledged, timed-out or unknown outcomes in either mode. `--superseded-ok` permits an older revision. An optional 1–500 trimmed-character reason is stored with the new attempt cycle. Prints each destination's result and exits 1 if any was rejected. |
| `packie messages close <delivery-id> --reason "…" [--json]` | Section 5 close: the full delivery UUID and a 1–500 trimmed-character reason. Does not contact the destination. JSON contains the closed delivery. |
| `packie messages export [message-filters] [--format csv\|jsonl] [--bodies] [-o\|--output <file>]` | Section 6 export, oldest first; format defaults to CSV. Uses the message filters below, without list pagination or sort flags. Writes to standard output or a newly created file (existing files refused); `--bodies` explicitly includes original content. Prints count on standard error for stdout data, or a confirmation on stdout after writing a file. |
| `packie messages verify --channel <id> --since <time> [--until <time>] [--csv\|--json] [-o\|--output <file>]` | Section 6 transmission report, with `until` defaulting to now. Default output is a per-destination summary; `--csv` prints delivery rows, `--json` the whole report. Those format flags conflict. Output files must be new; `--output` implies CSV unless `--json` is present. Check `truncated` in JSON or the truncation warning in the text summary; a truncated CSV ends with a `# truncated:` line and the CLI warns on standard error. A channel that is neither configured nor has stored messages is `not_found` (exit 1); a removed channel's stored history stays reportable. |
| `packie secrets list [--json]` | Sorted stored names, never values; JSON is `{ "names": […] }`. Requires the running engine's secret provider. |
| `packie secrets set <name>` | Name matches `^[a-z0-9][a-z0-9_.-]{0,62}$`. Prompts without echo on a terminal; otherwise reads all UTF-8 standard input, removing one final LF or CRLF. Empty values are refused. Values are never arguments or printed. Referencing endpoints use the new value on their next connection. |
| `packie secrets delete <name>` | Removes the named secret through the engine; endpoints that still reference it cannot connect until it is set again. A name that is not stored is `not_found` ("No secret … is stored; nothing was removed", exit 1). |
| `packie logs [-f\|--follow] [log-filters] [--limit N] [--json]` | The newest matching ring events, printed oldest first; limit defaults to 100, minimum 1. `--follow` continues until Ctrl-C, applies the same filters to new events, and conflicts with `--until`. Reports skipped events when the output falls behind. JSON is one event per line. |
| `packie audit list [audit-filters] [--limit N] [--before-seq N] [--json]` | Newest first: sequence, time, account, origin, action, object, outcome, then next `--before-seq`. Limit defaults to 50, range 1–1000. JSON contains the page and next cursor. |
| `packie audit export [audit-filters] [--format csv\|jsonl] [--output <file>]` | Matching events oldest first; CSV by default. Writes to stdout or a new owner-only file; an existing file is refused. Removes an incomplete output file on failure. Audited. |
| `packie audit verify [--dir <directory>] [--json]` | Walks the running engine's chain (audited), or verifies an audit directory directly with `--dir` (no engine connection). Use a backup copy or a stopped engine's audit directory. Prints the head hash to compare with the last `audit.anchor` log line. Exits 1 on verification problems, including with JSON. |
| `packie backup create [<dir>] [--json]` | Creates a backup in a new or empty directory; CLI relative paths resolve to absolute. Without a directory, creates `packie-backup-<time>` under `backup.directory`. Uses the engine when its instance lock is held, otherwise works offline holding that lock. Both paths audit the action. Text output includes directory, file count/size, message count, configuration version, audit head, time and duration. |
| `packie backup verify <dir> [--json]` | Offline: re-hashes listed files, checks the store copy and audit chain. Prints all problems and exits 1 on verification failure, including with JSON. |
| `packie backup restore <dir> --yes [--json]` | Offline, engine stopped: verifies, then restores the store, audit and configuration, moving replaced state to `pre-restore-<time>/`. `--yes` is mandatory. Reports the preserved state directory and restore audit event. Secret values are not restored; enter them again when moving to a new computer. |
| `packie backup status [--json]` | Directory, schedule/next run, stale limit, last attempt/success and staleness; uses IPC while the engine holds the instance lock, otherwise reads state offline. Schedules run only while the engine runs. |
| `packie config show` (alias `packie config export`) | Active runtime document as JSON, including `config_version`; secrets appear as references by name, never values. Redirect it to a file to export the configuration; importing is `packie config apply <file>` (validate, save, apply). |
| `packie config validate <file>` | Offline parse and semantic validation: counts when valid, or field problems and exit 1. Does not contact the engine or produce an apply plan. |
| `packie config apply <file> --note "…" [--force]` | Validates, saves and applies through `put_config`. Required note is 1–500 trimmed characters. Uses the document's positive `config_version` as base; if absent/zero or with `--force`, reads the active version first. A stale base is rejected as `version_conflict`; even `--force` can conflict with a concurrent apply. Prints new version, changed-channel outcomes and warnings; partial channel failure still exits 0. |
| `packie config history` | Saved versions with time, OS account, endpoint/channel counts, apply outcomes, latest verification's time/account (or "not verified since change") and note. Rollback is a console action using normal apply, with no separate CLI subcommand. |
| `packie config verify <version> --note "…"` | Signs off an existing saved version with OS account, time and a required 1–500 trimmed-character reference. Prints confirmation; `config history` shows the latest sign-off. |
| `packie config diff <a> <b> [--json]` | What changes from `a` to `b`; each side is a saved version number, `active`, or a runtime document file (files are validated first; versions need the engine). Endpoints and channels are matched by id and listed as added, removed or changed, then other sections; each changed field prints as `Field › Subfield: old → new`, with array members matched by their id (`destination_id`) or position. Defaults count as written, and `schema_version`/`config_version` are ignored. Equal documents print "configure the same". Not audited (read-only). |
| `packie endpoints list-serial [--json]` | `list_serial_ports`: the serial ports the OS reports, with bus kind, USB vendor/product ids and device text where known, and the configured endpoints that name each port (`(open by <channel>)` when a running channel holds it). Opens nothing. |
| `packie endpoints test <endpoint-id>\|--kind <kind> --settings '<json>' [--timeout <s>] [--json]` | `test_endpoint` ([diagnostics.md §10](diagnostics.md#10-setup-checks)): probes a configured endpoint's active settings, or an unsaved definition, without sending data. Prints each step with result, time and detail, then a summary; exits 1 when a step failed. An invalid definition prints its field problems (paths relative to the endpoint, such as `settings.port`) and exits 1. Timeout 1–60 s, default 10. Audited as `endpoint_test`. |
| `packie notify test [--json]` | `test_notification`: sends one test message to each configured notification target and prints each result; exits 1 when any target failed. With no target it says so and exits 0. Audited as `notification_test`. |
| `packie run [--service]` | Foreground engine until Ctrl-C or SIGTERM (Ctrl-Break on Windows), holding the state instance lock. `--service`, passed by the service manager, writes logs only to the log directory. |
| `packie service install\|uninstall\|start\|stop\|status [--user]` | Controls the OS service directly, without IPC. Install also starts it; uninstall preserves state/logs. System installation/removal needs sudo or an administrator terminal. On macOS `--user` selects a login agent without sudo; without it, scope follows elevation. `--user` on other platforms is rejected (exit 1). Status shows installation/running state and available pid, start time, bootstrap, IPC and definition path. |

The filter groups above contain these flags:

| Group | Flags and matching |
| --- | --- |
| Message filters | `--channel <id>`, `--state <state>`, `--delivery-state <state>` and `--direction inbound\|a_to_b\|b_to_a` accept repeated or comma-separated values (any value within a criterion, all criteria together). `--attention` selects rejected, failed, timed-out or unknown-outcome deliveries. `--since <time>` is inclusive, `--until <time>` exclusive; together, until must be later. `--identifier <value>` matches control/sample/patient id exactly; `--prefix` requires it and matches beginnings. `--text <value>` matches a substring of the control, sample or patient id or the message type, never message bodies (at least 3 trimmed characters). `--id <prefix>` matches 8–32 hex digits of message id, any case, dashes optional. |
| Log filters | `--level trace\|debug\|info\|warn\|error` (minimum, case-insensitive), `--channel <id>`, `--endpoint <id>`, `--text <value>` (case-insensitive substring of message, target or fields), `--event <name>` (exact catalog event), `--since <time>` (inclusive), `--until <time>` (exclusive). |
| Audit filters | `--since <time>` (inclusive), `--until <time>` (exclusive), `--actor <name-or-uid-or-SID>` (exact), `--action <action>`, `--origin console\|cli\|engine\|other`, `--outcome requested\|succeeded\|rejected\|failed`, `--object-type <type>`, `--object <id>` (exact). Action and object-type values are the [audit catalog](audit.md#3-actions); an invalid value prints the accepted names. |

Message arguments to `show` and `resend` accept a full UUID or its first 8–32 hex digits
(dashes optional, any case). An ambiguous prefix lists candidates and fails; supply more
digits. The first 12 digits encode receipt time, so nearby receipts often need more digits.
Channel, endpoint and destination ids are operator-chosen names; delivery ids are full UUIDs.

Times print in UTC (`2026-11-06 12:00:00Z`) and accept RFC 3339 or a non-negative integer age
with `s`, `m`, `h` or `d`, such as `90s`, `15m`, `2h`, `7d`. State and audit enum values use
their wire names (`timed_out`); `--help` lists command usage. Content is explicitly requested
with `messages show --body`, `--fields` or `messages export --bodies`; backups contain stored
content too. JSON is available only on commands whose syntax above includes `--json`;
exports use their format flags, and `config show` always emits JSON.
