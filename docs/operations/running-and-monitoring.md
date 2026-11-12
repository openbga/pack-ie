# Running and monitoring

The PACK-IE engine runs on the lab computer. The console and command line connect to its
local management interface. Closing the console leaves the engine and its channels
running.

Use the [laboratory compliance guide](compliance.md) for validation evidence, retention,
incident records and downtime/restart procedures.

## Check the engine

Open **Dashboard** first. The header shows engine state, version, uptime, active
configuration version, a pending version during apply, channel counts, open alerts,
deliveries needing attention, message-store state and free disk space.

| Engine state | Meaning |
| --- | --- |
| Healthy | The engine has an active configuration and usable store, disk is above the critical limit, and enabled channels are running or starting. |
| Degraded | A channel needs recovery, the store is unavailable, disk is critically low, or no valid configuration is active. Open the alerts and affected channel. |
| Stopped | Shutdown has completed. |

Paused and disabled channels do not by themselves make the engine degraded. A healthy
engine can still have deliveries needing an operator's decision.

The connection indicator describes the console's connection to the engine. During
reconnection, dimmed values are the last known state. If the engine is not running,
the console offers to start the installed service or explains how to start it. For the
packaged system service, use the platform commands in [Install](install.md#service-commands);
the button does not request administrator privileges and on macOS targets a user agent.

| Task | Command |
| --- | --- |
| Check the engine, channels, endpoints and active alerts | `packie status` |
| Read the complete snapshot in a script | `packie status --json` |
| Check or start the installed service | Use the elevated platform `service status` / `service start` commands in [Install](install.md#service-commands). |
| Run in the foreground for troubleshooting | `packie run` |

For foreground operation, Ctrl-C stops the engine (SIGTERM on Unix and Ctrl-Break on Windows do the same, for a process manager). Service operation continues without
an open terminal. Service installation and startup settings are covered in
[installation](install.md).

CLI commands use `config/bootstrap.json` by default. Set `PACKIE_BOOTSTRAP`, or pass an
explicit path, for example `packie --bootstrap /path/to/bootstrap.json status`.
`packie --ipc <socket-or-pipe> status` connects directly to a management socket or pipe;
`PACKIE_IPC` sets the same override.

## Read the Dashboard

The channel table puts trouble first. Within each rank, an error in the last five
minutes comes first, followed by the newest error, then the channel label A to Z.

| Rank | Channels |
| --- | --- |
| 1 | Any channel with an open critical alert |
| 2 | Retrying |
| 3 | Degraded, or stopped because of an error or invalid configuration |
| 4 | Running with a warning alert or deliveries needing attention |
| 5 | Running with traffic in the last 60 seconds |
| 6 | Running and idle |
| 7 | Starting |
| 8 | Other stopped channels, including paused and disabled |

A critical alert adds red emphasis and a **Critical alert** label to the row. While
the pointer is over the table or a row has keyboard focus, rows hold their positions;
their values still update. Open a row for the channel's detail page.

**Endpoints** shows A and B for a bridge, or the source and destinations for a message
channel. **Queue** shows the fullest bridge buffer and any overflow, or queued and
unresolved deliveries. On narrow windows, Retry, Last error, Session, Last activity,
Traffic and then Queue disappear; Channel, State and Endpoints remain.

State labels and shapes remain meaningful without color. Routine states are neutral;
blue means work in progress, amber means a warning, and red means a failure.

| Channel label | Shape | Color | Meaning |
| --- | --- | --- | --- |
| Running | Filled circle | Neutral | The channel is working. Check delivery counts separately. |
| Starting | Spinner | Blue | A new instance is becoming ready. |
| Retrying | Circular arrow | Amber | Recovery is scheduled or in progress. Read the retry countdown and last error. |
| Degraded | Triangle | Amber | The channel is running with an impairment, such as buffer pressure or a destination that is down. |
| Paused by operator | Pause bars | Neutral | Intentionally stopped; no retry is scheduled. |
| Disabled | Dash | Neutral | Disabled in the active configuration. |
| Stopped: configuration invalid / Stopped: error | Square | Red | Read the cause and correct it before restarting. |
| Other Stopped labels | Square | Neutral | The reason names a replacement, shutdown or other stop. |

| Endpoint label | Shape | Color | Meaning |
| --- | --- | --- | --- |
| Connected | Check | Neutral | Ready for traffic. |
| Listening | Dotted circle | Neutral | The listener is ready; waiting for a peer is normal. |
| Connecting | Spinner | Blue | Connection or opening is in progress. |
| Retrying | Circular arrow | Amber | The engine is trying again. |
| Disconnected | Dashed circle | Neutral | No current connection. Read the channel state for the reason. |
| Error | Cross | Red | Correct the reported cause, such as permission, credentials or host key. |
| Disabled | Dash | Neutral | Disabled in configuration. |

For a bridge that connects its receiver first, arrows identify the receiving and
sending sides. A disconnected or error side has a cross beside its arrow.

## Inspect channels and endpoints

**Channels** can be searched by channel or endpoint name and filtered by kind, state
and **Needs attention only**. **Endpoints** can be searched by name, address or channel
and filtered by kind and state.

| Detail page | What to check |
| --- | --- |
| Bridge channel | Session state; A and B states and active peers; bytes in both directions; each buffer's occupancy, limits and overflow count; retry countdown; last error; alerts. Framed message counts appear when a tap is configured. |
| Message channel | Source protocol, session, last receipt and received/duplicate/invalid/filtered/aborted counters. For each destination: queued, sending and unresolved counts, oldest queued age, next attempt, last acknowledgement and last error. Links open this channel's messages or unresolved deliveries. |
| Endpoint | Address/settings summary, state and its duration, active peer, failure streak, retry time, bytes, last activity and last error. Check which channels use it and which channel holds an exclusive serial port or listening socket. Secrets are not shown. |

Read an error's operation, cause and next action together. A listener waiting for its
first peer can wait indefinitely without increasing its retry count. For a bridge,
the second side must connect within the pairing timeout. A bridge closes the other
side when either side ends, then reconnects according to its retry policy.

Bridge buffer pressure holds the producer back; it does not silently discard bytes.
Check the receiving system when occupancy stays high. A bridge tap only observes:
an **Observed** message is a traffic record and has no deliveries.

When `tap.record_messages` is `true`, stored bridge observations use `tap.retention`:
`max_age_days` defaults to `30` and `max_messages` defaults to `null` (no count limit).
The retention run, after startup reconciliation and every ten minutes, purges recordings
beyond either limit. Setting both limits to `null` raises the `retention_unbounded`
configuration warning only when recording is enabled. Choose limits that fit the lab's
evidence policy and disk capacity; see the [bridge fields](configuration-reference.md#bridge-fields).

Traffic counters reset when a channel instance restarts. Endpoint counters cover all
channels since the engine started. Queue and unresolved counts come from the store
and survive restarts.

## Discover and test endpoints

Use the running engine to discover ports or check a connection before applying a definition.
The checks run with the engine's service-account access, not your terminal account's access.

| Task | Command |
| --- | --- |
| List OS-reported serial ports and configured use | `packie endpoints list-serial` |
| Inspect USB/device details and channel ownership as JSON | `packie endpoints list-serial --json` |
| Test a saved endpoint's active settings | `packie endpoints test <endpoint-id>` |
| Test an unsaved TCP client definition | `packie endpoints test --kind tcp_client --settings '{"host":"127.0.0.1","port":3001}' --timeout 10` |
| Read a structured test result | `packie endpoints test <endpoint-id> --json` |

Serial discovery opens nothing. It shows port name, bus kind, USB vendor/product ids and
device text where known, plus configured endpoints using the port and the channel holding
it open. An empty result means the OS reported no ports; check the cable and driver.

**Endpoint tests never send application data to an instrument or destination.** Each test
validates the definition, then reports each step's result, duration and detail. The overall
`--timeout` is 1–60 seconds (default 10); after a failure later steps are omitted. A failed
step or invalid definition exits 1. `--kind` and `--settings` test an unsaved definition in
place of an endpoint id; settings use the runtime schema and secret references.

| Kind | Steps and limits |
| --- | --- |
| `serial` | Open with the configured baud, framing and flow control, then close; check `virtual_link` instead when set. A port held by this engine is reported in use without reopening it. |
| `tcp_client` | Resolve the host, connect and report latency, then close without sending. |
| `tcp_listener` | Bind the address/port, then release it. A running listener's occupied port fails as in use. |
| `folder` | Find the folder, read/count matching files, then create and delete one hidden empty file to check write access. Reports counts, never file names; leaves no test file. |
| `sftp` | Resolve secret references, resolve the host, connect and verify the pinned host key, authenticate, open SFTP, list/count the remote directory, then disconnect. Writes nothing. |

Tests are audited as `endpoint_test` with kind, target and result. A successful transport
check does not validate protocol exchange, remote processing or final report correctness.
Use synthetic peers or the partner's test environment for those checks. See
[failed setup steps](troubleshooting.md#endpoint-test-failures).

## Pause, resume and restart

Use the channel page or its row menu. The console confirms pause and restart and
explains the interrupted session. An unavailable action is omitted.

| Action | Effect | CLI |
| --- | --- | --- |
| Pause | Closes the session, cancels retries and leaves accepted messages queued. A listener refuses new connections while paused. The pause survives an engine restart. | `packie channels pause <channel-id>` |
| Resume | Starts an operator-paused, enabled channel from the active configuration. | `packie channels resume <channel-id>` |
| Restart | Closes the session and creates a new instance from the active definition; instance counters and retry state reset. Use Resume for a paused channel. | `packie channels restart <channel-id>` |

Pause and restart interrupt transfers; bridge stops do not drain their buffers. On a
message channel, an interrupted delivery can need a decision because the destination
may already have received it. Check Messages after restarting.

On an endpoint page, **Restart the channels that use it** lists the channels to restart
and those left alone. Its effect is the same as restarting each listed channel. There
is no separate CLI endpoint-restart command. CLI channel commands act immediately and
print the outcome; a rejected command explains why and exits with code 1.

## Find and inspect messages

**Messages** starts with the last 24 hours, newest first. Each row shows receipt time,
channel, direction, message state, one delivery chip per destination, message type,
identifier, patient ID and size. The identifier is the control ID, falling back to the
sample ID. **Columns** adds message ID, source peer, configuration version and the
message it was resent from. Filtering, sorting and paging happen in the engine.

| Filter | Use |
| --- | --- |
| Time range | Narrow a busy list, or widen it to find old unresolved work. |
| Channels | Select one or several channels. |
| Direction | Inbound, A to B or B to A. Bridge observations use the latter two. |
| Message state | Receiving, received, routed, filtered, duplicate, invalid, aborted or observed. |
| Delivery state | Find messages with a delivery in the selected state. |
| Identifier | Match a control, sample or patient ID exactly or by its beginning. |
| Text | At least three characters; searches identifiers, message type and stored bodies within the time range. |
| Quick filters | Needs attention, Queued, Duplicates, Invalid or aborted, each with a count. These replace the state selections. |

Identifier and text searches may contain patient data. They are kept out of the page
address and cleared when leaving the page. **Clear filters** resets the selection.
An empty page distinguishes an unavailable store, no stored messages, no messages in
the time range and no filter matches.

**Receiving** means a transfer is underway; **Received** means stored and awaiting
processing. **Routed** means deliveries were created. **Filtered**, **Duplicate**,
**Invalid** and **Aborted** send nothing. **Observed** records bridge traffic without
processing it.

Open a message for its identifiers, times, source, configuration version and response
to the sender. Related records link to the original duplicate, earlier revision or
resend source. **Show original message** loads its body on request, as text with named
control characters or hex. Each destination shows its transformed body, state,
acknowledgement and attempt history.

| Delivery state | Meaning and action |
| --- | --- |
| Queued / Sending | Waiting for its turn or retry / currently attempting delivery. |
| Acknowledged | Reached the required acknowledgement level; read that level below. |
| Rejected | The destination answered no; it is not automatically retried. Correct the cause, then resend or close. |
| Failed | Not delivered. Correct the cause, then resend or close. |
| No acknowledgement in time | May have been delivered. Check the destination before resending. |
| Outcome unknown | May have been delivered. Check the destination before resending. |
| Closed | Ended with a stored reason. |

| Acknowledgement level | What was proved |
| --- | --- |
| Handed off | Written to the destination; not confirmed by it. Files, AU class A and HL7 without an ACK reach this level. |
| Received | Received by the destination; processing not confirmed. ASTM and AU class B reach this level. |
| Accepted | Accepted by the destination's application. This does not mean a person read it. |

### Resend or close

Open **Resend…**, select destinations and choose the mode. The console starts with
destinations needing attention. Read the per-destination result: one refusal does not
mean every selected destination was refused.

| Mode | Use and effect |
| --- | --- |
| Same message | Tries the existing deliveries again using their stored transformed output. If a mapping failure left no output, the current mapping runs against the original. No new message is created. |
| New message | Creates a record linked to the original and processes the original bytes through current filters and mappings for the selected destinations. The old record and deliveries remain unchanged. Use after a filter or mapping change when old output must be replaced. |

Invalid, aborted, filtered and duplicate messages offer only **New message**. Messages
still receiving or awaiting processing, and bridge observations, cannot be resent
from the console. Same message cannot resend a queued or sending delivery. The
destination must still exist and be enabled.

Both modes refuse a superseded revision by default: a newer message references it through
`revision_of`. Inspect the newer revision and the destination's current result first.
Only after the lab approves sending the older content, add `--superseded-ok` to the CLI
resend; this is separate from duplicate-risk confirmation and is recorded in the audit.

Both modes require duplicate-risk confirmation for acknowledged, timed-out or
unknown-outcome deliveries. Verify the destination first. Creating a new message
does not resolve the old delivery; close the old one with a reason if appropriate.

**Close delivery** accepts queued, rejected, failed, timed-out and unknown-outcome
deliveries. Enter a reason of 1–500 characters and confirm. Closing contacts no
destination; it records the decision and changes the delivery to Closed. A sending
delivery cannot be closed. Unresolved messages are retained until resolved.

| Task | Command |
| --- | --- |
| Recent channel messages | `packie messages list --channel <channel-id> --since 24h` |
| Unresolved work, including older records | `packie messages list --attention --since 30d` |
| Exact identifier / prefix | `packie messages list --identifier <value>` / `packie messages list --identifier <value> --prefix` |
| Inspect a message without printing its body | `packie messages show <message-id>` |
| Read original text / hex | `packie messages show <message-id> --body original` / `packie messages show <message-id> --body original --as hex` |
| Read a destination's stored output | `packie messages show <message-id> --body <destination-id>` |
| Read the original's parsed field paths and values | `packie messages show <message-id> --fields` |
| Retry stored output for one destination | `packie messages resend <message-id> --destination <destination-id>` |
| Reprocess with current configuration | `packie messages resend <message-id> --destination <destination-id> --new-message` |
| Confirm a checked duplication risk | Add `--confirm-duplicate-risk` to either resend command. |
| Close a delivery | `packie messages close <delivery-id> --reason "Verified at destination"` |

CLI lists have no default time limit and print 20 rows by default. Use `--limit` (1–1000)
and `--offset` for pages, or `--oldest-first`. Additional filters are `--state`,
`--delivery-state`, `--direction`, `--until` and `--text`. Channel, state,
delivery-state and direction accept repeated flags or comma-separated values. Times
accept RFC 3339 or an age such as `15m`, `2h` or `7d`; states use names such as
`timed_out` and `outcome_unknown`.

CLI resend defaults to every destination when `--destination` is omitted. Resend and
close act immediately; they have no interactive confirmation. List, show, resend and close
accept `--json` for scripts. Bodies print only with `show --body`, parsed fields only with
`show --fields`; other message facts can still contain patient data. Body/field requests
and identifier/text/id-prefix searches are audited against the connecting OS account.

## Transmission reports and message export

**Reports and exports can contain PHI even without bodies.** Choose a restricted evidence
directory, protect its permissions/encryption and track copies under the lab's retention
policy. These commands use the running engine and are audited against your OS account. The
console offers the same reports and exports: **Transmission report** under Diagnose, and
**Export…** on **Messages**.

| Task | Command |
| --- | --- |
| Summarize a channel's transmission evidence | `packie messages verify --channel <channel-id> --since 24h` |
| Export delivery rows to a new CSV file | `packie messages verify --channel <channel-id> --since <start-time> --until <end-time> --csv --output <new-file>` |
| Preserve the complete report and truncation flag | `packie messages verify --channel <channel-id> --since 24h --json --output <new-file>` |
| Export matching message metadata as CSV | `packie messages export --channel <channel-id> --since 24h --format csv --output <new-file>` |
| Explicitly include original content as JSON Lines | `packie messages export --channel <channel-id> --since 24h --format jsonl --bodies --output <new-file>` |

The transmission report selects messages received in one channel within `[since, until)`;
`--until` defaults to now. Its summary counts each destination's accepted, received, handed-off,
unresolved, closed and pending deliveries. CSV has one row per delivery: message id, receipt
time, message state, sender control id, original SHA-256 (`content_hash`), destination, delivery
state, ACK level/code, **`ack_control_id`** (the ACK message's own control id, HL7 ACK `MSH-10`),
stored transformed SHA-256 (`sent_hash`), attempt count, first-attempt time and ACK time.
A missing sent hash does not prove nothing was sent; pass-through deliveries can have none.
An ACK proves only its recorded level, not clinical correctness at the final destination.

Reports cap delivery rows at 10,000. Check the text summary's warning or JSON `truncated`
before filing CSV evidence: CSV has no truncation marker. When truncated, the message count
covers the whole selection, but destination totals cover only processed pages. Split the
period until reports are complete. `--csv` and `--json` conflict; `--output` implies CSV
unless `--json` is present. The audit action is `transmission_verify`.

Message export uses the same filters as `messages list`, including time, channel, direction,
message/delivery state, attention, identifier, text and id prefix. It exports all matches,
oldest first, without list pagination or sort flags. CSV is the default; JSON Lines is
`--format jsonl`. Metadata includes identifiers and each delivery's destination/state/ACK
level (CSV joins these as `destination:state:ack_level`, separated by semicolons).
`--bodies` adds original content as UTF-8 text with invalid byte sequences replaced; an
unreadable or incomplete original is exported as empty text. This is not a byte-exact body
archive. The first export chunk is audited as `message_export` with filters, format, content
choice and matching count; `--bodies` makes the content access explicit.

Both commands write to stdout without `--output`, or create a new file and refuse an existing
one. Export's count goes to stderr when stdout carries data. If writing fails, treat any
partial file as incomplete. See [validation and evidence](compliance.md#3-interface-validation-and-change-control).

## Configuration notes and verification

Start an edit with `packie config show` so its `config_version` detects intervening changes.
Validate before applying. Every apply needs a reason of 1–500 characters; use a change-record
reference and an explanation. A stale version is rejected as `version_conflict`; reload and
review rather than routinely using `--force`.

| Task | Command |
| --- | --- |
| Export the active runtime document | `packie config export > runtime-review.json` (alias of `config show`; secrets stay references). |
| Compare active configuration with a proposed file | `packie config diff active runtime-review.json` |
| Compare two saved versions | `packie config diff <old-version> <new-version> --json` |
| Check a document without applying it | `packie config validate <file>` |
| Apply with a change note | `packie config apply <file> --note "Change record reference and reason"` |
| Read versions and latest verification | `packie config history` |
| Record completed validation of a version | `packie config verify <version> --note "Validation record reference"` |

`config diff` accepts a saved version number, `active`, or a runtime document file on each
side; files are validated first, while saved versions and `active` need the engine. It
lists added, removed and changed endpoints/channels by id, then other sections and changed
fields. Defaults count as written; `schema_version` and `config_version` are ignored.
Equal documents print "configure the same". Diff is read-only and not audited. Review the
diff before applying, and keep configuration exports restricted: labels, paths, constants
and notes can contain sensitive information even though secret values are excluded.

In **Settings → Versions**, the change-control history records the OS account that applied
each version, time, required note and verification sign-offs. Record a sign-off there with
**Mark as verified…** or with the CLI above, using the lab's record reference. A version with no sign-off shows **Not verified since change**. Verification
records account, time and a 1–500-character reference; it does not run tests or replace the
director's approval. Sign-offs can be recorded more than once.

Check every affected channel's apply outcome, including a partial apply: the failed channel
may still run its previous definition. Preserve the returned per-channel output in the lab's
change record; neither version metadata nor the audit's overall action outcome proves all
channels applied. Complete the lab's tests before signing off, and keep the validation evidence
referenced by the note. Versions are kept without limit
by default (`runtime.versions_kept: null`); a configured count cannot prune versions younger
than `audit.retention_days`. Older versions may have only legacy notes and no account record.
See [interface validation](compliance.md#3-interface-validation-and-change-control).

## Review the audit log

Use **Audit** in the console, or the CLI below. The audit view
provides account, time, action, object and outcome filters; CLI filters also include origin.
The OS account identifies the operator; `origin` (`console`, `cli`,
`engine`, `other`) is the client type. Give each person an individual OS account.

| Task | Command |
| --- | --- |
| Review recent activity | `packie audit list --since 24h` |
| Filter by account | `packie audit list --since 7d --actor <account>` |
| Find body views for a message | `packie audit list --action message_body_view --object-type message --object <message-id>` |
| Read the next page | `packie audit list --since 24h --before-seq <next-seq> --limit 50` |
| Export matching evidence to a new file | `packie audit export --since 24h --format jsonl --output <new-file>` |
| Export CSV to standard output | `packie audit export --since 24h --format csv` |
| Verify the engine's audit chain | `packie audit verify` |
| Verify a preserved audit directory offline | `packie audit verify --dir <copy-audit-dir>` |

`list` defaults to 50 events, newest first, with `--before-seq` for the next page; range
1–1000. Add `--json` for a page/report. Shared list/export filters are `--since`, `--until`,
`--actor` (exact account name or uid/SID), `--action`, `--origin`, `--outcome`, `--object-type`
and `--object`. Times accept RFC 3339 or an age such as `2h` or `7d`. Export writes every
matching event oldest first, defaults to CSV, and accepts `--format jsonl`. Its first chunk
is audited; `--output` creates an owner-only file and refuses to overwrite an existing one.
Export has no `--json` flag; choose `--format jsonl` for JSON Lines.

Commands that change state have a `requested` event and a later `succeeded`, `rejected` or
`failed` event. Content views/searches and audit export/verification are recorded before
returning content. If an initial audit write fails, the audited command/content view is
refused; message traffic continues. An outcome write can fail after an action ran: use the
command result and affected data to resolve a request without an outcome.

Audit files live in `<state>/audit/`, separate from the message store and diagnostic logs.
Details may contain PHI in searches and operator notes/reasons, never message bodies. Keep
exports and copies restricted. Audit writes survive engine crashes; a power loss can lose
about the last second before checkpoint. Retention removes sealed yearly files whole and
records each removal. Default `audit.retention_days` is 2190; see the lab's
[retention policy](compliance.md#5-retention-and-purge).

Verification exits 1 on problems and returns the retained chain's head sequence/hash.
Compare it with `audit.anchor` in a separately preserved diagnostic log; a valid shorter
chain alone cannot reveal deletion of the newest events. Anchors are published daily as
events arrive and on shutdown. Host administrators can alter or destroy local files and
anchors. See [audit behavior](../spec/audit.md) for the checks and trust boundary.

## Monitor backups

Use **Settings → Backup** in the console, or `packie backup status` (`--json` for scripts).
Check the configured directory, local daily schedule/next run, stale limit, newest attempt
and its error, and newest success. A recent failed attempt does not replace the latest good
backup.

`packie backup create` takes a consistent snapshot to a new directory under `backup.directory`;
`packie backup create <new-or-empty-directory>` uses an explicit target. The running engine writes
as its service account. With no engine running, the CLI locks the state directory and works
offline. Run `packie backup verify <backup-directory>` after copying backups and as part of
restore tests. A successful creation or verification does not test emergency operation at the
lab; exercise a restore using [installation's restore procedure](install.md#backup-and-restore).

Scheduling is off by default. With a schedule, the stale threshold defaults to 26 hours;
`backup_stale` warns if the last success is too old or none succeeds within the threshold
after engine start. Fix target mount, free space or permissions and check the last error,
then create a backup. Backups contain PHI and exclude secret values, diagnostic logs and
folder/SFTP endpoint files. Encrypt the target, preserve required external files separately
and arrange off-computer copies under the lab's custody and retention policy.

## Read logs

**Logs** filters by minimum level, time range, channel, endpoint, event and text.
An event filter matches the whole name, for example `endpoint.retry_scheduled`.
Text matches the message, target or fields without regard to case.

Follow adds new events as they arrive. Scrolling away from the newest row or opening
a row pauses the display and counts new events. **Jump to live** catches up. Expand
a row for its event, target, sequence, message and fields; scope links open the
channel, endpoint or message.

The page holds the newest 10,000 rows. **Load older events** reads earlier entries
still in the engine's memory. **Show events before** opens an earlier time range when
the window is full. While disconnected, the console waits and resumes following on
reconnection. It reads missed entries still in the engine's log ring and marks any
gap. After an engine restart, previous in-memory events are unavailable. Rotating
files remain in the configured log directory for retained history.

| Investigating | Useful event names |
| --- | --- |
| Starts, stops and operator actions | `node.started`, `node.stopped`, `channel.started`, `channel.stopped`, `channel.state_changed`, `operator.command` |
| Connection failures and recovery | `endpoint.state_changed`, `endpoint.connected`, `endpoint.retry_scheduled`, `endpoint.retry_succeeded`, `endpoint.write_failed`, `listener.peer_rejected` |
| Bridge sessions and pressure | `bridge.session_opened`, `bridge.session_closed`, `bridge.close_propagated`, `buffer.overflow_detected`, `buffer.overflow_cleared`, `tap.dropped` |
| Receipt, input problems and duplicates | `message.committed`, `message.invalid`, `message.aborted`, `message.duplicate`, `message.identity_reused`, `message.filtered`, `file.consume_failed` |
| Delivery decisions | `delivery.attempt_started`, `delivery.state_changed`, `delivery.transform_failed`, `delivery.ack_unmatched`, `delivery.resend_requested` |
| Store, configuration and alerts | `store.error`, `reconciliation.completed`, `retention.purged`, `config.applied`, `config.rejected`, `alert.raised`, `alert.cleared` |

Logs contain metadata, counts, codes and engine-generated IDs. Message bodies, patient
and sample identifiers, message file names, secrets and close-reason text do not
belong in logs. Read requested bodies in Messages or a live transcript. Peer IP addresses
remain, so body-free logs are not de-identified. Audit details are separate and can contain
PHI; use Audit rather than Logs to review content access and operator reasons.

| Task | Command |
| --- | --- |
| Newest 100 matching events | `packie logs` |
| Follow warnings and errors for a channel | `packie logs --follow --level warn --channel <channel-id>` |
| Investigate peer refusals | `packie logs --event listener.peer_rejected --endpoint <endpoint-id> --since 2h` |
| Bounded historical range in memory | `packie logs --since 2h --until 1h --limit 500` |
| Follow as JSON lines | `packie logs --follow --json` |

Other flags are `--text`, `--event`, `--channel`, `--endpoint`, `--level`, `--since`,
`--until` and `--limit`. Levels are `trace`, `debug`, `info`, `warn`, `error`.
CLI output prints matching history oldest first, then follows with `--follow` (`-f`).
Ctrl-C stops following. `--follow` cannot be combined with `--until`; a closed engine
connection ends CLI following, so run it again after reconnection.

## Respond to alerts

**Alerts** opens on Open alerts. Filter by Open, Cleared or All, severity, or search
by alert, channel or endpoint. Open alerts sort critical first, then warning, then
first seen; cleared alerts sort most recently cleared first. Each row gives its scope,
first and last seen, occurrences and next action.

Critical is red and needs action now; Warning is amber and needs attention soon.
Thresholds are in **Settings → Engine**. Alerts cannot be dismissed: correct the
condition and they clear automatically. Cleared records remain visible for 30 days.
Pause-related endpoint and backlog conditions are suppressed for intentionally paused
or disabled channels.

| Alert code | Severity | Next action and clearing condition |
| --- | --- | --- |
| `endpoint_down` | Warning; critical for prolonged outage or Error | Check the device, remote system and network path; read the retry countdown. For Error, fix the reported permission, credentials or host key. Clears when the endpoint recovers. |
| `no_peer_connected` | Warning | Check that the instrument is on and connects to the listener's configured address and port. Clears when a peer connects; the threshold can be disabled. |
| `peer_rejected` | Warning | Check the peer and rejection reason. Add a legitimate peer to the allowlist; a second peer cannot replace an active session. Clears after ten minutes without rejection. |
| `bridge_buffer_pressure` | Warning; critical during overflow | Check the slower receiving system. The sender is held back. Clears after overflow ends and occupancy falls below the alert threshold. |
| `message_store_unavailable` | Critical | Check state-directory permissions and free space. Incoming messages receive no positive storage acknowledgement until storage works. Clears when storage recovers. |
| `audit_unavailable` | Critical | Check free disk space and `<state>/audit` permissions. Audited commands and content views are refused; message traffic continues. Clears on a successful audit write; maintenance retries every minute and commands try again. |
| `notifications_failing` | Warning | Check relay/webhook reachability, security settings and secrets; run `packie notify test`. Clears only after every failing target has a successful send, including a test. |
| `backup_stale` | Warning | Check target availability, space, permissions and the last error with `packie backup status`; create a backup after fixing the cause. Clears after a successful backup. |
| `disk_low` | Warning; critical below the critical limit | Free space or reduce retention. Unresolved messages are never purged automatically. Clears above the warning level. |
| `delivery_needs_attention` | Warning; critical at the configured count | Open the channel's unresolved Messages and inspect each destination. Fix the cause, then resend or close with a reason. Check the destination first for possible delivery. Clears when no unresolved delivery remains. |
| `transform_failed` | Warning | Inspect the failed delivery for the rule and path; correct the mapping, apply and resend. Clears when no unresolved mapping failure remains. |
| `delivery_backlog` | Warning; critical at three times the count or age limit | Check the destination endpoint. Stored messages wait in order for recovery. Clears below both enabled thresholds. |
| `invalid_messages` | Warning | Check the sender's protocol configuration and inspect the stored originals. Clears when invalid/aborted traffic falls below the count in the configured window. |
| `config_rejected` | Warning | Read validation errors in Settings → Versions, fix and apply again. The previous valid configuration stays active. Clears after a successful apply. |
| `config_apply_failed` | Warning | Read the affected channel's error, fix it and apply again. The alert explains that the previous definition remains active. Clears after recovery or a later successful apply. |

`packie status` prints active alerts and their next actions; `packie status --json` includes
the snapshot data. There is no separate CLI alert-history command.

## Alert notifications

Configure bootstrap `notifications.email` and/or `notifications.webhook`, then restart the
engine; targets are off by default. Each target selects `warning` and/or `critical` alerts.
The engine sends in the background when an alert is raised, changes severity or clears.
The default rate limit is one message per alert code/channel/endpoint/destination per
15 minutes, shared by all targets. Raise and clear share that window, so a quick recovery
may produce no clear message. Suppressed events log `notification.suppressed` at debug.

Notification content is designed to be PHI-free: node display name, alert code/severity/state,
scope ids, channel/endpoint labels, summary, next action, first/last seen and occurrence count.
It excludes alert `details`, message bodies, parsed values and message identifiers. Keep
operator-entered names and labels free of PHI; `peer_rejected` next actions can contain a
peer IP address. Review that metadata before choosing recipients. These are operational
alerts, not patient-result notifications.

| Target or test | Behavior |
| --- | --- |
| E-mail | Plain text; subject starts `[PACK-IE <node>] <SEVERITY> <code> raised` or `cleared`, with scope id where applicable. |
| Webhook | JSON POST with `Content-Type: application/json` and optional bearer authorization. Only 2xx is success; redirects fail. |
| `packie notify test` | Sends one `test` / `info` / `test` message to every target, regardless of severity selection; prints each result. Audited as `notification_test`. Add `--json` for structured results. |

A test exits 1 if any target fails; with no targets it says so and exits 0, which does not
prove notification delivery. Confirm the message reached the intended mailbox/receiver.
Secrets are resolved when sending, so `packie secrets set <name>` can repair a missing password
or bearer token without a restart. Changing bootstrap target settings requires a restart.

Failed sends log `notification.failed` without content or secret values and are not retried.
`notifications_failing` names all targets whose latest send failed and their newest errors;
a later successful send, including a test, clears that target's failure. The alert clears
when all targets recover. Successful sends log `notification.sent` at debug. Monitor local
Alerts/status as well: a failed notification route cannot be relied on to report its own
outage. See [notification troubleshooting](troubleshooting.md#notification-failures) and
[relay setup](install.md#notification-relay-setup).

## Watch a live transcript

On a channel page, choose **Start live transcript** to capture traffic. Stop it with
**Stop live transcript**; leaving the page or disconnecting ends the subscription.
The transcript is separate from stored messages and logs and stays in local memory.

| Control or pane | Effect |
| --- | --- |
| Bridge panes | A → B and B → A show the two byte directions. |
| Message panes | Source shows incoming bytes and engine responses. Destinations shows sent bytes and replies, with destination tags when supplied by the engine. |
| Show hex | Adds byte values beside text with named control characters. |
| Freeze / Unfreeze | Holds the display while capture continues / shows the newest captured segments. This does not pause the channel. |
| Autoscroll | Follows each pane; scrolling up stops that pane following. |
| Clear | Empties the displayed history and its counts. |
| Copy | Copies the displayed transcript to the clipboard. It can contain patient data. |

Each pane retains the newest 1,000 segments; older segments leave the view. A slow
viewer can miss observations while the channel continues. The transcript survives
endpoint reconnects while its subscription stays open. Stopping capture leaves the
already displayed bytes available in that page until cleared or closed. PACK-IE does not
save transcript bytes to a file automatically. There is no CLI transcript command.

See [channel behavior](../spec/bridge-channels.md),
[message behavior](../spec/message-channels.md) and the
[diagnostics catalog](../spec/diagnostics.md) for field and event definitions.
