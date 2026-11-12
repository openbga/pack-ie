# Requirements

High-level, stable requirements for Positive ACK. Each line is an outcome, not a design;
[architecture.md](architecture.md) and `docs/spec/` say how. Version 1 is defined at the end.

## Users and setting

Small and mid-size clinical laboratories and the people who keep their interfaces running:
lab IT, a lab manager, an interface analyst at a support provider. One computer inside the lab
network runs the engine around the clock; staff open the console when something needs
attention or configuration. No cloud service, subscription or internet access is required.

## Engine and hosting

| ID | Requirement |
| --- | --- |
| ENG-01 | The engine runs unattended as an OS service on Windows, macOS and Linux, starting with the computer, and also runs in a console for development and troubleshooting. |
| ENG-02 | One failing channel or endpoint never stops the engine or unrelated channels. |
| ENG-03 | Configuration changes apply without restarting the engine; only the channels that changed are restarted. |
| ENG-04 | The engine exposes a local management interface that the console and command line use; it never requires a network port for management. |
| ENG-05 | The engine starts from its last valid configuration after a crash or power loss and reconciles unfinished work. |

## Endpoints

| ID | Requirement |
| --- | --- |
| EPT-01 | Serial ports (COM/tty) with baud, data bits, parity, stop bits and flow control; a port can be shared by the channels that need it and reopened automatically. |
| EPT-02 | TCP client (connect out) and TCP listener (accept in) with connect/idle timeouts, keepalive and an optional peer allowlist. |
| EPT-03 | Folders, local or on a lab share, watched for new complete files or polled on a schedule, as sources and as destinations. |
| EPT-04 | SFTP servers as a destination, and as a source where a partner publishes files. |
| EPT-05 | Virtual serial ports for testing and for software that only speaks COM. |
| EPT-06 | Every endpoint shows its state, bytes in and out, last activity and last error. |

## Bridge channels

| ID | Requirement |
| --- | --- |
| BRG-01 | A bridge connects two endpoints and passes bytes in both directions unchanged, so an instrument on a serial port can talk to a system expecting TCP, and vice versa. |
| BRG-02 | A bridge holds one live session, pairs the two sides according to a configured strategy, propagates a close from one side to the other and reconnects with backoff. |
| BRG-03 | Buffers are bounded per direction; pressure is visible and never silently drops data. |
| BRG-04 | A bridge may optionally frame its traffic (ASTM, HL7/MLLP) to count and record messages without altering the stream. |
| BRG-05 | An operator can pause and resume a bridge, and view a live, bounded transcript of its traffic. |

## Message channels

| ID | Requirement |
| --- | --- |
| MSG-01 | A message channel has one source (endpoint + protocol), optional filters and transformations, and one or more destinations (endpoint + protocol). |
| MSG-02 | Every received message is stored durably, with its original bytes, before the protocol acknowledgement is returned to the sender. |
| MSG-03 | Every message carries explicit states from receipt to delivery per destination (queued, sending, acknowledged, rejected, timed out, outcome unknown, failed, cancelled), so a transport acknowledgement is never shown as success. |
| MSG-04 | Delivery retries are bounded and visible; failed and unknown outcomes stay listed until resolved by resend or by closing with a reason. |
| MSG-05 | Operators can search messages by time, channel, state, identifiers and text, inspect the original and transformed content, and resend to one or all destinations. |
| MSG-06 | Duplicate messages are detected by source identity and content, and shown as such rather than processed twice. |
| MSG-07 | Transformations are declarative in version 1: field mapping between protocol models, code tables, constants, conditions, exact-decimal arithmetic (calculated and appended results); filters select messages by field values. No scripting language. |
| MSG-08 | Retention per channel: how long or how many messages to keep; purging never removes undelivered or unresolved messages. |

## Protocols

| ID | Requirement |
| --- | --- |
| PRO-01 | ASTM E1381/E1394 (CLSI LIS01-A2/LIS02-A2): receive results and send orders/queries over serial and TCP, with checksum, retransmission and contention handling. |
| PRO-02 | Olympus/Beckman AU400/AU640 fixed-width protocol over serial and TCP. |
| PRO-03 | HL7 v2.x over MLLP as client and as listener, with original and enhanced acknowledgement modes and per-message acknowledgement tracking. |
| PRO-04 | Delimited files (CSV and similar) in and out, with templates that describe columns and the order of records. |
| PRO-05 | Raw bytes (for bridges and for taps). |
| PRO-06 | Each protocol has a conformance corpus of synthetic messages, including malformed input, and never panics on any input. |

## Console

