//! Local catalog ownership and restartable volume initialization.
use crate::{Config, Error, Result};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const MARKER: &str = ".modelprepper-volume.json";
const PENDING: &str = ".modelprepper-volume.pending";

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Identity {
    schema_version: u32,
    vault_id: String,
    volume_id: String,
    label: String,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub schema_version: u32,
    pub state: String,
    pub vault_id: String,
    pub volumes: Vec<VolumeStatus>,
}

#[derive(Debug, Serialize)]
pub struct VolumeStatus {
    pub label: String,
    pub volume_id: String,
    pub path: PathBuf,
    pub state: &'static str,
    pub available_bytes: Option<u64>,
}

struct Catalog {
    db: Connection,
    _writer: File,
}

struct CatalogIdentity {
    vault_id: String,
    state: String,
    volumes: Vec<(Identity, PathBuf)>,
}

fn io(error: std::io::Error) -> Error {
    Error::new("vault_io_failed", error.to_string())
}
fn db(error: rusqlite::Error) -> Error {
    Error::new("catalog_failed", error.to_string())
}

/// Absolute paths are relative to the configuration file, never the caller's cwd.
pub fn locate(config: &Config, config_file: &Path) -> Result<Config> {
    // Verbatim Windows PathBuf joins normalize dots, so inspect input first.
    for path in std::iter::once(&config.catalog_path).chain(config.volumes.iter().map(|v| &v.path))
    {
        if path
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|part| matches!(part, "." | ".."))
        {
            return Err(Error::new(
                "unsafe_vault_path",
                "vault paths cannot contain dot segments",
            ));
        }
    }
    let config_file = fs::canonicalize(config_file).map_err(io)?;
    let base = config_file
        .parent()
        .ok_or_else(|| Error::new("invalid_config", "config has no parent directory"))?;
    let mut located = config.clone();
    if located.catalog_path.is_relative() {
        located.catalog_path = base.join(&located.catalog_path);
    }
    for volume in &mut located.volumes {
        if volume.path.is_relative() {
            volume.path = base.join(&volume.path);
        }
    }
    located.validate()?;
    validate_locations(&located)?;
    Ok(located)
}

fn validate_locations(config: &Config) -> Result<()> {
    #[cfg(windows)]
    if config.catalog_path.components().any(|component| matches!(component,
        std::path::Component::Prefix(prefix) if matches!(prefix.kind(), std::path::Prefix::UNC(_, _) | std::path::Prefix::VerbatimUNC(_, _)))) {
        return Err(Error::new("unsupported_catalog_filesystem", "UNC shares cannot hold the local SQLite catalog"));
    }
    let mut paths = vec![config.catalog_path.clone()];
    paths.extend(config.volumes.iter().map(|v| v.path.clone()));
    for path in &paths {
        if !path.is_absolute() || path.to_str().is_none() {
            return Err(Error::new(
                "unsafe_vault_path",
                "vault operations require absolute UTF-8 paths",
            ));
        }
        if path
            .to_str()
            .expect("checked UTF-8")
            .split(['/', '\\'])
            .any(|part| matches!(part, "." | ".."))
        {
            return Err(Error::new(
                "unsafe_vault_path",
                "vault paths cannot contain dot segments",
            ));
        }
        check_path(path)?;
    }
    for (index, path) in paths.iter().enumerate() {
        for other in &paths[index + 1..] {
            // Reject case aliases conservatively on every OS for portability.
            let left = PathBuf::from(path.to_string_lossy().to_lowercase());
            let right = PathBuf::from(other.to_string_lossy().to_lowercase());
            if left.starts_with(&right) || right.starts_with(&left) {
                return Err(Error::new(
                    "overlapping_vault_paths",
                    "catalog and volumes need separate, non-overlapping directories",
                ));
            }
        }
    }
    Ok(())
}

