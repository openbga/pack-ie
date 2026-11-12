# Version 1 acceptance

The checks that decide whether version 1 is done, derived from the "Version 1" section of
[requirements.md](../requirements.md) and from behavior in the other specs. Each row is one
scenario and the outcome an observer sees, with the requirement ids it covers. Names in
backticks are files in [`tests/scenarios/`](../../tests/scenarios/) described in
[scenarios.md](scenarios.md).

**Verified by** values: a scenario name; `crate: <name>` (the crate's own tests, including
conformance corpora); `console smoke` (page smoke journey against a simulator-backed engine,
[testing.md](../testing.md)); `benchmark` (the `packie-sim` benchmark scenario, run at the
gate); `install test` (per-OS installer check); `inspection` (a reviewed property such as a
permission or an absent port).

## 1. Version 1 journey

The release sentence of requirements.md, clause by clause.

| Clause | Done when | Rows |
| --- | --- | --- |
| Run the engine as a service | One computer runs it unattended on Windows, macOS and Linux | A-01, K-01 |
| Bridge a serial instrument to a TCP listener and watch its traffic | Bytes pass both ways; the transcript shows them | C-02, C-09, C-10 |
| Receive ASTM and AU results into stored messages | Committed before any ACK; visible in Messages | D-02, E-01, E-03 |
| Send and receive HL7 v2 over MLLP with tracked acknowledgements | Each delivery shows what was acknowledged | E-04, D-05 |
| Import and export delimited files | Files in become messages; messages become files | E-05 |
| Map fields between an instrument message and an HL7 result | Declarative mapping delivers an HL7 result | D-13, D-14 |
| See at a glance which channels are unhealthy and why | Dashboard ranks and explains | H-02, G-04 |
| Search, inspect and resend messages | Grid, detail, resend | D-09, D-10, D-11 |
| Read the logs | Logs page and `packie logs` | G-02, G-07, H-06 |
| Change configuration without restarting | Per-channel apply | A-04, F-01 |
| On Windows, macOS and Linux, from installers built here | | K-01, K-02 |

## 2. Engine and hosting

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| A-01 | Install the service, reboot the computer | The service is running after boot without anyone signed in; `packie service status` says so | ENG-01, PKG-01 | install test |
| A-02 | `packie run` in a terminal, then Ctrl+C | Runs in the foreground; on stop each channel stops, `channel.stopped` and `node.stopped` are logged, final stopped snapshot is published | ENG-01 | `crate: packie` |
| A-03 | One channel fails (refused endpoint, retrying) while a second channel carries traffic | The second channel is unaffected; the node is `degraded`, not `stopped` | ENG-02 | `bridge-reconnect`, `crate: packie-routing` |
| A-04 | Apply a document that changes one of two channels | Only the changed channel restarts; the other keeps its connection and instance; outcomes listed per channel | ENG-03 | `config-apply-per-channel`, `bridge-listener-allowlist` |
| A-05 | Inspect the process's open sockets with all channels running | Only configured listeners exist; no management port; management works over the local socket or pipe | ENG-04, SEC-02 | inspection |
| A-06 | Kill the engine while a delivery is in flight and a message is queued, restart | Acknowledged messages are stored; the in-flight delivery is `outcome_unknown`; the queued one is delivered next; nothing is resent automatically | ENG-05 | `msg-crash-restart-reconcile` |
| A-07 | Kill the engine mid-ASTM-transfer, twice | Frames committed before the crash survive; an incomplete transfer becomes `aborted`; a complete one is recovered and routed | ENG-05, MSG-02 | `msg-astm-interrupted-transfer` |
| A-08 | Start with an unreadable or invalid `runtime.json` | The engine starts from the newest valid stored version and raises `config_rejected`; with none it runs with zero channels and a degraded node | ENG-05 | `crate: packie-config` |
| A-09 | Start with an invalid `bootstrap.json` | The engine refuses to start, prints every error, exits with code 2 | ENG-01 | `crate: packie-config` |
| A-10 | Stop the engine while a bridge has bytes queued | Queued bytes are delivered within the drain budget before the connections close; a peer that drops during the drain causes no error or retry | ENG-01, BRG-01 | `crate: packie-routing` |

