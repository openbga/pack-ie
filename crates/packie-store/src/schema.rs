//! Embedded, forward-only store migrations.

use rusqlite::{Connection, TransactionBehavior};

/// The baseline schema is kept available for migration regression tests.
pub(crate) const V1: &str = r"
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY) STRICT;

CREATE TABLE messages (
    id TEXT PRIMARY KEY NOT NULL,
    channel_id TEXT NOT NULL,
    channel_label TEXT NOT NULL,
    seq INTEGER NOT NULL,
    received_at TEXT NOT NULL,
    committed_at TEXT,
    direction TEXT NOT NULL,
    origin TEXT NOT NULL,
    protocol TEXT NOT NULL,
    state TEXT NOT NULL,
    control_id TEXT,
    sample_id TEXT,
    patient_id TEXT,
    message_type TEXT,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    content_hash TEXT NOT NULL,
    source_identity TEXT,
    source_connector_id TEXT,
    source_endpoint_id TEXT,
    source_peer TEXT,
    source_file TEXT,
    config_version INTEGER NOT NULL,
    duplicate_of TEXT REFERENCES messages(id) ON DELETE SET NULL,
    revision_of TEXT REFERENCES messages(id) ON DELETE SET NULL,
    resent_from TEXT REFERENCES messages(id) ON DELETE SET NULL,
    response_code TEXT,
    responded_at TEXT,
    abort_reason TEXT,
    reason_code TEXT,
    searchable_text TEXT NOT NULL DEFAULT '',
    UNIQUE(channel_id, seq)
) STRICT;