fn check_path(path: &Path) -> Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        if matches!(component, std::path::Component::Prefix(_)) {
            continue;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                let linked = metadata.file_type().is_symlink();
                #[cfg(windows)]
                let linked = {
                    use std::os::windows::fs::MetadataExt;
                    linked || metadata.file_attributes() & 0x400 != 0
                };
                if linked {
                    return Err(Error::new(
                        "unsafe_vault_path",
                        "symlinks and reparse points cannot own vault paths",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io(error)),
        }
    }
    Ok(())
}

fn only_entries(path: &Path, allowed: &[&str]) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(path).map_err(io)? {
        let entry = entry.map_err(io)?;
        if !allowed.iter().any(|name| entry.file_name() == *name) {
            return Err(Error::new(
                "directory_not_empty",
                "initialization never adopts unrelated files",
            ));
        }
    }
    Ok(())
}

fn open_catalog(config: &Config, create: bool) -> Result<Catalog> {
    config.validate()?;
    validate_locations(config)?;
    check_path(&config.catalog_path)?;
    let database = config.catalog_path.join("catalog.sqlite3");
    let new = !database.exists();
    if new && !create {
        return Err(Error::new(
            "vault_not_initialized",
            "initialize the vault before reading its catalog",
        ));
    }
    if new {
        only_entries(&config.catalog_path, &["writer.lock"])?;
        for volume in &config.volumes {
            only_entries(&volume.path, &[])?;
        }
        fs::create_dir_all(&config.catalog_path).map_err(io)?;
    }
    let lock = config.catalog_path.join("writer.lock");
    check_path(&lock)?;
    check_path(&database)?;
    for sidecar in [
        "catalog.sqlite3-wal",
        "catalog.sqlite3-shm",
        "catalog.sqlite3-journal",
    ] {
        check_path(&config.catalog_path.join(sidecar))?;
    }
    let writer = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(lock)
        .map_err(io)?;
    writer
        .try_lock()
        .map_err(|_| Error::new("writer_busy", "another process owns the vault writer lock"))?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | if new {
            OpenFlags::SQLITE_OPEN_CREATE
        } else {
            OpenFlags::empty()
        };
    let connection = Connection::open_with_flags(database, flags).map_err(db)?;
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db)?;
    if (!new && version != 1) || (new && version != 0) {
        return Err(Error::new(
            "unsupported_catalog_schema",
            "catalog schema is not supported",
        ));
    }
    connection
        .execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
        .map_err(db)?;
    if new {
        for volume in &config.volumes {
            check_path(&volume.path)?;
            only_entries(&volume.path, &[])?;
        }
        connection.execute_batch("BEGIN IMMEDIATE; CREATE TABLE vault (id TEXT PRIMARY KEY, state TEXT NOT NULL); CREATE TABLE volumes (label TEXT PRIMARY KEY, id TEXT NOT NULL UNIQUE, path TEXT NOT NULL UNIQUE); PRAGMA user_version=1;").map_err(db)?;
        connection
            .execute(
                "INSERT INTO vault VALUES (?1, 'initializing')",
                [Uuid::new_v4().to_string()],
            )
            .map_err(db)?;
        for volume in &config.volumes {
            let path = volume
                .path
                .to_str()
                .ok_or_else(|| Error::new("unsafe_vault_path", "vault paths must be UTF-8"))?;
            connection
                .execute(
                    "INSERT INTO volumes VALUES (?1, ?2, ?3)",
                    params![volume.label, Uuid::new_v4().to_string(), path],
                )
                .map_err(db)?;
        }
        connection.execute_batch("COMMIT;").map_err(db)?;
    }
    Ok(Catalog {
        db: connection,
        _writer: writer,
    })
}

