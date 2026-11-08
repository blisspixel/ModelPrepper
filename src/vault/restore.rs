//! Offline replica, catalog rebuild, local-baseline import, and one-plan verification.
//! These paths do not open a socket, do not require a grant, and do not copy one.
use super::{adoption, check_path, open_catalog, open_recovered_catalog, report, seal, status};
use crate::{Config, Error, Result, inventory::Source, plan::digest, validation::normalize_repo};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const BUNDLE_NAME: &str = "modelprepper-bundle.json";

pub const PROVENANCE_LOCAL_BASELINE: &str = "local-baseline";
pub const PROVENANCE_UNRECORDED: &str = "sealed bundle (provenance unrecorded)";

/// Where a replica test stops. Neither point inserts a healthy replica row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplicaStop {
    BeforeRename,
    BeforeCatalog,
}

/// A verified second copy of one sealed revision. This is not a separate failure domain.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Replica {
    pub schema_version: u32,
    pub status: &'static str,
    pub plan_id: String,
    pub repo_id: String,
    pub resolved_revision: String,
    pub provenance: String,
    pub volume_label: String,
    pub path: PathBuf,
    pub file_count: usize,
    pub size_bytes: u64,
    pub failure_domain: &'static str,
    pub transfer_authorized: bool,
    pub workspace_bytes: Option<u64>,
}

/// Custody and replica rows rebuilt from on-volume bundles. Approvals are not transferred.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Rebuild {
    pub schema_version: u32,
    pub status: &'static str,
    pub vault_id: String,
    pub custody_count: u64,
    pub replica_count: u64,
    pub damaged: Vec<String>,
    pub conflicts: Vec<String>,
    pub offline_volumes: Vec<String>,
    pub approvals_transferred: bool,
    pub transfer_authorized: bool,
    pub workspace_bytes: Option<u64>,
}

/// A copy recorded without upstream proof. Matching bytes do not relabel it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BaselineImport {
    pub schema_version: u32,
    pub status: &'static str,
    pub plan_id: String,
    pub repo_id: String,
    pub resolved_revision: String,
    pub provenance: &'static str,
    pub path: PathBuf,
    pub file_count: usize,
    pub size_bytes: u64,
    pub consistent_with: Option<String>,
    pub transfer_authorized: bool,
    pub workspace_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
struct ManifestFile {
    path: String,
    sha256: String,
    size_bytes: u64,
}

struct CustodyRow {
    plan_id: String,
    repo: String,
    revision: String,
    provenance: String,
    volume_label: String,
    relative: String,
    manifest: String,
}

struct Found {
    bundle_id: String,
    repo: String,
    revision: String,
    provenance: String,
    volume_label: String,
    relative: String,
    manifest: String,
    files: Vec<ManifestFile>,
}

/// Copy one sealed revision onto another mounted volume and record it only after the hashes match.
pub fn replica(config: &Config, plan_id: &str, volume_label: &str) -> Result<Replica> {
    replica_with(config, plan_id, volume_label, None)
}

