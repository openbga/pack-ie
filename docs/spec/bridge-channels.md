# Bridge channel specification

Runtime behavior of bridge channels: lifecycle, pairing, buffering, retry, close propagation
and transport rules. Vocabulary is in [architecture §2](../architecture.md#2-domain-model);
settings are in [configuration.md](configuration.md#62-bridge-channel); state fields, events
and alerts are in [diagnostics.md](diagnostics.md). Requirements: BRG-01..05, EPT-01, EPT-02,
EPT-05, EPT-06, ENG-02, ENG-03, SEC-01, PRF-01.

## 1. Scope

A bridge channel connects endpoint **A** and endpoint **B** and passes bytes in both
directions unchanged. Endpoints are `serial`, `tcp_client` or `tcp_listener`; the sides have
no inherent roles (instrument or system), only the labels the operator gives them.

| Rule | Statement |
| --- | --- |
| Ownership | One channel supervisor owns one bridge channel, its two endpoint instances, its buffers and its health state. A failure stays inside that channel. |
| Session | Each channel instance holds at most one live session. Client-only replacements can overlap with the previous instance during configuration apply (section 13). |
| Bytes | Bytes keep their order within a direction and are never altered. Buffer pressure does not discard bytes while the session lives. Buffers are in memory; a failed session or a stop can discard undelivered bytes, which are not replayed after reconnect. Internal buffering works on byte segments, not protocol messages. |
| Isolation | Channel health, retry and buffering never depend on protocol parsing. A tap ([section 11](#11-framing-and-tap)) only observes. |
| Independence | Bridge traffic does not depend on the console, the store or any other channel. |
| Latency | PRF-01's target is under 2 ms added p99 latency. `packie-sim bench --target prf01` measures loopback round trips through a TCP bridge against a direct connection to the same echo peer. |

Flow: `A reader → A→B buffer → B writer` and `B reader → B→A buffer → A writer`.

## 2. Channel lifecycle

| State | Meaning |
| --- | --- |
| `starting` | Initial state of a channel instance, until the first session is established or the first attempt fails. |
| `running` | The session is bridged and traffic flows normally. |
| `retrying` | An endpoint failed or a session ended and a reconnect, relisten or re-pairing is scheduled or in progress. |
| `degraded` | The session is up with a buffer overflow episode in progress. |
| `stopped` | Not running, with a stop reason: `operator_paused`, `disabled_by_config`, `removed_by_config`, `replaced_by_config`, `shutdown`. |

| From | To | Trigger |
| --- | --- | --- |
| (new) | `starting` | Instance created. |
| `starting`, `retrying` | `running` | Both endpoints ready and the session is established. |
| `starting`, `running`, `degraded` | `retrying` | An endpoint failure, a session end or a pairing timeout; the first attempt failing leaves `starting` for `retrying`. |
| `running` | `degraded` | An overflow episode starts ([section 6](#6-buffers-and-backpressure)). |
| `degraded` | `running` | The episode ends. |
| any | `stopped` | Disable, removal, replacement, operator pause, shutdown. |
| `stopped` (`operator_paused`) | `starting` | Resume: a new instance starts from the active definition. |

Rules:

- `starting` is only the initial state. Once the first attempt fails or a session ends,
  every later attempt reports `retrying` until a session is established again, so a channel
  that is down never reads as starting. While an attempt waits for its peers the session
  state is `waiting_for_peer` (section 3).
- A channel that waits for an instrument to connect is one long attempt: the retry counter
  does not rise while waiting. The wait reads `starting` the first time and `retrying` after
  a session has ended, with session state `waiting_for_peer` and the receiving endpoint
  `listening`. Alert `no_peer_connected` keys off time in `waiting_for_peer`, using
  `alerts.no_peer_seconds` (`0` disables it).
- An intentionally stopped channel schedules no retry work.
- Whether a channel degrades the node is defined in [diagnostics.md](diagnostics.md#3-engine-and-node-snapshot).

## 3. Session lifecycle

| State | Meaning |
| --- | --- |
| `waiting_for_peer` | An attachment attempt is in progress; zero or one side is attached. |
| `bridged` | Both sides are attached and the pumps run. |
| `closing` | The session has ended; the supervisor is completing the transition to retry. |
| `closed` | No session exists. |

- Session state is tracked separately from channel state.
- A session is healthy only while both directions are readable and writable according to
  the transports' capabilities.
- When one side closes or fails, the other is closed and the channel follows the retry
  policy ([sections 7 and 8](#7-retry-and-backoff)).

## 4. Endpoint lifecycle

Endpoint states are `disabled`, `connecting`, `listening`, `connected`, `retrying`,
`disconnected`, `error` ([architecture §2](../architecture.md#2-domain-model)).

| State | Meaning inside a bridge channel |
| --- | --- |
| `disabled` | The endpoint is disabled in configuration. An enabled channel cannot reference it. |
| `connecting` | Opening a connection or port. |
| `listening` | A listener socket is bound and accepting. |
| `connected` | Ready for I/O (client connected, peer accepted, serial port open). |
| `retrying` | The last attempt failed and a retry is scheduled. |
| `disconnected` | Intentionally idle: the channel is stopped, or the endpoint has no running channel. |
| `error` | A persistent attachment failure, such as permission denied, unsupported serial settings or a bind failure other than address in use. Retries continue at `max_delay_ms`. |

Rules:

- The two endpoints of a channel track state independently.
- Endpoint health belongs to the channel instance per endpoint role and survives reconnect
  attempts. Links are attached per attempt; bound listeners are kept. While a
  retry is scheduled, an endpoint that must reconnect or reopen reports `retrying`; a
  listener whose socket is bound keeps reporting `listening`, and the channel is `retrying`.
- `consecutive_failures` counts failed attempts and session ends attributed to that
  endpoint (connect, listen, accept, open, read, write, or a pairing timeout on that side).
  It clears, with an `endpoint.retry_succeeded` event, once a session is stable again
  (section 7). At `retry.max_consecutive_failures_before_alert`, retry scheduling is logged
  at error level. Alert `endpoint_down` requires an endpoint in `retrying`, `error` or
  `disconnected`, and either that failure threshold or `alerts.endpoint_down_seconds`.
- Channel state derives from the supervisor phase and buffer health.
- A listener endpoint may stay `listening` while the session is `waiting_for_peer`.
- A paused channel leaves its endpoints `disconnected` and schedules no retry.

## 5. Pairing strategies

`connection.connect_strategy` decides in what order the two sides are brought up and which
waits are bounded.

| Strategy | Behavior |
| --- | --- |
| `parallel` | Both sides start at once. `pairing_timeout_ms` starts when the first side attaches and bounds the wait for the second. A transport failure on either side (for example a refused connect) fails the attempt immediately. Waiting for the first side is unbounded. |
| `wait_for_listener_then_pair` | Accepted for compatibility; behaves exactly like `parallel`. Configuration requires a `tcp_listener` endpoint in the channel. |
| `receiver_then_sender` | The side named by `connection.receiver` (`a` or `b`) is prepared and accepted first. The other side (the sender) is not connected, accepted or, for serial, opened until the receiver has attached. `pairing_timeout_ms` starts when the receiver attaches and bounds only the second leg: from "receiver attached" to "sender connected". |

The first attachment has no pairing deadline. A listener can wait for an idle instrument
for hours without increasing the retry counter or recording a failure. Transport failures,
TCP connect timeouts and channel stops still end the attempt; under `parallel`, a failure on
the other side also ends it.

Further rules:

- Under `receiver_then_sender`, a listener on the sender side is bound only while the
  receiver is attached. This is the one exception to "listeners stay bound across retry
  backoff" ([section 10](#10-listener-and-tcp-client-behavior)).
- If the second side does not attach within `pairing_timeout_ms`, the already attached peer
  is closed, the failure is attributed to the side that did not attach (an `accept` failure
  for a listener, a `connect` failure for a TCP client or an `open` failure for serial, with
  the cause "pairing timeout"), and the channel goes to `retrying`.

## 6. Buffers and backpressure

Each direction has its own bounded buffer; bounds are `buffer.max_buffered_segments` and
`buffer.max_bytes`, enforced together.

| Rule | Statement |
| --- | --- |
| Reservation | Bytes are reserved before a segment enters the buffer. A single segment larger than the whole byte budget is admitted only when the buffer is empty, so one oversized read cannot deadlock the channel. |
| Counting | Channel byte counters count a segment only after it has been accepted into the buffer. Endpoint read counters advance when bytes are read, before enqueueing. |
| Overflow action | `block_then_degrade` (the only action). A write into a full buffer blocks for up to `block_timeout_ms`. |
| Episode start | When the block timeout is exceeded, an overflow episode starts for that direction: its `overflow_count` increases once and one `buffer.overflow_detected` event is logged. The channel is `degraded` while either direction overflows. The blocked push then keeps waiting without a timeout. |
| Backpressure | A blocked buffer push stops the reader, so pressure reaches the transport (TCP flow control, configured serial hardware handshake). Nothing is discarded while the session lives. |
| Episode end | The episode ends, and the channel returns from `degraded` to `running`, when the overflowing buffer has drained to half of both limits and no other direction of the channel is still overflowing. |
| Scope | Per channel and per direction, never global. |
| Visibility | Segments, bytes, limits and the overflowing flag per direction are in the snapshot. Occupancy at `buffer_high_percent` of either limit for `buffer_high_seconds` raises `bridge_buffer_pressure`; an overflow episode raises it without that wait. |

A slow consumer stalls its producer. The occupancy gauges count queued segments; a segment
already held by a writer and the reader's pending segment are outside those gauges.

## 7. Retry and backoff

| Rule | Statement |
| --- | --- |
| Policy | Both endpoints use the channel's `retry` policy: exponential backoff with jitter ([configuration.md](configuration.md#63-retry-policy)). |
| One loop | Each instance's supervisor establishes a session, pumps until it ends, waits on a stop-cancellable retry timer, and repeats. Serial opens and transient TCP accept failures have short retries inside their adapters (sections 9 and 10). |
| `retry_count` | Increases once per scheduled recovery cycle. |
| Reset | `retry_count` resets when the re-established session is **stable**: its first forwarded write, or the session staying up for `max_delay_ms`; and when the channel enters intentional `stopped`. Reaching `running` alone does not reset it, so a peer that accepts and immediately drops keeps backing off exponentially instead of being redialled at `initial_delay_ms` forever. It can therefore be above 0 briefly after a reconnect. |
| `next_retry_at` | Set while a scheduled retry waits; cleared when the attempt starts. |
| Failure streak | `consecutive_failures` per endpoint (section 4); a reconnect that becomes stable clears it. |
| Effect | An endpoint failure on either side moves the channel to `retrying`. A buffer overflow episode moves a running channel to `degraded`. |

## 8. Close propagation

- EOF, disconnect or a fatal write failure on either side closes the other side.
- A clean end of stream first delivers what that side already sent: the other side is closed
  once that direction's buffer has drained, or after `block_timeout_ms`, whichever comes
  first. A read or write failure closes both sides at once.
- Propagation is bidirectional and not configurable.
- After propagation completes the channel enters `retrying` unless it was intentionally
  stopped.
- A `bridge.close_propagated` event names the channel, the side that ended, the cause and the
  next action.
- When both endpoints fail in the same recovery window the engine runs one recovery cycle and
  emits one next-action summary, not duplicated retry cascades. The first EOF or pump failure
  supplies the retry cause; later errors do not schedule additional recovery cycles.
- A close that happens because the channel is stopping is part of the stop: it sets no error
  summary, counts no endpoint failure and schedules no retry.

## 9. Serial endpoints

| Rule | Statement |
| --- | --- |
| Open | A serial endpoint leases or opens its port during the channel attempt, in strategy order (under `receiver_then_sender` a sender-side port is not opened before the receiver is ready). OS opens run on blocking threads; stopping cancels the channel's wait, and a cancelled open releases the port's claim at once, so the next attempt or instance can open it. |
| Open retry | A port that is briefly locked after a previous close (a Windows sharing violation, a device still releasing) is retried with short asynchronous waits (up to 2 s in total) without holding up other channels. A port that stays locked fails the attempt. |
| Port host | Physical ports are owned by a process-wide port registry and stay open across reconnect attempts. A channel attempt leases the port and releases it when the session ends. |
| Single holder | A port is held by one channel instance at a time. A replacement whose attempt starts while the previous instance still holds the port fails that attempt (`endpoint in use by channel <id>`) and succeeds after release, so two instances never split the device's bytes. A port shared by several channels (configuration allows it with a warning) is therefore used by one channel at a time. |
| Lease key | The registry uses the lowercased port name, or the `virtual_link` path, as its key. It reuses an unleased port only if the full serial settings match and the reader thread is still alive; otherwise it closes and reopens the port. Any claim while another holder exists is refused. |
| Read pacing | The registry reads only while a channel holds the port, and at most a small fixed number of chunks wait between its reader thread and the channel. While no channel holds the port, or the channel applies backpressure, reading stops, so the device buffer and hardware flow control (`rts_cts`) hold the instrument back instead of memory growing; the bytes arrive once the channel reads again. |
| Read timeout | `read_timeout_ms` is a poll window. A read that times out with no data is not a disconnect: the endpoint keeps waiting until data arrives, the channel stops or the port errors. |
| Write timeout | Each driver write uses `write_timeout_ms`. A write error or timeout ends the session and triggers reconnect. A partly sent segment is not replayed after reconnect. |
| Hot unplug | A read or write error from a removed device ends the session; the channel enters `retrying`, reports the serial failure, and reconnects when the device is back. |
| Virtual ports | A configured virtual COM or pseudo-terminal port can use the physical-port adapter. On Unix, `virtual_link` creates a pseudo-terminal pair and exposes its peer through a symlink; the engine keeps that peer open, so closing an external program does not simulate unplugging it. `virtual_link` is not supported on Windows. |

## 10. Listener and TCP client behavior

### Listener (`tcp_listener`)

| Rule | Statement |
| --- | --- |
| Single peer | A listener accepts at most one active peer session per channel. |
| Allowlist | A peer whose address does not match `allowed_peer_cidrs` is rejected at accept, before any byte is read, and `listener.peer_rejected` is logged (alert `peer_rejected`). A non-empty allowlist is enforced; an empty one accepts any peer. |
| Additional peer | A peer that connects while a session is active is reset, logged as a warning and counted as `listener.peer_rejected`. The active session is never replaced by the newcomer. A peer arriving as the previous connection closes waits up to 250 ms for that session to release the listener before it is refused, so a sender that reconnects at once is accepted. A rebooted instrument whose old connection is half-open is accepted once keepalive or `idle_timeout_seconds` has dropped the old connection. |
| No state change | Rejecting a peer does not change channel state unless a pairing timeout or other failure is already in progress. |
| Stays bound | The listening socket stays bound across peer read/write failures, pairing timeouts, failure of the other side and retry backoff. An allowed peer arriving between sessions can be accepted and held by the transport until the next attachment attempt. An error returned by the acceptor drops it so the next attempt binds again. |
| Accept errors | A TCP socket accept error is logged as `listener.accept_failed`; the transport keeps the socket bound and retries after 100 ms. |
| Exception | The sender-side listener under `receiver_then_sender` is bound only while the receiver is attached. |
| Pairing | See section 5: the first attach is unbounded; the second side must attach within `pairing_timeout_ms`, otherwise the accepted peer is closed and the channel goes to `retrying`. |
| Disconnect | When the peer disconnects the session is torn down, the endpoint returns to `listening` and the channel to `retrying`. |
| Idle timeout | With `idle_timeout_seconds > 0` a peer silent in both directions that long is dropped; the listener keeps listening. |
| Keepalive | Enabled on the accepted socket by default; detects a peer that vanishes without closing. |
| Bind | Only to `bind_address`. A bind failure puts the endpoint in `retrying` (address in use) or `error` and is reported with the port and address. |

### Client (`tcp_client`)

| Rule | Statement |
| --- | --- |
| Connect | One dial operation per channel attempt, bounded by `connect_timeout_ms` including DNS resolution. Resolved addresses are tried in order until one connects; if all fail, the last error is reported as a `connect` failure of that endpoint. |
| Keepalive and idle | As configured. A peer that accepts and immediately resets is a normal session end and backs off exponentially (section 7). |
| Reconnect | After any end of the session the channel reconnects through the retry policy. |

## 11. Framing and tap

- A bridge is raw byte pass-through. Buffers hold segments; health, retry and buffering do
  not depend on message parsing.
- `tap.framer` (`astm`, `hl7_mllp`, `au`) attaches a **tap** to each direction. It receives
  copies of segments accepted into the bridge buffer, before their delivery is known. It
  cannot alter the forwarded bytes. Each framer has a queue of 256 segments; enqueueing
  never waits. A full or closed framer queue drops that segment's observation and increments
  `bridge.tap.tap_dropped`; the bridge keeps running. Session end aborts the tap tasks, so
  queued observations can also be lost.
- The tap counts complete messages per direction (`bridge.a_to_b.messages`,
  `bridge.b_to_a.messages`) and framing errors (`bridge.tap.framing_errors`). Counts are
  `null` when no framer is active.
- With `tap.record_messages=true`, complete observations are offered to the store sink.
  Successful records have origin `tap`, state `observed`, direction `a_to_b` or `b_to_a`,
  and no deliveries. Stored bytes exclude protocol framing. The store does not deduplicate
  these messages; identical complete messages can be recorded repeatedly. Within an ASTM
  transfer, a valid frame with the same number as the last accepted frame is ignored as a
  retransmission.
- Recording is best effort. The store sink has a queue of 1024 complete observations; a
  full or closed queue logs `tap.dropped`, and a failed commit logs `store.error` with
  `tap_record_failed`. These losses do not increase `bridge.tap.tap_dropped`.
- Recorded messages are kept by `tap.retention` (default 30 days, no count limit; the same
  shape as a message channel's `retention`). The store's retention run, after startup
  reconciliation and every ten minutes, purges a bridge's oldest recorded messages beyond
  either limit; they have no deliveries, so none is held back.
- If the store cannot open at startup, bridges and framing counters keep running. Store
  opening retries after 10 s, doubling to 5 min. Observations before it opens are not saved
  or counted as `tap_dropped`. Once it opens, recording starts without restarting bridges;
  earlier observations are not recovered ([decision 21](../decisions.md)).
- Framers count garbage runs, malformed frames and over-long messages as framing errors
  and resynchronize. `tap.max_message_bytes` limits the unframed message payload. HL7 stores
  the MLLP payload; ASTM joins frame text through the terminator record; AU joins block text
  through the final frame. A partial message at session end is not emitted.
- The AU tap assumes class A framing: `STX`/`ETX`, a block check character, `ETB` blocks
  and a 1024-byte wire-frame limit. A bridge has no AU profile setting; other instrument
  framing settings can produce errors instead of observations.
- The live traffic transcript is a separate facility ([operator-console.md](operator-console.md#7-live-traffic-transcript)).

## 12. Startup and operator stop

### Startup order for one channel

1. The engine validates the definition before apply. The registry creates the channel
   state, endpoint cells and buffers, launches the supervisor, and publishes the instance
   without waiting for its endpoints.
2. The supervisor resolves the two endpoint definitions and creates their openers.
3. Under `parallel` or `wait_for_listener_then_pair`, bind listeners first, then attach
   both sides concurrently. Under `receiver_then_sender`, attach the receiver before
   preparing the sender.
4. Pair the two links and publish `running` with session state `bridged`.

If an attachment attempt fails the channel enters `retrying`, the last error is captured,
and retry follows policy. Creating an instance launches its lifecycle in the background
and returns immediately; startup and resume do not wait for peers. Configuration apply can
wait for a client-only replacement within the bound in section 13. The instance runs
independently of the request that started it until stopped.

### Stop, pause, resume, restart

| Operation | Effect |
| --- | --- |
| Pause | Channel becomes `stopped` with `operator_paused`; pending retry work is cancelled; the session closes without draining; endpoints go `disconnected`. Pause flags are saved atomically in `<state>/routing/paused.json`. A failed save logs `routing.paused_not_saved` and holds only until restart; a missing or unreadable file starts no channels paused. |
| Resume | Allowed only for a channel `stopped` with `operator_paused` that still exists and is enabled. A new instance starts from the active definition; the stop reason clears only once the new instance is created. Rejected when the channel is missing, disabled by configuration or already active. |
| Restart | Allowed for an enabled channel that is not operator-paused. Stops the instance (no drain, reason `operator_restart` on its `channel.stopped` event only; the replaced instance is never published as stopped) and starts a new one from the active definition; counters of the instance reset, retry state clears. The session is lost, so the console asks for confirmation. |
| Repeated commands | Registry operations are serialized. Pause on an already stopped channel is rejected as `already_stopped`; resume on a channel not operator-paused is rejected as `not_paused`. Once a stop has begun, the channel only moves to `stopped`: a session that collapses while stopping reports no `retrying` and no error summary. Resuming creates a new instance. |

## 13. Configuration apply

- Apply is serialized with every other operation that starts, stops or replaces channels
  (startup, pause, resume, restart, shutdown, other applies) through one registry, so an apply
  never interleaves with a pause or resume. A pause remains effective when an apply replaces
  the same enabled channel.
- Unchanged channels are not restarted ([configuration.md](configuration.md#10-versioning-and-apply)).
- Bridge label and tag changes apply in place. Endpoint changes and other bridge settings
  replace the instance. `config_version` in its snapshot remains the version that started
  the instance, including after an in-place update.
- **Break-before-make** for a changed channel that holds an exclusive resource (a
  `tcp_listener` port or a serial port): the old instance stops with `replaced_by_config`,
  which releases the resource, and then the new instance starts and binds it. Starting the new
  instance first would only fail to bind.
- **Make-before-break** for a channel with no exclusive resource (both endpoints `tcp_client`):
  the new instance starts first and takes over once paired; the old instance then stops with
  `replaced_by_config`. Both instances can briefly have live sessions. "Ready to take
  ownership" means the replacement reached `running` or `degraded`
  within `pairing_timeout_ms` plus the larger `connect_timeout_ms` plus one second (at most
  15 s).
- If the replacement is not ready within that time it takes over anyway: the previous
  instance stops with `replaced_by_config`, a `config.replacement_not_ready` warning is
  logged, and the replacement keeps attempting under its retry policy. A previous instance
  that is not working (not `running` or `degraded`) is replaced break-before-make.
- Channels are replaced one at a time. If one apply moves a listener port from channel X to
  channel Y, Y's replacement may fail its first bind while X's old instance still holds the
  port; Y binds on its next attempt after one backoff.
- A channel removed by an apply stops with `removed_by_config` and disappears from the
  configured channel list. Disabled channels stay listed with `disabled_by_config`.
- An operator-paused channel stays paused while enabled through an apply; disabling it
  reports `disabled_by_config` while retaining the pause flag for re-enabling. Removing
  the channel forgets the flag.
- While the node is shutting down nothing starts channels, not even an apply.

## 14. Shutdown

The engine is the single owner of shutdown. From the moment it begins no channel starts
(applies still store definitions; resume and restart are rejected). Every channel then stops
in parallel, each with a drain budget of `hosting.shutdown_drain_seconds`:

1. Readers stop before their next transport read; each finishes enqueueing a segment it
   already holds if the drain budget allows it.
2. Both buffers close to new segments.
3. The writers deliver everything still buffered until both buffers are empty or the budget
   (or the host's own stop timeout) runs out.
4. Anything still queued is discarded, the endpoints close, and the channel reports `stopped`
   with `shutdown`. After a completed drain each TCP connection is first shut down for
   sending, and what the peer still sends (for example acknowledgements of the drained
   messages) is read and discarded for up to one second; closing with unread input would
   reset the connection and could discard the tail of the drained bytes. This final
   read-and-discard can add up to one second after the buffer drain.

A session that ends during the drain (the peer drops) is part of the stop, not a failure.
Stops for any other reason (pause, disable, removal, replacement, restart) do not drain.
Each channel reports `stopped` and emits `channel.stopped` as its stop completes. The final
registry snapshot and `node.stopped` follow completion of all channel stops.

## 15. Counters

Per channel (names as in the snapshot, [diagnostics.md](diagnostics.md#4-channel-snapshots)):

| Counter | Meaning |
| --- | --- |
| `bridge.a_to_b.bytes`, `bridge.b_to_a.bytes` | Bytes accepted into each buffer; also `traffic.bytes_in` and `traffic.bytes_out`, respectively. |
| `bridge.a_to_b.messages`, `bridge.b_to_a.messages` | Framed messages observed by the tap, or `null`. |
| `bridge.<direction>.buffered_segments`, `.buffered_bytes` | Current queued occupancy. |
| `bridge.<direction>.overflow_count` | Overflow episodes for that direction since the instance started. |
| `retry_count` | Section 7. |
| `bridge.endpoint_a.last_activity`, `bridge.endpoint_b.last_activity` | Last successful read or complete segment write and flush per side. |
| `last_error` | Latest operator-facing failure with separate `summary` and `next_action` (section 16); retained after recovery. |
| `stop_reason` | When `stopped`. |

The endpoint references carry `consecutive_failures`; endpoint snapshots report the maximum
among active channel uses. Endpoint traffic counters count reads and completed segment
writes and survive channel replacement while the endpoint remains configured. Channel
traffic counters start fresh with each instance.

## 16. Error summaries

Endpoint failure summaries name the channel, the endpoint role and label, the failed
operation, a short cause and the current retry state or next action:

```text
Channel hematology-bridge endpoint B "Remote HL7 Receiver" connect failed: connection timed out. Retrying in 5s.
```

Format: `Channel {channel_id} endpoint {A|B} "{endpoint_label}" {operation} failed: {cause}. {next action}.`
Operations are `connect`, `listen`, `accept`, `open`, `read`, `write`. A clean close by the
peer is a `read` that failed with "peer closed the connection". The next action is added when
the retry is scheduled, so it names the actual delay. Causes never contain payload bytes.
In snapshots the failure sentence and the next action are separate fields (`summary`,
`next_action`); retry events join them in `error_summary` as above. An unexpected runner
failure instead reports `Channel {channel_id} failed unexpectedly: internal error.` with a
restart delay.

## 17. Non-goals

- No payload transformation, no fan-out (one channel, two endpoints), no multi-source
  aggregation.
- No reordering or repair of bytes to compensate for endpoint failure.
- No hiding of endpoint failure: it is surfaced at once in state, counters and diagnostics.
- No dependence of a bridge on the store, on other channels or on a connected console.