## 3. Endpoints

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| B-01 | Serial endpoint with a virtual pair; unplug and replug | Opens with the configured settings; unplug ends the session and reports the serial failure; replug recovers | EPT-01, EPT-05 | `bridge-serial-tcp` |
| B-02 | Idle serial line for longer than several read timeouts | The endpoint stays `connected`; no failure is counted | EPT-01 | `bridge-serial-tcp` |
| B-03 | A second channel references a serial port already held | Its attempt fails with "in use by channel"; it opens after the first channel pauses or stops; the validator warns | EPT-01 | `crate: packie-transport` |
| B-04 | Virtual serial link for software that only opens COM or tty | The engine exposes a pseudo-terminal path and bytes flow to and from it | EPT-05 | `crate: packie-transport` |
| B-05 | TCP client to an unreachable or late host | Connect failures count toward the streak, back off, alert, and succeed when the host appears | EPT-02 | `bridge-reconnect` |
| B-06 | TCP keepalive and idle timeout | A silent half-open peer is dropped by keepalive; `idle_timeout_seconds` drops a silent peer and the listener keeps listening | EPT-02 | `crate: packie-transport` |
| B-07 | Listener with an allowlist | A peer outside it is reset before any byte is read, `peer_rejected` is raised, channel state is unchanged; the peer is accepted after the allowlist is changed | EPT-02, SEC-01 | `bridge-listener-allowlist` |
| B-08 | Listener binds only the configured address | Connections to other local addresses are refused | EPT-02, SEC-01 | `crate: packie-transport` |
| B-09 | Watched folder source: file written slowly, partial suffix, then complete | A file is read only once complete (stable size, no ignored suffix, no writer lock) and then moved to `processed` | EPT-03 | `crate: packie-transport`, `msg-hl7-out-ack-tracked` |
| B-10 | Polled folder and a share path | New files are found by polling at the configured interval | EPT-03 | `crate: packie-transport` |
| B-11 | Folder destination | Files appear complete (rename after write); existing names follow `on_exists` | EPT-03 | `msg-hl7-in-ack`, `crate: packie-files` |
| B-12 | SFTP destination and source against a local test server | Upload and rename complete; a changed host key puts the endpoint in `error` (`host_key_mismatch`); polling finds partner files | EPT-04, SEC-03 | `crate: packie-transport` |
| B-13 | Look at any endpoint during traffic and failure | State, bytes in and out, last activity, last error and failure streak are shown and update | EPT-06 | `bridge-tcp-full-duplex`, `bridge-reconnect`, `console smoke` |

## 4. Bridge channels

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| C-01 | Listener to client bridge, text and 64 KiB each way | Bytes arrive unchanged and in order; counters equal the bytes sent; state `running`, session `bridged` | BRG-01 | `bridge-tcp-full-duplex` |
| C-02 | Serial instrument to TCP system | Binary data (including all byte values) passes unchanged both ways | BRG-01, EPT-05 | `bridge-serial-tcp` |
| C-03 | Second peer connects while a session is active | The newcomer is reset, the session continues untouched, an instrument reconnecting during backoff is accepted by the next attempt | BRG-02 | `bridge-listener-single-session` |
| C-04 | `receiver_then_sender` with an idle instrument for longer than the pairing timeout | No failure, no retry count; the sender-side listener is not bound until the receiver attaches | BRG-02 | `bridge-receiver-then-sender` |
| C-05 | `receiver_then_sender`, receiver attached, sender never | After `pairing_timeout_ms` the receiver is closed, the failure is attributed to the sender side with "pairing timeout", the channel retries | BRG-02 | `bridge-receiver-then-sender` |
| C-06 | `parallel` and `wait_for_listener_then_pair` strategy matrix | The first attach is unbounded, the second is bounded; the compatibility strategy behaves like `parallel` | BRG-02 | `crate: packie-routing` |
| C-07 | One side closes | The other side is closed, `bridge.close_propagated` is logged, the channel retries and recovers with a new session | BRG-02 | `bridge-close-propagation` |
| C-08 | Remote system starts late, instrument disconnects and returns | Backoff with visible retry count and next retry; `endpoint_down` after the streak; counters reset when the session is stable | BRG-02, EPT-06 | `bridge-reconnect` |
| C-09 | Operator pauses then resumes a bridge | `stopped` with `operator_paused`, no retry, listener released, node still healthy; resume restarts from the active definition; wrong-state commands are rejected with reasons | BRG-05, CON-03 | `bridge-pause-resume` |
| C-10 | Open the live transcript on a bridge, then close it | Timestamped bytes per direction appear in bounded panes; nothing is captured with no viewer; the ring is dropped when the last viewer leaves; a second viewer is rejected | BRG-05, LOG-02 | `crate: packie-routing`, `console smoke` |
| C-11 | Receiver is slow (stops reading) | Buffer fills, channel degrades once, alert raised, sender held back, no byte lost, recovery when drained | BRG-03 | `bridge-overflow-degrade` |
| C-12 | HL7 tap on a bridge | Messages counted and stored per direction as `observed`, stream bytes unchanged, repeats not deduplicated | BRG-04, PRO-05 | `bridge-tap-hl7` |
| C-13 | ASTM tap on a bridge with garbage and over-long input | Framer resynchronizes, counts framing errors, the stream is unaffected | BRG-04 | `crate: packie-routing` |
| C-14 | Bridge latency on loopback | p99 added latency under 2 ms | PRF-01 | `benchmark` |

