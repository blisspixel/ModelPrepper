//! Experimental durable single-file staging. No approval or vault publication.
//!
//! `spent` is the sum of segment allowances charged before each request. It is
//! not reduced when recovery discards a tail, and it is not a bandwidth
//! measurement or a vault reservation.
use super::{Checkpoint, Segment, disk, file_identity, recover_stage, stage_with};
use crate::{
    Error, Result,
    http::Http,
    inventory::{Source, SourceFile},
    plan::digest,
    validation::validate_hex,
    vault::check_path,
    writer_lock::WriterLock,
};
use rusqlite::{Connection, OpenFlags, params};
use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

const APPLICATION_ID: u32 = 0x4d50534a;
/// One checkpoint database or its rollback journal. This is a disk cap, not
/// process memory, a TLS buffer, or a measured transfer rate.
const SIDE_FILE_CAP: u64 = 1024 * 1024;

/// Extra disk the single-file session may use besides `payload.part`: the
/// checkpoint database and its rollback journal. Preview `workspace_bytes`
/// stays unset until a bound also covers every selected file.
pub fn staging_workspace_bound() -> u64 {
    SIDE_FILE_CAP * 2
}
const ENTRIES: &[&str] = &[
    "writer.lock",
    "payload.part",
    "checkpoint.sqlite3",
    "checkpoint.sqlite3-journal",
];

fn journal(error: rusqlite::Error) -> Error {
    Error::new("checkpoint_journal_failed", error.to_string())
}

fn invalid() -> Error {
    Error::new(
        "invalid_checkpoint_journal",
        "retain the staging directory; its committed checkpoint cannot be trusted",
    )
}