| ID | Requirement |
| --- | --- |
| CON-01 | A desktop application that attaches to the running engine; if none is running it says so and can start the installed service. |
| CON-02 | Dashboard: engine state, channels and endpoints with state coloring, counters, last activity, queue pressure, alerts; a failing channel is obvious within seconds of opening the app. |
| CON-03 | Channel and endpoint pages with detail, live traffic transcript, pause/resume, restart. |
| CON-04 | Messages page: a data grid with sensible default columns, filters that operators actually use (time range, channel, direction, state, identifier, text), paging that stays fast at hundreds of thousands of rows, detail view, resend. |
| CON-05 | Logs page: structured log viewer with filters by level, time, channel, endpoint and text; follows live. |
| CON-06 | Configuration editors for endpoints and channels, including mappings and filters, with validation before apply and a visible version history. |
| CON-07 | Organized navigation; every screen has loading, empty and error states; destructive actions confirm and explain. |
| CON-08 | Built-in help per page, short and accurate. |

## Logs and diagnostics

| ID | Requirement |
| --- | --- |
| LOG-01 | Structured logs with channel, endpoint and message identifiers, written to rotating files with retention, and available live through the management interface. |
| LOG-02 | Message bodies are not written to logs by default; a bounded traffic tap exists for live troubleshooting. |
| LOG-03 | Alerts for operator-relevant conditions (endpoint down past a threshold, deliveries failing, queue pressure, disk low) with a clear next action. |
| LOG-04 | Command-line status and log tail for the terminal. |

## Performance

| ID | Requirement |
| --- | --- |
| PRF-01 | A bridge adds under 2 ms (p99) of latency on the local network. |
| PRF-02 | A message channel sustains 100 messages per second with durable storage; an engine runs 50 channels; aggregate durable throughput across all channels is at least 500 messages per second (one SQLite writer bounds it; a laboratory's total traffic is far below this). |
| PRF-03 | The engine idles under 80 MB of memory; the console stays responsive with 500,000 stored messages. |

## Security (proportionate to a private lab network)

| ID | Requirement |
| --- | --- |
| SEC-01 | Listeners bind only to configured addresses; peer allowlists are available and on by default for listeners facing other hosts. |
| SEC-02 | The management interface is local to the computer and restricted to its user accounts by the OS. |
| SEC-03 | Secrets (SFTP passwords, keys) are stored in the OS key store where available, otherwise in a file readable only by the service account; never in logs or exports. |
| SEC-04 | Stored messages are PHI: database and log files live under the service's state directory with restrictive permissions, and the console never sends data off the computer. |

## Packaging

| ID | Requirement |
| --- | --- |
| PKG-01 | Installers for Windows, macOS and Linux that install the engine as a service and the console; one command installs or removes the service. |
| PKG-02 | Reproducible builds from this repository; version visible in console and command line. |

## Compliance and hardening (phase 1, version 0.2.0)

What a laboratory needs from the engine to meet its own HIPAA, CLIA and accreditation duties
(the [compliance guide](operations/compliance.md) maps these to the rules). The ids `P1`–`P12`
are used by [acceptance.md](spec/acceptance.md#12-phase-1-version-020).

| ID | Requirement |
| --- | --- |
| P1 | Every management request is attributed to the OS account of the connecting client, read by the engine from the connection and never supplied by the client; engine-initiated actions are attributed to the system. |
| P2 | An append-only, tamper-evident audit log, separate from the message store, records operator actions and content views; it can be listed, exported and verified, and an audited command is refused when its audit record cannot be written. |
| P3 | Management access is limited to a named OS group on every platform. |
| P4 | Every configuration version records who applied it, when, a required note and each channel's apply outcome; versions are kept without limit by default and can carry a verification sign-off. |
| P5 | Backups of the store, bodies, configuration history and audit log are consistent while the engine runs, verifiable by hash and restorable; a schedule and a staleness alert exist; secret values are never included. |
| P6 | Message bodies, parsed fields and identifier or text searches are explicit actions, each audited; browsing by channel, state or time is not. |
| P7 | A transmission verification report shows, per channel and period, what was received, delivered and acknowledged, with content hashes. |
| P8 | Retention can cover decades (audit log default 6 years); a message that a kept message revises is kept with it. |
| P9 | Selected messages can be exported (metadata, optionally bodies), audited. |
| P10 | Resending a message that a newer revision supersedes is refused unless the operator explicitly overrides it, and the override is audited. |
| P11 | The console conceals message content after an idle timeout. |
| P12 | An operator guide documents where PHI lives, what the product provides per safeguard and what the laboratory must do. |

## Version 1

Version 1 is done when one computer can run the engine as a service and an operator, using
the console, can: bridge a serial instrument to a TCP listener and watch its traffic; receive
ASTM and AU results into stored messages; send and receive HL7 v2 over MLLP with tracked
acknowledgements; import and export delimited files; map fields between an instrument message
and an HL7 result; see at a glance which channels are unhealthy and why; search, inspect and
resend messages; read the logs; and change configuration without restarting. All on Windows,
macOS and Linux, from installers built by this repository.

Out of scope for version 1 (recorded in [decisions.md](decisions.md)): a scripting language
for transformations, FHIR, a remote management service, TLS on MLLP, a terminal UI.