pub(crate) fn replica_with(
    config: &Config,
    plan_id: &str,
    volume_label: &str,
    stop: Option<ReplicaStop>,
) -> Result<Replica> {
    let row = {
        let catalog = open_catalog(config, false)?;
        load_custody(&catalog, plan_id)?.ok_or_else(|| {
            Error::new(
                "revision_not_sealed",
                "replica requires a sealed revision in the catalog",
            )
        })?
    };
    if row.volume_label == volume_label {
        return Err(Error::new(
            "replica_same_volume",
            "a second folder on the sealing volume is not a replica",
        ));
    }
    let current = status(config)?;
    let source_volume = mounted(&current, &row.volume_label)?;
    let destination = mounted(&current, volume_label)?;
    let source_root = fs::canonicalize(&source_volume.path).map_err(super::io)?;
    let dest_root = fs::canonicalize(&destination.path).map_err(super::io)?;
    check_path(&source_root)?;
    check_path(&dest_root)?;
    crate::filesystem::admit(&destination.path, crate::filesystem::Purpose::Payload)?;
    let source = source_root.join(&row.relative);
    let files = parse_manifest(&row.manifest)?;
    let bundle = seal::read_published_bundle(&source.join(BUNDLE_NAME))?;
    if bundle.bundle_id != row.plan_id
        || bundle.repo_id != row.repo
        || bundle.resolved_revision != row.revision
        || !same_files(&bundle.files, &files)
    {
        return Err(conflict());
    }
    let size = sweep(&source, &files)?;
    seal::reservation_fits(
        destination.available_bytes.ok_or_else(|| {
            Error::new(
                "volume_unavailable",
                "connect the configured destination disk with its original identity",
            )
        })?,
        size,
        config.storage.min_free_bytes,
    )?;
    let published = adoption::publish_dir(&dest_root, &row.repo, &row.revision);
    let staging = dest_root.join(".modelprepper-adopt").join(&row.plan_id);
    check_path(&published)?;
    check_path(&staging)?;
    if directory_exists(&published)? {
        sweep(&published, &files)?;
        let copy = seal::read_published_bundle(&published.join(BUNDLE_NAME))?;
        if copy.bundle_id != bundle.bundle_id || !same_files(&copy.files, &files) {
            return Err(conflict());
        }
        record_replica(config, &row, volume_label)?;
        return Ok(replica_view(
            &row,
            volume_label,
            published,
            files.len(),
            size,
        ));
    }
    if let Some(parent) = staging.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    for file in &files {
        copy_hashed(
            &crate::verify::local_path(&source, &file.path),
            &crate::verify::local_path(&staging, &file.path),
            file,
        )?;
    }
    copy_bytes(&source.join(BUNDLE_NAME), &staging.join(BUNDLE_NAME))?;
    adoption::sync_dir(&staging)?;
    if stop == Some(ReplicaStop::BeforeRename) {
        return Err(Error::new(
            "replica_interrupted",
            "stopped before the replica was published",
        ));
    }
    if let Some(parent) = published.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    fs::rename(&staging, &published).map_err(super::io)?;
    if let Some(parent) = published.parent() {
        adoption::sync_dir(parent)?;
    }
    sweep(&published, &files)?;
    if stop == Some(ReplicaStop::BeforeCatalog) {
        return Err(Error::new(
            "replica_interrupted",
            "stopped before the replica was recorded",
        ));
    }
    record_replica(config, &row, volume_label)?;
    Ok(replica_view(
        &row,
        volume_label,
        published,
        files.len(),
        size,
    ))
}

/// Read sealed bundles into a catalog. A missing database is rebuilt from volume markers.
/// Grants, proposals, and approval decisions are not created.
pub fn rebuild(config: &Config) -> Result<Rebuild> {
    let database = config.catalog_path.join("catalog.sqlite3");
    let catalog = if database.exists() {
        open_catalog(config, false)?
    } else {
        open_recovered_catalog(config)?
    };
    let current = report(&catalog, config)?;
    drop(catalog);
    let mut found = Vec::new();
    let mut damaged = Vec::new();
    let mut offline_volumes = Vec::new();
    for volume in &current.volumes {
        if volume.state != "mounted" {
            offline_volumes.push(volume.label.clone());
            continue;
        }
        let root = fs::canonicalize(&volume.path).map_err(super::io)?;
        check_path(&root)?;
        let preserved = root.join("preserved");
        if !preserved.exists() {
            continue;
        }
        if linked_path(&preserved)? {
            damaged.push(format!("{}:preserved", volume.label));
            continue;
        }
        for bundle_path in find_bundles(&preserved, &volume.label, &mut damaged)? {
            match inspect(&root, &volume.label, &bundle_path) {
                Ok(item) => found.push(item),
                Err(_) => damaged.push(format!("{}:{}", volume.label, bundle_path.display())),
            }
        }
    }
    found.sort_by(|left, right| {
        (&left.bundle_id, &left.volume_label).cmp(&(&right.bundle_id, &right.volume_label))
    });
    let mut conflicts = Vec::new();
    let mut index = 0;
    while index < found.len() {
        let end = found[index..]
            .iter()
            .position(|item| item.bundle_id != found[index].bundle_id)
            .map(|offset| index + offset)
            .unwrap_or(found.len());
        let group = &found[index..end];
        if group.iter().any(|item| item.manifest != group[0].manifest) {
            conflicts.push(group[0].bundle_id.clone());
        } else if let Err(error) = record_found(config, group) {
            if error.code == "destination_conflict" {
                conflicts.push(group[0].bundle_id.clone());
            } else {
                return Err(error);
            }
        }
        index = end;
    }
    conflicts.sort();
    conflicts.dedup();
    damaged.sort();
    damaged.dedup();
    let catalog = open_catalog(config, false)?;
    let custody_count = u64::try_from(count_rows(&catalog, "custody")?).unwrap_or(0);
    let replica_count = u64::try_from(count_rows(&catalog, "replicas")?).unwrap_or(0);
    let fresh = report(&catalog, config)?;
    Ok(Rebuild {
        schema_version: 1,
        status: "rebuilt",
        vault_id: fresh.vault_id,
        custody_count,
        replica_count,
        damaged,
        conflicts,
        offline_volumes,
        approvals_transferred: false,
        transfer_authorized: false,
        workspace_bytes: None,
    })
}