## 5. Message channels

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| D-01 | Channel with one source, filters and several destinations | Accepted by validation; each destination gets its own delivery and outcome | MSG-01 | `msg-hl7-in-ack`, `crate: packie-config` |
| D-02 | ASTM transfer with store commits held | No frame is acknowledged until the commit; after release the message is `routed` and delivered | MSG-02, PRO-01 | `msg-astm-receive-ack-after-commit` |
| D-03 | Commit failure while receiving HL7 | No positive acknowledgement (`AE` or `CE`); `message_store_unavailable` raised; recovers when commits work | MSG-02 | `crate: packie-routing` |
| D-04 | HL7 in, original then enhanced mode | `AA` after commit; `CA` then `AA` in enhanced mode; ACK echoes the control id | MSG-02, PRO-03 | `msg-hl7-in-ack` |
| D-05 | Invalid HL7 message | Stored as `invalid`, answered `AR`, no deliveries | MSG-03 | `msg-hl7-in-ack` |
| D-06 | Message excluded by a source filter | Acknowledged, stored as `filtered` with its reason, no deliveries | MSG-07 | `msg-hl7-in-ack` |
| D-07 | Four deliveries answered AA, AR, silence, connection drop | `acknowledged` (accepted), `rejected`, `timed_out`, `outcome_unknown`; attempts show the codes; a single alert counts the three needing attention | MSG-03, MSG-04, PRO-03 | `msg-hl7-out-ack-tracked` |
| D-08 | Look at a delivery that reached only a lower acknowledgement level | The label says what was proven ("Written to the destination. Not confirmed by it."); no "delivered" or "success" wording | MSG-03, CON-04 | `console smoke` |
| D-09 | Destination fails writes or is unreachable | Write failures retry with backoff up to `max_attempts` then `failed`; an unreachable destination consumes no attempts and its queue waits in order | MSG-04 | `crate: packie-routing`, `msg-pause-resume` |
| D-10 | Unresolved deliveries | They stay listed and block purging until resent and acknowledged or closed with a reason | MSG-04, MSG-08 | `msg-resend`, `msg-crash-restart-reconcile` |
| D-11 | Search 500,000 stored messages by time, channel, state, identifier and text | Filtered pages (offset and limit, with the total count) return fast at any depth | MSG-05, CON-04, PRF-03 | `benchmark`, `crate: packie-store` |
| D-12 | Open a message | Original bytes, parsed fields, transformed bytes, deliveries and attempts are shown | MSG-05 | `console smoke` |
| D-13 | Resend a rejected delivery, an acknowledged one, and as a new message | Same-message resend makes a new attempt cycle on the existing delivery; resending an acknowledged delivery needs confirmation; new-message resend creates a linked message through the pipeline | MSG-05 | `msg-resend` |
| D-14 | Close a rejected delivery | A reason is required; the delivery becomes `cancelled` and the alert clears | MSG-04 | `msg-resend` |
| D-15 | Same message repeated | Acknowledged and recorded as `duplicate`, not delivered again; same identity with new content is processed with a revision link | MSG-06 | `msg-duplicate` |
| D-16 | Mapping, code table, datetime and constants | HL7 result becomes a CSV row; delimited rows become HL7 ORU; instrument ASTM becomes HL7 | MSG-07, PRO-04 | `msg-hl7-in-ack`, `msg-hl7-out-ack-tracked`, `crate: packie-transform` |
| D-17 | Mapping error (unmapped code) | Message kept and acknowledged; delivery `failed` with `transform_error`; alerts raised; an in-place mapping fix and a resend deliver it; other deliveries unaffected | MSG-07, MSG-03 | `msg-transform-error` |
| D-18 | Retention with unresolved messages | Old acknowledged messages are purged by age and count; unresolved ones are never purged; purges are logged without content | MSG-08 | `crate: packie-store` |
| D-19 | Pause a message channel with a queued delivery, resume | Source stops accepting (sender refused), queue is intact, resume delivers in order; receipt continues while a destination is down | MSG-04, CON-03 | `msg-pause-resume` |
| D-20 | Deliveries to one destination | They are sent in receipt order, one at a time; an unresolved one does not block later ones | MSG-03 | `msg-hl7-out-ack-tracked`, `crate: packie-routing` |

