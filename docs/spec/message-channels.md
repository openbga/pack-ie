# Message channel specification

Store-and-forward behavior of message channels: durable receipt, message and delivery states,
retries, duplicates, resend, retention, pause and crash recovery. Settings are in
[configuration.md](configuration.md#64-message-channel); fields and alerts are in
[diagnostics.md](diagnostics.md); shared transport rules (endpoint states, listeners,
serial ports, retry formula) are in [bridge-channels.md](bridge-channels.md). Requirements:
MSG-01..08, PRO-01..04, ENG-02, ENG-05, SEC-04.

## 1. Scope

A message channel has one **source connector**, ordered **filters**, and one or more
**destination connectors** ([architecture §2](../architecture.md#2-domain-model)).

| Rule | Statement |
| --- | --- |
| Durable first | A received message is committed to the store with its original bytes before the protocol acknowledgement goes back to the sender (MSG-02). |
| Explicit outcome | Every delivery ends in an explicit state. A transport or protocol acknowledgement is never shown as processing success (MSG-03). |
| At least once | Delivery is at-least-once. Exactly-once is not claimed; a case where duplication is possible is a visible state (`outcome_unknown`, `timed_out`), never a silent retry. |
| Order | Messages of a channel get a receipt sequence number at commit. Each destination delivers in that order, one message at a time. A delivery that ends unresolved (`rejected`, `failed`, `timed_out`, `outcome_unknown`) does not block later ones. |
| Isolation | A failing channel, destination or endpoint never stops another channel (ENG-02). A destination that is down does not stop the source from receiving and storing. |
| Fan-out | One message creates one delivery per destination that passes its filters; destinations succeed or fail independently. |
| Raw | `raw` is not a message channel protocol. |
| Throughput | 100 messages per second for one channel with durable storage; 50 channels per engine with at least 500 messages per second aggregate (PRF-02), measured by the benchmark scenario. |

Data model: a **message** (original bytes, identifiers, state), its **deliveries** (one per
destination) and their **attempts**. Message and delivery records are queryable by the console
and the CLI; scenario assertions use the field names below.

## 2. Source connector session

### 2.1 Rule

For every protocol the same order holds:

1. Bytes of one protocol unit (frame, message or file) arrive.
2. The unit is committed in one store transaction: original bytes, message row, duplicate
   check ([section 7](#7-duplicate-detection)), identifiers.
3. Only after the commit returns does the engine send the protocol acknowledgement, or, for
   files, move or delete the source.
4. Pipeline processing ([section 6](#6-pipeline)) follows and never influences step 3 except
   for the syntax verdict of HL7 (below).

If the commit fails, no positive acknowledgement is sent, `message_store_unavailable` is
raised, and the sender retries by its own rules. The engine never acknowledges first and
stores later.

### 2.2 Per protocol

| Protocol | Unit committed | Acknowledgement after commit | Not committed |
| --- | --- | --- | --- |
| `astm` | Each accepted frame (exact bytes) is committed, then ACKed. The completed message (ENQ … EOT, records H…L assembled) becomes `received` at EOT; its original bytes are the assembled records, each ended by CR, which replace the committed frames. | Frame ACK after the frame commit. The ENQ ACK and EOT handling carry no data and need no commit. A frame with a bad checksum or sequence is NAKed and not committed. A commit failure withholds the ACK (the sender's timer expires). | A transfer that ends before completion: `aborted`. |
| `au` | Each accepted frame's exact wire bytes are committed; the sample message completes after its final block. Class A (no ACK/NAK on receive) commits frames without any response. Instrument queries and session messages are stored and closed `filtered` (`control_message`); well-framed text that does not parse is stored `invalid` (`unparseable`); neither is routed. Host-direction frames are rejected without being stored. | Class B: each block ACK waits for its frame commit and the 500 ms turnaround. The final ACK precedes the transaction that finalizes the sample and checks duplicates; all of its wire frames are already durable. | An incomplete sample: `aborted`. |
| `hl7_mllp` | One MLLP frame (the whole message). Bytes between the start and end block characters are the original bytes. | Per `ack_mode` (below). | A started frame that receives no further bytes for `frame_timeout_ms` (counted while the engine waits for them, not while it commits the frames before it) is discarded and the connection closed. A connection the peer closes or resets ends at once, inside a frame too. An operating system can ignore a reset that arrives right behind bytes it has not yet acknowledged (macOS does on a busy connection), leaving the connection open with nothing to read; so while a TCP source waits for the rest of a message (an HL7 frame, an ASTM or AU transfer with its timer running) it probes the peer with a TCP keep-alive after 1 s of silence, whatever the endpoint's `keep_alive` settings, which apply again once the message is complete. The peer's system answers the probe with a reset that is taken, so a dead sender does not hold the source's single session. A frame left open by either is discarded (`source.frame_abandoned`); it was never acknowledged, so the sender sends it again. |
| `delimited_file` | Each record or group of a complete file is one message; all of a file's messages are committed in one transaction. A message's original bytes are its rows rewritten with the source columns, including a header when `header_row` is true, without skipped lines or a BOM. The stored form uses the codec's 1 GiB limit because quoting and line endings can make it larger than the input. Its `source_file` holds the file name, the SHA-256 of the whole file and its row range (data rows counted from 1). | The source file is moved, deleted or left per `on_consumed`, only after the commit. A file that cannot be moved or deleted is logged (`file.consume_failed`); reading it again records duplicates when duplicate detection is enabled. | A file over `max_file_bytes` is not read into memory (the transport stops at the limit); one `aborted` message (`file_too_large`, no bytes, `source_file` with its name and an empty hash) records it. A file that does not parse as configured is committed whole as one `invalid` message (`malformed_file`, `encoding`). Both are then consumed like any other file. A commit failure leaves the file where it is; the source reopens the endpoint after the retry delay and reads it again. |

HL7 acknowledgement content (source side):

| `ack_mode` | After commit | Codes |
| --- | --- | --- |
| `original` | One `ACK` after the commit and the syntax check | `AA` valid; `AR` the message cannot be accepted (unparseable, required MSH fields missing); `AE` temporary engine error (for example storage unavailable; the sender should retry). |
| `enhanced`, sender asks for commit acknowledgement (MSH-15) | Commit acknowledgement after the commit | `CA` committed; `CE` commit error (storage unavailable). A syntax-invalid message uses the minimal negative reply described below. |
| `enhanced`, sender asks for application acknowledgement (MSH-16) and `send_application_ack` | Second message after the syntax check and routing decision | `AA` accepted by the engine (stored and the pipeline completed, including a filtered message; this says nothing about downstream delivery), `AR`, `AE`. |
| `enhanced`, MSH-15 and MSH-16 empty | Behaves as `original` | |

The syntax verdict parses the message structure and encoding characters and requires
nonempty MSH-9, MSH-10, MSH-11 and MSH-12. Filters, mappings and delivery results never decide
an acknowledgement code. An ACK echoes the received MSH-10 in MSA-2, carries a generated
MSH-10, and its error text names a field or condition, never a field value. A syntax-invalid
message receives a minimal `AR` after its bytes are committed, or `AE` if the commit failed,
including in enhanced mode; MSA-2 is echoed only when it can be safely extracted. A committed
syntax-invalid message is marked `invalid` after the reply.

With `reply_type: order_response` the application reply to an order message is the order
response of its type (`OML^O33` to `ORL^O34`, `ORM^O01` to `ORR^O02`, see
[configuration.md §7.3](configuration.md#73-hl7_mllp)) with the same MSA codes (MSH and MSA only: `AA` says the order was stored, not that a
per-order request was accepted); every other message
and the commit acknowledgement stay `ACK`. With `require_end_cr: false` a frame ends at
the end block and a carriage return after it is swallowed; the acknowledgement and the
original bytes are unaffected.

A duplicate ([section 7](#7-duplicate-detection)) is acknowledged positively after the
duplicate record is committed.

### 2.3 Session rules

| Rule | Statement |
| --- | --- |
| Listener source | A `tcp_listener` source serves one peer connection at a time, rejects others and peers outside the allowlist exactly as bridge listeners do ([bridge-channels.md](bridge-channels.md#10-listener-and-tcp-client-behavior)). The channel is `running` while the listener is `listening`; waiting for the peer is not a failure. The source endpoint reads `listening` while the socket is bound, with or without a peer; the source `session` tells `waiting_for_peer` from `idle` and `receiving`, and `active_peer` names the attached peer. |
| Client source | A `tcp_client` source connects out and receives. It reconnects with the channel `retry` policy; while not connected the channel is `retrying`. |
| Recovery | The source's failure streak and retry count clear at the first unit committed on a new connection. |
| Serial source | The port is leased from the port registry and held while the channel runs. Read timeouts are not disconnects. |
| File source | Detection and completion follow the endpoint settings (`stable_seconds`, `ignore_suffixes`, `exclusive_open_check`); partially written files are never read. Complete files are taken one at a time in order of modified time, then name, and the records of a file become messages in row order. |
| Transfer limits | ASTM enforces `max_message_bytes`; exceeding it aborts the transfer (`aborted`) with a reason code. A frame with more text than `max_frame_chars` is NAKed like any invalid frame. AU limits each frame by `framing.max_wire_len` and by `frame_timeout_ms` from its first byte (an unfinished frame is discarded without a response), and applies `block_timeout_ms` between the blocks of an incomplete sample; it has no `max_message_bytes` setting. |
| Response writes | Every response a source writes (ACK, NAK, HL7 acknowledgement) must be written within the protocol's `frame_timeout_ms` (HL7, AU) or `receive_timeout_ms` (ASTM). A peer that does not read its responses in time loses the connection (`source.response_stalled`, shown as the source endpoint's last error); a listener then takes the next peer. A pause or stop ends a write at once, a shutdown drain at the earlier of its deadline and the write limit. |
| File names | A source file name and a destination `file_name` may carry identifiers. They are stored with the message (`source_file`) but never appear in log events, snapshots or error summaries; those name the message or delivery id. |
| Peer errors | Protocol errors follow the protocol driver's response and abort rules. Malformed, oversized or out-of-frame MLLP bytes are discarded and logged as `source.framing_error`; they create no message row. A complete MLLP payload that fails the syntax check is stored `invalid`. |

## 3. Message states

| State | Meaning | Deliveries |
| --- | --- | --- |
| `receiving` | A multi-frame transfer (ASTM, AU) is in progress; frames are committed as they arrive. | none |
| `received` | The complete original bytes are committed; the pipeline has not finished. | none yet |
| `routed` | The pipeline finished and at least one delivery exists. Each delivery has its own state ([section 4](#4-delivery-states)). | ≥ 1 |
| `filtered` | A filter or the absence of an enabled destination excluded it; reason code recorded. | none |
| `duplicate` | Same source identity and content as an earlier message. | none |
| `invalid` | The bytes are complete but do not parse or validate; reason code recorded. | none |
| `aborted` | The transfer ended without a complete message; `abort_reason` recorded. | none |
| `observed` | Recorded from a bridge tap; never processed. | none |

| From | To | Condition |
| --- | --- | --- |
| (new) | `receiving` | First frame committed (ASTM, AU). |
| (new), `receiving` | `received` | Complete message committed. |
| `receiving` | `aborted` | Timeout, retry limit, lost connection, size limit, channel stop, or startup reconciliation found it unfinished and not recoverable. |
| (new) | `duplicate` | Duplicate rule matched at commit. |
| `received` | `invalid` | Parse or validation failed. |
| `received` | `filtered` | Source filters false, or no destination is enabled. |
| `received` | `routed` | Deliveries created (also when all of them fail at creation). |

Abort reasons: `retry_limit_exceeded`, `receive_timeout`, `incomplete_record`, `empty_transfer`,
`message_too_large`, `connection_lost`, `transport_error`, `channel_stopped`, `file_too_large`,
`startup_interrupted`.

Identifiers are extracted from the parsed message as the protocol provides them:

| Protocol | `control_id` | `sample_id` | `patient_id` | `message_type` |
| --- | --- | --- | --- | --- |
| `hl7_mllp` | MSH-10 | The specimen id SPM-2 (placer, else filler assigned, entity identifier) of the first SPM; without one OBR-3 of the first OBR (else OBR-2) | PID-3.1 of the first PID | MSH-9 |
| `astm` | H.3 | O.3 of the first O record, or the source's `sample_id_path` (for example `O.4.3`) | P.3 of the first P record | `astm` |
| `au` | none | sample id field | none | `au` |
| `delimited_file` | column `control_id` | column `sample_id` | column `patient_id` | `delimited` |

An identifier that is absent is `null`.

**Message record fields** (queryable): `message_id` (UUID v7), `channel_id`, `seq`,
`direction` (`inbound`, `a_to_b`, `b_to_a`), `state`, `origin` (`received`, `resend`,
`tap`), `resent_from`, `duplicate_of`, `revision_of`, `received_at`, `committed_at`,
`protocol`, `message_type`, `byte_length`, `content_hash`, `source_identity`, `control_id`,
`sample_id`, `patient_id` (identifiers as the protocol provides them, nullable),
`source_endpoint_id`, `source_peer`, `source_file` (name, file hash, row range, for files),
`response_code`, `responded_at`, `abort_reason`, `reason_code` (for `filtered` and `invalid`),
`config_version`, and the derived `delivery_summary`: `none`, `pending` (any `queued` or
`sending`, none needing attention), `attention` (any `rejected`, `failed`, `timed_out`,
`outcome_unknown`), `complete` (every delivery `acknowledged` or `cancelled`).

## 4. Delivery states

A delivery is one message for one destination. States are `queued`, `sending`,
`acknowledged`, `rejected`, `timed_out`, `outcome_unknown`, `failed`, `cancelled`.

| State | Meaning | Terminal | Safe to resend |
| --- | --- | --- | --- |
| `queued` | Waiting for an attempt: new, waiting for the destination to be ready, or waiting for a retry (`next_attempt_at`). | no | n/a |
| `sending` | An attempt is in progress: writing, or waiting for the acknowledgement. | no | n/a |
| `acknowledged` | Acknowledgement at or above `ack_required` arrived; `ack_level` says which. | yes | duplicate |
| `rejected` | The destination answered with a negative acknowledgement (HL7 `AE`, `AR`, `CE`, `CR`). Not retried automatically. | until resolved | after the cause is fixed |
| `timed_out` | A required acknowledgement did not arrive within `ack_timeout_seconds` on an open connection (HL7: after the whole frame; AU class B: after any frame). It may have been processed. | until resolved | check first |
| `outcome_unknown` | The connection ended while waiting for an acknowledgement, or the engine stopped or crashed during an attempt. Some or all of the message may have been processed. | until resolved | check first |
| `failed` | Attempts were exhausted after write failures or send timeouts, or a local transform error prevented an attempt. | until resolved | allowed; check partial writes |
| `cancelled` | Closed by an operator with a reason, or its destination or channel was removed. | yes | yes |

"Until resolved" means the delivery stays listed, counts toward `delivery_needs_attention`,
and blocks purging until it is resent and acknowledged or closed with a reason (MSG-04).

`ack_level` (recorded on every delivery and shown next to the state): `none`, `handed_off`
(the bytes or the complete file left the engine; no protocol confirmation), `received` (the
peer's protocol layer confirmed receipt of the whole message), `accepted` (the peer's
application accepted it). It records what was proven; it never turns a lower level into
success ([section 8](#8-acknowledgement-levels)).

| From | To | Condition |
| --- | --- | --- |
| (create) | `queued` | The pipeline created the delivery. |
| (create) | `failed` | The destination mapping failed (`transform_error`); no attempt was made ([section 6](#6-pipeline)). |
| `queued` | `sending` | Head of the destination's queue, destination endpoint ready, `next_attempt_at` reached. The attempt record is committed before the first byte is written. |
| `sending` | `acknowledged` | Required `ack_level` reached. |
| `sending` | `rejected` | Negative acknowledgement. |
| `sending` | `queued` | A write failure or `send_timeout_seconds` expiry, with attempts remaining: `next_attempt_at` set by backoff. |
| `sending` | `failed` | As above with no attempts left. |
| `sending` | `timed_out` | Required acknowledgement did not arrive within `ack_timeout_seconds` on an open connection. With `retry_unacknowledged` and attempts remaining: `queued` instead. |
| `sending` | `outcome_unknown` | Connection ended while waiting for an acknowledgement; engine stop or crash during the attempt. With `retry_unacknowledged` and attempts remaining: `queued` instead. |
| `queued`, `rejected`, `failed`, `timed_out`, `outcome_unknown` | `cancelled` | Operator closes it with a reason; destination or channel removed. |
| `rejected`, `failed`, `timed_out`, `outcome_unknown`, `cancelled`, `acknowledged` | `queued` | Operator resend of the same message ([section 9](#9-resend-and-close)): new attempt cycle. |

The engine retries write failures and send timeouts as incomplete attempts. This
classification does not prove that a destination discarded every byte: peer behavior and
the file endpoint's write mode determine whether partial work can remain.

**Delivery record fields** (queryable): `delivery_id`, `message_id`, `channel_id`,
`destination_id`, `state`, `ack_level`, `attempts` (current cycle), `total_attempts`,
`created_at`, `updated_at`, `next_attempt_at`, `reason_code` (why the state: for example
`transform_error`, `engine_restarted`, `channel_paused`, `destination_removed`),
`closed_reason` (operator text), `mapping_version` (the `config_version` that produced the
transformed body), `transformed_hash`.

**Attempt record fields**: `delivery_id`, `attempt_no`, `started_at` (committed before any byte
is written), `ended_at`, `result` (`acknowledged`, `rejected`, `write_failed`, `send_timeout`,
`ack_timeout`, `connection_lost_after_send`, `engine_stopped`, `transform_error`), `ack_code`
(protocol code received, for example `AA`, `CA`, `AR`), `ack_control_id` (the acknowledgement
message's own control id, HL7 `MSH-10`; its `MSA-2` is the original's), `error_code`,
`ack_level`.

## 5. Delivery attempts and retries

| Rule | Statement |
| --- | --- |
| Bounded | A delivery gets at most `max_attempts` attempts per attempt cycle. A cycle starts at creation and again at every operator resend. |
| Backoff | Between attempts the delay is the retry formula ([configuration.md](configuration.md#63-retry-policy)) with `delivery.retry`; `next_attempt_at` and the attempt count are visible. |
| What retries | Write failures and send timeouts, including connection loss while writing. Rejections are never retried automatically. Acknowledgement timeouts and connection loss while awaiting an acknowledgement are retried only with `retry_unacknowledged`. |
| Waiting is not failing | While the destination endpoint is not ready the delivery stays `queued` and consumes no attempt; the endpoint retries by its own policy and failure streak. The wait is unbounded and visible through queue age and `delivery_backlog`. |
| Attempts per destination | One in flight per destination. Lock-step protocols (ASTM, AU, MLLP acknowledgement) allow no more. |
| Final state | When the last allowed attempt fails: `failed` for `write_failed`, `send_timeout`, connection lost before the last byte; `timed_out` or `outcome_unknown` as defined in section 4. |
| Visibility | A delivery that needs attention appears in the Messages page, the channel and node counts and the `delivery_needs_attention` alert until it is resolved. |

### 5.1 Per destination protocol

| Protocol | One attempt |
| --- | --- |
| `hl7_mllp` | One MLLP frame, then the acknowledgement per `ack_mode` ([section 8](#8-acknowledgement-levels)), correlated by the sent MSH-10. The reply's message code (MSH-9.1) must be `ACK` or one listed in `ack_message_types` (an order response such as `ORL`); any other reply is unmatched ([section 5.2](#52-outcome-unknown-and-timed-out)). |
| `astm` | One transfer (ENQ … EOT) of the stored records, framed per LIS01-A2 (one record per frame; a record longer than `max_frame_chars` is split with ETB) and driven by the connector's link `role` (contention follows it). ENQ is sent up to `enq_attempts` times, `busy_retry_ms` apart, before the attempt fails as a write failure. |
| `au` | The frames of the stored AU message, in order, framed per the destination's profile. Test codes are `format.test_code_digits` wide. Class B waits for each frame's ACK and sends a frame again after a NAK, at most 3 times, then fails the attempt as a write failure. AU messages cannot be a mapping target, so an AU destination carries AU messages from an AU source unchanged; only the framing is the destination's. |
| `delimited_file` | One file per delivery in the endpoint's folder or SFTP directory, written with the endpoint's `write_mode`. Its name is `file_name` with `{message_id}`, `{control_id}`, `{sample_id}`, `{date}` (`yyyyMMdd`), `{time}` (`HHmmss`) and `{seq}` (the receipt sequence) filled in; characters other than letters, digits, `-`, `_` and `.` in a value become `_`. `on_exists` decides what happens when the name exists. Without a mapping the delivery writes the message's original bytes. A mapped file larger than the destination's `max_file_bytes` is not written: the delivery is created `failed` with `transform_error`. Each file is flushed to disk before it counts as written, so a burst of messages faster than the disk syncs files builds a temporary backlog of `queued` deliveries that drains in order afterwards. |

### 5.2 Outcome unknown and timed out

`outcome_unknown` and `timed_out` exist because a message may have been received and
processed although the engine saw no acknowledgement.

- They are never retried automatically unless the destination sets `retry_unacknowledged`.
- The console labels them "may have been delivered" and states which acknowledgement was and
  was not received (`ack_level`).
- Resending requires the operator to confirm the duplicate risk.
- Version 1 has no automatic reconciliation against the destination: there is no query
  step. The operator checks the destination and then resends or closes with a reason.
- At startup, deliveries found `sending` become `outcome_unknown` with reason
  `engine_restarted` ([section 13](#13-startup-reconciliation)).
- An unsolicited, late or mismatching acknowledgement (for example an HL7 `MSA-2` that does
  not match any message in flight) is ignored and logged as `delivery.ack_unmatched`. It does
  not change any delivery.

## 6. Pipeline

```
receive → commit (+ duplicate check) → [acknowledge sender]
        → parse → source filters → for each enabled destination:
              destination filters → mapping → create delivery → queue
```

| Stage | Rule |
| --- | --- |
| Parse | Original bytes are parsed into the protocol model. A failure makes the message `invalid` (reason code), raises no delivery, counts toward `invalid_messages`. The original bytes stay. |
| Source filters | The channel `filters` are evaluated in order; all must be true. Otherwise the message is `filtered` (reason `source_filter`), no deliveries. A filter reading an absent field sees empty text. |
| Destination filters | Each enabled destination's `filters`, in order. A false result creates no delivery for that destination; other destinations proceed. If no destination remains the message is `filtered` (reason `destination_filters` or `no_destination`). |
| Mapping | Each destination's mapping runs on the parsed model and produces that destination's output message, encoded for its protocol. A same-protocol HL7 or ASTM target starts as a copy of the source message; an HL7 target of another protocol starts with MSH-3 to MSH-7 and MSH-10 to MSH-12 from the destination's settings, a new control id and the current time; an ASTM target starts with the header and terminator records; a delimited target always starts empty and is written with the destination's columns. With `mapping: null` the original bytes are passed through unchanged, except that AU wire frames are rebuilt for the destination's profile. Output and `mapping_version` (the configuration version the mapping came from) are stored with the delivery. |
| Delivery creation | Delivery rows for all destinations and the message's move to `routed` are written in one transaction. A crash leaves the message `received` and the pipeline is re-run at startup ([section 13](#13-startup-reconciliation)). |
| Order | Rules and filters run in the configured order; destinations are processed in configured order but deliver independently. |

**Transform error** (a `required` field missing, `lookup` with `on_missing: error`, a
`datetime` that cannot be converted, an output that cannot be encoded): the message and its
original bytes are kept and stay `routed`; the delivery for that destination is created
`failed` with reason `transform_error` and zero attempts, the error names the rule and path
but not the value, an alert `delivery_needs_attention` counts it, and the event
`delivery.transform_failed` is logged. Other destinations are not affected. After the
mapping is corrected, a resend of the same message re-runs the new mapping on the stored
original ([section 9](#9-resend-and-close)).

Filters and mappings are compiled when a configuration is applied. A channel whose filters
or mappings do not compile gets the apply outcome `failed`, naming each problem's JSON path;
it keeps its previous definition and instance, or does not start when it is new.

The channel keeps accepting messages while a destination is failing; there is no automatic
pause.

## 7. Duplicate detection

A duplicate is a message with the same **source identity** and the same **content hash** as
an earlier message of the same channel (MSG-06; architecture invariant 7). Detection is on
by default per source connector (`duplicate_detection`).

| Protocol | Source identity (parts joined, missing parts empty) |
| --- | --- |
| `hl7_mllp` | MSH-3, MSH-4, MSH-10 (sending application, sending facility, message control id). |
| `astm` | H.5 (sender name), H.14 (message date and time), first specimen id of the message (O.3, or the source's `sample_id_path`). |
| `au` | Unit, rack, cup and sample id as present in the profile, then the source endpoint id. |
| `delimited_file` | Relative file path, and for a record or group its row range. |

The **content hash** is SHA-256 of the stored original bytes: the MLLP payload without block
characters, assembled ASTM records with CR separators, exact AU wire frames including their
framing and check bytes, or the rewritten rows of one delimited record or group.

| Rule | Statement |
| --- | --- |
| Check | Performed inside the commit transaction by the single store writer, so two copies in flight cannot both pass. |
| Match | Same identity and same hash: the new message is stored as `duplicate` with `duplicate_of`, its body is not stored again (same hash), the sender receives a positive acknowledgement, no pipeline runs, a `message.duplicate` event is logged, and the channel's `duplicates` counter increases. |
| Same identity, different content | Not a duplicate: it is processed normally with `revision_of` pointing at the latest earlier message of that identity, and a `message.identity_reused` event is logged. |
| No identity | When every part of the identity is empty the message is never treated as a duplicate (matching on content alone could swallow legitimate repeats). |
| Scope | Per channel. A message is matched only against earlier messages in state `received`, `routed` or `filtered` (never `invalid`, `aborted`, `receiving`), so a retransmitted invalid message is rejected again. The index lives as long as the original message; purging the original ends detection for it. It survives restart. |
| Exclusions | Tap-observed messages and messages with `origin=resend` are never duplicates. |
| Visible | A duplicate appears in the Messages page with its state, the link to the original and the filter "duplicates". |

## 8. Acknowledgement levels

What each protocol can prove, and what `rejected` means for it:

| Protocol | `handed_off` | `received` | `accepted` | `rejected` |
| --- | --- | --- | --- | --- |
| `delimited_file` (folder, SFTP) | File complete in the destination (rename done, SFTP upload and rename done). Highest level. | no | no | never; write errors are attempt failures |
| `astm` | no | Receiver ACKed every frame and the transfer ended with EOT. Highest level. | no | never; a NAK after `max_frame_retries` aborts the attempt as a write failure |
| `au` class B | no | The instrument ACKed every frame. Highest level. | no | never; a frame NAKed after 3 retries fails the attempt as a write failure |
| `au` class A | Every frame was written; class A has no acknowledgement, so `ack_required` defaults to `handed_off` and higher levels are rejected. | no | no | never |
| `hl7_mllp` `ack_mode=none` | The frame was written. Highest level. | no | no | never |
| `hl7_mllp` `original` | no | no | `AA` | `AE`, `AR` |
| `hl7_mllp` `enhanced` | no | `CA` | `AA` (application acknowledgement) | `CE`, `CR`, `AE`, `AR` |

Rules:

- `ack_required` names the level at which the delivery becomes `acknowledged`. Reaching a
  lower level is recorded in `ack_level` but leaves the delivery `sending` until the required
  level arrives or `ack_timeout_seconds` passes (`timed_out`, with the reached level shown).
- An acknowledgement correlates by the sent control id (HL7 MSA-2). Anything else is ignored
  ([section 5.2](#52-outcome-unknown-and-timed-out)).
- A file destination never claims more than `handed_off`: a written file does not prove that
  the partner read it.
- An ACK code never proves that anyone read or acted on a result; the console says so in its
  labels ([operator-console.md](operator-console.md#6-messages)).

## 9. Resend and close

Operator commands ([operator-console.md](operator-console.md#5-operator-commands)).

### Resend

Parameters: the message, the destinations (`all` or a list), a mode,
`confirm_duplicate_risk`, and an optional `reason` (1 to 500 characters). The reason is stored
with the attempt cycle the resend starts (`resend_reason` on the delivery; for `new_message`,
on the new message's deliveries) and is replaced by the next resend; it is never logged.
A message that a later message revises (`revised_by`, the newest of its revision chain) is
refused with `superseded`, naming the newer message, unless `confirm_superseded` is set:
resending an older version may reissue a corrected result (CLIA §493.1291(k)).

| Mode | Effect |
| --- | --- |
| `same_message` (default) | A **new attempt cycle on the existing deliveries**. Each selected delivery returns to `queued` with attempts reset to zero (`total_attempts` keeps counting) and `next_attempt_at` now. It sends the stored transformed body, so the frozen mapping version applies. If the delivery has no stored body (`transform_error`), the destination's current mapping runs on the stored original, and the new body and `mapping_version` replace the empty ones. No new message is created. |
| `new_message` | A **new message** with `origin=resend` and `resent_from` set, carrying the same original bytes (stored once by hash). It runs the pipeline from the filters with the **current** configuration for the selected destinations; duplicate detection is skipped and no acknowledgement goes to any sender. The original message and its deliveries are untouched. Use it after a mapping or filter fix when the old transformed output must not be reused. |

Preconditions:

| Delivery state | `same_message` | `new_message` |
| --- | --- | --- |
| `queued`, `sending` | rejected: "delivery in progress" | allowed |
| `failed`, `rejected`, `cancelled` | allowed | allowed |
| `timed_out`, `outcome_unknown`, `acknowledged` | allowed only with `confirm_duplicate_risk` | allowed only with `confirm_duplicate_risk` |

Also required: the destination exists in the active configuration and is enabled. For a
message in state `invalid` only `new_message` is offered; `aborted`, `filtered` and
`duplicate` can use `new_message` too (it re-evaluates filters). The command result lists,
per destination, the new delivery or the reason none was created. A resend is logged
(`delivery.resend_requested`) with its mode.

`new_message` rejects `receiving` and tap `observed` messages. If the channel is paused or
disabled, the new message stays `received` until the channel runs; a removed channel cannot
be resent.

### Close

`close_delivery` moves a delivery in `queued`, `rejected`, `failed`, `timed_out` or
`outcome_unknown` to `cancelled`. A reason of 1–500 characters is required and stored in
`closed_reason`. A delivery in `sending` cannot be closed. Closing does not contact the
destination.

## 10. Retention

Per channel `retention.max_age_days` and `retention.max_messages` (MSG-08). Retention is
store maintenance and runs at start (after reconciliation) and every 10 minutes. The oldest
eligible messages are selected on a read connection, then deleted in transactions of at most
100 messages, each checked again, so receipts are not blocked and a run's memory stays
bounded (a transaction holds every page it changes until it commits). A body's stored
content goes in the same transaction as its last reference, also when a later frame of a
transfer replaces it.

| Message state | Eligible for purge |
| --- | --- |
| `receiving`, `received` | never |
| `routed` | only when every delivery is `acknowledged` or `cancelled` |
| `filtered`, `duplicate`, `invalid`, `aborted`, `observed` | by age or count (no delivery depends on them) |

Rules:

- A message with any delivery `queued`, `sending`, `rejected`, `failed`, `timed_out` or
  `outcome_unknown` is never purged, whatever its age or the count limit (MSG-08). Unresolved
  work can therefore outlive the limits; the Messages page filters it.
- A message that a kept message revises (its `revision_of`) is kept as long as the revision
  is, so an original stays linked to its correction; it becomes eligible once the revision
  is purged.
- Either limit triggers a purge; the oldest eligible messages go first.
- A purge removes the message, its bodies, deliveries and attempts. A duplicate's reference
  to an original disappears with it. Aggregate counters are kept.
- Each purge logs `retention.purged` with counts per channel and no content.
- Messages of a channel removed by configuration follow the default limits (90 days).
- Low disk raises `disk_low`; the engine never purges unresolved messages to make room.

## 11. Pause and resume

| | Source connector | Destinations | Store |
| --- | --- | --- | --- |
| **Pause** (operator) | Stops accepting: a listener closes (new connections are refused, so the sender sees no acknowledgement and retries), a client disconnects, a serial port is released, a folder watch stops. A message transfer in progress may finish and commit within the drain budget (`hosting.shutdown_drain_seconds`); otherwise it is aborted (`channel_stopped`). | No new attempt starts. An attempt in flight waits for its outcome up to the drain budget: if still unresolved it becomes `outcome_unknown` (reason `channel_paused`) when the message was fully sent, otherwise returns to `queued` without being counted. | Queued deliveries stay `queued`, visible and intact. `delivery_backlog` is not raised for a paused channel. Retention continues. |
| **Resume** | The source opens again; a new instance starts from the active configuration. | Delivery workers continue from the queue in receipt order; deliveries whose `next_attempt_at` has passed start at once. | Nothing is replayed or duplicated. |

Rules:

- Pause and resume apply to the channel as a whole; individual destinations are stopped by
  disabling them in configuration. A disabled destination gets no deliveries for messages
  received meanwhile, its already queued deliveries wait, and re-enabling does not create
  the missing deliveries (use `new_message` resend for those).
- The paused flag is stored and survives an engine restart; a channel disabled in
  configuration is not resumable by the operator (the command is rejected with a
  configuration reason).
- Preconditions and wording are in [operator-console.md](operator-console.md#5-operator-commands).

## 12. Configuration apply

| Change | Effect |
| --- | --- |
| Filters, destination filters, mappings, delivery policy, retention, label, tags | Applied **in place** between messages (outcome `updated`); endpoints and the source connection are not touched, so the instrument connection survives. Messages already routed keep their transformed bodies; queued deliveries keep the frozen output and the policy frozen when they were created (`ack_required`, `max_attempts`, `retry_unacknowledged`): an attempt waits for the frozen acknowledgement level even after `ack_required` changed. A changed delivery policy applies to deliveries created afterwards; its timeouts and retry delays apply from the next attempt. |
| Source or destination connector protocol or settings, referenced endpoint, endpoint definition | The channel is replaced **break-before-make**: the old instance stops as in a pause (in-flight work finishes within the drain budget; stop reason `replaced_by_config`), then the new instance starts. Store state is unaffected. |
| Destination removed | Its `queued`, `rejected`, `failed`, `timed_out` and `outcome_unknown` deliveries become `cancelled` with reason `destination_removed`; acknowledged history stays. |
| Channel removed | Stopped with `removed_by_config`; its unresolved deliveries become `cancelled` (`channel_removed`); stored messages remain readable until retention purges them. |
| Channel disabled | Stopped with `disabled_by_config`; nothing is cancelled; queued deliveries wait. |
| Channel replaced while paused | Stays paused; the new definition takes effect on resume. |

Validation, versioning and failure handling are in [configuration.md](configuration.md#10-versioning-and-apply).

## 13. Startup reconciliation

Runs once at engine start, **before any source connector opens** (ENG-05), after the store is
opened (WAL recovery, forward-only migrations) and the last valid configuration is loaded.
Steps, in order, each idempotent:

1. **Interrupted transfers.** An ASTM message in `receiving` is recovered when every
   committed frame decodes, the last frame is final and its last record starts with `L`.
   Its assembled records become the original bytes and it becomes `received` (counted
   `recovered`); normal pipeline parsing follows. Other interrupted transfers become
   `aborted` with `startup_interrupted`, their frames are kept, and they are never
   processed (counted `aborted`). AU transfers are not recovered from their frames.
2. **Unfinished pipelines.** Every message in `received` is queued to run through the
   pipeline again from parse when its channel runs (counted `requeued`). The pipeline,
   source and delivery workers then run concurrently. Deliveries are created in one
   transaction, so a message is never half routed. A paused, disabled or absent channel's
   message stays `received`.
3. **Deliveries in flight.** Every delivery in `sending` becomes `outcome_unknown` with
   reason `engine_restarted` and its open attempt is closed with result `engine_stopped`.
   The attempt record is committed before the first byte is written, so a delivery never
   starts sending without a record. Counted `unknown`.
4. **Queue.** Deliveries in `queued` are kept in receipt order with their `next_attempt_at`.
5. **Paused channels.** The stored operator-paused flags are restored; those channels stay
   stopped.
6. **Report.** One `reconciliation.completed` event with the counts; each `unknown` delivery
   is visible through `delivery_needs_attention`, each aborted transfer through its message.
   Alerts are recomputed from the store on the first snapshot.

Then sources open and deliveries resume. A crash during reconciliation repeats it. A
reconciliation that fails (the store cannot commit) is not taken as done: it is logged
(`store.error`, `reconciliation_failed`), message channels stay `stopped` with
`store_unavailable`, the node snapshot reports the store `unavailable`, and it is tried
again (10 s, doubling to 5 min) until it completes; then the message channels start. Bridge
channels have nothing to reconcile and start from the configuration.

If the store cannot be opened, the engine starts degraded: bridge channels run, message
channels report `stopped` with reason `store_unavailable` (no operator action applies to
them), the node snapshot reports the store `unavailable` and `message_store_unavailable`
(critical) is raised. The engine tries to open the store again after 10 seconds, doubling the
wait up to 5 minutes (`store.error`, `open_failed` per failed attempt). When it opens
(`store.opened`), startup reconciliation runs, then the waiting message channels start; bridge
channels are not restarted, and their taps start recording from then on. The alert clears.

## 14. Shutdown

As bridge channels ([bridge-channels.md](bridge-channels.md#14-shutdown)) with the message
rules: sources stop accepting and finish committing transfers in progress; destinations wait
for in-flight attempts up to `hosting.shutdown_drain_seconds`; an attempt still unresolved
becomes `outcome_unknown` (reason `engine_stopped`) if the message was fully sent, else
returns to `queued` uncounted. Queued deliveries persist for the next start. Final snapshots
follow.

## 15. Counters

Per message channel and per destination (full schema in
[diagnostics.md](diagnostics.md#4-channel-snapshots)): `received`, `duplicates`, `invalid`,
`filtered`, `aborted`, `bytes_in`; per destination `queued`, `sending`, `acknowledged`,
`rejected`, `timed_out`, `outcome_unknown`, `failed` (of which `transform_errors` failed in
their mapping), `cancelled`, `oldest_queued_at`, `next_attempt_at`, `last_ack_at`,
`last_error_summary`.

## 16. Non-goals

- No scripting in transformations (decisions #6); no FHIR; no TLS on MLLP.
- No fan-in (several sources into one channel) and no channel feeding another channel.
- No exactly-once claim and no automatic reconciliation against a destination.
- No priorities, no reordering, no scheduled delivery windows.
- No message bodies in logs, snapshots or alerts ([diagnostics.md](diagnostics.md#8-what-never-appears)).