/// Copy a local folder and record it as a baseline. Upstream proof is not invented.
pub fn import_baseline(
    config: &Config,
    source: &Path,
    repo: &str,
    revision: &str,
    volume_label: &str,
) -> Result<BaselineImport> {
    let repo = normalize_repo(repo)?;
    if revision.len() != 40
        || !revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::new(
            "invalid_revision",
            "a local-baseline revision is a 40-hex label, not an upstream pin",
        ));
    }
    let source = adoption::checked_source(config, source)?;
    let names = collect_files(&source)?;
    if names.is_empty() {
        return Err(Error::new(
            "empty_revision",
            "a local baseline needs at least one file",
        ));
    }
    if names.len() > 1024 {
        return Err(Error::new(
            "too_many_files",
            "a local baseline is limited to 1024 files",
        ));
    }
    let mut files = Vec::with_capacity(names.len());
    for name in names {
        let path = crate::verify::local_path(&source, &name);
        let meta = fs::symlink_metadata(&path).map_err(super::io)?;
        if linked(&meta) || !meta.is_file() {
            return Err(Error::new(
                "unsafe_path",
                "a local baseline contains a non-regular file",
            ));
        }
        let hashed = crate::integrity::hash_exact(
            File::open(&path).map_err(super::io)?,
            meta.len(),
            |_| {},
        )?;
        files.push(ManifestFile {
            path: name,
            sha256: hashed.sha256,
            size_bytes: meta.len(),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let plan_id = baseline_id(&repo, revision, &files)?;
    let size = files
        .iter()
        .try_fold(0_u64, |total, file| total.checked_add(file.size_bytes))
        .ok_or_else(|| Error::new("size_overflow", "baseline size exceeds u64 capacity"))?;
    let current = status(config)?;
    let volume = mounted(&current, volume_label)?;
    crate::filesystem::admit(&volume.path, crate::filesystem::Purpose::Payload)?;
    let available = volume.available_bytes.ok_or_else(|| {
        Error::new(
            "volume_unavailable",
            "connect the configured destination disk with its original identity",
        )
    })?;
    seal::reservation_fits(available, size, config.storage.min_free_bytes)?;
    let root = fs::canonicalize(&volume.path).map_err(super::io)?;
    check_path(&root)?;
    let published = adoption::publish_dir(&root, &repo, revision);
    let staging = root.join(".modelprepper-adopt").join(&plan_id);
    check_path(&published)?;
    check_path(&staging)?;
    let consistent_with = consistent_plan(config, &plan_id, &files)?;
    if directory_exists(&published)? {
        sweep(&published, &files)?;
        let bundle = seal::read_published_bundle(&published.join(BUNDLE_NAME))?;
        if bundle.bundle_id != plan_id
            || bundle.provenance != PROVENANCE_LOCAL_BASELINE
            || !same_files(&bundle.files, &files)
        {
            return Err(conflict());
        }
        record_baseline(config, &plan_id, &repo, revision, volume_label, &files)?;
        return Ok(baseline_view(
            plan_id,
            repo,
            revision,
            published,
            files.len(),
            size,
            consistent_with,
        ));
    }
    if let Some(parent) = staging.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    for file in &files {
        copy_hashed(
            &crate::verify::local_path(&source, &file.path),
            &crate::verify::local_path(&staging, &file.path),
            file,
        )?;
    }
    write_baseline_bundle(&staging, &plan_id, &repo, revision, &files)?;
    if let Some(parent) = published.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    fs::rename(&staging, &published).map_err(super::io)?;
    if let Some(parent) = published.parent() {
        adoption::sync_dir(parent)?;
    }
    sweep(&published, &files)?;
    record_baseline(config, &plan_id, &repo, revision, volume_label, &files)?;
    Ok(baseline_view(
        plan_id,
        repo,
        revision,
        published,
        files.len(),
        size,
        consistent_with,
    ))
}

fn record_found(config: &Config, group: &[Found]) -> Result<()> {
    let catalog = open_catalog(config, false)?;
    let existing = load_custody(&catalog, &group[0].bundle_id)?;
    let primary = if let Some(row) = existing {
        if parse_manifest(&row.manifest)? != group[0].files {
            return Err(conflict());
        }
        row.volume_label
    } else {
        // The configured default volume stays the custody location when it still
        // holds a matching bundle. Otherwise the first label wins. Both copies stay sealed.
        let chosen = group
            .iter()
            .find(|item| item.volume_label == config.storage.default_volume)
            .unwrap_or(&group[0]);
        adoption::record_custody(
            &catalog,
            &adoption::CustodyRecord {
                plan_id: &chosen.bundle_id,
                repo: &chosen.repo,
                revision: &chosen.revision,
                provenance: &chosen.provenance,
                volume_label: &chosen.volume_label,
                relative: &chosen.relative,
                manifest: &chosen.manifest,
            },
        )?;
        chosen.volume_label.clone()
    };
    for item in group {
        if item.volume_label == primary {
            continue;
        }
        insert_replica(
            &catalog,
            &item.bundle_id,
            &item.volume_label,
            &item.relative,
            &item.manifest,
        )?;
    }
    Ok(())
}

fn record_replica(config: &Config, row: &CustodyRow, volume_label: &str) -> Result<()> {
    let catalog = open_catalog(config, false)?;
    let current = load_custody(&catalog, &row.plan_id)?.ok_or_else(|| {
        Error::new(
            "revision_not_sealed",
            "the sealed revision was removed before the replica was recorded",
        )
    })?;
    if current.manifest != row.manifest || current.provenance != row.provenance {
        return Err(conflict());
    }
    insert_replica(
        &catalog,
        &row.plan_id,
        volume_label,
        &adoption::relative_dir(&row.repo, &row.revision),
        &row.manifest,
    )
}

fn record_baseline(
    config: &Config,
    plan_id: &str,
    repo: &str,
    revision: &str,
    volume_label: &str,
    files: &[ManifestFile],
) -> Result<()> {
    let manifest = manifest_json(files)?;
    let catalog = open_catalog(config, false)?;
    match load_custody(&catalog, plan_id)? {
        Some(row) if row.provenance != PROVENANCE_LOCAL_BASELINE || row.manifest != manifest => {
            Err(conflict())
        }
        Some(_) => Ok(()),
        None => adoption::record_custody(
            &catalog,
            &adoption::CustodyRecord {
                plan_id,
                repo,
                revision,
                provenance: PROVENANCE_LOCAL_BASELINE,
                volume_label,
                relative: &adoption::relative_dir(repo, revision),
                manifest: &manifest,
            },
        ),
    }
}

fn insert_replica(
    catalog: &super::Catalog,
    plan_id: &str,
    volume_label: &str,
    relative: &str,
    manifest: &str,
) -> Result<()> {
    let stored = match catalog.db.query_row(
        "SELECT manifest FROM replicas WHERE plan_id = ?1 AND volume_label = ?2",
        rusqlite::params![plan_id, volume_label],
        |row| row.get::<_, String>(0),
    ) {
        Ok(value) => Some(value),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(error) => return Err(super::db(error)),
    };
    match stored {
        Some(existing) if existing != manifest => Err(conflict()),
        Some(_) => Ok(()),
        None => {
            catalog
                .db
                .execute(
                    "INSERT INTO replicas (plan_id, volume_label, relative_dir, manifest) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![plan_id, volume_label, relative, manifest],
                )
                .map_err(super::db)?;
            Ok(())
        }
    }
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SealedVerification {
    pub schema_version: u32,
    pub action: &'static str,
    pub plan_id: String,
    pub repo_id: String,
    pub resolved_revision: String,
    /// True only when every sealed file matches. An empty manifest is not a match.
    pub matched: bool,
    pub files: Vec<SealedFile>,
    pub network_contact: bool,
    pub transfer_authorized: bool,
    pub vault_mutated: bool,
    pub downloads_started: bool,
    pub workspace_bytes: Option<u64>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct SealedFile {
    pub path: String,
    pub status: &'static str,
}

/// Hash one sealed plan with the existing file hash. Other plans are not read.
pub fn verify_sealed(config: &Config, plan_id: &str) -> Result<SealedVerification> {
    if plan_id.len() != 64
        || !plan_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::new(
            "invalid_plan",
            "a plan id is 64 lowercase hex characters",
        ));
    }
    let catalog = open_catalog(config, false)?;
    let Some(row) = load_custody(&catalog, plan_id)? else {
        return Err(Error::new("plan_not_stored", "that plan is not in custody"));
    };
    drop(catalog);
    let files: Vec<ManifestFile> = serde_json::from_str(&row.manifest)
        .map_err(|_| Error::new("invalid_catalog", "custody manifest is invalid"))?;
    let current = status(config)?;
    let volume = current
        .volumes
        .iter()
        .find(|volume| volume.label == row.volume_label)
        .ok_or_else(|| Error::new("unknown_volume", "the named volume is not in this vault"))?;
    if volume.state != "mounted" {
        return Ok(sealed_report(&row, "offline", false, Vec::new()));
    }
    let root = fs::canonicalize(&volume.path).map_err(super::io)?;
    let directory = adoption::publish_dir(&root, &row.repo, &row.revision);
    let mut checked = Vec::with_capacity(files.len());
    let mut matched = !files.is_empty();
    for file in &files {
        let status = sealed_status(&directory, file);
        if status != "match" {
            matched = false;
        }
        checked.push(SealedFile {
            path: file.path.clone(),
            status,
        });
    }
    Ok(sealed_report(&row, "verified", matched, checked))
}

fn sealed_report(
    row: &CustodyRow,
    action: &'static str,
    matched: bool,
    files: Vec<SealedFile>,
) -> SealedVerification {
    SealedVerification {
        schema_version: 1,
        action,
        plan_id: row.plan_id.clone(),
        repo_id: row.repo.clone(),
        resolved_revision: row.revision.clone(),
        matched,
        files,
        network_contact: false,
        transfer_authorized: false,
        vault_mutated: false,
        downloads_started: false,
        workspace_bytes: None,
    }
}

fn sealed_status(directory: &Path, file: &ManifestFile) -> &'static str {
    if !safe_relative(&file.path) {
        return "mismatch";
    }
    let path = crate::verify::local_path(directory, &file.path);
    match hash_file(&path, file) {
        Ok(()) => "match",
        Err(error) if error.code == "hash_mismatch" && fs::symlink_metadata(&path).is_err() => {
            "missing"
        }
        Err(error) if error.code == "hash_mismatch" => "mismatch",
        Err(_) => "unreadable",
    }
}

fn load_custody(catalog: &super::Catalog, plan_id: &str) -> Result<Option<CustodyRow>> {
    match catalog.db.query_row(
        "SELECT plan_id, repo_id, resolved_revision, provenance, volume_label, relative_dir, manifest FROM custody WHERE plan_id = ?1",
        [plan_id],
        |row| {
            Ok(CustodyRow {
                plan_id: row.get(0)?,
                repo: row.get(1)?,
                revision: row.get(2)?,
                provenance: row.get(3)?,
                volume_label: row.get(4)?,
                relative: row.get(5)?,
                manifest: row.get(6)?,
            })
        },
    ) {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(super::db(error)),
    }
}

fn inspect(root: &Path, label: &str, bundle_path: &Path) -> Result<Found> {
    let bundle = seal::read_published_bundle(bundle_path)?;
    let directory = bundle_path
        .parent()
        .ok_or_else(|| Error::new("not_sealed", "the bundle manifest is missing or unreadable"))?;
    let expected = adoption::publish_dir(root, &bundle.repo_id, &bundle.resolved_revision);
    if directory != expected {
        return Err(Error::new(
            "not_sealed",
            "a bundle outside its revision directory is not a seal",
        ));
    }
    let files = bundle
        .files
        .iter()
        .map(|file| ManifestFile {
            path: file.path.clone(),
            sha256: file.sha256.clone(),
            size_bytes: file.size_bytes,
        })
        .collect::<Vec<_>>();
    sweep(directory, &files)?;
    let provenance = if bundle.provenance.is_empty() {
        PROVENANCE_UNRECORDED.into()
    } else {
        bundle.provenance
    };
    let relative = adoption::relative_dir(&bundle.repo_id, &bundle.resolved_revision);
    Ok(Found {
        bundle_id: bundle.bundle_id,
        repo: bundle.repo_id,
        revision: bundle.resolved_revision,
        provenance,
        volume_label: label.into(),
        relative,
        manifest: manifest_json(&files)?,
        files,
    })
}

fn find_bundles(preserved: &Path, label: &str, damaged: &mut Vec<String>) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![preserved.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir).map_err(super::io)? {
            entries.push(entry.map_err(super::io)?.path());
        }
        entries.sort();
        for path in entries {
            if linked_path(&path)? {
                damaged.push(format!("{label}:{}", path.display()));
                continue;
            }
            let meta = fs::symlink_metadata(&path).map_err(super::io)?;
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file()
                && path.file_name().and_then(|name| name.to_str()) == Some(BUNDLE_NAME)
            {
                found.push(path);
            }
        }
    }
    Ok(found)
}