## 6. Protocols

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| E-01 | ASTM receive over TCP and serial: good, bad checksum, retransmission, contention, timeouts | Checksummed frames are acknowledged after commit, NAK triggers retransmission, limits abort with a reason | PRO-01 | `msg-astm-receive-ack-after-commit`, `crate: packie-astm` |
| E-02 | ASTM send of orders and queries | The engine runs the sender side (ENQ, frames, EOT) and records `received` on completion | PRO-01 | `msg-astm-receive-ack-after-commit`, `crate: packie-astm` |
| E-03 | AU400/AU640 class A and B over serial and TCP; send worklists | Class B ACKs follow durable commit; incomplete samples abort; settings are frozen per receipt | PRO-02 | `crate: packie-au` |
| E-04 | HL7 MLLP client and listener in both acknowledgement modes | Per-message acknowledgement tracking; mismatched acknowledgements are ignored and logged | PRO-03 | `msg-hl7-in-ack`, `msg-hl7-out-ack-tracked` |
| E-05 | Delimited files in and out, grouped records, quoting | Columns and order as templated; completion detection; oversize file aborted | PRO-04 | `msg-hl7-out-ack-tracked`, `crate: packie-files` |
| E-06 | Raw bytes through bridges and taps | No parsing; counters and observed messages work with a framer | PRO-05 | `bridge-tcp-full-duplex`, `bridge-tap-hl7` |
| E-07 | Conformance corpora including malformed and truncated input; property tests on framing and escaping | Typed errors for malformed input, never a panic, for every protocol | PRO-06 | `crate: packie-astm`, `packie-au`, `packie-hl7`, `packie-files` |

## 7. Configuration

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| F-01 | Validate and apply a document with errors | Every problem returned with code and path; nothing changes; `config_rejected` raised; the previous configuration stays active; a later good apply clears it | CON-06, ENG-03 | `config-apply-per-channel`, `crate: packie-config` |
| F-02 | Version history, comparison, rollback and a stale `base_version` | Versions listed with time, origin and result; rollback is a new version; a stale base is rejected as `version_conflict` | CON-06 | `crate: packie-config`, `console smoke` |
| F-03 | Inline secret in a document; resolving secrets | Rejected as `secret_inline`; references resolve at connect time; values never reach logs, snapshots or exports | SEC-03 | `crate: packie-config` |
| F-04 | Power loss while writing configuration | `runtime.json` is either the old or the new complete document | ENG-05 | `crate: packie-config` |
| F-05 | Non-loopback listener without allowlist | Rejected as `allowlist_required` unless `allow_any_peer` is set | SEC-01 | `crate: packie-config` |
| F-06 | Channel-only changes (label, mapping, filters) | Applied in place; the instrument connection survives | ENG-03 | `config-apply-per-channel`, `msg-transform-error` |

