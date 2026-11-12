# Laboratory compliance and interface validation

Use this guide to assign responsibilities, validate interfaces and write the laboratory's
operating procedures. It covers US federal requirements as the sources stood on 2026-11-06.
Source IDs such as `H1` link to the [source list](compliance-sources.md), which records
the date, status and verification of each source and where it is uncertain.
The laboratory decides which federal, state, accreditation and contractual rules apply.
PHI means protected health information; ePHI is its electronic form. A protocol
acknowledgement is abbreviated ACK.

This guide describes the current engine controls, including OS-user attribution, the audit
log, change-control records and backups. Check `packie --version` and the console version when
following it. Transmission reports and message export are available in the console and the CLI.

## 1. Intended use and responsibilities

PACK-IE runs on one laboratory-controlled computer. It transfers, stores, converts formats and
displays laboratory data and applies the formulas the laboratory defines. It has no built-in
interpretation, abnormal flags or autoverification and does not decide whether to release a
result. Operational alerts concern
connections, queues, storage and delivery. Do not configure result-value filters to make clinical decisions.

The FD&C Act §520(o)(1)(D) excludes software that only transfers, stores, converts formats
or displays laboratory data and results, unless it interprets or analyzes them. FDA's guidance
places a laboratory information system's transfer, storage, format conversion and display
outside the device definition, and treats conversion "in accordance with a preset
specification" as non-device data handling. Functions that analyze data to give a notification
or flag, such as an out-of-range alert, or that raise alarms or prioritize patient information
remain device functions. [[F1]][F1] [[F2]][F2] [[F3]][F3]

What follows for configuration: routing, holding, dropping or alerting on result values,
abnormal flags, reference ranges, delta checks and autoverification are not provided and
should not be built with filters or mappings. Mapping a test code, renaming a unit string,
changing a date format and adding administrative constants are format conversion. Rewriting
a clinical value through a code table or constant modifies data; it is the laboratory's
configuration and carries the laboratory's responsibility. Operator-defined arithmetic
(unit factors, calculated tests) falls near 21 CFR 862.2100, "Calculator/data processing
module for clinical use" (Class I, exempt from premarket notification subject to §862.9);
this guide draws no classification conclusion for it. [[F5]][F5]

The laboratory is the HIPAA covered entity and CLIA certificate holder. Installing PACK-IE does
not establish compliance. The lab owns risk analysis, workforce training, access approval,
physical security, interface validation, director approval, retention, downtime procedures
and breach assessment. The project has no access to the lab's ePHI in this local deployment;
the project's conclusion that it is not a business associate depends on that condition
(**interpretation**). Arrangements with the lab's support providers are the lab's responsibility.

Outside PACK-IE, and the laboratory's to provide:

- HIPAA risk analysis and management, workforce training and sanctions, and business associate
  agreements with its own vendors. [[H3]][H3]
- OS account management: one account per person, password and MFA policy, OS logon
  auditing, screen lock and prompt removal of departed staff.
- Full-disk and backup-media encryption, off-site storage, and disposal or re-use of disks.
  [[H4]][H4]
- Network segmentation, VPNs and firewalls, and the documented decision whether plaintext
  MLLP is acceptable on its network. [[H1]][H1]
- Interface validation: test plans, choosing samples, comparing with the LIS or EHR, director
  approval and the frequency of re-verification. PACK-IE supplies evidence and a sign-off
  record, not the validation. [[C2]][C2]
- Retention periods, legal holds, the frequency of audit-log review, breach assessment and
  notification, patient access requests, public-health reporting, state-law analysis,
  accreditation inspections, the procedure manual and physical security of the computer.

PACK-IE has no application login, roles beyond OS groups, remote management service, SIEM or
cloud audit forwarding, or database encryption, and it makes no FDA submission. It is not
ONC-certified and claims no HL7 implementation-guide conformance beyond the behavior tested
by its [conformance corpus and scenarios](../testing.md). Validate the partner's actual profile
and reporting requirements for each connection.

## 2. HIPAA Security Rule crosswalk

The Security Rule binds covered entities and business associates, not software. This table
covers the specifications that bear on a PACK-IE installation, and says what the product
contributes toward each and what the laboratory must still do. **R** means required; **A** means addressable; **standard** means a safeguard standard.
Addressable does not mean optional: assess whether the measure is reasonable and appropriate;
if it is not, document why and an equivalent measure where reasonable. [[H2]][H2]

