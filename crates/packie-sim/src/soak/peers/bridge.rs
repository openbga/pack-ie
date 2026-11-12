//! The peers of a bridge: the instrument writes a steady stream of numbered records, the
//! sink checks every record and answers each with a short acknowledgement.
//!
//! A plain bridge carries newline terminated records that check their own length and
//! checksum. A tapped bridge carries MLLP framed HL7 messages and the sink answers with HL7
//! acknowledgements. A bridge delivers bytes, not messages, so a connection break may lose
//! the records in flight; the ledger accepts that only when a break is recorded nearby.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedReadHalf;
use tokio::sync::watch;

use super::{Pacer, connect, drop_link, while_stalled};
use crate::soak::chan::{ChannelRt, Control, Env, Progress};
use crate::soak::topology::Kind;
use crate::soak::util::Rng;
use crate::soak::wire::{self, FrameBuf};

/// The bytes of record `seq` as the instrument sends them.
fn encode(env: &Env, ch: &ChannelRt, seq: u64) -> Vec<u8> {
    let size = env.record_size(ch.plan.kind);
    if ch.plan.kind == Kind::TapBridge {
        wire::mllp(&wire::hl7_message(&ch.plan, seq, size))
    } else {
        wire::record_line(&ch.plan, seq, size)
    }
}

/// Reads what the sink sends back and tells the writer when the connection ended.
async fn read_back(mut half: OwnedReadHalf, ch: Arc<ChannelRt>, closed: watch::Sender<bool>) {
    let mut chunk = [0_u8; 4096];
    while let Ok(read) = half.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        Progress::add(&ch.prog.bytes_back, read as u64);
    }
    closed.send_replace(true);
}

/// The instrument: connects, sends at the configured rate, reconnects after a break.
pub(crate) async fn instrument(env: Arc<Env>, ch: Arc<ChannelRt>) {
    let mut rng = Rng::derive(env.seed, &ch.plan.id);
    let interval =
        (env.record_size(ch.plan.kind) as u64 * 1000 / env.bridge_bytes_per_sec.max(1)).max(1);
    let mut pacer = Pacer::new(interval, &mut rng);
    while let Some(stream) = connect(&env, &ch, ch.plan.in_port).await {
        let ended_by_stop = session(&env, &ch, stream, &mut pacer, &mut rng).await;
        if ended_by_stop {
            return;
        }
        ch.ledger.add_break(env.now_ms());
        env.stop
            .sleep(Duration::from_millis(rng.between(300, 1200)))
            .await;
    }
}

/// One connection; `true` when the run ended it.
async fn session(
    env: &Env,
    ch: &Arc<ChannelRt>,
    stream: TcpStream,
    pacer: &mut Pacer,
    rng: &mut Rng,
) -> bool {
    let (read, mut write) = stream.into_split();
    let (closed_tx, mut closed) = watch::channel(false);
    let mut reader = tokio::spawn(read_back(read, Arc::clone(ch), closed_tx));
    let stopped = loop {
        if !pacer.wait(&env.stop, rng).await {
            break true;
        }
        if *closed.borrow() {
            break false;
        }
        let seq = ch.ledger.alloc();
        let bytes = encode(env, ch, seq);
        ch.ledger.sent(seq, env.now_ms());
        if Control::take(&ch.ctl.source_disconnects) {
            let _ = write.write_all(&bytes[..bytes.len() / 2]).await;
            Progress::add(&ch.prog.source_disconnects_done, 1);
            if ch.ctl.wants_reset() {
                let _ = AsRef::<TcpStream>::as_ref(&write).set_zero_linger();
            } else {
                let _ = write.shutdown().await;
            }
            reader.abort();
            let _ = (&mut reader).await;
            break false;
        }
        tokio::select! {
            result = write.write_all(&bytes) => {
                if result.is_err() {
                    break false;
                }
                Progress::add(&ch.prog.bytes_sent, bytes.len() as u64);
            }
            _ = closed.wait_for(|c| *c) => break false,
        }
    };
    reader.abort();
    stopped
}

/// One connection of the sink.
pub(crate) async fn sink(env: Arc<Env>, ch: Arc<ChannelRt>, mut stream: TcpStream) {
    let mut parser = Parser::default();
    let mut chunk = [0_u8; 8192];
    loop {
        if !while_stalled(&env, &ch).await {
            return;
        }
        let Ok(read) = stream.read(&mut chunk).await else {
            break;
        };
        if read == 0 {
            break;
        }
        Progress::add(&ch.prog.bytes_received, read as u64);
        if !parser.take(&env, &ch, &mut stream, &chunk[..read]).await {
            break;
        }
        if Control::take(&ch.ctl.sink_disconnects) {
            Progress::add(&ch.prog.sink_disconnects_done, 1);
            drop_link(stream, ch.ctl.wants_reset()).await;
            ch.ledger.add_break(env.now_ms());
            return;
        }
    }
    if !env.halt.is_set() {
        ch.ledger.add_break(env.now_ms());
    }
}

/// Cuts the bytes a sink reads into records.
#[derive(Default)]
struct Parser {
    lines: Vec<u8>,
    frames: FrameBuf,
}

impl Parser {
    /// Handles `bytes`; `false` when answering failed.
    async fn take(
        &mut self,
        env: &Env,
        ch: &ChannelRt,
        stream: &mut TcpStream,
        bytes: &[u8],
    ) -> bool {
        if ch.plan.kind == Kind::TapBridge {
            self.take_frames(env, ch, stream, bytes).await
        } else {
            self.take_lines(env, ch, stream, bytes).await
        }
    }

    async fn take_frames(
        &mut self,
        env: &Env,
        ch: &ChannelRt,
        stream: &mut TcpStream,
        bytes: &[u8],
    ) -> bool {
        self.frames.push(bytes);
        while let Some(message) = self.frames.next_frame() {
            let seq = wire::field(&message, "MSH", 10).and_then(|id| ch.plan.parse_control_id(id));
            if !record(env, ch, seq.ok_or_else(|| "unknown id".to_owned())) {
                continue;
            }
            let reply = wire::mllp(&wire::hl7_ack(&message, "AA"));
            if stream.write_all(&reply).await.is_err() {
                return false;
            }
        }
        true
    }

    async fn take_lines(
        &mut self,
        env: &Env,
        ch: &ChannelRt,
        stream: &mut TcpStream,
        bytes: &[u8],
    ) -> bool {
        self.lines.extend_from_slice(bytes);
        while let Some(end) = self.lines.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.lines.drain(..=end).collect();
            let parsed = wire::parse_record_line(&ch.plan, &line[..line.len() - 1]);
            let Ok(seq) = parsed else {
                ch.ledger.corrupt();
                continue;
            };
            record(env, ch, Ok(seq));
            let reply = format!("A{seq:09}\n");
            if stream.write_all(reply.as_bytes()).await.is_err() {
                return false;
            }
        }
        true
    }
}

/// Records one record or message; `false` when it was damaged.
fn record(env: &Env, ch: &ChannelRt, parsed: Result<u64, String>) -> bool {
    match parsed {
        Ok(seq) => {
            ch.ledger.received_in_order(seq, env.now_ms());
            Progress::add(&ch.prog.records_received, 1);
            ch.prog.last_recv_ms.store(env.now_ms(), Ordering::Relaxed);
            true
        }
        Err(_) => {
            ch.ledger.corrupt();
            false
        }
    }
}