fn sweep(root: &Path, files: &[ManifestFile]) -> Result<u64> {
    let mut total = 0_u64;
    for file in files {
        if !safe_relative(&file.path) {
            return Err(Error::new(
                "hash_mismatch",
                format!("{} hash_mismatch", file.path),
            ));
        }
        let path = crate::verify::local_path(root, &file.path);
        hash_file(&path, file)?;
        total = total
            .checked_add(file.size_bytes)
            .ok_or_else(|| Error::new("size_overflow", "sealed size exceeds u64 capacity"))?;
    }
    Ok(total)
}

fn hash_file(path: &Path, file: &ManifestFile) -> Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(_) => {
            return Err(Error::new(
                "hash_mismatch",
                format!("{} hash_mismatch", file.path),
            ));
        }
    };
    if linked(&meta) || !meta.is_file() || meta.len() != file.size_bytes {
        return Err(Error::new(
            "hash_mismatch",
            format!("{} hash_mismatch", file.path),
        ));
    }
    let hashed = crate::integrity::hash_exact(
        File::open(path).map_err(super::io)?,
        file.size_bytes,
        |_| {},
    )?;
    if hashed.sha256 != file.sha256 {
        return Err(Error::new(
            "hash_mismatch",
            format!("{} hash_mismatch", file.path),
        ));
    }
    Ok(())
}