CREATE TABLE deliveries (
    id TEXT PRIMARY KEY NOT NULL,
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    channel_id TEXT NOT NULL,
    destination_id TEXT NOT NULL,
    destination_label TEXT NOT NULL,
    state TEXT NOT NULL,
    ack_level TEXT NOT NULL,
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    total_attempts INTEGER NOT NULL CHECK (total_attempts >= 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    next_attempt_at TEXT,
    reason_code TEXT,
    closed_reason TEXT,
    closed_at TEXT,
    mapping_version INTEGER,
    transformed_hash TEXT,
    max_attempts INTEGER NOT NULL CHECK (max_attempts > 0),
    retry_unacknowledged INTEGER NOT NULL CHECK (retry_unacknowledged IN (0, 1)),
    ack_required TEXT NOT NULL,
    last_error TEXT,
    UNIQUE(message_id, destination_id)
) STRICT;

CREATE TABLE delivery_attempts (
    id TEXT PRIMARY KEY NOT NULL,
    delivery_id TEXT NOT NULL REFERENCES deliveries(id) ON DELETE CASCADE,
    attempt_no INTEGER NOT NULL CHECK (attempt_no > 0),
    started_at TEXT NOT NULL,
    ended_at TEXT,
    result TEXT,
    ack_code TEXT,
    error_code TEXT,
    error_summary TEXT,
    ack_level TEXT NOT NULL,
    UNIQUE(delivery_id, attempt_no)
) STRICT;

CREATE TABLE message_bodies (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    delivery_id TEXT REFERENCES deliveries(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('original', 'transformed')),
    content_hash TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    inline_bytes BLOB,
    file_path TEXT,
    CHECK ((inline_bytes IS NULL) != (file_path IS NULL)),
    CHECK ((kind = 'original' AND delivery_id IS NULL)
        OR (kind = 'transformed' AND delivery_id IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX message_bodies_original ON message_bodies(message_id)
    WHERE kind = 'original';
CREATE UNIQUE INDEX message_bodies_transformed ON message_bodies(delivery_id)
    WHERE kind = 'transformed';

CREATE TABLE channel_counters (
    channel_id TEXT NOT NULL,
    destination_id TEXT NOT NULL DEFAULT '',
    received INTEGER NOT NULL DEFAULT 0,
    filtered INTEGER NOT NULL DEFAULT 0,
    duplicates INTEGER NOT NULL DEFAULT 0,
    errors INTEGER NOT NULL DEFAULT 0,
    invalid INTEGER NOT NULL DEFAULT 0,
    aborted INTEGER NOT NULL DEFAULT 0,
    bytes_in INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    acknowledged INTEGER NOT NULL DEFAULT 0,
    rejected INTEGER NOT NULL DEFAULT 0,
    timed_out INTEGER NOT NULL DEFAULT 0,
    outcome_unknown INTEGER NOT NULL DEFAULT 0,
    failed INTEGER NOT NULL DEFAULT 0,
    bytes_out INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(channel_id, destination_id)
) STRICT;

CREATE TABLE channel_sequences (
    channel_id TEXT PRIMARY KEY NOT NULL,
    last_seq INTEGER NOT NULL
) STRICT;

CREATE TABLE alerts (
    id TEXT PRIMARY KEY NOT NULL,
    identity TEXT NOT NULL,
    code TEXT NOT NULL,
    severity TEXT NOT NULL,
    state TEXT NOT NULL,
    channel_id TEXT,
    endpoint_id TEXT,
    destination_id TEXT,
    summary TEXT NOT NULL,
    next_action TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    cleared_at TEXT,
    occurrences INTEGER NOT NULL CHECK (occurrences > 0),
    details_json TEXT NOT NULL
) STRICT;

INSERT INTO schema_version(version) VALUES (1);
";

const V2: &str = r"
CREATE TABLE resend_targets (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    destination_id TEXT NOT NULL,
    PRIMARY KEY(message_id, destination_id)
) STRICT;
CREATE TABLE channel_flags (
    channel_id TEXT PRIMARY KEY NOT NULL,
    paused INTEGER NOT NULL CHECK (paused IN (0, 1))
) STRICT;
CREATE INDEX messages_channel_time ON messages(channel_id, received_at DESC, id DESC);
CREATE INDEX messages_time ON messages(received_at DESC, id DESC);
CREATE INDEX messages_state_time ON messages(state, received_at DESC, id DESC);
CREATE INDEX messages_channel_state_time ON messages(channel_id, state, received_at DESC);
CREATE INDEX messages_control_id ON messages(control_id) WHERE control_id IS NOT NULL;
CREATE INDEX messages_sample_id ON messages(sample_id) WHERE sample_id IS NOT NULL;
CREATE INDEX messages_patient_id ON messages(patient_id) WHERE patient_id IS NOT NULL;
CREATE INDEX messages_source_identity_hash ON messages(channel_id, source_identity, content_hash, seq DESC)
    WHERE source_identity IS NOT NULL;
CREATE INDEX messages_duplicate_of ON messages(duplicate_of) WHERE duplicate_of IS NOT NULL;
CREATE INDEX messages_revision_of ON messages(revision_of) WHERE revision_of IS NOT NULL;
CREATE INDEX messages_resent_from ON messages(resent_from) WHERE resent_from IS NOT NULL;
CREATE INDEX deliveries_channel_state ON deliveries(channel_id, state, updated_at);
CREATE INDEX deliveries_destination_queue ON deliveries(destination_id, state, next_attempt_at, message_id);
CREATE INDEX deliveries_state ON deliveries(state, updated_at);
CREATE INDEX message_bodies_hash ON message_bodies(content_hash);
CREATE INDEX message_bodies_delivery ON message_bodies(delivery_id) WHERE delivery_id IS NOT NULL;
CREATE INDEX message_bodies_file ON message_bodies(file_path) WHERE file_path IS NOT NULL;
CREATE INDEX alerts_state_time ON alerts(state, last_seen_at DESC);
CREATE INDEX alerts_identity ON alerts(identity);
INSERT INTO schema_version(version) VALUES (2);
";

const V3: &str = r"
CREATE TABLE body_contents (
    content_hash TEXT PRIMARY KEY NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    inline_bytes BLOB,
    file_path TEXT,
    CHECK ((inline_bytes IS NULL) != (file_path IS NULL))
) STRICT;
INSERT INTO body_contents(content_hash, byte_length, inline_bytes, file_path)
    SELECT content_hash, byte_length, inline_bytes, file_path FROM message_bodies
    WHERE rowid IN (SELECT MIN(rowid) FROM message_bodies GROUP BY content_hash);
CREATE TABLE message_bodies_v3 (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    delivery_id TEXT REFERENCES deliveries(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('original', 'transformed')),
    content_hash TEXT NOT NULL REFERENCES body_contents(content_hash),
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    CHECK ((kind = 'original' AND delivery_id IS NULL)
        OR (kind = 'transformed' AND delivery_id IS NOT NULL))
) STRICT;
INSERT INTO message_bodies_v3(message_id, delivery_id, kind, content_hash, byte_length)
    SELECT message_id, delivery_id, kind, content_hash, byte_length FROM message_bodies;
DROP TABLE message_bodies;
ALTER TABLE message_bodies_v3 RENAME TO message_bodies;
CREATE UNIQUE INDEX message_bodies_original ON message_bodies(message_id) WHERE kind = 'original';
CREATE UNIQUE INDEX message_bodies_transformed ON message_bodies(delivery_id) WHERE kind = 'transformed';
CREATE INDEX message_bodies_hash ON message_bodies(content_hash);
CREATE INDEX message_bodies_delivery ON message_bodies(delivery_id) WHERE delivery_id IS NOT NULL;
CREATE INDEX body_contents_file ON body_contents(file_path) WHERE file_path IS NOT NULL;
INSERT INTO schema_version(version) VALUES (3);
";

/// Messages-grid indexes in the grid's default order (received time, then receipt
/// sequence, then id), so a filtered page reads 100 index entries instead of sorting every
/// matching row (acceptance D-11).
const V4: &str = r"
DROP INDEX messages_channel_time;
DROP INDEX messages_time;
DROP INDEX messages_state_time;
DROP INDEX messages_channel_state_time;
CREATE INDEX messages_time ON messages(received_at DESC, seq DESC, id DESC);
CREATE INDEX messages_channel_time ON messages(channel_id, received_at DESC, seq DESC, id DESC);
CREATE INDEX messages_state_time ON messages(state, received_at DESC, seq DESC, id DESC);
CREATE INDEX messages_channel_state_time
    ON messages(channel_id, state, received_at DESC, seq DESC, id DESC);
INSERT INTO schema_version(version) VALUES (4);
";

/// Destination gauges kept up to date on write instead of scanning every delivery and
/// attempt of a destination for each snapshot: last attempt start, last acknowledgement,
/// last failure. Unresolved delivery states are counted through one index.
const V5: &str = r"
ALTER TABLE channel_counters ADD COLUMN last_attempt_at TEXT;
ALTER TABLE channel_counters ADD COLUMN last_ack_at TEXT;
ALTER TABLE channel_counters ADD COLUMN last_error TEXT;
ALTER TABLE channel_counters ADD COLUMN last_error_at TEXT;
CREATE INDEX deliveries_channel_destination_state ON deliveries(channel_id, destination_id, state);
INSERT INTO schema_version(version) VALUES (5);
";

/// Fills the schema-5 gauges from the history an upgraded store already holds.
const V5_BACKFILL: &str = r"
INSERT OR IGNORE INTO channel_counters(channel_id, destination_id)
    SELECT DISTINCT channel_id, destination_id FROM deliveries;
UPDATE channel_counters SET
    last_attempt_at = (SELECT MAX(a.started_at) FROM delivery_attempts a
        JOIN deliveries d ON d.id = a.delivery_id
        WHERE d.channel_id = channel_counters.channel_id
          AND d.destination_id = channel_counters.destination_id),
    last_ack_at = (SELECT MAX(a.ended_at) FROM delivery_attempts a
        JOIN deliveries d ON d.id = a.delivery_id
        WHERE d.channel_id = channel_counters.channel_id
          AND d.destination_id = channel_counters.destination_id
          AND a.result = 'acknowledged')
    WHERE destination_id <> '';
UPDATE channel_counters SET (last_error, last_error_at) = (
    SELECT summary, occurred_at FROM (
        SELECT a.error_summary AS summary, a.ended_at AS occurred_at, a.id AS id
        FROM delivery_attempts a JOIN deliveries d ON d.id = a.delivery_id
        WHERE d.channel_id = channel_counters.channel_id
          AND d.destination_id = channel_counters.destination_id
          AND a.error_summary IS NOT NULL AND a.ended_at IS NOT NULL
        UNION ALL
        SELECT d.last_error, d.created_at, d.id FROM deliveries d
        WHERE d.channel_id = channel_counters.channel_id
          AND d.destination_id = channel_counters.destination_id
          AND d.last_error IS NOT NULL AND d.total_attempts = 0)
    ORDER BY occurred_at DESC, id DESC LIMIT 1)
    WHERE destination_id <> '';
";

/// Each delivery carries its message's receipt sequence, so the head of a destination's
/// queue is one index entry instead of a sort of every queued delivery.
const V6: &str = r"
ALTER TABLE deliveries ADD COLUMN seq INTEGER;
UPDATE deliveries SET seq = (SELECT m.seq FROM messages m WHERE m.id = deliveries.message_id);
CREATE INDEX deliveries_queue_head ON deliveries(channel_id, destination_id, state, seq);
INSERT INTO schema_version(version) VALUES (6);
";

/// Indexes for the commit path, whose lookups otherwise read every message or body of a
/// channel: the latest message of a source identity (revision check, any content) and a
/// message's bodies by message (original or transformed).
const V7: &str = r"
CREATE INDEX messages_source_identity_seq ON messages(channel_id, source_identity, seq DESC)
    WHERE source_identity IS NOT NULL;
CREATE INDEX message_bodies_message ON message_bodies(message_id, delivery_id);
INSERT INTO schema_version(version) VALUES (7);
";

/// The operator's resend reason: on a delivery for its current attempt cycle, and on the
/// resend targets of a new message until its pipeline creates the deliveries.
const V8: &str = r"
ALTER TABLE deliveries ADD COLUMN resend_reason TEXT;
ALTER TABLE resend_targets ADD COLUMN reason TEXT;
INSERT INTO schema_version(version) VALUES (8);
";

/// The acknowledgement message's own control id (HL7 MSH-10 of the ACK), for the
/// transmission verification report.
const V9: &str = r"
ALTER TABLE delivery_attempts ADD COLUMN ack_control_id TEXT;
INSERT INTO schema_version(version) VALUES (9);
";

/// Two delivery indexes no query uses: `deliveries_queue_head` (channel, destination,
/// state, seq) serves every per-destination lookup, of which the first is an exact
/// prefix, and cancellation goes by channel. Each cost a write on every delivery insert
/// and state change.
const V10: &str = r"
DROP INDEX IF EXISTS deliveries_channel_destination_state;
DROP INDEX IF EXISTS deliveries_destination_queue;
INSERT INTO schema_version(version) VALUES (10);
";

pub(crate) fn migrate(connection: &mut Connection) -> crate::Result<()> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY) STRICT;",
    )?;
    let version: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?;
    if !(0..=10).contains(&version) {
        return Err(crate::Error::Invalid(format!(
            "unsupported store schema version {version}"
        )));
    }
    if version < 1 {
        transaction.execute_batch(V1)?;
    }
    if version < 2 {
        transaction.execute_batch(V2)?;
    }
    if version < 3 {
        transaction.execute_batch(V3)?;
    }
    if version < 4 {
        transaction.execute_batch(V4)?;
    }
    if version < 5 {
        transaction.execute_batch(V5)?;
        transaction.execute_batch(V5_BACKFILL)?;
    }
    if version < 6 {
        transaction.execute_batch(V6)?;
    }
    if version < 7 {
        transaction.execute_batch(V7)?;
    }
    if version < 8 {
        transaction.execute_batch(V8)?;
    }
    if version < 9 {
        transaction.execute_batch(V9)?;
    }
    if version < 10 {
        transaction.execute_batch(V10)?;
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_v1_and_is_idempotent() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(V1).unwrap();
        migrate(&mut connection).unwrap();
        migrate(&mut connection).unwrap();
        let versions: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(versions, 10);
        let dropped: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name IN
                 ('deliveries_channel_destination_state', 'deliveries_destination_queue')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(dropped, 0);
        let strict_tables: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_list WHERE schema = 'main' AND strict = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(strict_tables, 11);
    }

    #[test]
    fn refuses_future_schema_without_altering_it() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_version(version INTEGER PRIMARY KEY) STRICT;
                 INSERT INTO schema_version VALUES(11);",
            )
            .unwrap();
        assert!(migrate(&mut connection).is_err());
        assert_eq!(
            connection
                .query_row("SELECT MAX(version) FROM schema_version", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            11
        );
    }

    #[test]
    fn v2_upgrade_deduplicates_contents_without_losing_references() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        connection.execute_batch(V1).unwrap();
        connection.execute_batch(V2).unwrap();
        connection
            .execute_batch(
                "INSERT INTO messages(id,channel_id,channel_label,seq,received_at,direction,
                    origin,protocol,state,byte_length,content_hash,config_version)
                 VALUES ('first','channel','Test',1,'2026-01-01','inbound','received','hl7_mllp',
                    'received',3,'same_hash',1),
                    ('second','channel','Test',2,'2026-01-01','inbound','received','hl7_mllp',
                    'received',3,'same_hash',1);
                 INSERT INTO message_bodies(message_id,kind,content_hash,byte_length,inline_bytes)
                    VALUES ('first','original','same_hash',3,X'616263');
                 INSERT INTO message_bodies(message_id,kind,content_hash,byte_length,file_path)
                    VALUES ('second','original','same_hash',3,'sa/same_hash');",
            )
            .unwrap();
        migrate(&mut connection).unwrap();
        let contents: i64 = connection
            .query_row("SELECT COUNT(*) FROM body_contents", [], |row| row.get(0))
            .unwrap();
        assert_eq!(contents, 1);
        let references: i64 = connection
            .query_row("SELECT COUNT(*) FROM message_bodies", [], |row| row.get(0))
            .unwrap();
        assert_eq!(references, 2);
        let bytes: Vec<u8> = connection
            .query_row("SELECT inline_bytes FROM body_contents", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(bytes, b"abc");
        let violations: i64 = connection
            .query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(violations, 0);
    }

    #[test]
    fn the_schema_5_backfill_derives_destination_gauges_from_history() {
        let mut connection = Connection::open_in_memory().unwrap();
        migrate(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO messages(id,channel_id,channel_label,seq,received_at,direction,origin,
                    protocol,state,byte_length,content_hash,source_endpoint_id,config_version)
                 VALUES('m1','c','C',1,'2026-01-01T00:00:00.000000000Z','inbound','received',
                    'hl7_mllp','routed',1,'h','e',1);
                 INSERT INTO deliveries(id,message_id,channel_id,destination_id,destination_label,
                    state,ack_level,attempts,total_attempts,created_at,updated_at,max_attempts,
                    retry_unacknowledged,ack_required)
                 VALUES('d1','m1','c','lis','LIS','acknowledged','accepted',2,2,
                    '2026-01-01T00:00:01.000000000Z','2026-01-01T00:00:09.000000000Z',3,0,'accepted');
                 INSERT INTO delivery_attempts(id,delivery_id,attempt_no,started_at,ended_at,result,
                    error_summary,ack_level)
                 VALUES('a1','d1',1,'2026-01-01T00:00:02.000000000Z','2026-01-01T00:00:03.000000000Z',
                    'write_failed','connection refused','none'),
                    ('a2','d1',2,'2026-01-01T00:00:05.000000000Z','2026-01-01T00:00:06.000000000Z',
                    'acknowledged',NULL,'accepted');",
            )
            .unwrap();
        connection.execute_batch(V5_BACKFILL).unwrap();
        let gauges: (String, String, String) = connection
            .query_row(
                "SELECT last_attempt_at,last_ack_at,last_error FROM channel_counters
                 WHERE channel_id='c' AND destination_id='lis'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            gauges,
            (
                "2026-01-01T00:00:05.000000000Z".to_owned(),
                "2026-01-01T00:00:06.000000000Z".to_owned(),
                "connection refused".to_owned()
            )
        );
    }
}
