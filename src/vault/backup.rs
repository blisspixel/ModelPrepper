//! Standalone, private SQLite catalog snapshots.
use super::{check_path, db, identities, io, open_catalog, sync_directory};
use crate::{Config, Error, Result, config::paths_overlap, filesystem, integrity::hash_exact};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct CatalogBackup {
    pub schema_version: u32,
    pub vault_id: String,
    pub output: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Copy committed SQLite pages, including WAL state, under the normal catalog lock.
/// A catalog snapshot includes historical grants and private reviews.
pub fn backup_catalog(config: &Config, output: &Path) -> Result<CatalogBackup> {
    let catalog = open_catalog(config, false)?;
    let ownership = identities(&catalog, config)?;
    if ownership.state != "ready" {
        return Err(Error::new(
            "vault_not_ready",
            "finish vault initialization before backing up the catalog",
        ));
    }
    let requested = std::env::current_dir().map_err(io)?.join(output);
    let leaf = requested
        .file_name()
        .ok_or_else(|| Error::new("invalid_output", "choose a new backup file"))?;
    if ["-wal", "-journal", "-shm"].iter().any(|suffix| {
        leaf.to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(suffix)
    }) {
        return Err(Error::new(
            "invalid_output",
            "a SQLite sidecar cannot be a snapshot filename",
        ));
    }
    let directory = requested
        .parent()
        .ok_or_else(|| Error::new("invalid_output", "choose a backup directory"))?;
    check_path(directory)?;
    let directory = directory.canonicalize().map_err(io)?;
    let output = directory.join(leaf);
    if paths_overlap(&output, &config.catalog_path)
        || config
            .volumes
            .iter()
            .any(|v| paths_overlap(&output, &v.path))
    {
        return Err(Error::new(
            "overlapping_vault_paths",
            "place the snapshot outside the catalog and all managed volume trees",
        ));
    }
    require_unused(&output)?;
    filesystem::admit(&directory, filesystem::Purpose::Catalog)?;
    // SQLite may create sidecars. Keep all intermediate files together and private.
    let work = tempfile::Builder::new()
        .prefix(".modelprepper-backup-")
        .tempdir_in(&directory)
        .map_err(io)?;
    let temporary = tempfile::NamedTempFile::new_in(work.path()).map_err(io)?;
    let mut snapshot =
        Connection::open_with_flags(temporary.path(), OpenFlags::SQLITE_OPEN_READ_WRITE)
            .map_err(db)?;
    if Backup::new(&catalog.db, &mut snapshot)
        .map_err(db)?
        .step(-1)
        .map_err(db)?
        != StepResult::Done
    {
        return Err(Error::new(
            "backup_busy",
            "SQLite could not finish the snapshot; retry after other database activity completes",
        ));
    }
    drop(catalog);
    let mode: String = snapshot
        .query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))
        .map_err(db)?;
    let integrity: String = snapshot
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(db)?;
    if mode != "delete" || integrity != "ok" {
        return Err(Error::new(
            "invalid_backup",
            "SQLite could not confirm a standalone, intact catalog",
        ));
    }
    snapshot.close().map_err(|(_, e)| db(e))?;
    temporary.as_file().sync_all().map_err(io)?;
    let size_bytes = temporary.as_file().metadata().map_err(io)?.len();
    let hash = hash_exact(temporary.reopen().map_err(io)?, size_bytes, |_| {})?;
    require_unused(&output)?;
    temporary.persist_noclobber(&output).map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::new("output_exists", "choose a new backup file")
        } else {
            io(e.error)
        }
    })?;
    sync_directory(&directory)?;
    Ok(CatalogBackup {
        schema_version: 1,
        vault_id: ownership.vault_id,
        output,
        size_bytes,
        sha256: hash.sha256,
    })
}

// A leftover SQLite sidecar could affect a subsequently opened snapshot.
fn require_unused(path: &Path) -> Result<()> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match fs::symlink_metadata(&name) {
            Ok(_) => {
                return Err(Error::new(
                    "output_exists",
                    "choose a new backup filename with no SQLite sidecars",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io(e)),
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/backup.rs"]
mod tests;