fn copy_hashed(from: &Path, to: &Path, file: &ManifestFile) -> Result<()> {
    if hash_file(to, file).is_ok() {
        return Ok(());
    }
    if to.exists() {
        fs::remove_file(to).map_err(super::io)?;
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    let partial = to.with_extension("partial");
    if partial.exists() {
        fs::remove_file(&partial).map_err(super::io)?;
    }
    let mut input = File::open(from).map_err(super::io)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(super::io)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut count = 0_u64;
    loop {
        let length = input.read(&mut buffer).map_err(super::io)?;
        if length == 0 {
            break;
        }
        count = count.saturating_add(length as u64);
        if count > file.size_bytes {
            drop(output);
            let _ = fs::remove_file(&partial);
            return Err(Error::new(
                "size_mismatch",
                format!("{} size_mismatch", file.path),
            ));
        }
        output.write_all(&buffer[..length]).map_err(super::io)?;
    }
    if count != file.size_bytes {
        drop(output);
        let _ = fs::remove_file(&partial);
        return Err(Error::new(
            "size_mismatch",
            format!("{} size_mismatch", file.path),
        ));
    }
    output.sync_all().map_err(super::io)?;
    drop(output);
    fs::rename(&partial, to).map_err(super::io)?;
    if let Some(parent) = to.parent() {
        adoption::sync_dir(parent)?;
    }
    hash_file(to, file)
}

fn copy_bytes(from: &Path, to: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(from).map_err(super::io)?;
    if linked(&meta) || !meta.is_file() || meta.len() > 1024 * 1024 {
        return Err(Error::new(
            "not_sealed",
            "the bundle manifest is missing or unreadable",
        ));
    }
    let bytes = fs::read(from).map_err(super::io)?;
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(to)
        .map_err(super::io)?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(super::io)?;
    Ok(())
}

fn write_baseline_bundle(
    directory: &Path,
    plan_id: &str,
    repo: &str,
    revision: &str,
    files: &[ManifestFile],
) -> Result<()> {
    let document = serde_json::json!({
        "schema_version": 1,
        "bundle_id": plan_id,
        "source": Source {
            endpoint: "local".into(),
            repo_type: "model".into(),
            repo_id: repo.into(),
            requested_revision: revision.into(),
            resolved_revision: revision.into(),
            gated: false,
            private: false,
        },
        "profile": "local-baseline",
        "rights": {"license_id": "unreviewed", "status": "unreviewed"},
        "files": files,
        "excluded": [],
        "dependencies": [],
        "provenance": PROVENANCE_LOCAL_BASELINE,
    });
    let mut bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| Error::new("output_failed", error.to_string()))?;
    bytes.push(b'\n');
    let path = directory.join(BUNDLE_NAME);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(super::io)?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(super::io)?;
    adoption::sync_dir(directory)
}

