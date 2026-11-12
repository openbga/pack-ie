//! The [`Audit`] handle: recording, availability tracking and review.

mod maintenance;

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use packie_ipc::audit::{
    AuditExportChunk, AuditExportRequest, AuditPage, AuditQuery, AuditVerification,
};

use crate::copy::{Copied, copy_dir};
use crate::writer::{Head, Writer};
use crate::{Entry, Error, Options, Result, Status, query, verify_dir};

/// The file [`Audit::check_writable`] writes and removes in the audit directory.
const WRITE_CHECK: &str = ".write-check";

/// Whether the last write succeeded, and why not.
struct Health {
    available: bool,
    last_error: Option<String>,
}

impl Health {
    const fn available() -> Self {
        Self {
            available: true,
            last_error: None,
        }
    }
}

struct Inner {
    dir: std::path::PathBuf,
    options: Options,
    writer: Mutex<Option<Writer>>,
    health: Mutex<Health>,
    /// Events committed since the last checkpoint.
    dirty: AtomicBool,
}

/// Cheap cloneable handle to the audit log of one state directory.
#[derive(Clone)]
pub struct Audit(Arc<Inner>);

impl Audit {
    /// Opens `<state>/audit` (`dir`), failing when it cannot be opened.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] with the cause when the directory or its file cannot be
    /// opened, created or migrated.
    pub fn open(dir: impl AsRef<Path>, options: Options) -> Result<Self> {
        let audit = Self::start(dir, options);
        let failure = {
            let health = audit.health();
            (!health.available).then(|| health.last_error.clone().unwrap_or_default())
        };
        match failure {
            None => Ok(audit),
            Some(cause) => Err(Error::Unavailable(cause)),
        }
    }

    /// Opens `dir` for the engine: when opening fails, the handle starts unavailable and
    /// every write (and the maintenance thread, every minute) tries to open it again.
    #[must_use]
    pub fn start(dir: impl AsRef<Path>, options: Options) -> Self {
        let dir = dir.as_ref().to_owned();
        let (writer, health) = match Writer::open(&dir, options.busy_timeout) {
            Ok(writer) => (Some(writer), Health::available()),
            Err(error) => {
                tracing::error!(event = "audit.unavailable", %error, "The audit log cannot be opened; audited commands are refused");
                let health = Health {
                    available: false,
                    last_error: Some(error.to_string()),
                };
                (None, health)
            }
        };
        let background = options.background;
        let audit = Self(Arc::new(Inner {
            dir,
            options,
            writer: Mutex::new(writer),
            health: Mutex::new(health),
            dirty: AtomicBool::new(false),
        }));
        if background {
            maintenance::spawn(Arc::downgrade(&audit.0));
        }
        audit
    }

    /// Records one event and returns its seq once it is committed.
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] when it cannot be written (the caller refuses the command);
    /// [`Error::Invalid`] for a malformed entry.
    pub fn record(&self, entry: &Entry) -> Result<u64> {
        let details = entry.checked_details()?;
        let mut slot = self.writer();
        if slot.is_none() {
            match Writer::open(&self.0.dir, self.0.options.busy_timeout) {
                Ok(writer) => *slot = Some(writer),
                Err(error) => return Err(self.failed(error)),
            }
        }
        let at = (self.0.options.clock)();
        match Writer::append(&mut slot, entry, &details, at) {
            Ok(seq) => {
                self.0.dirty.store(true, Ordering::Release);
                drop(slot);
                self.recovered();
                Ok(seq)
            }
            Err(error) => {
                // Reopen at the next write: the head is read again from the file.
                *slot = None;
                drop(slot);
                Err(self.failed(error))
            }
        }
    }

    /// Checks now, without recording an event, that the log can be written: the writer is
    /// open (reopened if needed) and a small file can be written and synced in the audit
    /// directory. A failure marks the log unavailable, as a failed write does; success does
    /// not mark it available (only a recorded event does).
    ///
    /// # Errors
    ///
    /// [`Error::Unavailable`] with the cause.
    pub fn check_writable(&self) -> Result<()> {
        {
            let mut slot = self.writer();
            if slot.is_none() {
                match Writer::open(&self.0.dir, self.0.options.busy_timeout) {
                    Ok(writer) => *slot = Some(writer),
                    Err(error) => return Err(self.failed(error)),
                }
            }
        }
        let probe = self.0.dir.join(WRITE_CHECK);
        let written = std::fs::File::create(&probe).and_then(|mut file| {
            file.write_all(b"1")?;
            file.sync_all()
        });
        let _ = std::fs::remove_file(&probe);
        written.map_err(|error| {
            self.failed(Error::Unavailable(format!(
                "the audit directory cannot be written: {error}"
            )))
        })
    }

    /// Whether the log can be written, and its head.
    #[must_use]
    pub fn status(&self) -> Status {
        let (head_seq, head_hash) = self
            .head()
            .map_or((0, String::new()), |head| (head.seq, head.hash));
        let health = self.health();
        Status {
            available: health.available,
            last_error: health.last_error.clone(),
            head_seq,
            head_hash,
        }
    }

    /// A page of events, newest first.
    ///
    /// # Errors
    ///
    /// [`Error::Invalid`] for a malformed query, or the audit files cannot be read.
    pub fn query(&self, query: &AuditQuery) -> Result<AuditPage> {
        query.check().map_err(Error::Invalid)?;
        query::page(&self.0.dir, query)
    }

    /// One export chunk, oldest first.
    ///
    /// # Errors
    ///
    /// The audit files cannot be read or the chunk cannot be encoded.
    pub fn export(&self, request: &AuditExportRequest) -> Result<AuditExportChunk> {
        query::export(&self.0.dir, request)
    }

    /// Walks the whole chain.
    ///
    /// # Errors
    ///
    /// The audit directory cannot be listed; unreadable files are reported as problems.
    pub fn verify(&self) -> Result<AuditVerification> {
        verify_dir(&self.0.dir)
    }

    /// Copies a consistent snapshot of every audit file into `dest` (the backup's `audit`
    /// directory), holding writes for the duration. Returns the files written and the head.
    ///
    /// # Errors
    ///
    /// The destination cannot be created or an audit file cannot be copied.
    pub fn backup_into(&self, dest: &Path) -> Result<Copied> {
        let _writes = self.writer();
        copy_dir(&self.0.dir, dest)
    }

    /// The chain head, `None` while the writer is closed.
    fn head(&self) -> Option<Head> {
        self.writer().as_ref().map(|writer| writer.head.clone())
    }

    fn writer(&self) -> MutexGuard<'_, Option<Writer>> {
        self.0.writer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn health(&self) -> MutexGuard<'_, Health> {
        self.0.health.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Marks the log unavailable and answers [`Error::Unavailable`] with the cause.
    fn failed(&self, error: Error) -> Error {
        let cause = match error {
            Error::Unavailable(cause) => cause,
            other => other.to_string(),
        };
        let mut health = self.health();
        if health.available {
            tracing::error!(event = "audit.unavailable", error = %cause, "The audit log cannot be written; audited commands are refused");
        }
        health.available = false;
        health.last_error = Some(cause.clone());
        Error::Unavailable(cause)
    }

    /// Marks the log available again after a successful write.
    fn recovered(&self) {
        let mut health = self.health();
        if !health.available {
            tracing::info!(
                event = "audit.available",
                "The audit log can be written again"
            );
        }
        *health = Health::available();
    }
}
