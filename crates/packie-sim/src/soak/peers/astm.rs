//! The ASTM instrument: sends one result message per transfer (ENQ, one frame per record,
//! EOT) and sends the whole transfer again when it was interrupted.
//!
//! The instrument keeps at least [`MIN_GAP`] between two transfers, however far behind it
//! is. Chaos can ask for one transfer to end with EOT and the next ENQ in a single write
//! (an instrument with queued results sends them like that); the engine must acknowledge
//! that ENQ like any other.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::{Pacer, connect, drop_link};
use crate::soak::chan::{ChannelRt, Control, Env, Progress};
use crate::soak::rig::Owner;
use crate::soak::util::Rng;
use crate::soak::wire::{self, ACK, ENQ, EOT, NAK};

/// How long the instrument waits for each acknowledgement byte.
const BYTE_WAIT: Duration = Duration::from_secs(60);
/// How long it waits for the acknowledgement of an ENQ sent together with an EOT: the
/// time LIS01-A2 allows (15 s) and a little more.
const CHAINED_WAIT: Duration = Duration::from_secs(20);
/// Retransmissions of one frame after a NAK.
const FRAME_RETRIES: u32 = 3;
/// The least time between two transfers.
const MIN_GAP: Duration = Duration::from_millis(250);

/// Why a transfer ended early.
enum Stop {
    /// The link failed or the peer answered unexpectedly; connect again.
    Link,
    /// Chaos cut the transfer on purpose.
    Cut,
}

/// The connection and whether its next transfer already has its ENQ acknowledged.
#[derive(Default)]
struct Line {
    stream: Option<TcpStream>,
    enq_acked: bool,
}

/// The instrument of an ASTM channel.
pub(crate) async fn source(env: Arc<Env>, ch: Arc<ChannelRt>) {
    let mut rng = Rng::derive(env.seed, &ch.plan.id);
    let mut pacer = Pacer::new(env.message_interval_ms, &mut rng).with_min_gap(MIN_GAP);
    let mut line = Line::default();
    while pacer.wait(&env.stop, &mut rng).await {
        let seq = ch.ledger.alloc();
        deliver(&env, &ch, &mut line, seq).await;
        pacer.finished();
    }
}

