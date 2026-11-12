//! Embedded, forward-only audit schema migrations.

use rusqlite::{Connection, TransactionBehavior};

use crate::{Error, Result};

/// The current schema version.
pub(crate) const VERSION: i64 = 1;

const V1: &str = r"
CREATE TABLE events (
    seq INTEGER PRIMARY KEY CHECK (seq > 0),
    at TEXT NOT NULL,
    actor_uid TEXT NOT NULL,
    actor_name TEXT NOT NULL,
    origin TEXT NOT NULL CHECK (origin IN ('console', 'cli', 'engine', 'other')),
    action TEXT NOT NULL,
    object_type TEXT,
    object_id TEXT,
    outcome TEXT NOT NULL CHECK (outcome IN ('requested', 'succeeded', 'rejected', 'failed')),
    details TEXT NOT NULL CHECK (json_valid(details)),
    prev_hash TEXT NOT NULL CHECK (length(prev_hash) = 64),
    hash TEXT NOT NULL UNIQUE CHECK (length(hash) = 64)
) STRICT;

CREATE INDEX events_at ON events(at);
CREATE INDEX events_action ON events(action, seq);
CREATE INDEX events_actor ON events(actor_name, seq);
CREATE INDEX events_object ON events(object_id, seq);

CREATE TRIGGER events_no_update BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'audit events are append-only'); END;

CREATE TRIGGER events_no_delete BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'audit events are append-only'); END;
";

/// Creates or upgrades the schema; refuses a newer one without touching it.
pub(crate) fn migrate(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY) STRICT;",
    )?;
    let version: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;
    if !(0..=VERSION).contains(&version) {
        return Err(Error::invalid(format!(
            "unsupported audit schema version {version}"
        )));
    }
    if version < 1 {
        transaction.execute_batch(V1)?;
        transaction.execute("INSERT INTO schema_version(version) VALUES (1)", [])?;
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_once_into_strict_append_only_tables() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let strict: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_list WHERE schema = 'main' AND strict = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(strict, 2);
        connection
            .execute(
                "INSERT INTO events VALUES (1, 't', 'u', 'n', 'cli', 'a', NULL, NULL, \
                 'succeeded', '{}', ?1, ?2)",
                [crate::chain::GENESIS, &"1".repeat(64)],
            )
            .unwrap();
        for statement in [
            "UPDATE events SET actor_name = 'other'",
            "DELETE FROM events",
        ] {
            let error = connection.execute(statement, []).unwrap_err();
            assert!(error.to_string().contains("append-only"), "{error}");
        }
    }

    #[test]
    fn refuses_a_newer_schema() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_version(version INTEGER PRIMARY KEY) STRICT;
                 INSERT INTO schema_version VALUES (2);",
            )
            .unwrap();
        assert!(migrate(&mut connection).is_err());
    }
}
