//! Durable message storage for the protocol pipeline and local Messages IPC handlers.
//!
//! One dedicated thread owns the write connection. Mutations run in IMMEDIATE transactions,
//! each under its own savepoint, and queued mutations share one transaction (group commit,
//! `writer`); each answers its async oneshot only after that FULL-synchronous WAL commit.
//! The pipeline awaits `commit_received` before acknowledging its sender and awaits
//! `begin_attempt` before writing any destination byte. A supplied engine test-hook watch
//! receiver holds transactions before commit. Dropping all handles drains and stops the
//! writer. Separate pooled connections serve coherent read snapshots without blocking it.
//!
//! Forward-only embedded migrations create STRICT messages, original/transformed bodies,
//! deliveries, attempts, alert episodes and persistent counters. Configuration identifiers
//! are TEXT, independent of their core representation. Body spills are synced before the
//! database references them; startup and retention collect unreferenced files. No body is
//! logged or searched: callers explicitly supply safe searchable text. IPC query methods
//! return the shared DTOs; body reads return bytes for the IPC renderer.
//!
//! Open does not reconcile automatically: the host loads current configuration, calls
//! `reconcile_at_startup`, reprocesses its returned receipt ids, restores paused channels,
//! and only then opens sources. The host schedules retention every ten minutes.
//!
//! Backups: [`Store::backup_into`] copies a consistent snapshot (online backup API from a
//! reader, so commits continue) and the body files it references, holding body-file
//! collection off meanwhile; [`backup_closed`] does the same for a store nobody has open.

#![warn(missing_docs)]

mod alerts;
mod backup;
mod bodies;
mod checkpoint;
mod commands;
mod counters;
mod finish;
mod lifecycle;
mod maintenance;
mod model;
mod query;
mod receipt;
mod receiving;
mod schema;
#[cfg(test)]
mod testing;
mod writer;

pub use backup::{BODIES_DIR, DATABASE_FILE, StoreCopy, backup_closed};
pub use bodies::content_hash;
pub use finish::{FrozenPolicy, delivery_state_after};
pub use model::*;

use bodies::BodyStorage;
use rusqlite::{Connection, Transaction};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use tokio::sync::{oneshot, watch};