## 8. Diagnostics, logs and alerts

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| G-01 | Subscribe to snapshots during traffic and failure | Node, channel, endpoint and delivery queue fields are complete, published every second and on change, consistent with each other | LOG-01, EPT-06, CON-02 | `crate: packie-diagnostics`, all scenarios |
| G-02 | Logs during a run | JSON lines with channel, endpoint and message ids; rotating files with retention; live ring with filters; each transition logged once | LOG-01 | `crate: packie-diagnostics`, `bridge-close-propagation` |
| G-03 | Messages carrying sentinel patient values through every path | The sentinels appear in no log, snapshot, alert or error | LOG-02 | `msg-astm-receive-ack-after-commit`, `msg-hl7-in-ack`, `msg-resend`, `bridge-tap-hl7` |
| G-04 | Alerts raised and cleared | `endpoint_down`, `peer_rejected`, `bridge_buffer_pressure`, `delivery_needs_attention`, `transform_failed`, `config_rejected` are raised with severity and next action, and clear with the condition | LOG-03 | `bridge-reconnect`, `bridge-listener-allowlist`, `bridge-overflow-degrade`, `msg-resend`, `msg-transform-error`, `config-apply-per-channel` |
| G-05 | Low disk and unavailable store | `disk_low` (warning, critical) and `message_store_unavailable` raised; unresolved messages are never purged to make room | LOG-03 | `crate: packie-diagnostics` |
| G-06 | Backlog, invalid-message burst, idle listener | `delivery_backlog`, `invalid_messages`, `no_peer_connected` raised at their thresholds and cleared | LOG-03 | `crate: packie-diagnostics` |
| G-07 | `packie status`, `packie channels pause\|resume\|restart`, `packie messages list\|show\|resend\|close`, `packie logs --follow` | Show engine, channel and endpoint state, pause, resume and restart a channel, show recent messages with their deliveries, resend and close from the terminal, and the live log | LOG-04 | `crate: packie` (`tests/e2e.rs`) |
| G-08 | Node state through failure, pause and recovery | `degraded` while a channel retries or degrades; `healthy` when channels are running or intentionally stopped | CON-02 | `bridge-reconnect`, `bridge-pause-resume`, `bridge-tcp-full-duplex` |
| G-09 | Any endpoint failure | `last_error_summary` follows the format: channel, endpoint role and label, operation, cause, next action; never a payload | LOG-03 | `bridge-close-propagation`, `bridge-serial-tcp`, `bridge-receiver-then-sender` |

## 9. Console

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| H-01 | Open the console with no engine running, then with one | It says so and offers to start the installed service; with an engine it attaches and reconnects after an engine restart | CON-01 | `console smoke`, `manual` |
| H-02 | Dashboard with a retrying, a degraded, a healthy and a paused channel | Ordered as in operator-console.md section 3; states carry text and shape, not color alone; the failing channel is obvious | CON-02 | `console smoke` |
| H-03 | Channel and endpoint pages | Required fields for each kind are present; errors show full context; paused reads as intentional | CON-03 | `console smoke` |
| H-04 | Pause, resume, restart from the console | Confirmations explain the effect; outcomes (accepted, rejected with reason) are shown in words | CON-03, CON-07 | `console smoke` |
| H-05 | Messages grid | Default columns and sort, filters (time, channel, direction, state, identifier, text), quick filters, detail, resend dialog | CON-04 | `console smoke` |
| H-06 | Logs page | Filters by level, time, channel, endpoint, text; follows live; jump to live; no bodies | CON-05 | `console smoke` |
| H-07 | Configuration editors | Endpoint and channel editors including mappings and filters; validation before apply; plan shown; version history; secrets write-only | CON-06 | `console smoke` |
| H-08 | Every page with loading, empty and error states; destructive actions | States render; destructive actions confirm and explain | CON-07 | `console smoke` |
| H-09 | Help on every page | A short panel exists and matches the behavior | CON-08 | `console smoke` |