| Safeguard and specification | What PACK-IE provides | What the laboratory must do |
| --- | --- | --- |
| Access control, §164.312(a)(1), standard [[H1]][H1] | Management is local only. `ipc.group` restricts access to designated accounts: default Unix group `packie`, Windows local group `PACK-IE Operators`, plus service/administrator access. | Approve and review group membership; remove access when staff leave. Check actual access using [installation](install.md#ipc-access-and-opening-the-console). |
| Unique user identification, §164.312(a)(2)(i), R [[H1]][H1] [[H20]][H20] | The engine obtains the account name and uid/SID from the IPC peer's OS identity. Audit records attribute actions to that account; `origin` identifies the client type. | Give each person an individual OS account. Shared bench accounts defeat attribution. |
| Emergency access procedure, §164.312(a)(2)(ii), R [[H1]][H1] | An authorized administrator can use the local CLI and restore a verified backup while the engine is stopped. Store and configuration locations are known. | Write and test administrator access, backup restoration and emergency data retrieval procedures. Keep recovery credentials available to authorized staff during an outage. |
| Automatic logoff, §164.312(a)(2)(iii), A [[H1]][H1] | The OS session is the primary control. The console also hides message content, parsed fields and audit details after an idle timeout (Settings → Privacy, default 15 minutes; a setting of that console, not a login). It does not lock the computer. | Set OS screen-lock policy, protect unattended displays and clear content/close views after use. Document the chosen measure. |
| Encryption at rest, §164.312(a)(2)(iv), A [[H1]][H1] | Store files are access-restricted but not encrypted by PACK-IE. | Enable full-disk encryption (BitLocker, FileVault or LUKS), protect recovery keys, and encrypt backup and export targets. Document the assessment. |
| Audit controls, §164.312(b), standard [[H1]][H1] [[H20]][H20] | A separate append-only, hash-chained audit log records account, time, action, object and outcome, including body/parsed-field views, identifier/text searches and audit exports. Audited commands/content views are refused if their initial audit event cannot be written; message traffic continues. | Protect audit copies, verify the chain against preserved anchors and review failures. Host administrators remain inside the trust boundary. |
| Information system activity review, §164.308(a)(1)(ii)(D), R [[H3]][H3] | `packie audit list`, `export` and `verify` support review and preservation. Audit records are separate from rotating diagnostic logs. | Name a reviewer and a review cadence based on risk; record the review, findings, escalation and resolution. Review OS activity too. |
| Log-in monitoring, §164.308(a)(5)(ii)(C), A [[H3]][H3] | There is no PACK-IE login. Audit records identify accounts for management actions and record access refusals where identity is available. | Enable OS logon auditing, review failed/unusual logons and membership changes, and set password/MFA policy at the OS or organization level. |
| Integrity, §164.312(c)(1), standard; authentication of ePHI, §164.312(c)(2), A [[H1]][H1] | Original and transformed bytes have SHA-256 hashes; stored bodies are checked when read. Receipt precedes positive data ACK. Audit verification checks hashes, sequence continuity and links across retained files. | Validate mappings against source and final reports; preserve hashes with evidence and investigate mismatches. A hash alone does not establish clinical correctness or prove the receiving system's contents. |
| Person or entity authentication, §164.312(d), standard [[H1]][H1] | The OS authenticates the login; the engine reads the IPC peer's OS identity rather than trusting a supplied account name. OS-login sufficiency is the project's **interpretation**; no HHS text was found either way. | Use individual accounts and controlled access; assess whether the OS controls meet the lab's authentication needs. Do not treat a client label as verified identity. |
| Transmission security, §164.312(e)(1), standard; integrity controls and encryption, §164.312(e)(2)(i)–(ii), A/A [[H1]][H1] | TCP/MLLP are plaintext; no TLS is provided. Listeners use configured bind addresses and peer allowlists. SFTP encrypts transport and pins the server key. | Document the network risk and chosen controls: segmentation, firewalls, allowlists and, where needed, a VPN. Protect folder shares and verify destination identity; SFTP does not encrypt remote files at rest. |
| Backup, disaster recovery and emergency-mode operation, §164.308(a)(7)(ii)(A)–(C), R/R/R [[H3]][H3] | `packie backup create` takes consistent store, body, audit and configuration copies, with a manifest; `verify` checks them and `restore` requires the engine stopped. A daily schedule and staleness alert are configurable. | Encrypt the target, keep custody/off-site records, and test restore and emergency operations. Preserve secret values separately; backups contain names only. See [backup and restore](install.md#backup-and-restore). |
| Documentation retention, §164.316(b)(2), R [[H5]][H5] | Configuration versions have attributed apply notes and verification sign-offs and are kept without limit by default. Audit retention defaults to 2190 days; diagnostic logs remain separate. | Keep required policies/action records for six years from creation or last effective date, whichever is later. Six years for raw audit logs is a conservative **interpretation**, not an explicit raw-log requirement. Set retention to cover full calendar periods and archive required lab records. |
| Media disposal/re-use and backup before equipment movement, §164.310(d)(2)(i), (ii), (iv), R/R/A [[H4]][H4] | The inventory below identifies data locations, including backups and pre-restore state. Uninstalling the service retains data. | Inventory all copies; authorize transfer, sanitization and destruction; keep custody and disposal records. Preserve a retrievable copy before moving equipment where appropriate. |

The HIPAA Security Rule amendments proposed on 2025-01-06 (which would, among other things,
require encryption at rest and in transit and an asset inventory) were still **proposed as of
2026-11-06**: the Federal Register showed no final rule or withdrawal, and the Unified Agenda
listed final action for 07/2027. Do not treat proposed requirements as current law; check
their status and review changes with the lab's compliance owner. If finalized they would bind
the laboratory, which is why the network-risk and encryption rows above ask for documented
decisions now. [[H9]][H9] [[H10]][H10] [[H11]][H11]

## 3. Interface validation and change control

The project's 0.2.0 [acceptance criteria](../spec/acceptance.md) were run against the
release build: 135 of 136 passed, none failed, and one is partial (the console window opened
and reconnecting on an unlocked desktop session, not yet checked by hand). They include the
[phase-1 controls](../spec/acceptance.md#12-phase-1-version-020) for attribution, audit,
change control, backup/restore, notifications, setup checks, exports/reports, resend safety,
idle protection, tap retention and performance. Some passes rely on crate tests or inspection
rather than live operation. An 8-hour and a 4-hour chaos soak with simulated peers lost or
duplicated no acknowledged message, and the defects they found were fixed before the 4-hour
run. Linux packaging, service, reboot and upgrade checks were run in a virtual machine
(Ubuntu 24.04) and the Windows test suite runs in continuous integration, but the Windows
installer, service, permission, attribution and upgrade checks and the macOS reboot check in
the [installer checklist](../../packaging/VERIFY.md) have not been recorded. The laboratory
must validate its installed build and actual interfaces on its own platform.

CLIA expects accurate, reliable and timely transmission from data entry to the final report
destination, with routine checks. The laboratory chooses the checks and frequency.
[[C2]][C2] [[C3]][C3] The CAP checklist evidence behind this guide is the **2016 edition**,
**secondary for current requirements**. Confirm your current checklist with the accreditor;
this guide does not assert current CAP item numbers or a mandatory biennial interval.
[[C4]][C4]

1. Define each interface's source, destinations, intended formats, fields, transformations,
   acknowledgement level and acceptance criteria. Choose representative synthetic cases:
   identifiers, units, decimals, report dates, statuses, corrections, missing fields, rejection,
   duplicate/retransmission and downtime recovery. Keep clinical meaning unchanged.
2. Use [scenario runs](../spec/scenarios.md) as repeatable engine evidence. From a source/test
   installation with `packie-sim` and `packie` available, run from the source checkout's root:
   `packie-sim run tests/scenarios/msg-crash-restart-reconcile.json`, for example. Retain the
   scenario, engine version, run output and pass/fail/skipped results. Scenarios use isolated
   test state and synthetic peers; they do not validate the real instrument-to-LIS interface.
   Never enable `test_hooks` on the production node.
3. Run the lab's end-to-end cases through each actual interface and compare the instrument
   record, PACK-IE original/transformed content and final LIS/EHR report. Check report dates,
   corrections and retrieval as well as successful delivery. An ACK is evidence of the
   recorded acknowledgement level, not proof of the final report's correctness.
4. Save the active configuration version and every affected channel's apply outcome.
   **Settings → Versions** and `packie config history` show saved versions, who applied them,
   required notes and verification sign-offs. Apply with
   `packie config apply <file> --note "Change record reference and reason"`. After the lab's tests
   pass, use `packie config verify <version> --note "Validation record reference"` to record
   account, time and reference. This records a sign-off; it does not run validation tests or
   replace the director's approval. Versions are retained without limit by default. Preserve
   the lab's comparisons and approvals with them. A saved or signed-off version alone does
   not prove that every channel applied successfully.
5. Record deviations, corrective action, retesting and the director's dated approval in the
   lab's records. Assess every software, endpoint, protocol, filter or mapping change for
   impact; repeat affected tests before authorizing clinical use. Rollback also needs review.
   Procedure/change approval is supported by **secondary** evidence for
   §493.1251(d)–(e); confirm applicable wording. [[C1]][C1] [[C4]][C4]

Use the transmission report and message export as validation and evidence tools alongside
the lab's source-to-final-report comparisons:

| Evidence task | Command and interpretation |
| --- | --- |
| Summarize receipt and delivery for a channel/time window | `packie messages verify --channel <channel-id> --since <start-time> --until <end-time>` counts accepted, received, handed-off, unresolved, closed and pending deliveries per destination. |
| Preserve delivery-level evidence | Add `--csv --output <new-file>` for rows with original/transformed SHA-256, states, ACK level/code, `ack_control_id` (the ACK's own control id; HL7 ACK `MSH-10`), attempts and times. Check the summary or `--json` report for `truncated`; CSV has no truncation marker. Split periods exceeding 10,000 delivery rows. |
| Preserve selected messages | `packie messages export --channel <channel-id> --since <start-time> --until <end-time> --format jsonl --output <new-file>` exports matching metadata and delivery summaries, oldest first. Add `--bodies` only when original content is required. |
| Compare original and transformed content locally | `packie messages show <message-id> --body original`, `--body <destination-id>` and `--fields` explicitly request content; `--json` without them shows metadata, including mapping versions and stored hashes. Displayed bodies can be truncated. |

Reports are audited as `transmission_verify`; message exports as `message_export`, including
filters, format, whether bodies were requested and matching count. Both record the OS
account. Existing output files are refused. Even metadata and hashes can be PHI; restrict
evidence access, encrypt storage/transfer, inventory recipients and retain/dispose copies
under the lab's policy. Exported bodies are UTF-8 text with invalid sequences replaced and
unreadable/incomplete originals exported as empty text, so they are not byte-exact archives.
Use [verified backups](install.md#backup-and-restore) when preserving the complete stored data.

A stored-byte hash does not prove clinical correctness, and a missing transformed hash does
not prove that nothing was sent. An ACK proves only its recorded level. Compare the source,
PACK-IE original/transformed data and the actual final LIS/EHR report; include corrections and
report dates. See [report/export limits](running-and-monitoring.md#transmission-reports-and-message-export).

Use this suggested record for initial validation, changes and recovery tests:

| Field | Laboratory entry |
| --- | --- |
| Record ID, purpose and date/time with time zone | |
| Node, engine/console versions; instrument and LIS/EHR versions | |
| Channel, source, every destination, protocol and required ACK level | |
| Configuration/mapping version, apply note and per-channel apply outcome; saved file reference | |
| Change description, risk/affected fields and rollback method | |
| Cases and acceptance criteria; source-to-final-report comparison | |
| Scenario files/run evidence, results and skipped coverage | |
| Message/delivery IDs; original/transformed SHA-256; ACK code, level, time and state | |
| Correction/report-date/retrieval checks and downtime reconciliation results | |
| Deviations, corrective action and retest evidence | |
| Tester identity/date; reviewer; director approval/signature/date | |
| Evidence location, access owner, retention and next review trigger/cadence | |

Keep original and corrected reports linked in the lab's records and avoid reissuing a
superseded result. PACK-IE records `revision_of` when source identity matches but content changes;
that is not a clinical correction review. Both resend modes refuse an older revision with
`superseded` when a newer revision exists. Inspect that newer record and the destination's
current report first. Only an approved decision to send older content warrants
`--superseded-ok`; the audit records `confirm_superseded`. This override is separate from
`--confirm-duplicate-risk`, which may also be required. **Same message** normally sends the
stored transformed output; **New message** uses current mappings. Neither mode changes
this safety check. Do not change a report date merely because a copy or resend occurs. [[C2]][C2]

## 4. Data inventory, permissions and disposal

`<state>` and `<logs>` mean the actual `paths.state_directory` and `paths.log_directory` in
the installed bootstrap. Use [installed paths](install.md#packages-and-installed-files) to find
the bootstrap; custom paths override the package defaults. Record the actual paths, volumes,
owners and backup destinations in the lab's asset inventory.

| Location | Contents and persistence | Permissions and laboratory action |
| --- | --- | --- |
| `<state>/store/messages.sqlite3` and SQLite sidecars (`-wal`, `-shm` when present) | PHI: identifiers, original/inline transformed bodies, source file names, delivery/attempt history, operator reasons and links. Persistent. | Unix store directories are `0700`, database/body files `0600`; service account owns them. Windows installer restricts state/log directories to SYSTEM and Administrators. Keep the whole store protected; do not copy just the live database or delete sidecars. |
| `<state>/store/bodies/<first two hash digits>/<hash>` | PHI: larger original and transformed bodies, shared by hash. Persistent, including temporary body files during writes. | Same private store permissions. Include bodies with the database in backup, restore and disposal; a filename hash is not de-identification. |
| `<logs>/packie.YYYY-MM-DD.log`, with `.1.log`, `.2.log`, etc.; engine log ring | No message bodies, parsed values, patient/sample/order/control IDs, message file names, secrets or operator reason text. Files persist; ring is memory only. Peer IP addresses and engine-generated IDs remain. | Log files are owner-only on Unix; Windows uses the protected log directory. Body-free logs are **not de-identified under HIPAA**. Include this sensitive metadata in incident scoping and access/disposal decisions. [[H7]][H7] |
| Engine buffers, console Messages views and live transcript | PHI in process memory. Transcript is opt-in, bounded and not automatically saved. Stopping capture can leave displayed bytes until the page is cleared/closed. | Local IPC access permits content access; it is not the same as direct file access. Clear views and protect displays. Include OS swap, hibernation and crash-dump locations in the host inventory and encryption policy. |
| `<state>/audit/audit.sqlite3`, SQLite sidecars and sealed `audit-<year>-<first seq, 12 digits>.sqlite3` files | Persistent audit records: account, action, target, time, outcome and potentially PHI in searches/notes/reasons; never message bodies. Separate from diagnostic logs. | Audit directory `0700`, current file `0600`, sealed files `0400` (read-only on Windows). Protect all audit files and exports as PHI. Retain preserved anchors separately. |
| `runtime.file` (default `<state>/runtime.json`), `<state>/config/runtime.vNNNNNNNNNN.json` and corresponding `.meta.json` records (legacy `.note` files); bootstrap | Persistent configuration/history. Not a message archive, but labels, paths, constants, code tables and free-text notes can carry sensitive data if entered there. | Keep configuration files private; avoid patient data in IDs, labels, paths, notes and mapping constants. Keep external approvals and comparisons with version records; check any configured pruning policy. |
| Folder endpoint `settings.path`, its `processed_directory` (default `processed`), destination and temporary files | PHI in incoming, processed, retained source and output files. `on_consumed` may move, delete or leave a source file after receipt. These copies are outside message-store retention. | Restrict local/share ACLs to the service and intended partners; inventory mapped shares and their backups. Verify service-account access and arrange independent cleanup/retention. |
| SFTP endpoint `host` / `remote_directory`, processed directory and partner output files | PHI on the remote host, including processed and temporary files. SFTP protects the transfer, not remote storage or backups. | Record remote owner, account permissions, encryption, retention and disposal arrangements. PACK-IE does not control the remote host's media. |
| `backup.directory/packie-backup-<UTC time>/`, explicitly chosen backup directories, snapshots and off-site copies | PHI in store, spilled bodies, audit and configuration copies. Includes `manifest.json`, reference bootstrap and secret names, never secret values. `<state>/backup/status.json` records attempts and success. | Backup directories are created `0700`, writable files `0600`; sealed audit copies remain read-only. Restrict target ACLs, encrypt each volume and record custodian, transfers, retention and restore tests. Endpoint files and diagnostic logs need separate preservation. |
| `<state>/pre-restore-<UTC time>/` | Previous store, bodies, audit, configuration history and runtime document moved/copied aside during restore. Persistent PHI until an operator removes it. | Keep access private and include it in disk-capacity, hold and disposal decisions. Scheduled backup cleanup does not remove it. |
| Lab-chosen evidence/export folders, redirected CLI output, clipboard, screenshots and support material | PHI may be copied from message metadata, bodies, parsed fields or transcripts. Message export optionally includes original content; transmission reports contain identifiers, hashes and ACK evidence; audit export includes operator details. | Choose a restricted destination and safe transfer method; track recipients and copies. Protect terminal/shell history and clipboard history. PACK-IE retention does not remove these copies. |

The [diagnostics exclusions](../spec/diagnostics.md#8-what-never-appears) define what PACK-IE keeps
out of logs, snapshots, alerts and errors. Do not infer that the absence of payloads makes
all host records anonymous. Secrets live in the service account's OS key store or configured
restricted secrets file; preserve/recover them separately and never include them in support
evidence.

Before equipment disposal, re-use or movement, account for every local and remote copy,
backups and recovery keys. Service removal alone retains data; package purge or file deletion
is not evidence of media sanitization. Follow the lab's approved media procedure and record
custody and destruction. HHS's encryption and destruction guidance (NIST SP 800-111 for data at rest, SP 800-88 for destruction) is **secondary** here;
confirm applicability before relying on an encryption safe harbor in a breach assessment.
[[H4]][H4] [[H13]][H13]

## 5. Retention and purge

Choose retention by data role and applicable policy. PACK-IE is usually an interface evidence
store; the LIS/EHR holds the report of record. A short PACK-IE retention period is appropriate
only if the lab retains required reports and evidence elsewhere and can retrieve them.
CLIA does not name interface logs as a distinct record category. Keeping delivery evidence
as long as the report it supports is the project's **interpretation**. [[C2]][C2]

| Record/policy example | Setting or action | Limit to account for |
| --- | --- | --- |
| Short operational history | Per-channel `retention.max_age_days` defaults to `90`; range 1–36,500, or `null` for no age limit. | Ninety days is a product default, not a regulatory period. |
| CLIA test reports, QC/analytic-systems records and original/transcribed copies: typically at least 2 years [[C2]][C2] | If PACK-IE evidence must cover that period, choose an age limit covering the full calendar interval, e.g. 731 days for two years, and retain required records in the lab archive. | Age is measured from receipt, not necessarily reporting. Allow for reporting delay; a count cap can purge earlier. |
| CLIA pathology reports: at least 10 years [[C2]][C2] | Where applicable to the evidence held here, use a sufficient day count, e.g. 3653 for ten calendar years, with allowance for reporting delay, or archive under the report policy. | PACK-IE does not replace the laboratory report archive. |
| HIPAA required policies/action documentation: 6 years [[H5]][H5] | Keep lab policies, reviews and action records for the applicable six-year interval. `audit.retention_days` defaults to `2190`, range 365–36,500; set it at least as long as the longest message retention and the full applicable calendar interval. | Raw audit-log retention for six years is an **interpretation**. A fixed 2190-day default can be shorter than six calendar years across leap days. Sealed audit files are purged whole only after their newest event ages out; the current file is kept. Each removal is audited. |
| Configuration/validation history | `runtime.versions_kept` defaults to `null` (all versions); a positive count permits pruning only of versions older than `audit.retention_days`. Keep tests and director approvals in the lab archive. | A version count is not a time guarantee. The 2016 CAP checklist calls for testing records through system life plus two years (**secondary for current requirements**); confirm the current checklist. [[C4]][C4] |
| Backup copies in the scheduled root | After a scheduled backup, `backup.schedule.keep` retains the newest 1–365 complete `packie-backup-*` directories (default 7), including manually created backups with that naming. | This count is separate from message/audit retention and off-site policy; pre-restore state and other directory names are not removed. |
| Diagnostic logs | `logging.retention_days` defaults to `30`, range 1–3650. | These are operational records, not the durable audit log. Longer log retention cannot recreate content-view attribution. |
| State/accreditation/contract requirements or legal hold | Configure the longer applicable period and preserve a protected external copy. With an approved hold, set both channel limits to `null` while arranging preservation. | No case-specific legal-hold facility exists. Examples of state rules (California 3 years; New York 7 years for most reports, 20 for tissue pathology) are **secondary**; confirm the jurisdiction's current rule. [[C14]][C14] [[C15]][C15] |

Set `retention.max_messages` to `null` when a minimum time period must be guaranteed;
otherwise either the age or count limit can trigger deletion. Both limits `null` produce an
unbounded-retention warning. Purge runs at startup and every ten minutes, removing eligible
messages, bodies, deliveries and attempts; shared bodies remain while referenced. Unfinished
receipt/routing and deliveries that are queued, sending, rejected, failed, timed out or
outcome unknown are protected. Acknowledged or closed deliveries can become eligible.
Filtered, duplicate, invalid, aborted and observed messages can also age out.

Do not close unresolved work simply to free disk space. Removing a channel cancels its
unresolved deliveries and its records follow the default 90-day retention; preserve evidence
first. An earlier revision stays while a retained revision references it; preserve required
original/corrected evidence together in the lab archive as well. Backups, endpoint files,
exports and lab validation records need their own retention and disposal rules. See
[retention behavior](../spec/message-channels.md#10-retention) and
[settings](configuration-reference.md#message-fields-and-retention).

## 6. Incident and breach-relevant records

Preserve a restricted copy of relevant evidence before rotation, purge or configuration
pruning. Record the incident window with time zone, node/version, affected channels,
accounts and hosts, and who collected each copy. Keep originals and work from copies.

| Question | Evidence PACK-IE can provide | What remains with the laboratory |
| --- | --- | --- |
| What was received, transformed and sent? | Messages, original/transformed hashes and requested bodies, receipt/configuration/mapping versions, destinations, attempts, ACK codes/levels/times and delivery states. | Compare instrument and destination records. An acknowledged delivery does not prove a person read a report. |
| What changed or was restarted? | Attributed configuration versions, required notes and sign-offs; audit events for configuration/channel actions, resend/close, backup/restore and service lifecycle; diagnostic reconciliation counts. | Preserve versions, audit and OS/lab change records, including per-channel apply output. The audit's overall action outcome does not prove every channel applied successfully. |
| Who viewed content or exported evidence, and when? | Audit events attribute body/parsed-field views, identifier/text searches, `message_export` (filters, format, content choice, matching count), `transmission_verify` (channel/period) and audit exports to an OS account. | Redirected output, screenshots, transcripts and direct file access still need OS/recipient evidence; a view/export event cannot establish who later received a copy. Absence of an entry is not proof of no access. |
| Was evidence altered or lost? | Stored-body hash checks; audit-chain verification across retained files, recorded audit pruning and daily/shutdown `audit.anchor` log events. | Compare the verified head sequence/hash against a separately preserved anchor. Removing newest events can leave a valid shorter chain; an administrator can destroy both evidence and anchors. |

Use `packie audit list --since 24h --actor <account>` for account activity and
`packie audit list --object-type message --object <message-id>` for a message's content-view
events. `packie audit export --since 24h --format jsonl --output <new-file>` preserves matching
events; choose a restricted evidence location. The export is audited and refuses an existing
output file. Exported audit details may contain PHI.

Run `packie audit verify` against the engine and `packie audit verify --dir <copy-audit-dir>`
against a preserved copy. Record the returned head sequence/hash and compare it with a
separately kept `audit.anchor` log line. Audit writes survive an engine crash, but a power
loss can lose about the last second before checkpoint. If an outcome record failed, a
`requested` event alone does not prove success; check the command result and affected data.
See [audit review and integrity limits](../spec/audit.md).

The HIPAA breach assessment includes whether PHI was actually acquired or viewed. The lab
performs that assessment and determines notification duties, including applicable deadlines;
PACK-IE does not decide whether an event is a reportable breach. [[H6]][H6] Review exported
material for PHI before sharing it; follow [support collection guidance](troubleshooting.md#collect-support-information-without-patient-data).

## 7. Downtime and restart reconciliation

Closing the console leaves the engine running. If the engine or computer stops, PACK-IE cannot
receive, acknowledge or forward new traffic. Instruments and sending systems may retain and
retransmit under their own retry rules; verify retry limits, queue capacity and recovery
steps with each vendor. PACK-IE cannot guarantee recovery of data a sender discards.

| Channel/protocol | During interruption | At recovery |
| --- | --- | --- |
| Bridge | Connections end; buffers are memory only. A crash loses buffered bytes; bounded orderly shutdown can leave undrained bytes. PACK-IE does not originate protocol ACKs for a bridge. | Reconnects from configuration; no durable bridge queue is replayed. A recording tap is observational evidence, not a delivery queue. Reconcile at both peers. |
| ASTM message source | Each accepted data frame is committed before its frame ACK. ENQ ACK is session negotiation, not proof a result was stored. A failed commit withholds the data ACK. | Sender may retransmit. Startup recovers complete valid committed transfers where possible; incomplete transfers become Aborted. Check them against the instrument's pending/results list. |
| HL7 message source | Original-mode positive ACK follows commit and syntax acceptance. Enhanced `CA` proves PACK-IE commit; application `AA` proves engine acceptance, not downstream delivery. On storage failure no positive storage ACK is sent. | Sender retries according to its ACK settings. PACK-IE processes stored unfinished messages and continues queued deliveries. Validate the sender's duplicate handling. |
| Durable message deliveries | Committed messages remain on disk. A destination may have received an in-flight message even if PACK-IE never recorded its ACK. | Reconciliation makes interrupted Sending deliveries `outcome_unknown`, keeps queued work in receipt order and restores paused flags. It does not query the LIS to resolve unknown outcomes. |
| Folder/SFTP source | Uncommitted files are not consumed by PACK-IE; a whole computer/share outage may prevent access. | Polling/watch resumes. Check input, processed and output folders and partner records; do not assume file handoff proves LIS import. |

At startup, reconciliation runs before message sources open and emits
`reconciliation.completed` with recovered, aborted, requeued and unknown counts. Inspect
**Queued**, **Needs attention**, **Duplicates**, and **Invalid or aborted** over the full
outage period, including older unresolved work. Queues resume when channels/destinations
are enabled and available; persisted operator pauses remain paused. Duplicate detection
uses channel, source identity and content hash; a changed identity or purged original can
prevent a match. See [reconciliation rules](../spec/message-channels.md#13-startup-reconciliation).

Adapt this procedure and assign named owners before go-live:

| Step | What the laboratory's procedure must specify |
| --- | --- |
| Detect and notify | Who watches Dashboard/Alerts, at what interval, and whom to call for `endpoint_down`, `message_store_unavailable`, `disk_low`, `delivery_backlog`, `delivery_needs_attention`, `invalid_messages`, `audit_unavailable`, `backup_stale` and `notifications_failing`. Configure/test operational notification routes and define an independent outage check: a stopped computer cannot send notifications. Process clinical/urgent-result notifications through the lab's own procedure. |
| Continue laboratory work | Instrument queue limits, manual/alternate result reporting, urgent-result communication, identification of affected specimens and control of later duplicate entry. CLIA requires a course of action for inoperable systems; current accreditation details need confirmation. [[C2]][C2] [[C4]][C4] |
| Recover safely | Authorized administrator, power/network/storage checks, the tested restore method, recovery secrets and escalation contacts. Start the correct service; confirm version, configuration, store and channel states. |
| Reconcile | Compare instrument pending/results lists, PACK-IE receipt/delivery records and final LIS/EHR reports across the outage window. Investigate missing, aborted, rejected and unknown outcomes. Check the destination before resending timed-out/unknown deliveries; close only with a verified reason. |
| Release and document | Verify queues drain, perform source-to-final-report checks and record gaps, duplicates, corrections and approval to resume routine operation. Use the validation record above and complete transmission reports for the outage period; preserve restricted message exports where needed. |

## 8. Go-live checklist

Record an owner, completion date and evidence reference for each row. Keep the completed
checklist with the laboratory's approved procedure and validation record.

| Check | Complete / owner / evidence |
| --- | --- |
| Intended use is transfer, storage, format conversion, display and the lab's own formulas. | |
| Lab/compliance/IT responsibilities, risk assessment and applicable state/accreditation requirements are assigned. | |
| Engine/console versions, bootstrap, state/log paths, endpoints and final report destinations are recorded. | |
| Individual OS accounts, approved IPC access/group membership, logon auditing, screen lock and access-removal procedures are tested on the installed platform, including Windows `PACK-IE Operators`. | |
| Full-disk and backup/export-target encryption, recovery-key custody, physical security and network controls are verified. | |
| All PHI locations, folder/SFTP copies, backups, exports and media custodians are inventoried with permissions and disposal rules. | |
| Consistent backup, off-site custody, restore and emergency administrator access have been tested. | |
| Each actual interface passed source-to-final-report validation, correction/retrieval checks and failure/recovery cases; skipped scenario coverage is documented. | |
| Active configuration/mapping versions, apply outcomes, notes and director approval are preserved; changes have retest/rollback rules. | |
| Message/count limits, log/history retention, external archives and hold procedures match policy; originals/corrections are retained together. | |
| Attribution, audit export and chain verification are checked; anchors are preserved and the reviewer, review cadence and incident evidence custodian are named. | |
| Downtime detection, contacts, manual reporting, sender retry limits, reconciliation and return-to-service approval are exercised; staff are trained. | |

[H1]: compliance-sources.md#hipaa-and-hhs-h
[H2]: compliance-sources.md#hipaa-and-hhs-h
[H3]: compliance-sources.md#hipaa-and-hhs-h
[H4]: compliance-sources.md#hipaa-and-hhs-h
[H5]: compliance-sources.md#hipaa-and-hhs-h
[H6]: compliance-sources.md#hipaa-and-hhs-h
[H7]: compliance-sources.md#hipaa-and-hhs-h
[H9]: compliance-sources.md#hipaa-and-hhs-h
[H10]: compliance-sources.md#hipaa-and-hhs-h
[H11]: compliance-sources.md#hipaa-and-hhs-h
[H13]: compliance-sources.md#hipaa-and-hhs-h
[H20]: compliance-sources.md#hipaa-and-hhs-h
[C1]: compliance-sources.md#clia-accreditation-and-states-c
[C2]: compliance-sources.md#clia-accreditation-and-states-c
[C3]: compliance-sources.md#clia-accreditation-and-states-c
[C4]: compliance-sources.md#clia-accreditation-and-states-c
[C14]: compliance-sources.md#clia-accreditation-and-states-c
[C15]: compliance-sources.md#clia-accreditation-and-states-c
[F1]: compliance-sources.md#fda-f
[F2]: compliance-sources.md#fda-f
[F3]: compliance-sources.md#fda-f
[F5]: compliance-sources.md#fda-f
