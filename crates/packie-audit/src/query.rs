//! Review queries over every audit file: pages newest first, export chunks oldest first.

use std::path::Path;

use packie_core::Timestamp;
use packie_ipc::audit::{
    AuditAction, AuditEvent, AuditExportChunk, AuditExportFormat, AuditExportRequest, AuditFilter,
    AuditObjectType, AuditOrigin, AuditOutcome, AuditPage, AuditQuery,
};
use rusqlite::Row;
use rusqlite::types::ToSql;

use crate::files::{self, AuditFile};
use crate::{Error, Result};

/// Events per export chunk; keeps a chunk far below the 16 MiB frame limit.
pub(crate) const EXPORT_CHUNK: usize = 1000;

/// The stored columns of an event, in the order [`Raw::read`] expects.
pub(crate) const COLUMNS: &str = "seq, at, actor_uid, actor_name, origin, action, object_type, \
                                  object_id, outcome, details, prev_hash, hash";

/// A page of events across all files, newest first.
pub(crate) fn page(dir: &Path, query: &AuditQuery) -> Result<AuditPage> {
    let limit = usize::try_from(query.limit).unwrap_or(usize::MAX);
    let mut events = Vec::new();
    for file in files::list(dir)?.iter().rev() {
        if query
            .before_seq
            .zip(file.first_seq)
            .is_some_and(|(before, first)| first >= before)
        {
            continue;
        }
        let wanted = limit + 1 - events.len();
        events.extend(select(
            file,
            &query.filter,
            Cursor::Before(query.before_seq),
            wanted,
        )?);
        if events.len() > limit {
            break;
        }
    }
    let more = events.len() > limit;
    events.truncate(limit);
    Ok(AuditPage {
        next_before_seq: more.then(|| events.last().map(|e| e.seq)).flatten(),
        events,
    })
}

/// One export chunk across all files, oldest first.
pub(crate) fn export(dir: &Path, request: &AuditExportRequest) -> Result<AuditExportChunk> {
    let mut events = Vec::new();
    for file in files::list(dir)? {
        let wanted = EXPORT_CHUNK + 1 - events.len();
        events.extend(select(
            &file,
            &request.filter,
            Cursor::After(request.after_seq),
            wanted,
        )?);
        if events.len() > EXPORT_CHUNK {
            break;
        }
    }
    let more = events.len() > EXPORT_CHUNK;
    events.truncate(EXPORT_CHUNK);
    let content = match request.format {
        AuditExportFormat::Csv => csv_text(&events, request.after_seq.is_none())?,
        AuditExportFormat::JsonLines => {
            let mut text = String::new();
            for event in &events {
                text.push_str(&serde_json::to_string(event)?);
                text.push('\n');
            }
            text
        }
    };
    Ok(AuditExportChunk {
        format: request.format,
        content,
        rows: u32::try_from(events.len()).unwrap_or(u32::MAX),
        next_after_seq: more.then(|| events.last().map(|e| e.seq)).flatten(),
        // The engine issues the export session.
        export_id: None,
    })
}

fn csv_text(events: &[AuditEvent], header: bool) -> Result<String> {
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    if header {
        writer.write_record(COLUMNS.split(", ").map(str::trim))?;
    }
    for event in events {
        let seq = event.seq.to_string();
        let at = event.at.to_string();
        let details = crate::chain::canonical_json(&event.details);
        writer.write_record([
            seq.as_str(),
            &at,
            &event.actor_uid,
            &event.actor_name,
            event.origin.as_str(),
            event.action.as_str(),
            event.object_type.map_or("", |t| t.as_str()),
            event.object_id.as_deref().unwrap_or(""),
            event.outcome.as_str(),
            &details,
            &event.prev_hash,
            &event.hash,
        ])?;
    }
    let bytes = writer
        .into_inner()
        .map_err(|error| Error::Io(error.into_error()))?;
    String::from_utf8(bytes).map_err(|_| Error::invalid("export is not UTF-8"))
}

#[derive(Clone, Copy)]
enum Cursor {
    /// Newest first, below this seq.
    Before(Option<u64>),
    /// Oldest first, above this seq.
    After(Option<u64>),
}

/// A `WHERE` clause under construction: conditions joined with `AND` and their values.
#[derive(Default)]
struct Conditions {
    clauses: Vec<&'static str>,
    values: Vec<Box<dyn ToSql>>,
}

