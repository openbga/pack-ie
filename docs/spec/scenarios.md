# Scenario format

Scenarios prove channel behavior end to end ([testing.md](../testing.md)). A scenario is one
JSON file in `tests/scenarios/` that loads an engine configuration, starts scripted peers
(actors) on loopback TCP, virtual serial ports and folders, runs ordered steps against the
engine, and asserts on bytes, snapshots, logs, alerts and the store. The runner lives in
`packie-sim`; this document is its contract. Behavior being tested is specified in
[bridge-channels.md](bridge-channels.md) and [message-channels.md](message-channels.md);
what each scenario proves is listed in [acceptance.md](acceptance.md).

## 1. Principles

| Rule | Statement |
| --- | --- |
| Declarative | A scenario is data. No code, no loops, no variables beyond placeholders (section 3). |
| Ordered | Steps run one after another; the next step starts when the previous one finishes. |
| Allocated resources | Ports, virtual serial pairs and folders are allocated by the runner per run and named in the file, so scenarios run in parallel without clashes ([testing.md](../testing.md#rules)). |
| Synthetic | Payloads are synthetic. Patient-like values are obviously fictional (`TEST^PATIENT`, `PID-TEST-001`). |
| Fail fast | The first failing step ends the scenario. Later steps are reported `not_run`. |
| Parse-time errors | A malformed file, unknown step, unknown actor or unresolved placeholder fails at load, before anything starts. |
| Observable | Assertions use only the public read models: bytes seen by actors, snapshots, alerts, log events, the store. |

## 2. File structure

```json
{
  "format": 1,
  "name": "bridge-tcp-full-duplex",
  "description": "One sentence.",
  "requirements": ["BRG-01"],
  "requires": [],
  "timeout_ms": 30000,
  "defaults": { "step_timeout_ms": 5000, "assert_within_ms": 5000 },
  "resources": { "ports": [], "serial_pairs": [], "folders": [] },
  "engine": { "bootstrap": {}, "runtime": {}, "start": "auto" },
  "actors": {},
  "steps": [],
  "no_body_leak": []
}
```

| Field | Type | Default | Rule |
| --- | --- | --- | --- |
| `format` | integer | none | `1`. |
| `name` | string | none | `^[a-z0-9-]+$`, equals the file name without `.json`, unique. |
| `description` | string | none | One sentence. |
| `requirements` | array of string | none | Requirement ids from [requirements.md](../requirements.md) the scenario proves. |
| `requires` | array of string | `[]` | Capabilities the host must provide: `serial_pair` (virtual serial pairs), `test_hooks` (engine test hooks: store commit gate and crash, section 7.3), `loopback_alias` (a second loopback address such as 127.0.0.2). A scenario whose capability is missing is reported `skipped`, never `passed`. |
| `timeout_ms` | integer | none | Whole-scenario limit; exceeded is a failure. |
| `defaults.step_timeout_ms` | integer | `5000` | Default `timeout_ms` of waiting steps. |
| `defaults.assert_within_ms` | integer | `5000` | Default `within_ms` of assertion steps. |
| `resources` | object | `{}` | Named allocations, section 3. |
| `engine` | object | none | Section 4. |
| `actors` | object | `{}` | Name to actor definition, section 5. Names match `^[a-z][a-z0-9_]*$`; `engine` is reserved. |
| `steps` | array | none | At least one step, section 6. |
| `no_body_leak` | array of string | `[]` | Sentinel strings that must appear nowhere in diagnostics (section 9). |

Unknown fields are errors.

## 3. Resources and placeholders

| Resource | Declared as | Allocated | Placeholders |
| --- | --- | --- | --- |
| Port | `"ports": ["name"]` | A free TCP port on 127.0.0.1, chosen at load. The engine binds it; the runner does not. | `${port.name}` |
| Serial pair | `"serial_pairs": ["name"]` | A virtual serial port pair created by the harness (needs `serial_pair`). | `${serial.name.engine}` (the path the engine opens), `${serial.name.peer}` (the end the actor drives) |
| Folder | `"folders": ["name"]` | A new empty temporary directory. | `${folder.name}` |
| Actor server | a `tcp_server` actor | Its listening port, bound at load when `listen` is `auto`, otherwise reserved. | `${actor.NAME.port}`, `${actor.NAME.host}` |
| Run | always | | `${state_dir}` (the engine state directory), `${scenario.name}` |

Substitution replaces every `${...}` in all string values of `engine`, `actors` and `steps`
before use. A string that consists of exactly one placeholder whose value is numeric becomes
a JSON number (`"port": "${port.in}"` is the integer). Any other occurrence is textual
interpolation. An unknown placeholder is a load error.

## 4. Engine

| Field | Type | Default | Rule |
| --- | --- | --- | --- |
| `engine.bootstrap` | object | `{}` | Fields merged over a generated bootstrap: node id `sim-<name>`, a fresh state directory, loopback-only, log level `debug`. May override, for example, `hosting.shutdown_drain_seconds`. |
| `engine.runtime` | object | none | The `runtime.json` document ([configuration.md](configuration.md#4-runtimejson)), after substitution. Exactly one of `runtime` and `runtime_file`. |
| `engine.runtime_file` | string | none | Path relative to the scenario file. |
| `engine.start` | string | `auto` | `auto`: the engine starts before step 1 and the runner waits until it reports `node.started` and its first snapshot. `manual`: it starts at the first `start_engine` step. |

The state directory is created fresh per run, survives `stop_engine`, `crash_engine` and
`start_engine` inside the run, and is kept (path printed) when the scenario fails.
Scenario configurations use small timers and `jitter_percent: 0` so timing is deterministic.
The runner starts the engine as a child process and uses its local management interface:
a Unix socket on Unix, or a named pipe on Windows. Each run gets its own endpoint, including
concurrent runs of the same scenario; restarts reuse the endpoint in the generated bootstrap.
`engine.bootstrap.ipc.path` can override it. Readiness requires `node.started` and a first
snapshot over that endpoint.

Graceful stop sends SIGTERM on Unix or Ctrl-Break to the engine's process group on Windows.
The Windows child shares the simulator's console and starts in its own process group, so
sibling scenarios are not signalled. If the signal cannot be sent (for example, without a
Windows console), or the child exceeds its stop timeout (the configured drain plus 10 seconds
for process exit), the runner forces termination and reports that the stop was not graceful.

Benchmark memory and CPU probes and soak descriptor counts are unavailable on Windows;
reports show those measurements as unavailable rather than zero.

## 5. Actors

An actor is a scripted peer or a folder owner. Steps address it with `"actor": "<name>"`.
The pseudo-actor `engine` receives control and assertion steps.

| Kind | Fields | Behavior |
| --- | --- | --- |
| `tcp_server` | `bind` (default `127.0.0.1`), `listen` (`auto` default: bound at scenario start; `manual`: bound by a `listen` step), `backlog` (default 16), `recv_buffer_bytes` (optional socket option) | Accepts connections into an **unclaimed queue**; the `accept` step claims the oldest and makes it the actor's current connection. |
| `tcp_client` | `bind_source` (optional local address, needs `loopback_alias` for anything but 127.0.0.1), `recv_buffer_bytes` | Connects to a target given in the `connect` step. |
| `serial_peer` | `pair` (a `serial_pairs` name), `baud_rate` (default 9600), `data_bits` (8), `parity` (`none`), `stop_bits` (`one`), `flow_control` (`none`) | Drives the peer end of a virtual pair. `connect` plugs the end in; `close` simulates unplugging (the engine's reads fail with an I/O error); a later `connect` plugs it back. |
| `folder` | `resource` (a `folders` name) | Reads and writes files in that directory. |

Every connection (TCP or serial) has:

- a **read buffer**: a background reader appends everything that arrives; `expect` steps
  consume from the front, so bytes beyond what a step expects stay for the next step;
- a **state**: `open`, `closed_by_peer` or `closed_by_actor`; bytes buffered before a close
  remain readable;
- a **reading** flag: `pause_reading` stops the reader draining the socket, so the operating
  system buffers fill and the sender is held back (backpressure); `resume_reading` restarts it.

An actor has at most one **current connection**: the last `accept` or `connect`. A step that
needs a connection when there is none fails.

## 6. Steps

Every step is an object:

| Field | Type | Rule |
| --- | --- | --- |
| `do` | string | The step type (section 7). Required. |
| `actor` | string | Required for actor steps; `engine` for engine steps. |
| `id` | string | Optional label used in reports. |
| `timeout_ms` | integer | Waiting steps: the limit (default `defaults.step_timeout_ms`). |
| `within_ms` | integer | Assertion steps: how long to poll (default `defaults.assert_within_ms`; `0` checks once). |
| `note` | string | Ignored, for readers. |

### Payload forms

`send`, `expect` and file steps take exactly one of:

| Form | Meaning |
| --- | --- |
| `"text": "..."` | The UTF-8 bytes of the string (JSON escapes such as `\r`, `\u000b` apply). |
| `"hex": "0b 4d 53"` | Hex digits; spaces and dashes ignored. |
| `"base64": "..."` | Standard base64. |
| `"generate": {"pattern": "counter32", "bytes": N, "seed": S}` | `N` bytes (a multiple of 4 is recommended) forming consecutive 32-bit little-endian counters starting at `S` (default 0). Detects loss, duplication and reordering of any whole number of words. Needs no memory in the file for large transfers. |

## 7. Step catalog

### 7.1 Actor steps

| `do` | Actors | Fields | Behavior and pass rule |
| --- | --- | --- | --- |
| `listen` | tcp_server | none | Binds the actor's port (again, after `stop_listening`, or for the first time when `listen` is `manual`). |
| `stop_listening` | tcp_server | none | Closes the listening socket. Connections already accepted continue; unclaimed queued ones are reset. |
| `accept` | tcp_server | `timeout_ms`, `expect_peer` (optional address), `expect` (`connection` default, or `none`) | `connection`: waits for an unclaimed connection and makes it current; fails on timeout or if the peer address differs. `none`: passes when no connection arrives within `timeout_ms`. |
| `connect` | tcp_client | `host`, `port`, `expect` (`ok` default, or `refused`) | Connects. `ok` passes when the TCP connection is established (the engine may reset it afterwards; observe that with `expect_closed`). `refused` passes when the connection is refused. |
| `connect` | serial_peer | none | Opens the peer end. |
| `send` | any connection actor | payload form; `chunk_size`, `chunk_delay_ms` (split the write); `async` (boolean) with `name` | Writes all bytes. With `async` the step returns at once and the write continues; finish it with `await_send`. A write failure fails the step. |
| `send_message` | any connection actor | `protocol`, protocol fields, `expect_reply`, `reply_timeout_ms`, `async`, `name` | Protocol-aware send, below. |
| `await_send` | any | `name`, `expect` (`complete` default, or `pending`), `for_ms` | `complete`: waits until the named async operation finished successfully. `pending`: passes when it has **not** finished after `for_ms`. |
| `expect` | any connection actor | payload form, `timeout_ms` | The next bytes in the read buffer, after waiting, are exactly the payload. Fails on timeout, mismatch (shows expected and observed) or close before enough bytes arrive. |
| `expect_message` | any connection actor | `protocol`, `reply`, `reply_delay_ms`, `assert`, `timeout_ms` | Protocol-aware receive, below. |
| `expect_closed` | any connection actor | `timeout_ms` | Within the timeout the connection reaches end of stream or reset **and** the read buffer is empty. Unread bytes fail the step ("N bytes before close"). |
| `expect_open` | any connection actor | `for_ms` (default 0) | The connection is open now and stays open (no end of stream, no reset) for `for_ms`. |
| `expect_no_data` | any connection actor | `for_ms` | The read buffer stays empty for `for_ms` and the connection stays open. |
| `pause_reading`, `resume_reading` | any connection actor | none | See section 5. |
| `close` | any connection actor | `abort` (default `false`) | Graceful close (FIN), or reset (`abort: true`). For `serial_peer`: unplug. |
| `wait` | any | `ms` | Sleeps. Use only to let wall-clock timers run; prefer assertions with `within_ms`. |
| `write_file` | folder | `name`, payload form, `atomic` (default `true`) | Writes `name` in the folder. `atomic`: written under a temporary name then renamed. |
| `expect_file` | folder | `name` or `pattern` (glob), `equals` (a payload form object, for example `{"text": "..."}`) or `contains` (array of strings), `timeout_ms` | Waits for a matching complete file whose content satisfies the rule. `name` and `pattern` are relative to the folder; `*` does not match `/`. |
| `expect_no_file` | folder | `pattern`, `for_ms` | No matching file appears during the window. |
| `delete_file` | folder | `name` | Removes a file. |

#### Protocol-aware send and receive

| `protocol` | `send_message` fields | `expect_message` behavior |
| --- | --- | --- |
| `hl7_mllp` | `segments`: array of segment strings. Joined with `\r` and framed with MLLP (0x0B … 0x1C 0x0D). `expect_reply`: one code or an array (`AA`, `AE`, `AR`, `CA`, `CE`, `CR`), or `none` (default `AA`). The reply frames must arrive in order with the given MSA-1 codes and, when the sent message has an MSH, MSA-2 equal to its MSH-10. A segment list without MSH (an invalid message on purpose) is sent as is. | Reads one MLLP frame, parses it, and checks `assert` (list of `{ "path", "equals" | "matches" | "exists" }` over the message paths of [configuration.md §8](configuration.md#8-filters-and-mappings)). Then sends `reply`: one code or an array sent in order `reply_delay_ms` apart, or `none`. Each reply is an ACK built from the received MSH-10. Default `reply` is `AA`. |
| `astm` | `records`: array of record strings (`H|\\^&|...`). The runner acts as the sender: ENQ, expects ACK; sends one frame per record with frame number and checksum (E1381), expects ACK for each; EOT. `partial` (optional): `{"frames": n}` stops after `n` frames and leaves the transfer open; `{"omit_eot": true}` sends every frame but not EOT. A missing ACK within `reply_timeout_ms` fails the step. | Acts as receiver: with `reply: "ack"` (default) answers ENQ with ACK, ACKs each valid frame (`nak_frames: n` optionally NAKs the first `n` frames once to force retransmission) and accepts EOT; with `reply: "none"` it never answers ENQ. After the transfer it checks `assert` over record paths (`R.3`, `O.3`). |
| `au` | not defined: AU scenarios use `send` and `expect` with `hex` frames of the profile under test. | |

### 7.2 Engine steps (`"actor": "engine"`)

| `do` | Fields | Behavior |
| --- | --- | --- |
| `start_engine` | none | Starts the engine on the existing state directory and waits for `node.started` (which follows startup reconciliation) and the first snapshot. |
| `stop_engine` | none | Sends SIGTERM on Unix or Ctrl-Break on Windows, with the configured drain; waits for exit and `node.stopped`. A forced kill fails the step. |
| `crash_engine` | none | Needs `test_hooks`. Ends the engine abruptly: no drain, no flush beyond what the store already committed, no graceful close, equivalent to killing the process. Sockets close at the operating system. |
| `pause_channel`, `resume_channel`, `restart_channel` | `channel`, `expect` (`accepted` default, or `rejected`), `reason_code` | Operator commands ([operator-console.md §5](operator-console.md#5-operator-commands)). With `rejected`, the result must carry `reason_code` when given. |
| `resend` | `channel`, `message` (selector, below), `destinations` (`all` or array), `mode` (`same_message` default, `new_message`), `confirm_duplicate_risk` (default `false`), `expect`, `reason_code` | Resend ([message-channels.md §9](message-channels.md#9-resend-and-close)). The selector must match exactly one message. |
| `close_delivery` | `channel`, `delivery` (selector over delivery fields, must match exactly one), `reason`, `expect`, `reason_code` | Close a delivery with a reason. |
| `validate_config` | `runtime` or `runtime_file`, `expect` (`valid` default, or `invalid`), `problems` (array of codes that must all be present) | Dry run. |
| `apply_config` | `runtime` or `runtime_file`, `expect` (`applied` default, `rejected`, or `partial`), `outcomes` (map channel id to outcome: `unchanged`, `updated`, `started`, `restarted`, `stopped`, `removed`, `failed`), `problems` (codes, for `rejected`) | Applies with the active version as `base_version`. When given, `outcomes` must match exactly for the listed channels. |
| `hold_store_commits`, `release_store_commits` | none | Needs `test_hooks`. While held, the store writer accepts requests but completes no commit; release lets them finish. Used to prove acknowledgements wait for the commit. |

### 7.3 Assertion steps (`"actor": "engine"`)

All poll until they pass or `within_ms` elapses, then fail showing the last observed values.

| `do` | Fields | Passes when |
| --- | --- | --- |
| `assert_snapshot` | `target` (`{"channel": id}`, `{"endpoint": id}` or `{"node": true}`), `expect` (map of path to matcher) | A freshly built snapshot satisfies every matcher. |
| `assert_alert` | `code`, `scope` (optional `channel`, `endpoint`, `destination`), `present` (default `true`), `expect` (map of alert field path to matcher) | An alert with that code and scope exists and matches (`present: true`), or no **active** alert matches (`present: false`, used to prove an alert clears or was never raised). |
| `assert_log` | `event`, `fields` (map of log field to matcher), `count` (integer or matcher, default `{"gte": 1}`) | The number of log events since the engine's first start that have this `event` and match `fields` satisfies `count`. |
| `assert_store` | `table` (`messages`, `deliveries`, `attempts`, `alerts`), `where` (map of field to matcher), `count` (integer or matcher, default `{"gte": 1}`), `each` (map of field to matcher every selected row must satisfy) | The store, read through a read-only connection, selects rows by `where`, the count satisfies `count`, and every row satisfies `each`. |

**Matchers.** A bare JSON value means equality (`null` allowed). An object with one operator:
`eq`, `ne`, `gt`, `gte`, `lt`, `lte`, `in` (array), `contains` (substring), `matches`
(regular expression), `exists` (boolean).

**Paths.** Snapshot and alert paths use dots into the JSON of
[diagnostics.md](diagnostics.md): `endpoint_a.state`, `counters.bytes_a_to_b`,
`buffers.a_to_b.overflowing`. An array element is selected by `[key=value]`:
`destinations[destination_id=lis].queue.queued`.

**Store fields.** `messages`: the message record fields of
[message-channels.md §3](message-channels.md#3-message-states) plus `original_text` and
`original_hex` (the stored original bytes). `deliveries`: the delivery record fields of
§4, `transformed_text`, `transformed_hex`, and the parent message fields as `message.<field>`
(for example `message.sample_id`). `attempts`: the attempt record fields and `delivery.<field>`
and `message.<field>`. `alerts`: the alert fields. Every table accepts `channel_id`.

A **selector** (in `resend`, `close_delivery`) is a map of field to matcher over the same
fields, evaluated as `assert_store` rows. `where` on `messages`, for example
`{"channel_id": "x", "sample_id": "SID-1"}`.

## 8. Execution semantics

| Topic | Rule |
| --- | --- |
| Order | Steps run strictly in order. A step finishes when its condition holds or its timeout elapses (failure). Asynchronous sends (`async`) are the only concurrency. |
| Start | Actors with `listen: auto` start listening before the engine starts. Serial pairs and folders exist before the engine starts. |
| Timeouts | Waiting steps use `timeout_ms`; assertions use `within_ms`; the scenario has `timeout_ms`. |
| Failure | A failing step records the expected and observed values, ends the scenario, closes all actors, stops the engine (graceful, then killed if signalling fails or the stop timeout expires), keeps the state directory, and reports `failed`. |
| Cleanup | On every outcome all actors are closed and temporary resources released; the state directory is deleted on success. |
| Skip | A missing capability reports `skipped` with the capability name. The gate may refuse skips (`--deny-skips`). |
| Reporting | One JSON line per step: scenario, index, `id`, `do`, `actor`, status (`passed`, `failed`, `not_run`), duration, detail; then a summary line. Payload bytes are shown for failures (all scenario data is synthetic). Exit code `0` all passed or skipped, `1` any failed, `2` a scenario file is invalid. |
| Parallel | Scenarios are independent and may run in parallel; they share nothing. |
| Time | Real time. There is no virtual clock in version 1; scenarios shorten timers in their configuration. |

## 9. Global assertions

Applied to every scenario at its end (and after each `stop_engine` or `crash_engine`), without
being listed in `steps`:

| Check | Rule |
| --- | --- |
| No panic | No panic or abnormal engine exit other than one caused by `crash_engine`. No log event with level `error` and event `store.error`, unless the scenario holds commits or crashes the engine. |
| `no_body_leak` | Each string in `no_body_leak` appears in none of: the log files, the log ring, any snapshot the runner took, any alert, any error summary. Matching is case-sensitive on the plain string; also checked in the hex and base64 encodings of the string. |
| Clean state | The engine stopped within its drain budget at the end. |

## 10. Scenario index

Files are in [`tests/scenarios/`](../../tests/scenarios/). The rows are grouped like
[acceptance.md](acceptance.md).

| Scenario | Proves | Capabilities |
| --- | --- | --- |
| [bridge-tcp-full-duplex](../../tests/scenarios/bridge-tcp-full-duplex.json) | Bytes pass unchanged in both directions; counters and states | none |
| [bridge-close-propagation](../../tests/scenarios/bridge-close-propagation.json) | Close on one side closes the other; retry and recovery | none |
| [bridge-listener-allowlist](../../tests/scenarios/bridge-listener-allowlist.json) | Unauthorized peer rejected, alert, channel unchanged; allowed after apply | none |
| [bridge-listener-single-session](../../tests/scenarios/bridge-listener-single-session.json) | Second peer reset, first session intact, listener stays bound | none |
| [bridge-reconnect](../../tests/scenarios/bridge-reconnect.json) | Late peer, backoff, retry state, reconnect, counter reset | none |
| [bridge-overflow-degrade](../../tests/scenarios/bridge-overflow-degrade.json) | `block_then_degrade`: degrade, alert, no loss, recovery | none |
| [bridge-receiver-then-sender](../../tests/scenarios/bridge-receiver-then-sender.json) | Unbounded receiver wait, sender listener bound only after the receiver attaches, bounded second leg | none |
| [bridge-tap-hl7](../../tests/scenarios/bridge-tap-hl7.json) | HL7 tap counts and records without altering the stream; no deduplication | none |
| [bridge-serial-tcp](../../tests/scenarios/bridge-serial-tcp.json) | Serial to TCP bridge, 8-bit clean, idle reads, hot unplug | `serial_pair` |
| [bridge-pause-resume](../../tests/scenarios/bridge-pause-resume.json) | Operator pause and resume of a bridge | none |
| [config-apply-per-channel](../../tests/scenarios/config-apply-per-channel.json) | Apply restarts only the changed channel; invalid document leaves the active one | none |
| [msg-astm-receive-ack-after-commit](../../tests/scenarios/msg-astm-receive-ack-after-commit.json) | ASTM receive; frame ACK only after the store commit; delivery to a destination | `test_hooks` |
| [msg-astm-interrupted-transfer](../../tests/scenarios/msg-astm-interrupted-transfer.json) | Crash mid-transfer: incomplete aborted, complete recovered | `test_hooks` |
| [msg-hl7-in-ack](../../tests/scenarios/msg-hl7-in-ack.json) | HL7 MLLP in: original and enhanced ACKs, invalid message, source filter, mapping to CSV | none |
| [msg-hl7-out-ack-tracked](../../tests/scenarios/msg-hl7-out-ack-tracked.json) | File in, HL7 out: acknowledged, rejected, timed out, outcome unknown tracked | none |
| [msg-duplicate](../../tests/scenarios/msg-duplicate.json) | Duplicate recorded not processed; same identity with new content processed | none |
| [msg-resend](../../tests/scenarios/msg-resend.json) | Resend same message, resend as new message, close with reason | none |
| [msg-transform-error](../../tests/scenarios/msg-transform-error.json) | Transform error keeps message, alerts, fixed by in-place apply and resend | none |
| [msg-pause-resume](../../tests/scenarios/msg-pause-resume.json) | Pause stops the source, resume continues; queue preserved | none |
| [msg-crash-restart-reconcile](../../tests/scenarios/msg-crash-restart-reconcile.json) | Crash and restart: in-flight becomes outcome unknown, queue continues in order | `test_hooks` |
| [msg-astm-enq-retries](../../tests/scenarios/msg-astm-enq-retries.json) | ASTM destination `enq_attempts` and `busy_retry_ms`: a second ENQ only after the busy wait, then the delivery fails | none |
| [msg-astm-long-records](../../tests/scenarios/msg-astm-long-records.json) | Long ASTM record received in ETB frames is stored byte for byte; a destination limited to 64-character frames sends the expected frames and checksums | none |
| [msg-astm-specimen-search](../../tests/scenarios/msg-astm-specimen-search.json) | ASTM source reading the specimen id from `O.4.3`: stored with that id, found by it, sent as the documented ORU | none |
| [msg-au-mapping](../../tests/scenarios/msg-au-mapping.json) | AU results received as framed wire text mapped to ORU^R01 with the AU preset | none |
| [msg-calc-exact-resend](../../tests/scenarios/msg-calc-exact-resend.json) | Exact-decimal `calculate`, `append` and text steps over repeated ASTM results; division by zero fails only one destination until an in-place fix and a resend to it alone | none |
| [msg-calc-in-place-sn](../../tests/scenarios/msg-calc-in-place-sn.json) | HL7 to HL7 `for_each` update: repeated OBX rewritten in place, NM and structured numeric values converted, SN range and trailing NTE kept | none |
| [msg-calc-multigroup-refused](../../tests/scenarios/msg-calc-multigroup-refused.json) | ASTM transmission with two patients and an appended calculation fails with a transform error; nothing is sent | none |
| [msg-hl7-end-without-cr](../../tests/scenarios/msg-hl7-end-without-cr.json) | `require_end_cr: false`: MLLP frames ending at 0x1C acknowledged one by one; a destination acknowledgement without the CR accepted | none |
| [msg-hl7-order-response](../../tests/scenarios/msg-hl7-order-response.json) | `reply_type: order_response`: OML^O33 answered with a commit ACK and an ORL^O34 reply; a destination accepting ORL records the order as acknowledged | none |
| [msg-order-download-limits](../../tests/scenarios/msg-order-download-limits.json) | HL7 order with 18 tests becomes one ASTM order record; 19 tests is stored and acknowledged, its delivery fails with a transform error naming the limit | none |
