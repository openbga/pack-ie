//! The folder channel's peers: a writer that drops delimited files into the source
//! folder, and a reader that collects the files the engine writes to the destination folder.
//!
//! A file counts as acknowledged when the engine has consumed it (the source endpoint
//! deletes a file once its content is committed). The writer normally writes a hidden
//! temporary file and renames it; when chaos asks, it writes the final name in two steps,
//! with a half row in between, as a slow writer would.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::Pacer;
use crate::soak::chan::{ChannelRt, Control, Env, Progress};
use crate::soak::util::Rng;
use crate::soak::wire;

/// Rows in each file: two messages per second means one file per second with two rows.
const ROWS_PER_FILE: u64 = 2;
/// The pause between the two halves of a slow write. It is shorter than the endpoint's
/// stability time, so the engine must not read the file in between.
const SLOW_GAP: Duration = Duration::from_millis(600);
/// How often the reader looks at the destination folder.
const SCAN_EVERY: Duration = Duration::from_millis(700);

/// A file the writer dropped and has not seen consumed yet.
struct Pending {
    path: PathBuf,
    seqs: Vec<u64>,
}

/// The writer.
pub(crate) async fn writer(env: Arc<Env>, ch: Arc<ChannelRt>) {
    let mut rng = Rng::derive(env.seed, &ch.plan.id);
    let interval = env.message_interval_ms * ROWS_PER_FILE;
    let mut pacer = Pacer::new(interval, &mut rng);
    let mut pending: Vec<Pending> = Vec::new();
    let mut file_no = 0_u64;
    while pacer.wait(&env.stop, &mut rng).await {
        let seqs: Vec<u64> = (0..ROWS_PER_FILE).map(|_| ch.ledger.alloc()).collect();
        file_no += 1;
        let path = ch.plan.in_dir.join(format!("in-{file_no:08}.csv"));
        let text = wire::csv_file(&ch.plan, &seqs);
        for seq in &seqs {
            ch.ledger.sent(*seq, env.now_ms());
        }
        if write_file(&env, &ch, &path, &text).await {
            pending.push(Pending { path, seqs });
        }
        collect_consumed(&env, &ch, &mut pending).await;
    }
    drain(&env, &ch, &mut pending).await;
}

/// Writes `text` to `path`; `false` when it could not be written.
async fn write_file(env: &Env, ch: &ChannelRt, path: &Path, text: &str) -> bool {
    if Control::take(&ch.ctl.slow_writes) {
        Progress::add(&ch.prog.source_disconnects_done, 1);
        let cut = text.len() - text.len() / 4;
        let (first, second) = text.split_at(cut);
        if tokio::fs::write(path, first).await.is_err() {
            return false;
        }
        env.stop.sleep(SLOW_GAP).await;
        return append(path, second).await;
    }
    let hidden = path.with_file_name(format!(
        ".{}.part",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    tokio::fs::write(&hidden, text).await.is_ok() && tokio::fs::rename(&hidden, path).await.is_ok()
}

async fn append(path: &Path, text: &str) -> bool {
    use tokio::io::AsyncWriteExt;
    let Ok(mut file) = tokio::fs::OpenOptions::new().append(true).open(path).await else {
        return false;
    };
    file.write_all(text.as_bytes()).await.is_ok()
}

/// Marks the rows of files that are gone as acknowledged.
async fn collect_consumed(env: &Env, ch: &ChannelRt, pending: &mut Vec<Pending>) {
    let mut still = Vec::new();
    for file in std::mem::take(pending) {
        if tokio::fs::try_exists(&file.path).await.unwrap_or(true) {
            still.push(file);
        } else {
            for seq in &file.seqs {
                ch.ledger.acked(*seq, env.now_ms());
            }
            ch.prog.last_ack_ms.store(env.now_ms(), Ordering::Relaxed);
        }
    }
    *pending = still;
}

/// After the run ends, waits a little for the engine to consume the last files.
async fn drain(env: &Env, ch: &ChannelRt, pending: &mut Vec<Pending>) {
    for _ in 0..20 {
        collect_consumed(env, ch, pending).await;
        if pending.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// The reader.
pub(crate) async fn reader(env: Arc<Env>, ch: Arc<ChannelRt>) {
    while env.halt.sleep(SCAN_EVERY).await {
        scan(&env, &ch).await;
    }
    scan(&env, &ch).await;
}

async fn scan(env: &Env, ch: &ChannelRt) {
    let Ok(mut entries) = tokio::fs::read_dir(&ch.plan.out_dir).await else {
        return;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || !name.contains(".csv") {
            continue;
        }
        let Ok(text) = tokio::fs::read_to_string(entry.path()).await else {
            continue;
        };
        for (id, value) in wire::parse_csv(&text) {
            take_row(env, ch, &id, &value);
        }
        let _ = tokio::fs::remove_file(entry.path()).await;
    }
}

fn take_row(env: &Env, ch: &ChannelRt, id: &str, value: &str) {
    let Some(seq) = ch.plan.parse_control_id(id) else {
        Progress::add(&ch.prog.unattributed, 1);
        return;
    };
    if value != wire::value_of(seq) {
        ch.ledger.corrupt();
    }
    ch.ledger.received(seq, env.now_ms());
    ch.prog.last_recv_ms.store(env.now_ms(), Ordering::Relaxed);
}
