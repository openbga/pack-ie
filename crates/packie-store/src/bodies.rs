//! Original and frozen transformed bodies, inline or durably spilled by hash.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

use crate::Cached;
use packie_core::{DeliveryId, MessageId};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

/// Most unreferenced contents one cleanup transaction deletes.
const SWEEP_BATCH: usize = 500;

/// Deletes `hash`'s content when no body references it any more. A spilled file stays
/// until the cleanup after the commit: a rollback brings the row back.
pub(crate) fn release(transaction: &Transaction<'_>, hash: &str) -> crate::Result<()> {
    transaction.execute_cached(
        "DELETE FROM body_contents WHERE content_hash = ?1
         AND NOT EXISTS (SELECT 1 FROM message_bodies WHERE content_hash = ?1)",
        [hash],
    )?;
    Ok(())
}

#[derive(Clone)]
pub(crate) struct BodyStorage {
    root: PathBuf,
    inline_cap: usize,
    access: Arc<RwLock<()>>,
}

impl BodyStorage {
    pub(crate) fn new(root: PathBuf, inline_cap: usize) -> crate::Result<Self> {
        crate::private_dir(&root)?;
        sync_directory(&root)?;
        if let Some(parent) = root
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            sync_directory(parent)?;
        }
        Ok(Self {
            root,
            inline_cap,
            access: Arc::new(RwLock::new(())),
        })
    }

    /// Returns only after a spilled body and its directory entry are durable. A rolled
    /// back transaction may leave an orphan file, collected at startup or retention.
    pub(crate) fn put(
        &self,
        transaction: &Transaction<'_>,
        message_id: MessageId,
        delivery_id: Option<DeliveryId>,
        bytes: &[u8],
    ) -> crate::Result<String> {
        let hash = content_hash(bytes);
        let length = i64::try_from(bytes.len())
            .map_err(|_| crate::Error::Invalid("body length exceeds store capacity".into()))?;
        let exists: bool = transaction.query_row_cached(
            "SELECT EXISTS(SELECT 1 FROM body_contents WHERE content_hash = ?1)",
            [&hash],
            |row| row.get(0),
        )?;
        if !exists {
            let (inline, file_path) = if bytes.len() <= self.inline_cap {
                (Some(bytes), None)
            } else {
                self.write_file(&hash, bytes)?;
                (None, Some(relative_path(&hash)))
            };
            transaction.execute_cached(
                "INSERT INTO body_contents(content_hash, byte_length, inline_bytes, file_path)
                 VALUES (?1, ?2, ?3, ?4) ON CONFLICT(content_hash) DO NOTHING",
                params![hash, length, inline, file_path],
            )?;
        }
        let kind = if delivery_id.is_some() {
            "transformed"
        } else {
            "original"
        };
        // A replaced content (each frame of a transfer replaces the one before) is
        // released in the caller's transaction; its spill file, if any, stays until
        // cleanup checks all committed body references.
        let replaced: Option<String> = transaction
            .query_row_cached(
                "SELECT content_hash FROM message_bodies WHERE message_id = ?1 AND delivery_id IS ?2",
                params![message_id, delivery_id],
                |row| row.get(0),
            )
            .optional()?;
        transaction.execute_cached(
            "DELETE FROM message_bodies WHERE message_id = ?1 AND delivery_id IS ?2",
            params![message_id, delivery_id],
        )?;
        transaction.execute_cached(
            "INSERT INTO message_bodies
                (message_id, delivery_id, kind, content_hash, byte_length)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![message_id, delivery_id, kind, hash, length],
        )?;
        if let Some(replaced) = replaced.filter(|replaced| *replaced != hash) {
            release(transaction, &replaced)?;
        }
        Ok(hash)
    }

    /// Where spilled body files live.
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Holds file collection off (it waits for the guard), so files a snapshot references
    /// stay on disk while they are copied. Take it after the reader connection, as `get`
    /// does.
    pub(crate) fn hold_files(&self) -> std::sync::RwLockReadGuard<'_, ()> {
        self.access.read().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn get(
        &self,
        connection: &Connection,
        message_id: MessageId,
        delivery_id: Option<DeliveryId>,
    ) -> crate::Result<Vec<u8>> {
        let _guard = self.access.read().unwrap_or_else(PoisonError::into_inner);
        let stored = connection
            .query_row_cached(
                "SELECT c.inline_bytes, c.file_path, c.content_hash, c.byte_length
                 FROM message_bodies b JOIN body_contents c ON c.content_hash = b.content_hash
                 WHERE b.message_id = ?1 AND b.delivery_id IS ?2",
                params![message_id, delivery_id],
                |row| {
                    Ok((
                        row.get::<_, Option<Vec<u8>>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or(crate::Error::NotFound)?;
        let (inline, path, hash, length) = stored;
        let bytes = match (inline, path) {
            (Some(bytes), None) => bytes,
            (None, Some(path)) if valid_hash(&hash) && path == relative_path(&hash) => {
                fs::read(self.root.join(path))?
            }
            _ => {
                return Err(crate::Error::Invalid(
                    "invalid body storage metadata".into(),
                ));
            }
        };
        if i64::try_from(bytes.len()).ok() != Some(length) || content_hash(&bytes) != hash {
            return Err(crate::Error::Invalid(
                "stored body integrity check failed".into(),
            ));
        }
        Ok(bytes)
    }

    /// Called by the single writer, after retention commits and before accepting new
    /// work at startup. Shared hash files survive until their last reference is gone.
    pub(crate) fn cleanup_unreferenced(&self, connection: &Connection) -> crate::Result<u64> {
        // Never unlink bytes referenced by a transaction that could still roll back.
        // Retention invokes this after committing its deleted message references.
        if !connection.is_autocommit() {
            return Ok(0);
        }
        let _guard = self.access.write().unwrap_or_else(PoisonError::into_inner);
        // Contents are released as their bodies go; this sweep catches what an older
        // version or an interrupted run left, in bounded transactions: a transaction
        // holds every page it changes in memory until it commits.
        let mut after = 0_i64;
        loop {
            let transaction =
                Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
            let rows: Vec<i64> = {
                let mut statement = transaction.prepare_cached(
                    "SELECT rowid FROM body_contents c WHERE rowid > ?1 AND NOT EXISTS
                        (SELECT 1 FROM message_bodies b WHERE b.content_hash = c.content_hash)
                     ORDER BY rowid LIMIT ?2",
                )?;
                statement
                    .query_map(params![after, SWEEP_BATCH], |row| row.get(0))?
                    .collect::<Result<_, _>>()?
            };
            for row in &rows {
                transaction.execute_cached("DELETE FROM body_contents WHERE rowid = ?1", [row])?;
            }
            transaction.commit()?;
            match rows.last() {
                Some(&last) if rows.len() == SWEEP_BATCH => after = last,
                _ => break,
            }
        }
        let mut statement = connection
            .prepare("SELECT DISTINCT file_path FROM body_contents WHERE file_path IS NOT NULL")?;
        let referenced = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<HashSet<_>, _>>()?;
        let mut removed = 0;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if name.len() == 2 && name.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                removed += cleanup_directory(&entry.path(), name, &referenced)?;
            }
        }
        if removed > 0 {
            sync_directory(&self.root)?;
        }
        Ok(removed)
    }

    fn write_file(&self, hash: &str, bytes: &[u8]) -> crate::Result<()> {
        let directory = self.root.join(&hash[..2]);
        crate::private_dir(&directory)?;
        sync_directory(&self.root)?;
        let path = directory.join(hash);
        match File::open(&path) {
            Ok(file) => {
                // Verify reused files instead of propagating a damaged shared body.
                if fs::read(&path)? != bytes {
                    return Err(crate::Error::Invalid(
                        "stored body integrity check failed".into(),
                    ));
                }
                private_file_permissions(&path)?;
                file.sync_all()?;
                sync_directory(&directory)?;
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let temporary = directory.join(format!(".{hash}.{}.tmp", MessageId::generate()));
        let result = (|| -> crate::Result<()> {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, &path)?;
            sync_directory(&directory)?;
            Ok(())
        })();
        if result.is_err() {
            // Keep the original I/O error; startup cleanup also handles interrupted writes.
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

/// SHA-256 of `bytes`, lowercase hex: the content hash of message bodies and files.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn relative_path(hash: &str) -> String {
    format!("{}/{hash}", &hash[..2])
}

fn cleanup_directory(
    directory: &Path,
    prefix: &str,
    referenced: &HashSet<String>,
) -> crate::Result<u64> {
    let mut removed = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let owned_file = valid_hash(name) || (name.starts_with('.') && name.ends_with(".tmp"));
        if owned_file && !referenced.contains(&format!("{prefix}/{name}")) {
            fs::remove_file(entry.path())?;
            removed += 1;
        }
    }
    if removed > 0 {
        sync_directory(directory)?;
    }
    Ok(removed)
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn private_file_permissions(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    // Windows does not expose directory syncing through std::fs. The body file is
    // always synced before rename; Unix also persists the directory entry explicitly.
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(connection: &Connection) -> MessageId {
        let id = MessageId::generate();
        connection
            .execute(
                "INSERT INTO messages
                    (id, channel_id, channel_label, seq, received_at, direction, origin,
                     protocol, state, byte_length, content_hash, config_version)
                 VALUES (?1, ?1, 'Test', 1, '2026-01-01T00:00:00Z', 'inbound', 'received',
                     'hl7_mllp', 'received', 0, '', 1)",
                [id],
            )
            .unwrap();
        id
    }

    #[test]
    fn stores_inline_and_shared_spills_and_cleans_after_last_reference() {
        let directory = tempfile::tempdir().unwrap();
        let storage = BodyStorage::new(directory.path().join("bodies"), 3).unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .pragma_update(None, "foreign_keys", true)
            .unwrap();
        crate::schema::migrate(&mut connection).unwrap();
        let first = message(&connection);
        let second = message(&connection);
        let inline = message(&connection);
        let inline_duplicate = message(&connection);
        let transaction = connection.transaction().unwrap();
        let hash = storage
            .put(&transaction, first, None, b"shared body")
            .unwrap();
        storage
            .put(&transaction, second, None, b"shared body")
            .unwrap();
        storage.put(&transaction, inline, None, b"abc").unwrap();
        storage
            .put(&transaction, inline_duplicate, None, b"abc")
            .unwrap();
        transaction.commit().unwrap();
        let contents: i64 = connection
            .query_row("SELECT COUNT(*) FROM body_contents", [], |row| row.get(0))
            .unwrap();
        assert_eq!(contents, 2);
        assert_eq!(
            storage.get(&connection, first, None).unwrap(),
            b"shared body"
        );
        assert_eq!(storage.get(&connection, inline, None).unwrap(), b"abc");
        connection
            .execute("DELETE FROM messages WHERE id = ?1", [first])
            .unwrap();
        assert_eq!(storage.cleanup_unreferenced(&connection).unwrap(), 0);
        assert!(storage.root.join(relative_path(&hash)).exists());
        connection
            .execute("DELETE FROM messages WHERE id = ?1", [second])
            .unwrap();
        assert_eq!(storage.cleanup_unreferenced(&connection).unwrap(), 1);
        assert!(!storage.root.join(relative_path(&hash)).exists());
    }

    #[test]
    fn rolled_back_body_is_reclaimed_and_corruption_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let storage = BodyStorage::new(directory.path().join("bodies"), 0).unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut connection).unwrap();
        let id = message(&connection);
        let transaction = connection.transaction().unwrap();
        storage.put(&transaction, id, None, b"rolled back").unwrap();
        transaction.rollback().unwrap();
        assert!(matches!(
            storage.get(&connection, id, None),
            Err(crate::Error::NotFound)
        ));
        assert_eq!(storage.cleanup_unreferenced(&connection).unwrap(), 1);
        let transaction = connection.transaction().unwrap();
        let hash = storage.put(&transaction, id, None, b"durable").unwrap();
        transaction.commit().unwrap();
        fs::write(storage.root.join(relative_path(&hash)), b"damaged").unwrap();
        assert!(matches!(
            storage.get(&connection, id, None),
            Err(crate::Error::Invalid(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn permissions_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let storage = BodyStorage::new(directory.path().join("bodies"), 0).unwrap();
        storage.write_file(&content_hash(b"body"), b"body").unwrap();
        assert_eq!(
            fs::metadata(&storage.root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let path = storage.root.join(relative_path(&content_hash(b"body")));
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    fn contents(connection: &Connection) -> i64 {
        connection
            .query_row("SELECT COUNT(*) FROM body_contents", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn a_replaced_body_releases_its_content_unless_another_body_shares_it() {
        let directory = tempfile::tempdir().unwrap();
        let storage = BodyStorage::new(directory.path().join("bodies"), 1024).unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut connection).unwrap();
        let (growing, sharing) = (message(&connection), message(&connection));
        let transaction = connection.transaction().unwrap();
        storage.put(&transaction, growing, None, b"H|").unwrap();
        storage.put(&transaction, growing, None, b"H|P|").unwrap();
        storage.put(&transaction, sharing, None, b"H|P|").unwrap();
        storage.put(&transaction, growing, None, b"H|P|L|").unwrap();
        transaction.commit().unwrap();
        // "H|" went with its replacement; "H|P|" stays for the other message.
        assert_eq!(contents(&connection), 2);
        assert_eq!(storage.get(&connection, growing, None).unwrap(), b"H|P|L|");
        assert_eq!(storage.get(&connection, sharing, None).unwrap(), b"H|P|");
    }

    #[test]
    fn the_cleanup_sweeps_more_orphans_than_one_transaction_takes() {
        let directory = tempfile::tempdir().unwrap();
        let storage = BodyStorage::new(directory.path().join("bodies"), 1024).unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        crate::schema::migrate(&mut connection).unwrap();
        let kept = message(&connection);
        let transaction = connection.transaction().unwrap();
        storage.put(&transaction, kept, None, b"kept").unwrap();
        for n in 0..SWEEP_BATCH * 2 + 7 {
            transaction
                .execute(
                    "INSERT INTO body_contents(content_hash, byte_length, inline_bytes)
                     VALUES (?1, 1, x'00')",
                    [format!("orphan-{n}")],
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        storage.cleanup_unreferenced(&connection).unwrap();
        assert_eq!(contents(&connection), 1);
        assert_eq!(storage.get(&connection, kept, None).unwrap(), b"kept");
    }
}