async fn deliver(env: &Env, ch: &ChannelRt, line: &mut Line, seq: u64) {
    let records = wire::astm_records(&ch.plan, seq);
    while !env.stop.is_set() {
        if line.stream.is_none() {
            line.stream = connect(env, ch, ch.plan.in_port).await;
            line.enq_acked = false;
            continue;
        }
        ch.ledger.sent(seq, env.now_ms());
        match transfer(env, ch, line, &records).await {
            Ok(()) => {
                ch.ledger.acked(seq, env.now_ms());
                ch.prog.last_ack_ms.store(env.now_ms(), Ordering::Relaxed);
                return;
            }
            Err(why) => {
                line.enq_acked = false;
                if let Some(stream) = line.stream.take() {
                    drop_link(stream, matches!(why, Stop::Cut) && ch.ctl.wants_reset()).await;
                }
                env.stop.sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

/// The stage name of the wait whose silence is the finding of a chaos episode.
const CHAINED_STAGE: &str = "ENQ after EOT";

/// The engine accepted the connection and then did not answer: nothing a fault of the soak
/// explains (a closed connection would have been seen at once).
fn report_silence(env: &Env, ch: &ChannelRt, stage: &str, waited: Duration) {
    env.fail(
        "source_no_answer",
        Owner::RoutingStore,
        format!(
            "{}: the engine kept an open connection but did not answer the instrument's {stage} within {} s (the instrument then started over on a new connection)",
            ch.plan.id,
            waited.as_secs()
        ),
    );
}

/// Reads one byte. A wait that times out or ends in a closed connection is noted with the
/// stage of the transfer it happened in.
async fn byte(
    env: &Env,
    ch: &ChannelRt,
    stream: &mut TcpStream,
    stage: &str,
    wait: Duration,
) -> Result<u8, Stop> {
    let mut one = [0_u8; 1];
    let result = tokio::time::timeout(wait, stream.read(&mut one)).await;
    match result {
        Ok(Ok(1)) => {
            if env.trace {
                env.note(
                    "astm_byte",
                    json!({ "channel": ch.plan.id, "stage": stage, "byte": one[0] }),
                );
            }
            Ok(one[0])
        }
        other => {
            if other.is_err() && stage != CHAINED_STAGE {
                report_silence(env, ch, stage, wait);
            }
            let what = match other {
                Err(_) => "timeout".to_owned(),
                Ok(Ok(_)) => "closed by the engine".to_owned(),
                Ok(Err(e)) => e.to_string(),
            };
            env.note(
                "astm_wait_failed",
                json!({ "channel": ch.plan.id, "stage": stage, "why": what }),
            );
            Err(Stop::Link)
        }
    }
}

async fn send(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), Stop> {
    stream.write_all(bytes).await.map_err(|_| Stop::Link)
}

/// Notes what is about to be written, when tracing.
fn trace_send(env: &Env, ch: &ChannelRt, what: &str) {
    if env.trace {
        env.note("astm_send", json!({ "channel": ch.plan.id, "what": what }));
    }
}

/// Opens a transfer with ENQ unless the previous one already did.
async fn establish(env: &Env, ch: &ChannelRt, line: &mut Line) -> Result<(), Stop> {
    let Some(stream) = line.stream.as_mut() else {
        return Err(Stop::Link);
    };
    if line.enq_acked {
        line.enq_acked = false;
        return Ok(());
    }
    trace_send(env, ch, "ENQ");
    send(stream, &[ENQ]).await?;
    let answer = byte(env, ch, stream, "ENQ", BYTE_WAIT).await?;
    if answer == ACK {
        return Ok(());
    }
    env.note(
        "astm_unexpected_byte",
        json!({ "channel": ch.plan.id, "stage": "ENQ", "byte": answer }),
    );
    Err(Stop::Link)
}

/// One transfer. Chaos cuts it after the second record, or ends it with EOT and the next
/// ENQ in one write.
async fn transfer(
    env: &Env,
    ch: &ChannelRt,
    line: &mut Line,
    records: &[String],
) -> Result<(), Stop> {
    establish(env, ch, line).await?;
    let cut = Control::take(&ch.ctl.source_disconnects);
    let chain = !cut && Control::take(&ch.ctl.back_to_back);
    let Some(stream) = line.stream.as_mut() else {
        return Err(Stop::Link);
    };
    for (index, record) in records.iter().enumerate() {
        if cut && index == 2 {
            Progress::add(&ch.prog.source_disconnects_done, 1);
            return Err(Stop::Cut);
        }
        frame(env, ch, stream, index + 1, record).await?;
    }
    if chain {
        return end_with_next_enq(env, ch, line).await;
    }
    trace_send(env, ch, "EOT");
    send(stream, &[EOT]).await
}

/// Ends the transfer with EOT and the ENQ of the next one in a single write, then waits for
/// the acknowledgement of that ENQ.
async fn end_with_next_enq(env: &Env, ch: &ChannelRt, line: &mut Line) -> Result<(), Stop> {
    let Some(stream) = line.stream.as_mut() else {
        return Err(Stop::Link);
    };
    trace_send(env, ch, "EOT+ENQ");
    send(stream, &[EOT, ENQ]).await?;
    let started = tokio::time::Instant::now();
    let answer = byte(env, ch, stream, CHAINED_STAGE, CHAINED_WAIT).await;
    Progress::add(&ch.prog.chained_done, 1);
    match answer {
        Ok(ACK) => {
            Progress::add(&ch.prog.chained_ok, 1);
            line.enq_acked = true;
            env.note(
                "astm_back_to_back",
                json!({ "channel": ch.plan.id, "ack_after_ms": started.elapsed().as_millis() as u64 }),
            );
            Ok(())
        }
        // The message itself was delivered; the instrument starts over on a new connection.
        _ => Ok(()),
    }
}

/// Sends one frame, again after a NAK.
async fn frame(
    env: &Env,
    ch: &ChannelRt,
    stream: &mut TcpStream,
    number: usize,
    record: &str,
) -> Result<(), Stop> {
    let bytes = wire::astm_frame(number, record);
    let stage = format!("frame {number}");
    for _ in 0..=FRAME_RETRIES {
        trace_send(env, ch, &stage);
        send(stream, &bytes).await?;
        match byte(env, ch, stream, &stage, BYTE_WAIT).await? {
            ACK => return Ok(()),
            NAK => {}
            other => {
                env.note(
                    "astm_unexpected_byte",
                    json!({ "channel": ch.plan.id, "stage": stage, "byte": other }),
                );
                return Err(Stop::Link);
            }
        }
    }
    Err(Stop::Link)
}
