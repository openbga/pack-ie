//! Process-wide `tracing` setup: JSON lines to rotating files, the in-memory ring, and an
//! optional human-readable console stream.

use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::ring::LogRing;
use crate::rotate::RotatingFile;

/// Environment variable that overrides the configured log filter.
pub const LOG_ENV: &str = "PACKIE_LOG";

/// Lines queued for the console writer before loggers wait.
const CONSOLE_LINES: usize = 8_192;

/// Where and how to log.
#[derive(Debug, Clone)]
pub struct LogOptions {
    /// Directory of the `packie.YYYY-MM-DD[.N].log` files (local dates); created owner-only.
    pub dir: PathBuf,
    /// Filter in env-filter syntax; [`LOG_ENV`] overrides it.
    pub filter: String,
    /// Days of files to keep.
    pub retention_days: u32,
    /// A file rotates before it would grow past this many bytes (and at local midnight).
    pub max_file_bytes: u64,
    /// Events kept in the ring.
    pub ring_capacity: usize,
    /// Also write human-readable lines to stderr (foreground runs).
    pub console: bool,
}

/// Logging could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum LogInitError {
    /// The filter text is not valid env-filter syntax.
    #[error("invalid log filter {filter:?}: {message}")]
    Filter {
        /// The filter.
        filter: String,
        /// Why it is invalid.
        message: String,
    },
    /// The log directory could not be created.
    #[error(transparent)]
    Directory(#[from] packie_config::ConfigError),
    /// The log file could not be opened.
    #[error("cannot open log files: {0}")]
    Appender(#[from] std::io::Error),
    /// A global subscriber is already installed.
    #[error("logging is already initialized: {0}")]
    AlreadyInitialized(#[from] tracing_subscriber::util::TryInitError),
}

/// Keeps logging alive; dropping it flushes the file and console writers.
#[derive(Debug)]
pub struct LogHandle {
    ring: LogRing,
    _file_guard: WorkerGuard,
    _console_guard: Option<WorkerGuard>,
}

impl LogHandle {
    /// The in-memory ring of recent events.
    #[must_use]
    pub fn ring(&self) -> &LogRing {
        &self.ring
    }
}

/// Installs the global subscriber. Call once per process.
///
/// # Errors
/// The filter is invalid, the log directory or file cannot be created, or a subscriber is
/// already installed.
pub fn init(options: &LogOptions) -> Result<LogHandle, LogInitError> {
    let filter_text = std::env::var(LOG_ENV).unwrap_or_else(|_| options.filter.clone());
    let filter = EnvFilter::try_new(&filter_text).map_err(|e| LogInitError::Filter {
        filter: filter_text.clone(),
        message: e.to_string(),
    })?;

    packie_config::create_private_dir(&options.dir)?;
    let appender =
        RotatingFile::open(&options.dir, options.max_file_bytes, options.retention_days)?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_span_list(true)
        .with_writer(writer);

    // The console is written by its own thread, like the files, so a busy stderr (a pipe
    // read slowly, a terminal) never holds up the thread that logged. It is lossless: a
    // full buffer makes callers wait rather than drop lines.
    let (console_layer, console_guard) = if options.console {
        let (writer, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
            .lossy(false)
            // The channel preallocates its slots; a console needs far fewer than a file.
            .buffered_lines_limit(CONSOLE_LINES)
            .thread_name("packie-console-log")
            .finish(std::io::stderr());
        let layer = tracing_subscriber::fmt::layer()
            .compact()
            .with_writer(writer);
        (Some(layer), Some(guard))
    } else {
        (None, None)
    };

    let ring = LogRing::new(options.ring_capacity);
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(ring.layer())
        .with(console_layer)
        .try_init()?;
    Ok(LogHandle {
        ring,
        _file_guard: guard,
        _console_guard: console_guard,
    })
}