/// Store operation result.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors contain metadata only; callers must never place payloads in error summaries.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The database failed.
    #[error("SQLite: {0}")]
    Sql(#[from] rusqlite::Error),
    /// A body or state-directory file operation failed, or a commit could not complete.
    #[error("body I/O: {0}")]
    Io(#[from] std::io::Error),
    /// Stored or supplied JSON is malformed.
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// The referenced record does not exist.
    #[error("record not found")]
    NotFound,
    /// The operation contradicts the record's state or the delivery rules.
    #[error("invalid operation: {0}")]
    Invalid(String),
    /// An operator command failed a precondition.
    #[error("command rejected ({code:?}): {message}")]
    Rejected {
        /// The machine-readable reason.
        code: packie_ipc::messages::RejectionCode,
        /// What the operator should do about it.
        message: String,
    },
    /// The writer thread is gone.
    #[error("store writer stopped")]
    Stopped,
}

struct Inner {
    sender: mpsc::Sender<writer::Job>,
    readers: Mutex<Vec<Connection>>,
    reader_ready: Condvar,
    database: PathBuf,
    bodies: BodyStorage,
}

/// Cheap cloneable handle. Writes are async; reads use a bounded connection pool.
#[derive(Clone)]
pub struct Store(Arc<Inner>);

impl Store {
    /// Opens or migrates `<state>/messages.sqlite3` with WAL and FULL durability.
    ///
    /// # Errors
    /// The state directory or database cannot be created, opened or migrated.
    pub fn open(state: impl AsRef<Path>, options: OpenOptions) -> Result<Self> {
        let state = state.as_ref();
        private_dir(state)?;
        let database = state.join(DATABASE_FILE);
        private_file(&database)?;
        let mut writer = connect(&database)?;
        writer.pragma_update(None, "journal_mode", "WAL")?;
        // Every grouped operation runs under a savepoint; its sub-journal (pages to undo
        // that one operation) stays in memory instead of a temporary file. It never holds
        // committed data, so durability is unchanged.
        writer.pragma_update(None, "temp_store", "MEMORY")?;
        writer.pragma_update(
            None,
            "wal_autocheckpoint",
            checkpoint::WRITER_BACKSTOP_PAGES,
        )?;
        schema::migrate(&mut writer)?;
        let bodies = BodyStorage::new(state.join(BODIES_DIR), options.inline_body_cap)?;
        bodies.cleanup_unreferenced(&writer)?;
        let readers = (0..options.read_connections.max(1))
            .map(|_| connect(&database))
            .collect::<Result<Vec<_>>>()?;
        let commits = checkpoint::spawn(connect(&database)?)?;
        let (sender, receiver) = mpsc::channel::<writer::Job>();
        let storage = bodies.clone();
        std::thread::Builder::new()
            .name("packie-store".into())
            .spawn(move || {
                writer::run(
                    &receiver,
                    &mut writer,
                    &storage,
                    options.commits_held,
                    &commits,
                );
            })?;
        Ok(Self(Arc::new(Inner {
            sender,
            readers: Mutex::new(readers),
            reader_ready: Condvar::new(),
            database,
            bodies,
        })))
    }

    /// Database location, useful for backup tooling and durability diagnostics.
    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.0.database
    }

    async fn write<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>, &BodyStorage) -> Result<T> + Send + 'static,
    {
        let (reply, receive) = oneshot::channel();
        self.0
            .sender
            .send(writer::Job::Grouped(Box::new(writer::Op {
                operation,
                reply,
            })))
            .map_err(|_| Error::Stopped)?;
        receive.await.map_err(|_| Error::Stopped)?
    }

    fn read<T>(&self, operation: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut readers = self
            .0
            .readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while readers.is_empty() {
            readers = self
                .0
                .reader_ready
                .wait(readers)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let mut conn = readers.pop().ok_or(Error::Stopped)?;
        drop(readers);
        let result = operation(&mut conn);
        self.0
            .readers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(conn);
        self.0.reader_ready.notify_one();
        result
    }

    /// Runs several reads against one coherent snapshot of the database.
    fn snapshot<T>(&self, operation: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        self.read(|conn| {
            let tx = conn.transaction()?;
            let result = operation(&tx)?;
            tx.commit()?;
            Ok(result)
        })
    }

    // File collection runs on the writer after the row-deletion commit. Its own content
    // deletion transaction commits first, so rollback can never restore a missing file.
    async fn cleanup_bodies(&self) -> Result<u64> {
        let (reply, receive) = oneshot::channel();
        self.0
            .sender
            .send(writer::Job::Exclusive(Box::new(
                move |conn, bodies, held| {
                    let result =
                        with_commit_gate(held.as_ref(), || bodies.cleanup_unreferenced(conn));
                    let _ = reply.send(result);
                },
            )))
            .map_err(|_| Error::Stopped)?;
        receive.await.map_err(|_| Error::Stopped)?
    }

    /// One page of the Messages grid with its total, under the query's filter and sort.
    ///
    /// # Errors
    /// [`Error::Invalid`] for a malformed query; the database cannot be read.
    pub fn query_messages(
        &self,
        query: &packie_ipc::messages::MessageQuery,
    ) -> Result<packie_ipc::messages::MessageList> {
        self.read(|conn| query::messages(conn, query))
    }
    /// Per-destination delivery totals of the messages `filter` selects (the transmission
    /// report's totals), destinations in the order of their first message.
    ///
    /// # Errors
    /// The database cannot be read.
    pub fn transmission_totals(
        &self,
        filter: &packie_ipc::messages::MessageFilter,
    ) -> Result<Vec<packie_ipc::reports::DestinationTransmission>> {
        self.read(|conn| query::transmission_totals(conn, filter))
    }
    /// A message with its deliveries and attempts.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown message.
    pub fn message(
        &self,
        id: packie_core::MessageId,
    ) -> Result<packie_ipc::messages::MessageRecord> {
        self.snapshot(|tx| query::message(tx, id))
    }
    /// A delivery with its attempt log.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an unknown delivery.
    pub fn delivery(
        &self,
        id: packie_core::DeliveryId,
    ) -> Result<packie_ipc::messages::DeliveryRecord> {
        self.snapshot(|tx| query::delivery(tx, id))
    }
    /// The original bytes of a message, or a delivery's transformed output.
    ///
    /// # Errors
    /// [`Error::NotFound`] when no such body is stored; [`Error::Invalid`] when the stored
    /// body fails its integrity check.
    pub fn body(
        &self,
        message: packie_core::MessageId,
        delivery: Option<packie_core::DeliveryId>,
    ) -> Result<Vec<u8>> {
        self.read(|conn| self.0.bodies.get(conn, message, delivery))
    }
}

/// Runs literal SQL through the connection's prepared-statement cache: the hot
/// operations repeat the same few dozen statements, and parsing them every time was a
/// large share of the writer's work.
pub(crate) trait Cached {
    /// [`Connection::query_row`] with a cached statement.
    fn query_row_cached<T, P, F>(&self, sql: &str, params: P, row: F) -> rusqlite::Result<T>
    where
        P: rusqlite::Params,
        F: FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>;
    /// [`Connection::execute`] with a cached statement.
    fn execute_cached<P: rusqlite::Params>(&self, sql: &str, params: P) -> rusqlite::Result<usize>;
}

impl Cached for Connection {
    fn query_row_cached<T, P, F>(&self, sql: &str, params: P, row: F) -> rusqlite::Result<T>
    where
        P: rusqlite::Params,
        F: FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    {
        self.prepare_cached(sql)?.query_row(params, row)
    }

    fn execute_cached<P: rusqlite::Params>(&self, sql: &str, params: P) -> rusqlite::Result<usize> {
        self.prepare_cached(sql)?.execute(params)
    }
}

fn connect(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    // The store runs about eighty distinct literal statements; keep them all prepared.
    conn.set_prepared_statement_cache_capacity(128);
    Ok(conn)
}

/// The first column of every row `sql` returns.
pub(crate) fn column<T: rusqlite::types::FromSql>(
    conn: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<T>> {
    Ok(conn
        .prepare(sql)?
        .query_map(params, |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

// Keep the watch read guard through the commit: enabling the hold cannot report
// success while an already-admitted commit is still becoming durable.
pub(crate) fn with_commit_gate<T>(
    held: Option<&watch::Receiver<bool>>,
    operation: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let Some(mut receiver) = held.cloned() else {
        return operation();
    };
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    runtime.block_on(async move {
        loop {
            let permit = receiver.borrow_and_update();
            if !*permit {
                let result = operation();
                drop(permit);
                return result;
            }
            drop(permit);
            receiver.changed().await.map_err(|_| Error::Stopped)?;
        }
    })
}

pub(crate) fn private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true).mode(0o700).create(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir_all(path)?;
    Ok(())
}

fn private_file(path: &Path) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
