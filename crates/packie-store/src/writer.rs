//! The single writer thread with group commit.
//!
//! Every queued mutation runs in one IMMEDIATE transaction under its own savepoint, so a
//! failing operation rolls back alone; the transaction then commits once (one FULL fsync)
//! and only after that commit is each caller answered. Under load many channels' commits
//! share an fsync; with one operation queued it is exactly the former one transaction per
//! operation. Operations that need the connection alone (body file collection) run between
//! groups.

use std::sync::mpsc;

use rusqlite::{Connection, Transaction, TransactionBehavior};
use tokio::sync::{oneshot, watch};

use crate::Cached;
use crate::bodies::BodyStorage;
use crate::checkpoint::Commits;
use crate::{Error, Result, with_commit_gate};

/// The most operations committed together.
const MAX_GROUP: usize = 64;

/// The commit-gate receiver the engine's test hooks may supply.
pub(crate) type Gate = Option<watch::Receiver<bool>>;

/// Work that needs the write connection alone.
pub(crate) type ExclusiveWork = Box<dyn FnOnce(&mut Connection, &BodyStorage, &Gate) + Send>;

/// Work for the writer thread.
pub(crate) enum Job {
    /// A mutation that joins the current group transaction.
    Grouped(Box<dyn Grouped>),
    /// Work that needs the connection alone.
    Exclusive(ExclusiveWork),
}