impl Conditions {
    /// Adds `clause`, which takes one `?` per value.
    fn add<T: ToSql + 'static, const N: usize>(&mut self, clause: &'static str, values: [T; N]) {
        self.clauses.push(clause);
        self.values
            .extend(values.into_iter().map(|v| Box::new(v) as Box<dyn ToSql>));
    }

    fn add_opt<T: ToSql + 'static>(&mut self, clause: &'static str, value: Option<T>) {
        if let Some(value) = value {
            self.add(clause, [value]);
        }
    }

    /// Everything the filter and cursor ask for.
    fn new(filter: &AuditFilter, cursor: Cursor) -> Self {
        let mut conditions = Self::default();
        match cursor {
            Cursor::Before(seq) => conditions.add_opt("seq < ?", seq),
            Cursor::After(seq) => conditions.add_opt("seq > ?", seq),
        }
        conditions.add_opt("at >= ?", filter.since);
        conditions.add_opt("at < ?", filter.until);
        if let Some(actor) = &filter.actor {
            conditions.add(
                "(actor_name = ? OR actor_uid = ?)",
                [actor, actor].map(Clone::clone),
            );
        }
        conditions.add_opt("action = ?", filter.action.map(AuditAction::as_str));
        conditions.add_opt("origin = ?", filter.origin.map(AuditOrigin::as_str));
        conditions.add_opt("outcome = ?", filter.outcome.map(AuditOutcome::as_str));
        conditions.add_opt(
            "object_type = ?",
            filter.object_type.map(AuditObjectType::as_str),
        );
        conditions.add_opt("object_id = ?", filter.object_id.clone());
        conditions
    }

    fn where_clause(&self) -> String {
        if self.clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", self.clauses.join(" AND "))
        }
    }
}

fn select(
    file: &AuditFile,
    filter: &AuditFilter,
    cursor: Cursor,
    limit: usize,
) -> Result<Vec<AuditEvent>> {
    let order = match cursor {
        Cursor::Before(_) => "DESC",
        Cursor::After(_) => "ASC",
    };
    let mut conditions = Conditions::new(filter, cursor);
    let sql = format!(
        "SELECT {COLUMNS} FROM events {} ORDER BY seq {order} LIMIT ?",
        conditions.where_clause()
    );
    conditions
        .values
        .push(Box::new(i64::try_from(limit).unwrap_or(i64::MAX)));
    let connection = files::read_only(&file.path)?;
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(
        rusqlite::params_from_iter(conditions.values.iter()),
        Raw::read,
    )?;
    let mut events = Vec::new();
    for row in rows {
        events.push(row?.event()?);
    }
    Ok(events)
}

/// One stored row, as text.
pub(crate) struct Raw {
    pub(crate) seq: u64,
    pub(crate) at: String,
    pub(crate) actor_uid: String,
    pub(crate) actor_name: String,
    pub(crate) origin: String,
    pub(crate) action: String,
    pub(crate) object_type: Option<String>,
    pub(crate) object_id: Option<String>,
    pub(crate) outcome: String,
    pub(crate) details: String,
    pub(crate) prev_hash: String,
    pub(crate) hash: String,
}

impl Raw {
    /// Reads a row selected with [`COLUMNS`].
    pub(crate) fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            seq: row.get(0)?,
            at: row.get(1)?,
            actor_uid: row.get(2)?,
            actor_name: row.get(3)?,
            origin: row.get(4)?,
            action: row.get(5)?,
            object_type: row.get(6)?,
            object_id: row.get(7)?,
            outcome: row.get(8)?,
            details: row.get(9)?,
            prev_hash: row.get(10)?,
            hash: row.get(11)?,
        })
    }

    /// The columns the hash covers.
    pub(crate) fn fields(&self) -> crate::chain::Fields<'_> {
        crate::chain::Fields {
            seq: self.seq,
            at: &self.at,
            actor_uid: &self.actor_uid,
            actor_name: &self.actor_name,
            origin: &self.origin,
            action: &self.action,
            object_type: self.object_type.as_deref(),
            object_id: self.object_id.as_deref(),
            outcome: &self.outcome,
            details: &self.details,
        }
    }

    /// As the IPC type. Unknown vocabulary (a newer engine's file) is an error.
    fn event(self) -> Result<AuditEvent> {
        fn word<T: std::str::FromStr>(what: &str, value: &str) -> Result<T> {
            value
                .parse()
                .map_err(|_| Error::invalid(format!("unknown {what} `{value}`")))
        }
        Ok(AuditEvent {
            seq: self.seq,
            at: Timestamp::parse(&self.at)
                .map_err(|_| Error::invalid(format!("event {} has no valid time", self.seq)))?,
            origin: word("origin", &self.origin)?,
            action: word("action", &self.action)?,
            object_type: self
                .object_type
                .as_deref()
                .map(|t| word("object type", t))
                .transpose()?,
            outcome: word("outcome", &self.outcome)?,
            details: serde_json::from_str(&self.details)?,
            actor_uid: self.actor_uid,
            actor_name: self.actor_name,
            object_id: self.object_id,
            prev_hash: self.prev_hash,
            hash: self.hash,
        })
    }
}
