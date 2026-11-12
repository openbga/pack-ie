# Troubleshoot PACK-IE

Start in **Dashboard** and **Alerts**, then open the affected channel or endpoint. Record the
state, last error, next action and retry time before changing anything. A listener waiting for
a peer is normal; a paused or disabled channel is intentionally stopped. Alerts clear when
their condition clears and cannot be dismissed.

For terminal checks, use `packie --bootstrap <installed-bootstrap> status` and
`packie --bootstrap <installed-bootstrap> logs --follow`. Use the executable and bootstrap paths
from [installation](install.md). Add `--channel <channel-id>` or `--endpoint <endpoint-id>` to
logs to narrow the view. See the [CLI reference](../spec/operator-console.md#12-command-line)
and [diagnostics catalog](../spec/diagnostics.md#6-alert-catalog).
Audit and backup commands also use that bootstrap; see
[running and monitoring](running-and-monitoring.md#review-the-audit-log).

## Connections and bridge traffic

| Symptom | Likely cause | Check | Fix |
| --- | --- | --- | --- |
| Channel stays Retrying; serial open says in use | Another process or channel owns the port; a previous session is releasing it | Endpoint **held by**, channels using the endpoint, OS serial-device list, last open error | Close the other serial application or pause the competing channel. Confirm the configured port. Short release delays are retried automatically; a port that remains locked follows channel backoff. |
| Serial endpoint shows Error or permission denied | Service account cannot access the device; port name changed after reconnect | Physical connection and current COM/tty name; device permissions under the service account | Correct the port or grant that service account device access. Linux service installation uses `dialout` when the group exists; check the actual device's group. Restore the device and allow the next retry. Physical serial needs qualification on the target computer. |
| TCP client says connection refused | Remote listener is stopped, wrong port, or remote firewall rejects the connection | Endpoint host/port and last error; remote listener and its logs; reachability from this computer | Start or correct the destination listener and allow the configured traffic. The endpoint retries with backoff; repeatedly restarting the channel does not repair the remote service. |
| TCP client connects then immediately closes, or times out | Peer allowlist, firewall, routing or protocol settings disagree | Peer logs and allowlist for this computer's actual source IP; address/port; configured protocol; network path | Correct the peer's allowlist or network rule and align protocol settings. A successful TCP connection alone does not prove application acceptance. |
| Listener cannot bind; address already in use | Another process or channel holds the same address and port | Endpoint bind address/port, **held by**, and OS listening sockets (`Get-NetTCPConnection`, `lsof`, or `ss`) | Stop the unintended owner or choose the correct port. A replaced channel may need one retry after the old holder releases the socket. |
| Listener cannot bind; address unavailable or permission denied | Bind address is not on this computer, or service account cannot bind that port | Configured `bind_address`, current network interfaces and last error | Bind to the intended local interface and use a port the service can open. Apply the endpoint correction; its channels restart. |
| Listener is Listening but instrument cannot connect | Wrong instrument target, firewall/routing, or pairing order | Instrument's configured destination; local bind address/port; host firewall; channel strategy and session state | Point the instrument at the reachable configured address and allow that port. Under `receiver_then_sender`, the sender-side listener is bound only after the receiver attaches. Waiting for the first peer has no pairing timeout. |
| `peer_rejected` alert or an immediate reset of a new inbound connection | Peer is outside the allowlist, or a session already has an active peer | `listener.peer_rejected` log, peer address, `error_code` (`not_allowed` or `session_active`), active peer | Add the legitimate peer's address/CIDR to the endpoint allowlist. For `session_active`, correct the duplicate client or let keepalive/idle timeout clear the old connection. The newcomer never replaces the active session. |
| Bridge is Degraded; buffer occupancy or overflow count grows | The receiving side is slower or stopped | A→B and B→A occupancy, limits, last activity and receiver state; `bridge_buffer_pressure` alert | Restore the slow receiving system and its serial flow control or network path. Bounded buffers hold the sender back rather than silently dropping traffic. An overflow clears after the affected buffers drain to half their limits. |
| Bridge closes both connections after one peer disconnects | Close propagation is part of bridge behavior | `bridge.close_propagated`, the side and cause, next retry time | Fix the originating peer or transport. The bridge closes the other side and reconnects according to policy. |

Do not pause or restart a bridge merely to clear pressure: those actions close the session
without draining, and bridge buffers are not a durable message queue. Engine shutdown has a
bounded drain budget; bytes still queued when it expires are discarded. See
[bridge behavior](../spec/bridge-channels.md#6-buffers-and-backpressure).

## Endpoint test failures

Run `packie endpoints test <endpoint-id>` for the active settings, or test an unsaved definition
as described in [setup checks](running-and-monitoring.md#discover-and-test-endpoints).
Read the first failed step; later steps are normally omitted. A timeout covers the whole
test, not each step. These checks never send application data and do not prove protocol
compatibility. Listing with `packie endpoints list-serial` opens nothing.

| Kind / failed step | What to check | Action |
| --- | --- | --- |
| Any kind / validation | Reported field path such as `settings.port`, kind and settings JSON | Correct the definition using the [configuration reference](configuration-reference.md#endpoints). An invalid definition is not probed. |
| `serial` / `open` or virtual-link check | Current OS port name, cable/driver, service-account device permissions, framing/baud/flow control; reported holder | Use serial discovery, correct settings/permissions, and close a competing serial app. A port held by the engine is reported in use; do not interrupt clinical traffic just to obtain a passing test. Test during an approved downtime window if opening it is necessary. |
| `tcp_client` / `resolve` | Host spelling and DNS available to this computer | Correct host/DNS; compare with the partner's assigned address. |
| `tcp_client` / `connect` | Remote listener/port, routing, firewall, allowlist and connection limit | Restore the intended listener/network path. A passing connection test sends no protocol probe. |
| `tcp_listener` / `bind` | Address belongs to this host; port permissions and existing listener/channel ownership | Correct the bind or competing owner. The active listener already holding the port is an expected in-use failure. |
| `folder` / `find folder` | Path exists, volume/share is mounted, service can traverse parents | Correct the path/mount and service access; a login user's mapped share may be unavailable to the service. |
| `folder` / `read directory` | Directory read access, pattern and actual matching file count | Correct service-account read access; zero matches can be normal. No file names are returned. |
| `folder` / `write test` | Permission to create and remove files, free space, read-only mount and service sandbox | Restore write/delete access. Sources also need it to move/delete consumed files. The test creates and removes one hidden empty file. |
| `sftp` / `credentials` | Secret names, provider access, private-key readability and optional passphrase | Set the named secret through `packie secrets set <name>`; use references in settings, never literal values. |
| `sftp` / `resolve` | Server name and local DNS | Correct the host/DNS. |
| `sftp` / `connect` | SSH port/reachability, pinned host-key fingerprint and handshake error | Restore reachability; verify any changed fingerprint through a trusted partner channel before changing the pin. |
| `sftp` / `authenticate` | Account, password/key/passphrase and server account policy | Correct the named credentials and partner-side access. |
| `sftp` / `open sftp` | Server enables SFTP for that account | Ask the partner to enable/correct its SFTP subsystem. |
| `sftp` / `list` | Absolute remote directory, account listing permissions and operation timeout | Correct the path/access. A passing list check does not prove remote write access; this test writes nothing. |

Tests are audited as `endpoint_test`. Inspect `--json` for individual results and durations;
raise `--timeout` only within 1–60 seconds when the expected setup latency requires it.

## Notification failures

| Symptom | What to check | Action |
| --- | --- | --- |
| `notifications_failing` | Alert summary names each target and newest error; `notification.failed` logs; host/port/security, network path and credentials | Fix each failing route and run `packie notify test`. Failed sends are not retried. A successful later send/test clears that target; the alert clears only when all targets recover. Check the actual mailbox/receiver too. |
| `notification.secret_unresolved` at startup, or a test/send names a missing notification secret | Event `field` and `secret`, bootstrap `notifications.email.password` / `notifications.webhook.bearer`, configured secrets provider | Use `packie secrets list` to check names, then `packie secrets set <name>` to enter the value without putting it in arguments or configuration. Secrets are read on each send; test again without restarting. Restart only if the bootstrap reference/provider changed. |
| SMTP TLS/certificate or login failure | STARTTLS versus TLS port, relay certificate chain/host name, username/password pair | Match relay settings, certificate host name and trust chain. Configure `notifications.ca_file` for a private CA; SMTP also trusts bundled Mozilla roots. See [relay and CA setup](install.md#notification-relay-setup). |
| `notification.ca_invalid`, or a TLS send reports an unusable CA file | `notifications.ca_file` path (relative paths resolve against the bootstrap file's directory), PEM certificates, service-account read access and service sandbox | Repair the CA bundle and restart to reload it, then run `packie notify test`. SMTP adds these roots to bundled roots; HTTPS webhooks trust only this bundle when set. TLS sends fail until it is usable; local plaintext targets are unaffected. |
| Webhook fails or returns a redirect | HTTPS address, certificate, bearer secret, response status and timeout | Use the final URL: redirects are not followed and only 2xx succeeds. HTTP is allowed only on loopback. |
| `packie notify test` reports no targets and exits successfully | Correct installed bootstrap and whether either target is configured | Configure targets and restart, then test. Success with no targets proves no delivery. |
| Raise arrived but no clear message, or repeat alert not sent | Severity selection, `notifications.rate_limit_minutes`, `notification.suppressed` (debug) | Raise/severity-change/clear share one window per alert scope and all targets; a quick clear can be suppressed. Read local Alerts/status for current state. |

Keep names/labels free of PHI and use local Alerts/status while notifications are failing.
See [notification content and behavior](running-and-monitoring.md#alert-notifications).

## Stored messages and deliveries

A message's state describes receipt and routing. Each destination has a separate delivery
state and acknowledgement level. Open **Messages**, widen the default last-24-hours range
when needed, and use **Needs attention**, **Queued** or **Duplicates**. Inspect the affected
destination and its attempts before deciding to resend.

| Symptom | Likely cause | Check | Fix |
| --- | --- | --- | --- |
| Delivery remains Queued | Destination is unavailable; next retry is pending; channel is paused/disabled; destination is disabled | Destination endpoint, queue age, `next_attempt_at`, enabled state and channel stop reason | Restore the endpoint, resume an operator-paused channel, or enable the intended configuration. Waiting for an endpoint consumes no delivery attempt. Existing queued deliveries persist across restart. |
| Delivery remains Sending | Bytes are being written or the required acknowledgement has not arrived | Attempt start time, send/ACK timeouts, destination logs and the reached ACK level | Allow the configured timeout and fix the destination's response behavior. Do not interrupt an in-flight attempt as a routine retry. A Sending delivery cannot be closed. |
| Timed out / No acknowledgement in time | Whole message sent; required ACK not received before the timeout | Destination record and ACK logs, ACK mode/level, timeout and control-id correlation | Check whether the destination processed it first. Resend only after resolving the duplicate risk, or close with a reason. Automatic retry is off by default for this case; `retry_unacknowledged` is only for destinations that tolerate duplicates. |
| Outcome unknown | Connection or engine ended before acknowledgement; delivery may have reached the destination | Destination records, delivery attempts, restart time and `engine_restarted`/`engine_stopped` reason | Check the destination before resending. Confirm duplicate risk if resending, or close with a reason when no further delivery is required. PACK-IE does not automatically query the destination to reconcile this state. |
| Rejected by destination | Destination returned a negative acknowledgement | ACK code, protocol settings and destination's explanation; inspect payload locally only if needed | Correct the cause. Rejections do not retry automatically. Choose the resend mode that matches whether the output must change. |
| Failed, not delivered | Attempts exhausted or local processing/send failure | Reason code, attempts, endpoint error and mapping error | Correct the reported cause and resend. For `transform_error`, use the mapping procedure below. |
| Acknowledged but staff cannot find a processed result | Confirmation may only prove handoff or receipt | Delivery's `ack_level` and destination workflow | `handed_off` proves bytes/file left PACK-IE; `received` proves protocol receipt; `accepted` proves application acceptance. No level proves a person read the result. Check the destination before replaying. |
| Duplicate message has no deliveries | Same channel, source identity and content as an earlier message | Duplicate's link to original and the original deliveries; sender retry history | Resolve the original's deliveries. The duplicate is stored and positively acknowledged but not processed again. Same identity with changed content is a revision and is processed normally. Use **New message** resend only when deliberately processing the original again. |
| Invalid, aborted or filtered message has no deliveries | Syntax/transfer failure or configured filters excluded it | Reason code, source settings, filters and original locally; **Invalid or aborted** filter | Correct the sender or configuration. Use **New message** resend to process stored original bytes through current configuration; incomplete bytes may still be unusable. |
| Resend refused with `superseded` | A later revision supersedes this message | Refusal's newer-message id, related revision chain and final destination record | Prefer the current revision. Only after the lab approves older content, use `packie messages resend <message-id> --superseded-ok --reason "Approved older revision; record reference"`. Both modes require this override; add duplicate-risk confirmation separately when required. The audit records the override. |
| Messages page is empty | Time/filter range excludes records, no traffic yet, or store unavailable | Empty-state explanation, time range, filters, channel counters and store state | Clear filters or widen time. If unavailable, follow the storage checks below. A bridge records messages only when its configured tap records them. |

For a mapping failure, open the failed destination delivery and note the rule/path and
`transform_error` reason. Correct the mapping or code table in the channel editor, **Validate**,
then **Save and apply**. Check that the affected channel's apply outcome succeeded and that a
new configuration version is active.

Resend with **New message** when output must be regenerated from the original using that new
mapping version. **Same message** normally reuses the stored output and frozen mapping
version; its special case is a transform failure with no stored output, where it runs the
current mapping. New-message resend leaves the original deliveries unchanged: after verifying
the replacement, close any original unresolved delivery that no longer needs delivery, with
a reason. Review the resend result for each selected destination. See
[resend rules](../spec/message-channels.md#9-resend-and-close).

## Startup, storage and console access

| Symptom | Likely cause | Check | Fix |
| --- | --- | --- | --- |
| Service fails immediately or foreground engine refuses to start | Missing/invalid bootstrap; state/log directory or secrets-file permissions | `service status` bootstrap path, service-manager output and log directory; installed bootstrap settings | Correct the named startup field or file access for the service account, then start again. Bootstrap edits require a restart. A schema/type error may need fixing before later errors can be reported. |
| Engine runs with no channels, or starts an older configuration | Runtime file is unreadable/invalid; no valid saved version available | Active version, `config_rejected`, Settings → Versions, `runtime.json` and version-file permissions | Fix and validate the runtime configuration, then apply it. Startup falls back to the newest valid saved version; with none it runs degraded with zero channels. An invalid apply leaves the previous configuration active. |
| Apply is partial; one channel uses its previous definition | That channel's replacement or compiled mapping/filter failed | Per-channel apply result, `config_apply_failed`, reported configuration path | Correct that channel and apply again. Other successful changes are already in effect. Confirm the new outcome rather than assuming the saved version means every channel changed. |
| Message store unavailable; message channels stopped at startup | Store could not open/migrate; disk full, permissions or database problem | `store.error` (`open_failed`), free disk, state/store ownership and access as service account | Fix disk or permissions. Opening retries after 10 seconds, doubling up to five minutes; on recovery, reconciliation runs and message channels start. Bridges continue meanwhile. Do not delete the database to clear this alert. |
| Store becomes unavailable during traffic; sender sees no positive ACK | Durable commit failed | `store.error`, free space and filesystem errors; sender retry state | Restore storage. PACK-IE withholds positive receipt acknowledgement until data is committed. Let the sender retry under its own protocol rules; verify store recovery and queued work. |
| Console says engine not running or keeps reconnecting | Service stopped, wrong IPC location, or stale saved override | `service status`, reported endpoint, `PACKIE_BOOTSTRAP`/`PACKIE_IPC`, saved `console.json` override | Start the correct service. The console looks for the installed bootstrap itself; for another engine, correct or remove the saved override or set `PACKIE_BOOTSTRAP`, as described in [installation](install.md#ipc-access-and-opening-the-console). Dimmed pages show stale data until reconnection. |
| Engine is running but Unix console/CLI gets permission denied | Operator not in `ipc.group`, stale login membership, owner-only socket, or inaccessible parent directory | `id`, socket ownership/mode, parent-directory traversal, `ipc.group_unavailable` event | Add the operator to the configured group and sign out/in. If group assignment failed, create/correct the group and restart the engine so its socket is recreated with group access. Keep state files private; do not make the directory or socket world-writable. |
| Windows console/CLI gets access denied after upgrade | Operator is not in the selected group, or has an old login token | Bootstrap `ipc.group`, local `PACK-IE Operators` membership, `ipc.group_unavailable`, actual elevated installing account | Default `packie` selects `PACK-IE Operators`; SYSTEM and Administrators retain access. Add authorized operators with the [installation commands](install.md#ipc-access-and-opening-the-console), sign out/in, and restart after a bootstrap/group-resolution change. Do not restore all-local-user access. |
| Start the service button fails | Engine executable not found or command needs elevation; macOS button selects user agent | Button error; `PACKIE_BIN`, executable location and `PATH`; installed service scope | Use the explicit elevated platform CLI command from [installation](install.md#service-commands). On macOS use `sudo` for the packaged system daemon. |
| Another engine already owns this state directory | Service and foreground process point at the same state | Service status and the instance-lock error (`packie.lock`, `packie.instance.json`) | Use the running engine, or stop its service before running a foreground instance. Do not remove the live lock to start a second process. |
| Low disk or increasing state size despite retention | Unresolved deliveries are protected; eligible retention or log volume exceeds available capacity | Settings → Retention, store size/free disk, queued and unresolved counts, logs volume | Free unrelated space or increase capacity. Adjust retention for eligible records and resolve deliveries by resend/close after checking outcomes. Never delete SQLite files, stored bodies or unresolved records manually. Lowering limits cannot purge protected work. |

Only use `packie --bootstrap <installed-bootstrap> run` for foreground diagnosis after stopping
the service and with the same service-account access. On Linux the installed unit restricts
writes outside state/log directories and protects home directories: a folder that your login
account can use may still be inaccessible to the service. Check paths under the service's
actual permissions. Restore the service when diagnosis is complete.

## Audit, backups and change-control records

| Symptom | Likely cause | Check | Fix |
| --- | --- | --- | --- |
| `audit_unavailable`; commands or content views return `unavailable` | Audit open/write failed: disk full, permissions, mount or database problem | `<state>/audit` free space and ownership as the service account; `audit.unavailable` / `audit.write_failed`; `packie status` | Restore space/mount/permissions without deleting audit files. Audited commands retry on each request; maintenance retries every minute. The next successful write clears the alert. Message traffic continues, so assess missing audit evidence separately. |
| Audit has `requested` without a final outcome | Action is unfinished, engine stopped, or outcome write failed after it ran | Command's returned result, affected version/channel/delivery and `audit.write_failed` | Do not assume it failed and repeat a resend blindly. Establish the actual result, preserve evidence, then record the lab's resolution. |
| Audit verification fails | Damaged, altered, missing or foreign audit file | `packie audit verify` report; verify a preserved copy with `packie audit verify --dir <copy-audit-dir>`; compare outside `audit.anchor` records | Preserve all files/anchors and escalate for investigation. Do not edit events or delete a file to make verification pass. A valid shorter chain still needs comparison with a separately preserved anchor. |
| Audit export refuses an output path | File exists or parent directory is inaccessible | `--output` path, parent permissions and disk space | Use a new filename in a restricted evidence directory; existing files are never overwritten. Use `--format jsonl`, not `--json`, for JSON Lines export. |
| `backup_stale` | Latest successful backup exceeds the stale limit, or no backup succeeded within the interval after start | `packie backup status`: newest attempt/error, newest success, next run, mounted target, free space and service-account write access | Fix the cause, then run `packie backup create` or supply a new/empty target. A success clears the alert. Scheduled Linux service writes must fit its sandbox; see [backup setup](install.md#backup-and-restore). |
| Backup creation says no directory | Neither an explicit target nor `backup.directory` was supplied | Installed bootstrap path and backup settings | Set a protected encrypted root and restart, or run `packie backup create <new-or-empty-directory>`. A schedule also requires `backup.directory`. |
| Restore refused while engine runs, or without confirmation | Restore protects the active state; `--yes` is required | Correct bootstrap, service status and instance-lock holder | Stop the correct engine; run `packie backup verify <backup-directory>`, then `packie backup restore <backup-directory> --yes` under the required service/administrator access. Follow the full [restore procedure](install.md#backup-and-restore). |
| Backup verification fails | Incomplete copy, changed/missing files, corrupt store or broken audit chain | `packie backup verify <backup-directory>` problems; `manifest.json`; original backup/copy integrity | Preserve the suspect copy; fix transfer/storage or use another verified backup. Do not modify the manifest to conceal the mismatch or force a restore. |
| Apply returns `reason_required`, or CLI reports missing `--note` | A client omitted the reason or sent a blank/invalid note | Apply note: 1–500 characters; engine/console versions | Supply `packie config apply <file> --note "Change record reference and reason"`, or update/use a console that sends the required note. Missing CLI `--note` is a usage error before apply. |
| Version shows **Not verified since change** | No verification sign-off is recorded for that version | `packie config history`, channel apply outcomes, validation record | Complete the lab's tests and approval, then run `packie config verify <version> --note "Validation record reference"`. The command records account/time/reference; it does not run tests or repair a partial apply. |

Restores preserve replaced state in `<state>/pre-restore-<UTC time>/`. These PHI-bearing
copies can consume substantial space and are not removed by the scheduled backup count.
Review the lab's retention/hold requirements before removing them.

## Find log files

The console and `packie logs` query recent in-memory events. For events from an earlier engine
run, inspect the retained JSON-lines files in the configured log directory:

| OS | Package default log directory |
| --- | --- |
| Windows | `C:\ProgramData\Positive ACK\logs` |
| macOS | `/Library/Logs/Positive ACK` |
| Linux | `/var/log/packie` |

Files are named `packie.YYYY-MM-DD.log`, then `packie.YYYY-MM-DD.1.log`, `.2.log` and so on.
They rotate at local midnight or the size limit (64 MiB by default), and are kept for 30 days
by default. A custom bootstrap can change the directory, size and retention. If startup
fails before file logging begins, check the service manager's error output or diagnose in
the foreground after stopping the service.

## Collect support information without patient data

There is no automatic support-bundle export. Assemble a small, reviewed folder locally and
share only the parts needed for the fault. Use synthetic traffic to reproduce it when possible.

| Include | Keep out or review before sharing |
| --- | --- |
| Engine/console versions, OS/architecture, service scope, UTC incident window and observed symptom | Patient, sample, order and protocol control identifiers in descriptions, screenshots or terminal history |
| `service status` and `status` output, channel/endpoint ids, states, counters, retry times, ACK codes/levels and apply outcome/version | Review operator-chosen labels, paths, host names and addresses for identifying information |
| Narrowly filtered structured log events around the incident; preserve event names, generated message/delivery ids and error codes | Message bodies, file names containing patient identifiers, copied live transcripts, message-detail screenshots and `messages show --body` output |
| A redacted account of relevant endpoint/protocol settings, mapping rule/path and configured timeout | Raw configuration exports until reviewed for labels/paths; passwords, keys, secret files or OS key-store contents |
| Disk space and filesystem/service-account permission findings | The state directory, SQLite database and sidecars, `bodies/`, incoming/outgoing files and backups |
| Audit/backup failure codes, chain-verification problem types and redacted status summaries | Audit databases/exports, search terms, free-text audit details, backup manifests/paths and pre-restore state until reviewed for PHI |

Logs, snapshots and alerts are designed to exclude message bodies and protocol identifiers;
review the collected material anyway. Live transcripts and message-detail views can contain
PHI. Starting a transcript is opt-in, local and bounded; copying it puts its content on the
clipboard. Keep it out of a PHI-free support bundle. See
[diagnostics exclusions](../spec/diagnostics.md#8-what-never-appears).