/// A queued mutation.
pub(crate) trait Grouped: Send {
    /// Runs inside the group transaction; the outcome is answered after the commit.
    fn run(self: Box<Self>, tx: &Transaction<'_>, bodies: &BodyStorage) -> Box<dyn Finish>;
    /// The group could not run it.
    fn fail(self: Box<Self>, cause: &str);
}

/// A mutation that ran and waits for its group's commit.
pub(crate) trait Finish: Send {
    /// The operation itself succeeded (its savepoint is kept).
    fn succeeded(&self) -> bool;
    /// Answers the caller: the operation's own error, or the commit's.
    fn finish(self: Box<Self>, committed: std::result::Result<(), &str>);
}

/// One caller's mutation and where its answer goes.
pub(crate) struct Op<T, F> {
    pub(crate) operation: F,
    pub(crate) reply: oneshot::Sender<Result<T>>,
}

impl<T, F> Grouped for Op<T, F>
where
    T: Send + 'static,
    F: FnOnce(&Transaction<'_>, &BodyStorage) -> Result<T> + Send + 'static,
{
    fn run(self: Box<Self>, tx: &Transaction<'_>, bodies: &BodyStorage) -> Box<dyn Finish> {
        let Self { operation, reply } = *self;
        Box::new(Done {
            result: operation(tx, bodies),
            reply,
        })
    }

    fn fail(self: Box<Self>, cause: &str) {
        let result = Err(unavailable(cause));
        log_failure(&result);
        drop(self.reply.send(result));
    }
}

struct Done<T> {
    result: Result<T>,
    reply: oneshot::Sender<Result<T>>,
}

impl<T: Send> Finish for Done<T> {
    fn succeeded(&self) -> bool {
        self.result.is_ok()
    }

    fn finish(self: Box<Self>, committed: std::result::Result<(), &str>) {
        let result = match (self.result, committed) {
            (Ok(value), Ok(())) => Ok(value),
            (Ok(_), Err(cause)) => Err(unavailable(cause)),
            (Err(error), _) => Err(error),
        };
        log_failure(&result);
        drop(self.reply.send(result));
    }
}

/// Logs a write that failed in storage (metadata only).
fn log_failure<T>(result: &Result<T>) {
    if matches!(result, Err(Error::Sql(_) | Error::Io(_))) {
        tracing::error!(
            event = "store.error",
            error_code = "write_failed",
            "Store write failed"
        );
    }
}

/// A commit-level failure as the I/O error callers treat as "store unavailable".
fn unavailable(cause: &str) -> Error {
    Error::Io(std::io::Error::other(cause.to_owned()))
}

/// Serves the queue until every handle is dropped.
pub(crate) fn run(
    receiver: &mpsc::Receiver<Job>,
    conn: &mut Connection,
    bodies: &BodyStorage,
    gate: Gate,
    commits: &Commits,
) {
    let mut group: Vec<Box<dyn Grouped>> = Vec::new();
    let commit = |conn: &mut Connection, group: &mut Vec<Box<dyn Grouped>>| {
        if !group.is_empty() {
            commit_group(conn, bodies, &gate, group);
            commits.committed();
        }
    };
    while let Ok(first) = receiver.recv() {
        let mut next = Some(first);
        while let Some(job) = next.take() {
            match job {
                Job::Grouped(op) => group.push(op),
                Job::Exclusive(work) => {
                    commit(conn, &mut group);
                    work(conn, bodies, &gate);
                }
            }
            if group.len() >= MAX_GROUP {
                commit(conn, &mut group);
            }
            next = receiver.try_recv().ok();
        }
        commit(conn, &mut group);
    }
}

/// Runs the queued mutations in one transaction, commits once, then answers them.
fn commit_group(
    conn: &mut Connection,
    bodies: &BodyStorage,
    gate: &Gate,
    group: &mut Vec<Box<dyn Grouped>>,
) {
    if group.is_empty() {
        return;
    }
    let ops = std::mem::take(group);
    let tx = match conn.transaction_with_behavior(TransactionBehavior::Immediate) {
        Ok(tx) => tx,
        Err(error) => {
            let cause = error.to_string();
            for op in ops {
                op.fail(&cause);
            }
            return;
        }
    };
    let mut done: Vec<Box<dyn Finish>> = Vec::with_capacity(ops.len());
    let mut broken: Option<String> = None;
    let mut queue = ops.into_iter();
    for op in queue.by_ref() {
        if let Err(error) = tx.execute_cached("SAVEPOINT op", []) {
            broken = Some(error.to_string());
            op.fail(&error.to_string());
            break;
        }
        let finished = op.run(&tx, bodies);
        let end = if finished.succeeded() {
            tx.execute_cached("RELEASE op", [])
        } else {
            tx.execute_cached("ROLLBACK TO op", [])
                .and_then(|_| tx.execute_cached("RELEASE op", []))
        };
        done.push(finished);
        if let Err(error) = end {
            broken = Some(error.to_string());
            break;
        }
    }
    let committed = match broken {
        Some(cause) => {
            // Dropping the transaction rolls every operation of the group back.
            drop(tx);
            for op in queue {
                op.fail(&cause);
            }
            Err(cause)
        }
        None => with_commit_gate(gate.as_ref(), || Ok(tx.commit()?)).map_err(|e| e.to_string()),
    };
    for finished in done {
        finished.finish(committed.as_ref().map_err(String::as_str).copied());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op<T: Send + 'static>(
        operation: impl FnOnce(&Transaction<'_>, &BodyStorage) -> Result<T> + Send + 'static,
    ) -> (Box<dyn Grouped>, oneshot::Receiver<Result<T>>) {
        let (reply, receive) = oneshot::channel();
        (Box::new(Op { operation, reply }), receive)
    }

    fn insert(name: &'static str) -> impl FnOnce(&Transaction<'_>, &BodyStorage) -> Result<()> {
        move |tx, _| {
            tx.execute("INSERT INTO t(name) VALUES(?1)", [name])?;
            Ok(())
        }
    }

    #[test]
    fn a_failing_operation_rolls_back_alone_and_the_group_commits_once() {
        let directory = tempfile::tempdir().unwrap();
        let bodies = BodyStorage::new(directory.path().join("bodies"), 0).unwrap();
        let mut conn = Connection::open(directory.path().join("db")).unwrap();
        conn.execute_batch("CREATE TABLE t(name TEXT NOT NULL)")
            .unwrap();
        let (first, first_answer) = op(insert("a"));
        let (failing, failing_answer) = op(|tx, _| {
            tx.execute("INSERT INTO t(name) VALUES('b')", [])?;
            Err::<(), _>(Error::Invalid("refused after writing".into()))
        });
        let (last, last_answer) = op(insert("c"));
        let mut group = vec![first, failing, last];
        commit_group(&mut conn, &bodies, &None, &mut group);
        assert!(first_answer.blocking_recv().unwrap().is_ok());
        assert!(matches!(
            failing_answer.blocking_recv().unwrap(),
            Err(Error::Invalid(_))
        ));
        assert!(last_answer.blocking_recv().unwrap().is_ok());
        let names: Vec<String> = conn
            .prepare("SELECT name FROM t ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(names, ["a", "c"]);
    }

    #[test]
    fn queued_operations_are_answered_only_after_their_commit() {
        let directory = tempfile::tempdir().unwrap();
        let bodies = BodyStorage::new(directory.path().join("bodies"), 0).unwrap();
        let path = directory.path().join("db");
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE t(name TEXT NOT NULL)")
            .unwrap();
        let (sender, receiver) = mpsc::channel();
        let mut answers = Vec::new();
        for name in ["a", "b", "c", "d"] {
            let (job, answer) = op(insert(name));
            sender.send(Job::Grouped(job)).unwrap();
            answers.push(answer);
        }
        drop(sender);
        run(&receiver, &mut conn, &bodies, None, &Commits::detached());
        let reader = Connection::open(&path).unwrap();
        for answer in answers {
            assert!(answer.blocking_recv().unwrap().is_ok());
        }
        let count: i64 = reader
            .query_row("SELECT COUNT(*) FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 4);
    }
}
