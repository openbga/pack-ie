# Fuzzing the codecs

libFuzzer harnesses ([cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html)) for the pure
protocol and file codecs. They back PRO-06: every codec returns a typed error and never panics,
hangs or exhausts memory on any input. This directory is its own Cargo workspace (the root
workspace only takes `crates/*`), so the nightly toolchain and sanitizer flags never touch the
engine's build.

## Running

```sh
cargo install cargo-fuzz --locked --version 0.13.2   # once
cd fuzz                                              # not the repository root, see below
cargo fuzz run hl7_parse_encode work/hl7_parse_encode corpus/hl7_parse_encode -- -max_len=4096
./run.sh 600                       # every target, ten minutes each
JOBS=6 ./run.sh 600 files_read     # chosen targets, six at a time
```

Run from `fuzz/`: the pinned nightly in [rust-toolchain.toml](rust-toolchain.toml) is found from
the working directory, and the repository's own `rust-toolchain.toml` would win from the root.

| Path | Holds |
| --- | --- |
| `fuzz_targets/` | One harness per target. |
| `src/lib.rs` | A front-to-back byte reader and the ASTM step decoder shared by the driver targets. |
| `corpus/<target>/` | Committed: seeds and minimized regression inputs. Only read by `run.sh`. |
| `work/<target>/` | Ignored: the corpus libFuzzer grows. `work/log/<target>.log` is the last run's log. |
| `artifacts/<target>/` | Ignored: crash, timeout and out-of-memory inputs. |
| `run.sh` | Runs targets for a fixed time with the per-target `-max_len`, `-timeout=10` and `-rss_limit_mb=2048`. |

[.github/workflows/fuzz.yml](../.github/workflows/fuzz.yml) runs every target weekly and on manual
dispatch (minutes per target and an optional target list are inputs), one job per target with the
working corpus cached between runs. Crash inputs and the log are uploaded when a target fails.
Nothing fuzzes on push or pull request.

## Targets

| Target | What it exercises | Input | Seeds |
| --- | --- | --- | --- |
| `hl7_parse_encode` | `Message::parse`. Where it succeeds: `encode` returns the input, the encoding parses to an equal message, and re-writing every field with its own decoded value (the writer, not the verbatim original) keeps every value. | The message bytes. | `packie-hl7/tests/fixtures`, valid and malformed. |
| `hl7_mllp_decoder` | `mllp::Decoder` with chunk sizes taken from the input and a small payload limit. Chunked and byte-at-a-time feeding must agree; every message re-encodes and decodes. | Byte 0: payload limit. Rest: the stream. | The fixtures framed, plus garbage, nested starts, oversize. |
| `hl7_ack` | `build_ack` on parsed messages with MSH-15, MSH-16 and the version overwritten, every stage and outcome, optional ERR details. The ACK must parse and carry the source's control id. | 8 control bytes, then the message. | Fixtures with four control-byte variants (original, enhanced AL, enhanced ER with details, legacy ERR). |
| `astm_frames` | `parse_frame` (and its re-encoding), `parse_message_records`, `assemble_message`, `parse_record` with delimiters picked by byte 0, the typed accessors, and `encode_records` round trip. | Raw bytes. | Every `packie-astm/tests/corpus` stream, every frame in them alone, their record text. |
| `astm_receive_driver` | `ReceiveDriver` fed bytes, well-formed frames, whole transfers, persistence confirmations and timer expiries in any order. | Opcode stream (`src/lib.rs`). | Transfers built from the conformance records. |
| `astm_link_driver` | `LinkDriver` with the same steps plus submissions; byte 0 picks line role, retry limits, and a cooperative peer that ACKs or NAKs what the driver writes. | Byte 0, then opcodes. | Submit, contention, inbound-during-send, timers only. |
| `au_frames_and_driver` | `StreamDecoder`, `encode_frame`, `encode_tri`, `parse_message` under an instrument profile from the input (sometimes invalid, to reach validation), then `ReceiveDriver`. | Profile, one codec round, driver opcodes. | A class-B single-result session, a profile alone, an invalid profile. |
| `files_read` | `read` with a template built from the first 22 bytes (delimiter, quote, charset, line ending, record mode, size limit, up to six typed columns) and the file from the rest; a file that reads is grouped, written and read again. | 22-byte template prefix, file bytes. | The `packie-files/tests/fixtures` files with matching prefixes, plus headerless and sparse-position cases. |
| `transform_path` | `Path::parse` in all four syntaxes, `resolved`, and the `Schema` read/write checks. | Text. | Paths from the spec and tests, plus malformed ones. |
| `transform_compile_apply` | `Filter`/`Filters::compile` and `Mapping::compile` from JSON, then `apply` (twice, onto its own output) over seed HL7, ASTM, delimited-row and AU messages; HL7 and ASTM output is encoded and parsed again. | Byte 0: source protocol. Byte 1: target protocol, bit 7 selects filter. Rest: JSON. | The mapping and filter examples from the spec and tests, including maximum indices. |
| `ipc_frame_decode` | `read_frame` on streams (short, oversized, several frames), `read_json` as `ClientFrame` and `ServerFrame` with and without a length prefix; decoded frames must survive encode and decode. | The stream. | Client and server frames, truncated and oversized lengths. |
| `config_parse` | `RuntimeConfig::parse`, `parse_with_warnings` and `from_value` on arbitrary JSON: `Ok` or a `ValidationErrors`, never a panic; an accepted document serializes and parses again to a stable configuration. | JSON text. | `config/runtime.json` and the runtime of every scenario. |