fn baseline_id(repo: &str, revision: &str, files: &[ManifestFile]) -> Result<String> {
    #[derive(Serialize)]
    struct Identity<'a> {
        provenance: &'a str,
        repo_id: &'a str,
        resolved_revision: &'a str,
        files: &'a [ManifestFile],
    }
    let bytes = serde_json::to_vec(&Identity {
        provenance: PROVENANCE_LOCAL_BASELINE,
        repo_id: repo,
        resolved_revision: revision,
        files,
    })
    .map_err(|error| Error::new("output_failed", error.to_string()))?;
    Ok(digest(&bytes))
}

fn consistent_plan(
    config: &Config,
    plan_id: &str,
    files: &[ManifestFile],
) -> Result<Option<String>> {
    let catalog = open_catalog(config, false)?;
    let mut query = catalog
        .db
        .prepare("SELECT plan_id, manifest FROM custody ORDER BY plan_id")
        .map_err(super::db)?;
    let rows = query
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(super::db)?;
    for row in rows {
        let (id, manifest) = row.map_err(super::db)?;
        if id == plan_id {
            continue;
        }
        if parse_manifest(&manifest).ok().as_deref() == Some(files) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn parse_manifest(text: &str) -> Result<Vec<ManifestFile>> {
    let mut files: Vec<ManifestFile> = serde_json::from_str(text)
        .map_err(|_| Error::new("invalid_catalog", "custody manifest is invalid"))?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn manifest_json(files: &[ManifestFile]) -> Result<String> {
    let entries = files
        .iter()
        .map(|file| {
            serde_json::json!({
                "path": file.path,
                "sha256": file.sha256,
                "size_bytes": file.size_bytes,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&entries).map_err(|error| Error::new("output_failed", error.to_string()))
}

fn same_files(published: &[seal::PublishedFile], files: &[ManifestFile]) -> bool {
    published.len() == files.len()
        && published.iter().zip(files).all(|(left, right)| {
            left.path == right.path
                && left.sha256 == right.sha256
                && left.size_bytes == right.size_bytes
        })
}

fn mounted<'a>(current: &'a super::Status, label: &str) -> Result<&'a super::VolumeStatus> {
    let volume = current
        .volumes
        .iter()
        .find(|volume| volume.label == label)
        .ok_or_else(|| Error::new("unknown_volume", "the named volume is not in this vault"))?;
    if volume.state != "mounted" {
        return Err(Error::new(
            "volume_unavailable",
            "connect the configured destination disk with its original identity",
        ));
    }
    Ok(volume)
}

fn collect_files(root: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir).map_err(super::io)? {
            entries.push(entry.map_err(super::io)?.path());
        }
        entries.sort();
        for path in entries {
            let meta = fs::symlink_metadata(&path).map_err(super::io)?;
            if linked(&meta) {
                return Err(Error::new(
                    "unsafe_path",
                    "a symlink is not a local baseline",
                ));
            }
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            if !meta.is_file() {
                return Err(Error::new(
                    "unsafe_path",
                    "a local baseline contains a non-regular file",
                ));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::new("unsafe_path", "baseline path escaped its source"))?;
            let mut parts = Vec::new();
            for component in relative.components() {
                let Some(text) = component.as_os_str().to_str() else {
                    return Err(Error::new("unsafe_path", "baseline paths must be UTF-8"));
                };
                parts.push(text);
            }
            let text = parts.join("/");
            if text == BUNDLE_NAME {
                continue;
            }
            if !safe_relative(&text) {
                return Err(Error::new(
                    "unsafe_path",
                    "baseline paths cannot contain dot segments",
                ));
            }
            files.push(text);
        }
    }
    files.sort();
    Ok(files)
}

fn count_rows(catalog: &super::Catalog, table: &str) -> Result<i64> {
    let sql = match table {
        "custody" => "SELECT COUNT(*) FROM custody",
        "replicas" => "SELECT COUNT(*) FROM replicas",
        _ => {
            return Err(Error::new("catalog_failed", "unknown catalog table"));
        }
    };
    catalog
        .db
        .query_row(sql, [], |row| row.get(0))
        .map_err(super::db)
}

fn replica_view(
    row: &CustodyRow,
    volume_label: &str,
    path: PathBuf,
    file_count: usize,
    size_bytes: u64,
) -> Replica {
    Replica {
        schema_version: 1,
        status: "replicated",
        plan_id: row.plan_id.clone(),
        repo_id: row.repo.clone(),
        resolved_revision: row.revision.clone(),
        provenance: row.provenance.clone(),
        volume_label: volume_label.into(),
        path,
        file_count,
        size_bytes,
        failure_domain: "unlabeled",
        transfer_authorized: false,
        workspace_bytes: None,
    }
}

fn baseline_view(
    plan_id: String,
    repo: String,
    revision: &str,
    path: PathBuf,
    file_count: usize,
    size_bytes: u64,
    consistent_with: Option<String>,
) -> BaselineImport {
    BaselineImport {
        schema_version: 1,
        status: "imported",
        plan_id,
        repo_id: repo,
        resolved_revision: revision.into(),
        provenance: PROVENANCE_LOCAL_BASELINE,
        path,
        file_count,
        size_bytes,
        consistent_with,
        transfer_authorized: false,
        workspace_bytes: None,
    }
}

fn conflict() -> Error {
    Error::new(
        "destination_conflict",
        "the published tree does not match this revision and was left in place",
    )
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn directory_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if linked(&meta) => Err(Error::new("unsafe_path", "a symlink is not a seal")),
        Ok(meta) if meta.is_dir() => Ok(true),
        Ok(_) => Err(Error::new(
            "unsafe_path",
            "the destination is not a directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(super::io(error)),
    }
}

fn linked_path(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(linked(&meta)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(super::io(error)),
    }
}

fn linked(meta: &fs::Metadata) -> bool {
    let linked = meta.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || meta.file_attributes() & 0x400 != 0
    };
    linked
}

#[cfg(test)]
#[path = "tests/restore.rs"]
mod tests;