## 10. Performance, security, packaging

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| I-01 | Bridge latency benchmark | p99 added latency under 2 ms on loopback | PRF-01 | `benchmark` |
| I-02 | 50 message channels, one at 100 messages per second, durable | Sustained, every message committed before its acknowledgement | PRF-02 | `benchmark` |
| I-03 | Idle engine; console with 500,000 messages | Engine under 80 MB resident; console stays responsive | PRF-03 | `benchmark`, `console smoke` |
| J-01 | Listener exposure | Binds only configured addresses; allowlists enforced and required off loopback | SEC-01 | `bridge-listener-allowlist`, `crate: packie-config` |
| J-02 | Management interface access | Socket or pipe restricted to the OS user accounts; no TCP port | SEC-02 | inspection |
| J-03 | Secret storage | Keystore or owner-only file; never in documents, logs, exports | SEC-03 | `crate: packie-config` |
| J-04 | State and log directories; console network activity | Owner-only permissions; the console makes no network request off the computer | SEC-04 | inspection |
| K-01 | Install and remove on each OS | One command installs or removes the service; the installer adds the console | PKG-01 | install test |
| K-02 | Rebuild from the repository; check versions | Reproducible build; version shown by `packie --version` and in the console About page | PKG-02 | `inspection`, `console smoke` |

## 11. Coverage of requirement ids

Every id of requirements.md appears in at least one row above:

| Area | Ids | Rows |
| --- | --- | --- |
| Engine | ENG-01..05 | A-01..A-10 |
| Endpoints | EPT-01..06 | B-01..B-13 |
| Bridge channels | BRG-01..05 | C-01..C-14 |
| Message channels | MSG-01..08 | D-01..D-20 |
| Protocols | PRO-01..06 | E-01..E-07 |
| Console | CON-01..08 | H-01..H-09 |
| Logs | LOG-01..04 | G-01..G-09 |
| Performance | PRF-01..03 | C-14, D-11, I-01..I-03 |
| Security | SEC-01..04 | A-05, B-07, B-08, F-03, F-05, J-01..J-04 |
| Packaging | PKG-01..02 | A-01, K-01, K-02 |

## 12. Phase 1 (version 0.2.0)

The compliance and hardening features of phase 1. Requirement ids `P1`–`P12` are defined in
[requirements.md](../requirements.md#compliance-and-hardening-phase-1-version-020) (not to be
confused with the scenario ids `P-01`..`P-05` of §12.5); the other rows extend version 1
requirements. Additional **Verified by** values: `cli` (the
command run against a release engine on a real state directory), `live` (the console's live
suites against a release engine), `soak` (a `packie-sim soak` report).

### 12.1 Attribution and access

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| L-01 | Open the console and run the CLI as an OS account of the management group | The console header reads "Signed in as `<account>`" from the engine's hello; CLI commands work | P1 | console `connection`, `live`, `cli` |
| L-02 | One change of every audited kind (apply, rollback, verify, channel commands, resend, close, secret set and delete, backup, export, body view, endpoint test, notification test) | Every audit event names the acting OS account; none lacks an actor | P1, P2 | `crate: packie-engine`, `cli` |
| L-03 | A management connection whose OS identity cannot be read | Refused before any request runs; recorded once as `access_refused` with actor `unknown` | P1 | `crate: packie-ipc` (`a_connection_without_an_identity_is_refused_before_any_request`), `crate: packie-engine` (`a_refused_connection_is_one_unattributed_access_refused_event`) |
| L-04 | Management access by group | Unix socket `0660` for the owner and `ipc.group` (`0600` when the group does not exist, `ipc.group_unavailable` logged); the Windows pipe admits SYSTEM, Administrators and the `ipc.group` only | P3, SEC-02 | inspection, `crate: packie-host` |

