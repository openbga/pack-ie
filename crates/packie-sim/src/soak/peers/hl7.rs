//! The HL7 instrument that sends ORU messages over MLLP, and the receiving system that
//! checks each message against what was sent and acknowledges it.
//!
//! The receiving system serves both HL7 channels (HL7 in, HL7 out) and ASTM channels (ASTM
//! in, mapped to HL7 out). Every message identifies itself by its sample id (OBR-3).

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::Instant;

use super::{Pacer, connect, drop_link, while_stalled};
use crate::soak::chan::{ChannelRt, Control, Env, Progress};
use crate::soak::rig::Owner;
use crate::soak::topology::Kind;
use crate::soak::util::Rng;
use crate::soak::wire::{self, FrameBuf};

/// How long the instrument waits for an acknowledgement before it sends again.
const ACK_WAIT: Duration = Duration::from_secs(75);
/// Rejections after which the instrument gives up on a message.
const MAX_REJECTIONS: u32 = 3;

/// An open connection with its read buffer.
struct Link {
    stream: TcpStream,
    frames: FrameBuf,
}

impl Link {
    async fn read_frame(&mut self, within: Duration) -> Result<String, String> {
        let deadline = Instant::now() + within;
        let mut chunk = [0_u8; 4096];
        loop {
            if let Some(text) = self.frames.next_frame() {
                return Ok(text);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            let read = tokio::time::timeout(left, self.stream.read(&mut chunk))
                .await
                .map_err(|_| format!("no reply within {} s", within.as_secs()))?
                .map_err(|e| e.to_string())?;
            if read == 0 {
                return Err("the connection was closed".to_owned());
            }
            self.frames.push(&chunk[..read]);
        }
    }
}

/// The instrument of an HL7 channel: sends messages at the configured rate, one at a time,
/// until each is acknowledged.
pub(crate) async fn source(env: Arc<Env>, ch: Arc<ChannelRt>) {
    let mut rng = Rng::derive(env.seed, &ch.plan.id);
    let mut pacer = Pacer::new(env.message_interval_ms, &mut rng);
    let mut link = None;
    while pacer.wait(&env.stop, &mut rng).await {
        let seq = ch.ledger.alloc();
        let frame = wire::mllp(&wire::hl7_message(&ch.plan, seq, env.message_size));
        deliver(&env, &ch, &mut link, seq, &frame).await;
    }
}

enum Attempt {
    Acked,
    Rejected,
    Failed,
}

async fn deliver(env: &Env, ch: &ChannelRt, link: &mut Option<Link>, seq: u64, frame: &[u8]) {
    let mut rejections = 0;
    while !env.stop.is_set() {
        if link.is_none() {
            let port = ch.plan.in_port;
            *link = connect(env, ch, port).await.map(|stream| Link {
                stream,
                frames: FrameBuf::default(),
            });
            continue;
        }
        ch.ledger.sent(seq, env.now_ms());
        if Control::take(&ch.ctl.source_disconnects) {
            cut_mid_message(env, ch, link, frame).await;
            continue;
        }
        let Some(open) = link.as_mut() else { continue };
        match exchange(env, ch, open, seq, frame).await {
            Attempt::Acked => return,
            Attempt::Rejected => {
                rejections += 1;
                if rejections >= MAX_REJECTIONS {
                    return;
                }
                env.stop.sleep(Duration::from_secs(1)).await;
            }
            Attempt::Failed => {
                *link = None;
                env.stop.sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

/// Writes half of `frame`, then drops the connection.
async fn cut_mid_message(env: &Env, ch: &ChannelRt, link: &mut Option<Link>, frame: &[u8]) {
    if let Some(mut open) = link.take() {
        let _ = open.stream.write_all(&frame[..frame.len() / 2]).await;
        let _ = open.stream.flush().await;
        drop_link(open.stream, ch.ctl.wants_reset()).await;
        Progress::add(&ch.prog.source_disconnects_done, 1);
    }
    env.stop.sleep(Duration::from_millis(400)).await;
}

async fn exchange(env: &Env, ch: &ChannelRt, link: &mut Link, seq: u64, frame: &[u8]) -> Attempt {
    if link.stream.write_all(frame).await.is_err() {
        return Attempt::Failed;
    }
    let id = ch.plan.control_id(seq);
    let reply = match link.read_frame(ACK_WAIT).await {
        Ok(reply) => reply,
        Err(why) => {
            env.note(
                "hl7_ack_failed",
                json!({ "channel": ch.plan.id, "why": why }),
            );
            if why.starts_with("no reply") {
                env.fail(
                    "source_no_answer",
                    Owner::RoutingStore,
                    format!(
                        "{}: the engine kept an open connection but did not acknowledge a message within {} s (the instrument then sent it again on a new connection)",
                        ch.plan.id,
                        ACK_WAIT.as_secs()
                    ),
                );
            }
            return Attempt::Failed;
        }
    };
    match wire::ack_of(&reply) {
        Some(("AA" | "CA", acked)) if acked == id => {
            ch.ledger.acked(seq, env.now_ms());
            ch.prog
                .last_ack_ms
                .store(env.now_ms(), std::sync::atomic::Ordering::Relaxed);
            Attempt::Acked
        }
        Some((_, acked)) if acked == id => {
            Progress::add(&ch.prog.rejected, 1);
            Attempt::Rejected
        }
        _ => Attempt::Failed,
    }
}

/// One connection of the receiving system.
pub(crate) async fn destination(env: Arc<Env>, ch: Arc<ChannelRt>, mut stream: TcpStream) {
    let mut rng = Rng::derive(env.seed ^ env.now_ms(), &ch.plan.id);
    let mut frames = FrameBuf::default();
    let mut chunk = [0_u8; 4096];
    loop {
        if !while_stalled(&env, &ch).await {
            return;
        }
        let Ok(read) = stream.read(&mut chunk).await else {
            return;
        };
        if read == 0 {
            return;
        }
        frames.push(&chunk[..read]);
        while let Some(message) = frames.next_frame() {
            if !handle(&env, &ch, &mut stream, &message, &mut rng).await {
                return;
            }
        }
    }
}

/// Handles one message; `false` when the connection ended.
async fn handle(
    env: &Env,
    ch: &ChannelRt,
    stream: &mut TcpStream,
    message: &str,
    rng: &mut Rng,
) -> bool {
    let seq = wire::field(message, "OBR", 3).and_then(|id| ch.plan.parse_control_id(id));
    if Control::take(&ch.ctl.sink_disconnects) {
        if let Some(seq) = seq {
            ch.ledger.discarded(seq);
        }
        Progress::add(&ch.prog.sink_disconnects_done, 1);
        if ch.ctl.wants_reset() {
            let _ = stream.set_zero_linger();
        }
        return false;
    }
    tokio::time::sleep(Duration::from_millis(rng.between(0, 30))).await;
    record(env, ch, message, seq);
    let reply = wire::mllp(&wire::hl7_ack(message, "AA"));
    stream.write_all(&reply).await.is_ok()
}

/// Records a received message after checking its content.
fn record(env: &Env, ch: &ChannelRt, message: &str, seq: Option<u64>) {
    let Some(seq) = seq else {
        Progress::add(&ch.prog.unattributed, 1);
        return;
    };
    if !content_matches(env, ch, message, seq) {
        ch.ledger.corrupt();
    }
    ch.ledger.received(seq, env.now_ms());
    ch.prog
        .last_recv_ms
        .store(env.now_ms(), std::sync::atomic::Ordering::Relaxed);
}

/// Whether `message` is what the channel should deliver for `seq`.
fn content_matches(env: &Env, ch: &ChannelRt, message: &str, seq: u64) -> bool {
    if ch.plan.kind == Kind::Astm {
        wire::field(message, "OBX", 5) == Some(wire::value_of(seq).as_str())
            && wire::field(message, "PID", 3) == Some(wire::astm_patient(seq).as_str())
    } else {
        // The original bytes are delivered unchanged.
        let sent = wire::hl7_message(&ch.plan, seq, env.message_size);
        message.trim_end() == sent.trim_end()
    }
}