fn identities(catalog: &Catalog, config: &Config) -> Result<CatalogIdentity> {
    let (vault_id, state): (String, String) = catalog
        .db
        .query_row("SELECT id, state FROM vault", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(db)?;
    Uuid::parse_str(&vault_id)
        .map_err(|_| Error::new("invalid_catalog", "invalid vault identity"))?;
    if !matches!(state.as_str(), "initializing" | "ready") {
        return Err(Error::new("invalid_catalog", "unknown vault state"));
    }
    let mut query = catalog
        .db
        .prepare("SELECT label, id, path FROM volumes ORDER BY label")
        .map_err(db)?;
    let rows = query
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(db)?;
    let mut result = Vec::new();
    for row in rows {
        let (label, volume_id, path) = row.map_err(db)?;
        Uuid::parse_str(&volume_id)
            .map_err(|_| Error::new("invalid_catalog", "invalid volume identity"))?;
        let path = PathBuf::from(path);
        if !config
            .volumes
            .iter()
            .any(|v| v.label == label && v.path == path)
        {
            return Err(Error::new(
                "volume_configuration_changed",
                "configured volume differs from catalog ownership",
            ));
        }
        result.push((
            Identity {
                schema_version: 1,
                vault_id: vault_id.clone(),
                volume_id,
                label,
            },
            path,
        ));
    }
    if result.len() != config.volumes.len() {
        return Err(Error::new(
            "volume_configuration_changed",
            "configured volumes differ from catalog ownership",
        ));
    }
    Ok(CatalogIdentity {
        vault_id,
        state,
        volumes: result,
    })
}

fn read_identity(path: &Path) -> Result<Identity> {
    check_path(path)?;
    serde_json::from_str(&crate::cli::read_text(path)?).map_err(|_| {
        Error::new(
            "invalid_volume_identity",
            "volume identity is invalid or incomplete",
        )
    })
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path).map_err(io)?.sync_all().map_err(io)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub fn initialize(config: &Config) -> Result<Status> {
    let catalog = open_catalog(config, true)?;
    let CatalogIdentity { state, volumes, .. } = identities(&catalog, config)?;
    if state == "initializing" {
        // Preflight every disk before adopting any of them.
        for (identity, path) in &volumes {
            check_path(path)?;
            only_entries(path, &[MARKER, PENDING])?;
            for name in [MARKER, PENDING] {
                if path.join(name).exists() && read_identity(&path.join(name))? != *identity {
                    return Err(Error::new(
                        "volume_identity_mismatch",
                        "volume belongs to another vault or disk",
                    ));
                }
            }
        }
        for (identity, path) in &volumes {
            fs::create_dir_all(path).map_err(io)?;
            let marker = path.join(MARKER);
            if marker.exists() && read_identity(&marker)? != *identity {
                return Err(Error::new(
                    "volume_identity_mismatch",
                    "volume identity changed during initialization",
                ));
            }
            if !marker.exists() {
                let pending = path.join(PENDING);
                if !pending.exists() {
                    let bytes = serde_json::to_vec(identity)
                        .map_err(|e| Error::new("output_failed", e.to_string()))?;
                    let mut file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&pending)
                        .map_err(io)?;
                    file.write_all(&bytes)
                        .and_then(|()| file.sync_all())
                        .map_err(io)?;
                }
                fs::rename(&pending, &marker).map_err(io)?;
                sync_directory(path)?;
            }
        }
        catalog
            .db
            .execute("UPDATE vault SET state='ready'", [])
            .map_err(db)?;
    }
    report(&catalog, config)
}

pub fn status(config: &Config) -> Result<Status> {
    report(&open_catalog(config, false)?, config)
}

fn report(catalog: &Catalog, config: &Config) -> Result<Status> {
    let CatalogIdentity {
        vault_id,
        state,
        volumes,
    } = identities(catalog, config)?;
    let mut reports = Vec::new();
    for (identity, path) in volumes {
        let marker = path.join(MARKER);
        let (condition, available_bytes) = if !path.exists() {
            ("offline", None)
        } else if !marker.exists() {
            ("identity_missing", None)
        } else if read_identity(&marker)? != identity {
            ("identity_mismatch", None)
        } else {
            ("mounted", Some(fs4::available_space(&path).map_err(io)?))
        };
        reports.push(VolumeStatus {
            label: identity.label,
            volume_id: identity.volume_id,
            path,
            state: condition,
            available_bytes,
        });
    }
    Ok(Status {
        schema_version: 1,
        state,
        vault_id,
        volumes: reports,
    })
}