### 12.2 Audit log

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| M-01 | Every action of [audit.md §3](audit.md#3-actions) | `requested` and outcome events with action, actor, object and details as listed; plain browsing, snapshots and logs write nothing | P2, P6 | `crate: packie-engine`, `crate: packie-audit`, `cli` |
| M-02 | The audit log cannot be written while an operator applies a change | The command is refused (`unavailable`, fail closed) and nothing changes; `audit_unavailable` is raised; writes resume when the log recovers | P2 | `crate: packie-audit`, `crate: packie-engine` |
| M-03 | `packie audit verify` on the log and on copies with an edited, a removed and a reordered event | The intact log verifies; each tampering is reported with the first bad sequence; the database refuses updates and deletes | P2 | `crate: packie-audit`, `cli` |
| M-04 | `packie audit list` and `packie audit export` (CSV, JSON Lines) | Filters and paging newest first; exports oldest first; the export and the verification are audited | P2 | `crate: packie-audit`, `cli` |
| M-05 | The turn of a year, then retention | The year's file is sealed read-only (`0400`) and verifies on its own; the chain continues; sealed files past `audit.retention_days` are removed whole after an `audit_prune` event | P2, P8 | `crate: packie-audit`, inspection |
| M-06 | Audit page | Events grid with filters and detail; export; chain verification with the result in words | P2 | console `audit`, `auditGrid` |
| M-07 | Message search by identifier or text, body and parsed-field views | Each is an explicit, audited view (`message_search`, `message_body_view`, `message_fields_view`); browsing by channel, state or time is not | P6 | `crate: packie-engine`, console `messageDetail` |
| M-08 | Recording one audit event, timed on an otherwise idle machine: `cargo test -p packie-audit --test audit -- --ignored recording_an_event_costs_under_a_millisecond_at_p99` | p99 under 1 ms (the guard is ignored in ordinary test runs because other load on the machine makes it fail) | P2 | `crate: packie-audit` (explicit run) |

### 12.3 Change control

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| N-01 | Apply or roll back a document without a note or with a blank one, then with one | The CLI requires `--note`; a blank note is refused by the engine (`reason_required`, audited `rejected`); with a note the version records account, time and note | P4 | `crate: packie-config`, `crate: packie-engine`, console `changeControl`, `cli` |
| N-02 | Apply a document that changes some channels | The version keeps each channel's apply outcome; `packie config history` and the console show them | P4 | `crate: packie-engine`, `cli` |
| N-03 | Sign off a saved version as verified | `packie config verify` and the console record account, time and note; audited as `config_verify` | P4 | `cli`, console `changeControl` |
| N-04 | Compare and report changes | `packie config diff` lists changes field by field; the console exports a change report for a range of versions | P4 | `cli`, console `changeReport` |
| N-05 | Many versions | With `runtime.versions_kept` null (the default) no version is removed | P4 | `crate: packie-config` |

### 12.4 Backup and restore

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| O-01 | `packie backup create` on a running engine with traffic | A consistent copy of the store, bodies, configuration history and audit log with a SHA-256 manifest; channels keep running; audited | P5 | `crate: packie-backup`, `cli` |
| O-02 | `packie backup verify` on a good backup and on damaged copies | The good one verifies; changed, missing, extra and incomplete files are reported; nothing is written | P5 | `crate: packie-backup`, `cli` |
| O-03 | `packie backup restore` | Refused while an engine runs and for a bad backup; otherwise restores messages, bodies, configuration and the audit chain transactionally, keeping the previous state aside; the restored engine starts and serves them | P5 | `crate: packie-backup`, `cli` |
| O-04 | A secrets file kept with the configuration | Never copied into a backup; after a restore the secrets are set again | P5, SEC-03 | `crate: packie-backup`, `cli` |
| O-05 | Schedule and staleness | Scheduled backups run daily at the configured time; `packie backup status` lists the newest attempts; `backup_stale` is raised when the newest good backup is too old and cleared by a good one | P5 | `crate: packie-engine`, `cli`, console `backup` |

### 12.5 Alert notifications

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| P-01 | An alert is raised and cleared with an e-mail relay and a webhook configured | One e-mail (subject `[PACK-IE <node>] <SEVERITY> <code> raised` or `cleared`) and one JSON `POST` per change; only the fields of [diagnostics.md §6.1](diagnostics.md#61-notifications), nothing from messages | LOG-03 | `crate: packie-notify`, `cli` (local SMTP and HTTP sinks) |
| P-02 | An alert that flaps | At most one message per alert per `rate_limit_minutes`; suppressed changes are logged at debug | LOG-03 | `crate: packie-notify` |
| P-03 | `packie notify test` and the console's Notifications tab | Every target is named with accepted or why not; audited as `notification_test` | LOG-03 | `cli`, console `notifications` |
| P-04 | A target that cannot be reached | `notification.failed` is logged; `notifications_failing` names the target; a later successful send clears it | LOG-03 | `crate: packie-notify`, `cli` |
| P-05 | `notifications.ca_file` | A private CA is trusted for TLS targets; an unusable file fails every TLS send naming the file and leaves other targets alone | LOG-03 | `crate: packie-notify` |

### 12.6 Setup checks

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| Q-01 | `packie endpoints list-serial` and the console's port picker | The OS's serial ports with bus kind and USB ids where known, each with the configured endpoints that use it (`used_by`) | EPT-01 | `crate: packie-transport`, `cli`, console `serialPorts` |
| Q-02 | Test an endpoint of every kind (serial, TCP client, TCP listener, folder, SFTP) | It opens, connects, binds, reads the folder (proving it writable with one hidden empty file it deletes again) or signs in and lists, within its time limit, and never sends data to a peer; a port a channel holds is reported without opening it | EPT-01..04 | `crate: packie-transport`, `cli` (loopback peers), console `endpointTest` |
| Q-03 | Any endpoint test | Audited once as `endpoint_test` with kind and target, never a secret | P2 | `crate: packie-engine`, `cli` |

### 12.7 Exports and reports

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| R-01 | `packie messages export` with filters, without and with `--bodies`; the console's export | CSV or JSON Lines, oldest first; bodies only when asked; never overwrites a file; audited as `message_export` with the count | P9 | `crate: packie`, `crate: packie-engine`, `cli`, console `messages.export` |
| R-02 | `packie messages verify` for a channel and period; the Transmission report page | Per destination what was received, delivered and acknowledged, with content hashes and the acknowledgement's own control id; totals count every delivery of the period; audited as `transmission_verify` | P7 | `crate: packie-engine`, `cli`, console `transmission` |
| R-03 | A report or export beyond its row limit | It says it is truncated and to choose a shorter period, in the console, JSON and CSV | P7, P9 | `crate: packie-engine`, `crate: packie`, `cli` |

### 12.8 Resend safety, idle protection, tap retention

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| S-01 | Resend a message that a newer revision supersedes | Refused naming the newer message and its time; `--superseded-ok` (CLI) or the console's confirmation overrides; the override is audited | P10 | `crate: packie-store`, console `messages.superseded`, `cli` |
| T-01 | The console is left idle past its idle timeout | Message content and searched identifiers are concealed until the operator returns | P11 | console `idle` |
| U-01 | A bridge tap records messages with `tap.retention` | Recorded messages beyond the age or count limit are purged by the retention run; the default keeps 30 days | MSG-08, BRG-04 | `crate: packie-routing`, `crate: packie-config` |

### 12.9 Performance and endurance

| ID | Scenario | Expected observable outcome | Requirements | Verified by |
| --- | --- | --- | --- | --- |
| V-01 | Bridge latency, 1 and 50 bridges | Added p99 under 2 ms | PRF-01 | `benchmark` |
| V-02 | Durable throughput, 1 channel and 50 channels with `--assert` | At least 100 msg/s for one channel and 500 msg/s aggregate for 50; with phase 1 about 1,250 msg/s and 2,450 msg/s, commit p99 under 50 ms at 50 channels | PRF-02 | `benchmark` |
| V-03 | Idle engine | Under 80 MB resident; with 50 running channels and no traffic under 3 % of one core | PRF-03 | `benchmark`, inspection |
| W-01 | Eight hours of steady load with chaos: `kill -9` every hour, a blocked store, destinations down for minutes, configuration applied every four minutes | Every assertion of the soak report passes: no message lost or duplicated, each fault handled as specified, memory and file descriptors bounded, the final reconciliation complete | ENG-02, ENG-05, MSG-02, MSG-04 | `soak` |

### 12.10 Coverage of phase-1 ids

| Id | Rows |
| --- | --- |
| P1 attribution | L-01..L-03 |
| P2 audit log | L-02, M-01..M-06, M-08, Q-03 |
| P3 management group | L-04 |
| P4 change control | N-01..N-05 |
| P5 backup and restore | O-01..O-05 |
| P6 audited views | M-01, M-07 |
| P7 transmission verification | R-02, R-03 |
| P8 retention | M-05 |
| P9 export | R-01, R-03 |
| P10 resend safety | S-01 |
| P11 idle protection | T-01 |
| P12 compliance documentation | `docs/operations/compliance.md` (reviewed, not a test row) |