fn location(root: &Path) -> Result<()> {
    if !root.is_absolute()
        || root.to_str().is_none()
        || root
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|s| matches!(s, "." | ".."))
    {
        return Err(Error::new(
            "unsafe_staging_path",
            "use an absolute staging directory without dot segments",
        ));
    }
    #[cfg(windows)]
    if root.components().any(|c| {
        matches!(c, std::path::Component::Prefix(p)
        if matches!(p.kind(), std::path::Prefix::UNC(_, _) | std::path::Prefix::VerbatimUNC(_, _)))
    }) {
        return Err(Error::new(
            "unsupported_staging_filesystem",
            "the checkpoint SQLite journal requires a local filesystem",
        ));
    }
    check_path(root)?;
    for entry in ENTRIES {
        let path = root.join(entry);
        check_path(&path)?;
        if path.exists() && !fs::metadata(path).map_err(disk)?.is_file() {
            return Err(invalid());
        }
    }
    if root.exists() {
        for entry in fs::read_dir(root).map_err(disk)? {
            if !ENTRIES
                .iter()
                .any(|name| entry.as_ref().is_ok_and(|e| e.file_name() == *name))
            {
                return Err(Error::new(
                    "staging_directory_not_owned",
                    "staging never adopts unrelated entries",
                ));
            }
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path).map_err(disk)?.sync_all().map_err(disk)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Exclusive, persistent staging for one identity-bound file. Experimental:
/// this owns checkpoint ordering, not approval, reservations, budgets, or seals.
pub struct StagingSession {
    // Release SQLite and payload handles before releasing writer ownership.
    db: Connection,
    file: File,
    _writer: WriterLock,
    source: Source,
    expected: SourceFile,
    identity: String,
    recovered_bytes: u64,
}

impl StagingSession {
    /// Exclusively create a new directory under an existing, trusted parent.
    /// Interrupted initialization is retained for inspection, never adopted.
    pub fn create(root: &Path, source: &Source, expected: &SourceFile) -> Result<Self> {
        let (_, identity) = file_identity(source, expected)?;
        location(root)?;
        fs::create_dir(root).map_err(disk)?;
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.join("writer.lock"))
            .map_err(disk)?;
        let writer = WriterLock::acquire(writer)
            .map_err(|_| Error::new("writer_busy", "another process owns this staging session"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.join("payload.part"))
            .map_err(disk)?;
        file.sync_all().map_err(disk)?;
        let db = Connection::open_with_flags(
            root.join("checkpoint.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(journal)?;
        configure(&db)?;
        db.execute_batch(
            "BEGIN IMMEDIATE;
            CREATE TABLE checkpoint (id INTEGER PRIMARY KEY CHECK(id=1), identity TEXT NOT NULL,
                bytes TEXT NOT NULL, prefix_sha256 TEXT NOT NULL, spent TEXT NOT NULL);",
        )
        .map_err(journal)?;
        db.execute(
            "INSERT INTO checkpoint VALUES (1, ?1, '0', ?2, '0')",
            params![identity, digest(b"")],
        )
        .map_err(journal)?;
        db.pragma_update(None, "application_id", APPLICATION_ID)
            .map_err(journal)?;
        db.pragma_update(None, "user_version", 1).map_err(journal)?;
        db.execute_batch("COMMIT;").map_err(journal)?;
        sync_directory(root)?;
        sync_directory(root.parent().ok_or_else(invalid)?)?;
        Ok(Self {
            db,
            file,
            _writer: writer,
            source: source.clone(),
            expected: expected.clone(),
            identity,
            recovered_bytes: 0,
        })
    }

    /// Reopen an existing journal, acquire ownership, and reconcile uncommitted
    /// bytes before any network request. Missing or foreign journals fail closed.
    pub fn open(root: &Path, source: &Source, expected: &SourceFile) -> Result<Self> {
        let (_, identity) = file_identity(source, expected)?;
        location(root)?;
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("writer.lock"))
            .map_err(disk)?;
        let writer = WriterLock::acquire(writer)
            .map_err(|_| Error::new("writer_busy", "another process owns this staging session"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("payload.part"))
            .map_err(disk)?;
        side_file_within_cap(root, "checkpoint.sqlite3")?;
        side_file_within_cap(root, "checkpoint.sqlite3-journal")?;
        let db = Connection::open_with_flags(
            root.join("checkpoint.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(journal)?;
        let app: u32 = db
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(journal)?;
        let version: u32 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(journal)?;
        if app != APPLICATION_ID || version != 1 {
            return Err(invalid());
        }
        configure(&db)?;
        let mut session = Self {
            db,
            file,
            _writer: writer,
            source: source.clone(),
            expected: expected.clone(),
            identity,
            recovered_bytes: 0,
        };
        // Reject a corrupt charge before recovery can truncate the payload.
        session.spent_bytes()?;
        let checkpoint = session.checkpoint()?;
        session.recovered_bytes = recover_stage(source, expected, &mut session.file, &checkpoint)?;
        Ok(session)
    }

    /// Last committed journal state. This is not a seal or final verification.
    pub fn checkpoint(&self) -> Result<Checkpoint> {
        let count: i64 = self
            .db
            .query_row("SELECT count(*) FROM checkpoint", [], |r| r.get(0))
            .map_err(journal)?;
        if count != 1 {
            return Err(invalid());
        }
        let row: (Option<String>, Option<String>, Option<String>) = self.db.query_row(
            "SELECT CASE WHEN length(identity)=64 THEN identity END,
                CASE WHEN length(bytes)<=20 THEN bytes END,
                CASE WHEN length(prefix_sha256)=64 THEN prefix_sha256 END FROM checkpoint WHERE id=1",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(journal)?;
        let identity = row.0.ok_or_else(invalid)?;
        let bytes: u64 = row.1.ok_or_else(invalid)?.parse().map_err(|_| invalid())?;
        let prefix_sha256 = row.2.ok_or_else(invalid)?;
        validate_hex(&prefix_sha256, 64).map_err(|_| invalid())?;
        if identity != self.identity || bytes > self.expected.size_bytes {
            return Err(invalid());
        }
        Ok(Checkpoint {
            schema_version: 1,
            identity,
            bytes,
            prefix_sha256,
        })
    }

    /// Bytes discarded during the most recent open or pre-segment recovery.
    /// Discarding bytes must never refund a future job's network spending.
    pub fn recovered_bytes(&self) -> u64 {
        self.recovered_bytes
    }

    /// Allowances charged before requests. Recovery and a failed body do not
    /// reduce it. A crash after this charge and before the checkpoint update
    /// still owes the allowance.
    pub fn spent_bytes(&self) -> Result<u64> {
        let text: Option<String> = self
            .db
            .query_row(
                "SELECT CASE WHEN length(spent)<=20 THEN spent END FROM checkpoint WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(journal)?;
        let text = text.ok_or_else(invalid)?;
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(invalid());
        }
        text.parse().map_err(|_| invalid())
    }

    /// Stage and sync a bounded segment, then commit its checkpoint atomically.
    /// The allowance is journaled before the request. Journal failure after the
    /// payload sync reports failure even if those bytes have reached disk.
    pub fn stage(&mut self, max_new_bytes: u64) -> Result<Segment> {
        self.stage_with(&Http::default(), "https://huggingface.co", max_new_bytes)
    }

    fn reserve(&mut self, amount: u64) -> Result<()> {
        if amount == 0 {
            return Ok(());
        }
        let next = self.spent_bytes()?.checked_add(amount).ok_or_else(|| {
            Error::new(
                "size_overflow",
                "staged transfer spending exceeds u64 capacity",
            )
        })?;
        let tx = self.db.transaction().map_err(journal)?;
        let changed = tx
            .execute(
                "UPDATE checkpoint SET spent=?1 WHERE id=1 AND identity=?2 AND length(spent)<=20",
                params![next.to_string(), self.identity],
            )
            .map_err(journal)?;
        if changed != 1 {
            return Err(invalid());
        }
        tx.commit().map_err(journal)
    }

    fn stage_with(&mut self, http: &Http, base: &str, allowance: u64) -> Result<Segment> {
        if allowance == 0 {
            return Err(Error::new(
                "invalid_limit",
                "segment allowance must be positive",
            ));
        }
        // Validate the charge before recovery so a bad journal cannot truncate.
        self.spent_bytes()?;
        let checkpoint = self.checkpoint()?;
        self.recovered_bytes =
            recover_stage(&self.source, &self.expected, &mut self.file, &checkpoint)?;
        let remaining = self
            .expected
            .size_bytes
            .checked_sub(checkpoint.bytes)
            .ok_or_else(invalid)?;
        let count = remaining.min(allowance);
        if count > 0 {
            self.reserve(count)?;
        }
        let segment = stage_with(
            http,
            base,
            &self.source,
            &self.expected,
            &mut self.file,
            Some(&checkpoint),
            allowance,
        )?;
        self.commit(&segment.checkpoint)?;
        Ok(segment)
    }

    fn commit(&mut self, checkpoint: &Checkpoint) -> Result<()> {
        let tx = self.db.transaction().map_err(journal)?;
        let changed = tx
            .execute(
                "UPDATE checkpoint SET bytes=?1, prefix_sha256=?2 WHERE id=1 AND identity=?3",
                params![
                    checkpoint.bytes.to_string(),
                    checkpoint.prefix_sha256,
                    self.identity
                ],
            )
            .map_err(journal)?;
        if changed != 1 {
            return Err(invalid());
        }
        tx.commit().map_err(journal)
    }
}

fn side_file_within_cap(root: &Path, name: &str) -> Result<()> {
    let path = root.join(name);
    if path.exists() && fs::metadata(&path).map_err(disk)?.len() > SIDE_FILE_CAP {
        return Err(invalid());
    }
    Ok(())
}

fn configure(db: &Connection) -> Result<()> {
    db.execute_batch(
        "PRAGMA trusted_schema=OFF; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;",
    )
    .map_err(journal)
}

#[cfg(test)]
#[path = "tests/session.rs"]
mod tests;
