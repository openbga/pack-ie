# Synthetic ASTM conformance corpus

Every `.raw` file contains peer-to-driver bytes exactly as fed to the pure
receive driver: ENQ, framed bytes, and EOT where applicable. All identifiers and
sender names are synthetic. The adjacent `.expected` file states the observable
outcome asserted by `tests/conformance.rs`; it is deliberately small so the test,
not a second parser in fixture data, owns detailed assertions.
`packed_records.hex` uses hexadecimal so reviewers can see both record
terminators inside its one low-level frame.

| Case | Coverage |
| --- | --- |
| `valid_single_result` | One H/P/O/R/L message and lexical result preservation |
| `multiple_patients_orders_results` | Scoped P, O, and R sequences and tree assembly |
| `record_spanning_etb` | A 500-character comment split over several ETB frames |
| `packed_records` | H and L records packed into one low-level frame |
| `custom_delimiters` | Header-selected field, repeat, component, and escape delimiters |
| `escape_sequences` | F, S, R, E, and even-pair hex escape decoding |
| `comments` | Patient-, order-, and result-level comments |
| `bad_checksum_then_retransmission` | NAK followed by successful retransmission |
| `out_of_sequence_frame` | Unexpected first frame number is NAKed |
| `duplicate_after_lost_ack` | Duplicate durable frame is re-ACKed without another persist |
| `eot_mid_record` | Partial record is aborted and never exposed as a message |
| `oversize_frame` | Text above the selected 240-byte frame profile is rejected |
| `disallowed_byte` | NUL in frame text is rejected by the conservative ASCII profile |
| `missing_terminator_record` | Link transfer completes, then hierarchy validation rejects missing L |
| `empty_transfer` | ENQ followed by EOT aborts without a message |
| `lowercase_checksum` | Checksum digits must be uppercase hexadecimal |
| `bad_frame_number` | Frame number outside 0 through 7 is NAKed |
| `missing_end_marker` | Frame without ETB/ETX is NAKed |
| `stray_byte_after_enq` | A byte that is neither STX nor EOT inside a transfer is NAKed |
| `noise_before_enq` | Bytes before the first ENQ are ignored; the message then completes |

The standards are licensed publications and no standard text is copied here.
These fixtures exercise the public profile described in the crate documentation;
they do not claim clinical validation or universal analyzer compatibility.

## Send side and link scenarios

`send/*.scenario` are scripts that drive a `LinkDriver` the way a session loop would and
state every action it must produce, in order. The directive list is in the header of
`tests/scenarios.rs`. Each script's first comment line says what it proves.

| Scenario | Coverage |
| --- | --- |
| `happy_path` | ENQ, ACK, one frame per record, EOT, message sent |
| `nak_then_retransmit` | A NAK (for example a checksum error) resends the identical frame |
| `retransmission_limit` | Original plus six retransmissions; the seventh NAK abandons the message |
| `frame_reply_timeout` | Silence after a frame: EOT and abort |
| `receiver_interrupt` | EOT in place of ACK stops the sender |
| `contention_computer_yields` | Both sides send ENQ; the computer yields, receives, then sends |
| `contention_computer_timeout` | The computer yielded but the peer never sent ENQ; retry after 20 s |
| `contention_instrument_retries` | The instrument waits 1 s and sends ENQ again |
| `busy_receiver_retry` | NAK to ENQ: wait 10 s and retry |
| `establishment_gives_up` | ENQ attempts exhausted: abort without sending a frame |
| `enq_timeout_retry` | Silence after ENQ: EOT, wait, retry, then give up |
| `enq_seven_attempts_busy` | `max_enq_attempts 7` with a NAK each time: seven ENQs spaced by `busy_retry_delay`, then abort |
| `enq_seven_attempts_silent` | `max_enq_attempts 7` with silence each time: seven ENQs, EOT and wait after each, then abort |
| `long_frames` | `max_frame_text 300` splits a 700-byte record into two ETB frames and one ETX frame |
| `submit_during_inbound` | A message submitted during an inbound transfer waits for EOT |
| `inbound_during_busy_wait` | The peer starts a transfer while a retry is pending; receive first |
| `second_message_after_sent` | The driver is reusable; frame numbering restarts at 1 |

`tests/loopback.rs` connects two real drivers over a simulated link with a virtual clock:
checksum damage, byte-by-byte chunking, ETB splitting with frame-number wrap and
simultaneous ENQ with both roles.