`FrameId` and `TimerId` cannot be forged outside their crates, so the driver targets replay the
identifiers the driver issued: any of them, in any order, repeatedly and after they are stale.

## Results

Last full run: every target at least ten minutes against the codec crates as they stood in the
working tree, the three drivers at least twenty-five, on a 20-core Apple-silicon machine with
six targets running at once (so execs/s is below the single-target rate), AddressSanitizer on.
Coverage is libFuzzer's edge count (`cov`) and feature count (`ft`) at the end of the run.

| Target | Fuzz time | Execs/s | Executions | cov / ft | Findings fixed |
| --- | --- | --- | --- | --- | --- |
| `hl7_parse_encode` | 11 min | 2,069 | 1.4 M | 927 / 5,266 | none |
| `hl7_mllp_decoder` | 11 min | 13,364 | 9.1 M | 291 / 1,658 | none |
| `hl7_ack` | 11 min | 7,655 | 5.2 M | 1,121 / 5,248 | none |
| `astm_frames` | 11 min | 21,973 | 15.2 M | 1,343 / 5,556 | none |
| `astm_receive_driver` | 26 min | 7,945 | 13.3 M | 1,558 / 8,551 | none |
| `astm_link_driver` | 26 min | 7,943 | 14.3 M | 1,070 / 7,041 | none |
| `au_frames_and_driver` | 26 min | 8,945 | 14.8 M | 2,053 / 10,226 | none |
| `files_read` | 11 min | 10,376 | 7.1 M | 1,536 / 7,075 | 1: `write` allocated a row per position |
| `transform_path` | 11 min | 19,186 | 12.7 M | 511 / 825 | none |
| `transform_compile_apply` | 11 min | 5,957 | 3.8 M | 15,228 / 56,789 | none |
| `ipc_frame_decode` | 11 min | 5,450 | 3.6 M | 2,978 / 9,571 | none |
| `config_parse` | 20 min | 6,153 | 7.3 M | 10,619 / 30,841 | none |

Execs/s is the average of the first ten-minute run.

### Findings

| # | Target | Input | Cause | Fix and regression |
| --- | --- | --- | --- | --- |
| 1 | `files_read` | A template with a column at position 4294967295 and any file. | `packie_files::write` sized every row, header included, to the highest position: 4.3 billion cells (a 103 GB allocation) for a two-column template. | `write` returns `Error::Serialize` above 4,096 cells per row. Test `a_sparse_position_is_refused_instead_of_allocating_the_gap` in `crates/packie-files/tests/writer.rs`; input `corpus/files_read/regress-sparse-position-write-oom`. |
| 2 | `ipc_frame_decode` | A `validate_config` frame whose document holds a 92-digit integer literal. | serde_json's default number parser is inexact for long literals: it decoded to 2.2222222222222225e+92, which re-encodes as 2.2222222222222223e+92 and decodes to a different `f64`, so the frame did not survive encode and decode. | The workspace enables serde_json's `float_roundtrip` (exact parsing; also used by `packie-config` for decimal text). Input `corpus/ipc_frame_decode/regress-long-integer-float-roundtrip`. |

Behaviour the harnesses surfaced that is not a panic and was left alone:

- `packie_files::write` ignores `skip_lines` and `max_file_bytes` (source-only settings), so a
  written file can be larger than the limit a reader of the same template enforces.

## A crash

1. `cargo fuzz run <target> artifacts/<target>/<file>` reproduces it;
   `cargo fuzz tmin <target> artifacts/<target>/<file>` minimizes it.
2. Fix the crate: a typed error or a bound, not a catch. Add a regression test next to the crate's
   tests.
3. Copy the minimized input to `corpus/<target>/regress-<what>` so every later run replays it.
4. Run the target again until it is clean for the full time.
