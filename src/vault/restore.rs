//! Offline replica, catalog rebuild, local-baseline import, and one-plan verification.
//! These paths do not open a socket, do not require a grant, and do not copy one.
use super::{Catalog, adoption, check_path, open_catalog, open_recovered_catalog, report, seal};
use crate::{Config, Error, Result, inventory::Source, plan::digest, validation::normalize_repo};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use super::seal::BUNDLE_NAME;

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
    pub unbundled_directories: Vec<String>,
    pub unconfirmed_copies: Vec<String>,
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
    let catalog = open_catalog(config, false)?;
    let row = load_custody(&catalog, plan_id)?.ok_or_else(|| {
        Error::new(
            "revision_not_sealed",
            "replica requires a preserved revision in the catalog",
        )
    })?;
    if row.volume_label == volume_label {
        return Err(Error::new(
            "replica_same_volume",
            "a second folder on the sealing volume is not a replica",
        ));
    }
    let current = report(&catalog, config)?;
    let source_volume = mounted(&current, &row.volume_label)?;
    let destination = mounted(&current, volume_label)?;
    let source_root = fs::canonicalize(&source_volume.path).map_err(super::io)?;
    let dest_root = fs::canonicalize(&destination.path).map_err(super::io)?;
    check_path(&source_root)?;
    check_path(&dest_root)?;
    crate::filesystem::admit(&destination.path, crate::filesystem::Purpose::Payload)?;
    if row.relative != adoption::relative_dir(&row.repo, &row.revision)
        || !safe_relative(&row.relative)
    {
        return Err(conflict());
    }
    let source = source_root.join(&row.relative);
    check_path(&source)?;
    let files = parse_manifest(&row.manifest)?;
    let manifest_path = source.join(BUNDLE_NAME);
    let review = if row.provenance == adoption::PROVENANCE_LOCAL_UPSTREAM {
        match super::review::load(&catalog, config, &row.plan_id) {
            Ok(proposal) if proposal.review.volume_label == row.volume_label => Some(proposal),
            Ok(_) => None, // A rebuilt copy can live on a different volume from the original review.
            Err(error) if error.code == "proposal_not_found" => None,
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let (bundle_bytes, verified_size) = if let Some(proposal) = review {
        let (bytes, size) = adoption_recovery_bundle(&proposal, &row, &source, &files)?;
        // A plain adoption has no published metadata. If a document exists, it must
        // match the full original evidence before it can be propagated.
        match fs::symlink_metadata(&manifest_path) {
            Ok(_) if seal::read_capped(&manifest_path, seal::BUNDLE_CAP)? != bytes => {
                return Err(conflict());
            }
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(super::io(error));
            }
            _ => {}
        }
        (bytes, Some(size))
    } else {
        (seal::read_capped(&manifest_path, seal::BUNDLE_CAP)?, None)
    };
    let bundle = seal::parse_published_bundle(&bundle_bytes)?;
    check_bundle_identity(&bundle, &row, &files)?;
    let size = match verified_size {
        Some(size) => size,
        None => sweep(&source, &files)?,
    };
    let published = adoption::publish_dir(&dest_root, &row.repo, &row.revision);
    let staging = dest_root.join(".modelprepper-adopt").join(&row.plan_id);
    check_path(&published)?;
    check_path(&staging)?;
    destination.require_identity(&current.vault_id)?;
    if directory_exists(&published)? {
        sweep(&published, &files)?;
        let copy = seal::read_capped(&published.join(BUNDLE_NAME), seal::BUNDLE_CAP)?;
        if copy != bundle_bytes {
            return Err(conflict());
        }
        destination.require_identity(&current.vault_id)?;
        record_replica(&catalog, &row, volume_label)?;
        return Ok(replica_view(
            &row,
            volume_label,
            published,
            files.len(),
            size,
        ));
    }
    seal::check_bundle_paths(files.iter().map(|file| file.path.as_str()))?;
    stage_payload_files(
        &source,
        &staging,
        &files,
        destination,
        &current.vault_id,
        config.storage.min_free_bytes,
    )?;
    seal::write_staged_manifest(&staging.join(BUNDLE_NAME), &bundle_bytes)?;
    adoption::sync_dir(&staging)?;
    if stop == Some(ReplicaStop::BeforeRename) {
        return Err(Error::new(
            "replica_interrupted",
            "stopped before the replica was published",
        ));
    }
    destination.require_identity(&current.vault_id)?;
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
    destination.require_identity(&current.vault_id)?;
    record_replica(&catalog, &row, volume_label)?;
    Ok(replica_view(
        &row,
        volume_label,
        published,
        files.len(),
        size,
    ))
}

/// Older and current local adoptions have a catalog manifest and a saved source review.
/// Build portable evidence only on the replica, using exactly the recorded custody files.
fn adoption_recovery_bundle(
    proposal: &super::review::Proposal,
    row: &CustodyRow,
    root: &Path,
    recorded: &[ManifestFile],
) -> Result<(Vec<u8>, u64)> {
    let inventory = &proposal.review.inventory;
    inventory.validate()?;
    if !inventory.complete
        || inventory.license.status != crate::inventory::EvidenceStatus::Matched
        || inventory.license.paths.is_empty()
        || inventory.source.repo_id != row.repo
        || inventory.source.resolved_revision != row.revision
    {
        return Err(conflict());
    }
    if inventory.license.paths.iter().any(|license| {
        !proposal
            .review
            .evidence
            .iter()
            .any(|item| &item.path == license)
    }) {
        return Err(conflict());
    }
    let mut files = Vec::with_capacity(recorded.len());
    let mut excluded = Vec::new();
    let mut total = 0_u64;
    for item in &inventory.files {
        let Ok(index) = recorded.binary_search_by(|file| file.path.cmp(&item.path)) else {
            if item.required {
                return Err(conflict());
            }
            excluded.push(item.path.clone());
            continue;
        };
        let expected = &recorded[index];
        if item.size_bytes != expected.size_bytes
            || item
                .path
                .split('/')
                .any(|part| part.eq_ignore_ascii_case(BUNDLE_NAME))
        {
            return Err(conflict());
        }
        crate::verify::metadata_nofollow(root, &item.path)?;
        let sha = adoption::matched_sha256(&crate::verify::local_path(root, &item.path), item)?
            .filter(|sha| sha == &expected.sha256)
            .ok_or_else(|| Error::new("hash_mismatch", format!("{} hash_mismatch", item.path)))?;
        total = total
            .checked_add(item.size_bytes)
            .ok_or_else(|| Error::new("size_overflow", "replica size exceeds u64 capacity"))?;
        files.push(seal::BundleFile {
            path: item.path.clone(),
            size_bytes: item.size_bytes,
            sha256: sha,
            upstream_hash: item.upstream_hash.clone(),
        });
    }
    if files.len() != recorded.len()
        || proposal.review.evidence.iter().any(|evidence| {
            !files
                .iter()
                .any(|file| file.path == evidence.path && file.sha256 == evidence.sha256)
        })
    {
        return Err(conflict());
    }
    let bytes = seal::encode_verified_bundle(
        &row.plan_id,
        &inventory.source,
        &inventory.layout,
        &inventory.license.license_id,
        files,
        &excluded,
        adoption::PROVENANCE_LOCAL_UPSTREAM,
    )?;
    Ok((bytes, total))
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
    let mut found = Vec::new();
    let mut damaged = Vec::new();
    let mut offline_volumes = Vec::new();
    let mut unbundled_directories = Vec::new();
    for volume in &current.volumes {
        if volume.state != "mounted" {
            offline_volumes.push(volume.label.clone());
            continue;
        }
        let root = fs::canonicalize(&volume.path).map_err(super::io)?;
        check_path(&root)?;
        let preserved = root.join("preserved");
        if linked_path(&preserved)? {
            damaged.push(format!("{}:preserved", volume.label));
            continue;
        }
        if !preserved.try_exists().map_err(super::io)? {
            continue;
        }
        for bundle_path in find_bundles(
            &preserved,
            &volume.label,
            &mut damaged,
            &mut unbundled_directories,
        )? {
            match inspect(&root, &volume.label, &bundle_path) {
                Ok(item) => found.push(item),
                Err(_) => damaged.push(format!("{}:{}", volume.label, bundle_path.display())),
            }
        }
    }
    // Hashing a large volume can span a mount change. Check all scanned
    // identities before adding the first custody or replica row.
    for volume in current.volumes.iter().filter(|v| v.state == "mounted") {
        volume.require_identity(&current.vault_id)?;
    }
    let mut unconfirmed: BTreeSet<_> = current
        .preserved
        .iter()
        .chain(&current.replicas)
        .filter(|copy| !offline_volumes.contains(&copy.volume_label))
        .map(|copy| (copy.volume_label.as_str(), copy.plan_id.as_str()))
        .collect();
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
        if group.iter().any(|item| {
            item.repo != group[0].repo
                || item.revision != group[0].revision
                || item.provenance != group[0].provenance
                || item.manifest != group[0].manifest
        }) {
            conflicts.push(group[0].bundle_id.clone());
        } else if let Err(error) = record_found(&catalog, &config.storage.default_volume, group) {
            if error.code == "destination_conflict" {
                conflicts.push(group[0].bundle_id.clone());
            } else {
                return Err(error);
            }
        } else {
            for item in group {
                unconfirmed.remove(&(item.volume_label.as_str(), item.bundle_id.as_str()));
            }
        }
        index = end;
    }
    conflicts.sort();
    conflicts.dedup();
    damaged.sort();
    damaged.dedup();
    let custody_count = u64::try_from(count_rows(&catalog, "custody")?).unwrap_or(0);
    let replica_count = u64::try_from(count_rows(&catalog, "replicas")?).unwrap_or(0);
    unbundled_directories.sort();
    let complete = damaged.is_empty()
        && conflicts.is_empty()
        && offline_volumes.is_empty()
        && unbundled_directories.is_empty()
        && unconfirmed.is_empty();
    Ok(Rebuild {
        schema_version: 1,
        status: if complete { "rebuilt" } else { "partial" },
        vault_id: current.vault_id,
        custody_count,
        replica_count,
        damaged,
        conflicts,
        offline_volumes,
        unbundled_directories,
        unconfirmed_copies: unconfirmed
            .into_iter()
            .map(|(label, id)| format!("{label}:{id}"))
            .collect(),
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
    let catalog = open_catalog(config, false)?;
    let current = report(&catalog, config)?;
    let volume = mounted(&current, volume_label)?;
    crate::filesystem::admit(&volume.path, crate::filesystem::Purpose::Payload)?;
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
        let meta = match crate::verify::metadata_nofollow(&source, &name)? {
            Some(meta) => meta,
            None => {
                return Err(Error::new(
                    "unsafe_path",
                    "a local baseline contains a non-regular file",
                ));
            }
        };
        if !meta.is_file() {
            return Err(Error::new(
                "unsafe_path",
                "a local baseline contains a non-regular file",
            ));
        }
        let path = crate::verify::local_path(&source, &name);
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
    volume.require_identity(&current.vault_id)?;
    let root = fs::canonicalize(&volume.path).map_err(super::io)?;
    check_path(&root)?;
    let published = adoption::publish_dir(&root, &repo, revision);
    let staging = root.join(".modelprepper-adopt").join(&plan_id);
    check_path(&published)?;
    check_path(&staging)?;
    let consistent_with = consistent_plan(&catalog, &plan_id, &files)?;
    if directory_exists(&published)? {
        sweep(&published, &files)?;
        let bundle = seal::read_published_bundle(&published.join(BUNDLE_NAME))?;
        if bundle.bundle_id != plan_id
            || bundle.repo_id != repo
            || bundle.resolved_revision != revision
            || bundle.provenance != PROVENANCE_LOCAL_BASELINE
            || !same_files(&bundle.files, &files)
        {
            return Err(conflict());
        }
        volume.require_identity(&current.vault_id)?;
        record_baseline(&catalog, &plan_id, &repo, revision, volume_label, &files)?;
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
    seal::check_bundle_paths(files.iter().map(|file| file.path.as_str()))?;
    stage_payload_files(
        &source,
        &staging,
        &files,
        volume,
        &current.vault_id,
        config.storage.min_free_bytes,
    )?;
    write_baseline_bundle(&staging, &plan_id, &repo, revision, &files)?;
    volume.require_identity(&current.vault_id)?;
    if let Some(parent) = published.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    fs::rename(&staging, &published).map_err(super::io)?;
    if let Some(parent) = published.parent() {
        adoption::sync_dir(parent)?;
    }
    sweep(&published, &files)?;
    volume.require_identity(&current.vault_id)?;
    record_baseline(&catalog, &plan_id, &repo, revision, volume_label, &files)?;
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

fn record_found(catalog: &Catalog, default_volume: &str, group: &[Found]) -> Result<()> {
    // All rows for one bundle commit together, including when a later replica conflicts.
    let tx = catalog.db.unchecked_transaction().map_err(super::db)?;
    let existing = load_custody(catalog, &group[0].bundle_id)?;
    let primary = if let Some(row) = existing {
        if row.repo != group[0].repo
            || row.revision != group[0].revision
            || (group[0].provenance != PROVENANCE_UNRECORDED
                && row.provenance != group[0].provenance)
            || parse_manifest(&row.manifest)? != group[0].files
        {
            return Err(conflict());
        }
        row.volume_label
    } else {
        // The configured default volume stays the custody location when it still
        // holds a matching bundle. Otherwise the first label wins. Both copies stay sealed.
        let chosen = group
            .iter()
            .find(|item| item.volume_label == default_volume)
            .unwrap_or(&group[0]);
        adoption::record_custody(
            catalog,
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
            catalog,
            &item.bundle_id,
            &item.volume_label,
            &item.relative,
            &item.manifest,
        )?;
    }
    tx.commit().map_err(super::db)
}

fn record_replica(catalog: &Catalog, row: &CustodyRow, volume_label: &str) -> Result<()> {
    insert_replica(
        catalog,
        &row.plan_id,
        volume_label,
        &adoption::relative_dir(&row.repo, &row.revision),
        &row.manifest,
    )
}

fn record_baseline(
    catalog: &Catalog,
    plan_id: &str,
    repo: &str,
    revision: &str,
    volume_label: &str,
    files: &[ManifestFile],
) -> Result<()> {
    let manifest = manifest_json(files)?;
    match load_custody(catalog, plan_id)? {
        Some(row)
            if row.provenance != PROVENANCE_LOCAL_BASELINE
                || row.repo != repo
                || row.revision != revision
                || row.relative != adoption::relative_dir(repo, revision)
                || parse_manifest(&row.manifest)? != files =>
        {
            Err(conflict())
        }
        Some(row) if row.volume_label != volume_label => {
            record_replica(catalog, &row, volume_label)
        }
        Some(_) => Ok(()),
        None => adoption::record_custody(
            catalog,
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
    let stored: Option<(String, String)> = catalog
        .db
        .query_row(
            "SELECT relative_dir, manifest FROM replicas WHERE plan_id = ?1 AND volume_label = ?2",
            rusqlite::params![plan_id, volume_label],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(super::db)?;
    match stored {
        Some((path, existing)) if path != relative || existing != manifest => Err(conflict()),
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
    pub volume_label: String,
    pub volume_state: &'static str,
    pub copy_kind: &'static str,
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
    verify_sealed_on_volume(config, plan_id, None)
}

/// Hash just the selected catalog location, including when another disk is offline.
pub fn verify_sealed_on_volume(
    config: &Config,
    plan_id: &str,
    selected_volume: Option<&str>,
) -> Result<SealedVerification> {
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
    let label = selected_volume.unwrap_or(&row.volume_label);
    let ownership = super::identities(&catalog, config)?;
    let (identity, path) = ownership
        .volumes
        .into_iter()
        .find(|(identity, _)| identity.label == label)
        .ok_or_else(|| Error::new("unknown_volume", "the named volume is not in this vault"))?;
    let is_replica = label != row.volume_label;
    let kind = if is_replica { "replica" } else { "custody" };
    let expected_relative = adoption::relative_dir(&row.repo, &row.revision);
    if normalize_repo(&row.repo).as_deref().ok() != Some(row.repo.as_str())
        || crate::validation::validate_hex(&row.revision, 40).is_err()
    {
        return Err(conflict());
    }
    let files = parse_manifest(&row.manifest)?;
    if is_replica {
        let (relative, manifest): (String, String) = catalog
            .db
            .query_row(
                "SELECT relative_dir, manifest FROM replicas WHERE plan_id=?1 AND volume_label=?2",
                rusqlite::params![plan_id, label],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(super::db)?
            .ok_or_else(|| {
                Error::new(
                    "replica_not_recorded",
                    "this plan has no recorded replica on that volume",
                )
            })?;
        if relative != expected_relative || parse_manifest(&manifest)? != files {
            return Err(conflict());
        }
    }
    let volume = super::volume_status(identity, path)?;
    drop(catalog);
    if volume.state != "mounted" {
        return Ok(sealed_report(
            &row,
            &volume,
            kind,
            "offline",
            false,
            Vec::new(),
        ));
    }
    let root = fs::canonicalize(&volume.path).map_err(super::io)?;
    let directory = root.join(expected_relative);
    check_path(&directory)?;
    if is_replica {
        let bundle = seal::read_published_bundle(&directory.join(BUNDLE_NAME))?;
        check_bundle_identity(&bundle, &row, &files)?;
    }
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
    volume.require_identity(&ownership.vault_id)?;
    Ok(sealed_report(
        &row, &volume, kind, "verified", matched, checked,
    ))
}

fn check_bundle_identity(
    bundle: &seal::PublishedBundle,
    row: &CustodyRow,
    files: &[ManifestFile],
) -> Result<()> {
    if bundle.bundle_id != row.plan_id
        || bundle.repo_id != row.repo
        || bundle.resolved_revision != row.revision
        || (bundle.provenance != row.provenance
            && (!bundle.provenance.is_empty()
                || row.provenance == adoption::PROVENANCE_LOCAL_UPSTREAM))
        || !same_files(&bundle.files, files)
    {
        return Err(conflict());
    }
    Ok(())
}

fn sealed_report(
    row: &CustodyRow,
    volume: &super::VolumeStatus,
    copy_kind: &'static str,
    action: &'static str,
    matched: bool,
    files: Vec<SealedFile>,
) -> SealedVerification {
    SealedVerification {
        schema_version: 1,
        action,
        volume_label: volume.label.clone(),
        volume_state: volume.state,
        copy_kind,
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
    match hash_file(directory, file) {
        Ok(()) => "match",
        Err(error) if error.code == "hash_mismatch" && fs::symlink_metadata(&path).is_err() => {
            "missing"
        }
        Err(error) if error.code == "hash_mismatch" || error.code == "unsafe_path" => "mismatch",
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
    if normalize_repo(&bundle.repo_id).as_deref().ok() != Some(bundle.repo_id.as_str())
        || directory != expected
    {
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

fn find_bundles(
    preserved: &Path,
    label: &str,
    damaged: &mut Vec<String>,
    unbundled: &mut Vec<String>,
) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![(preserved.to_path_buf(), false)];
    while let Some((dir, inside_payload)) = stack.pop() {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir).map_err(super::io)? {
            entries.push(entry.map_err(super::io)?.path());
        }
        entries.sort();
        // Keep revision directories visible when their rebuild document is
        // absent. Plain local adoptions can legitimately have this shape.
        let revision_root = dir.parent().and_then(Path::parent).and_then(Path::parent)
            == Some(preserved)
            && dir
                .file_name()
                .and_then(|v| v.to_str())
                .is_some_and(|v| crate::validation::validate_hex(v, 40).is_ok());
        if revision_root && entries.binary_search(&dir.join(BUNDLE_NAME)).is_err() {
            unbundled.push(format!("{label}:{}", dir.display()));
        }
        for path in entries {
            let meta = fs::symlink_metadata(&path).map_err(super::io)?;
            if linked(&meta) {
                damaged.push(format!("{label}:{}", path.display()));
                continue;
            }
            let bundle_name = path.file_name().and_then(|name| name.to_str()) == Some(BUNDLE_NAME);
            // Only the revision root reserves this filename. An owner or repo
            // directory can legally use it, as can files nested in a payload.
            if meta.is_dir() && !(revision_root && bundle_name) {
                stack.push((path, inside_payload || revision_root));
            } else if !inside_payload && bundle_name {
                if meta.is_file() {
                    found.push(path);
                } else {
                    damaged.push(format!("{label}:{}", path.display()));
                }
            }
        }
    }
    Ok(found)
}

fn sweep(root: &Path, files: &[ManifestFile]) -> Result<u64> {
    let mut total = 0_u64;
    for file in files {
        hash_file(root, file)?;
        total = total
            .checked_add(file.size_bytes)
            .ok_or_else(|| Error::new("size_overflow", "sealed size exceeds u64 capacity"))?;
    }
    Ok(total)
}

fn hash_file(root: &Path, file: &ManifestFile) -> Result<()> {
    if !safe_relative(&file.path) {
        return Err(Error::new(
            "hash_mismatch",
            format!("{} hash_mismatch", file.path),
        ));
    }
    // Walk every component. A directory symlink must not make the hash read outside the tree.
    let meta = match crate::verify::metadata_nofollow(root, &file.path)? {
        Some(meta) => meta,
        None => {
            return Err(Error::new(
                "hash_mismatch",
                format!("{} hash_mismatch", file.path),
            ));
        }
    };
    if !meta.is_file() || meta.len() != file.size_bytes {
        return Err(Error::new(
            "hash_mismatch",
            format!("{} hash_mismatch", file.path),
        ));
    }
    let path = crate::verify::local_path(root, &file.path);
    let hashed = crate::integrity::hash_exact(
        File::open(&path).map_err(super::io)?,
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

// Work only inside this plan's unpublished staging tree. Reclaim incomplete
// payloads and legacy scratch files before measuring the remaining capacity.
fn stage_payload_files(
    source: &Path,
    staging: &Path,
    files: &[ManifestFile],
    volume: &super::VolumeStatus,
    vault_id: &str,
    min_free_bytes: u64,
) -> Result<()> {
    volume.require_identity(vault_id)?;
    let mut pending: BTreeMap<_, _> = files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    if directory_exists(staging)? {
        for name in collect_files(staging)? {
            if let Some(file) = pending.get(name.as_str()) {
                match hash_file(staging, file) {
                    Ok(()) => {
                        pending.remove(name.as_str());
                        continue;
                    }
                    Err(error) if error.code == "hash_mismatch" => {}
                    Err(error) => return Err(error),
                }
            }
            volume.require_identity(vault_id)?;
            fs::remove_file(crate::verify::local_path(staging, &name)).map_err(super::io)?;
        }
    }
    let needed = pending
        .values()
        .try_fold(0_u64, |sum, file| sum.checked_add(file.size_bytes))
        .ok_or_else(|| {
            Error::new(
                "size_overflow",
                "remaining staged payload exceeds u64 capacity",
            )
        })?;
    let admit = |bytes| {
        volume.require_identity(vault_id)?;
        seal::reservation_fits(
            fs4::available_space(&volume.path).map_err(super::io)?,
            bytes,
            min_free_bytes,
        )
    };
    admit(needed)?;
    if let Some(parent) = staging.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    for file in pending.into_values() {
        admit(file.size_bytes)?;
        copy_hashed(source, staging, file)?;
    }
    volume.require_identity(vault_id)
}

fn copy_hashed(from_root: &Path, to_root: &Path, file: &ManifestFile) -> Result<()> {
    if crate::verify::metadata_nofollow(to_root, &file.path)?.is_some() {
        return Err(conflict());
    }
    if !crate::verify::metadata_nofollow(from_root, &file.path)?
        .is_some_and(|meta| meta.is_file() && meta.len() == file.size_bytes)
    {
        return Err(Error::new(
            "hash_mismatch",
            format!("{} hash_mismatch", file.path),
        ));
    }
    let to = crate::verify::local_path(to_root, &file.path);
    let from = crate::verify::local_path(from_root, &file.path);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    let mut input = File::open(&from).map_err(super::io)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&to)
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
            let _ = fs::remove_file(&to);
            return Err(Error::new(
                "size_mismatch",
                format!("{} size_mismatch", file.path),
            ));
        }
        output.write_all(&buffer[..length]).map_err(super::io)?;
    }
    if count != file.size_bytes {
        drop(output);
        let _ = fs::remove_file(&to);
        return Err(Error::new(
            "size_mismatch",
            format!("{} size_mismatch", file.path),
        ));
    }
    output.sync_all().map_err(super::io)?;
    drop(output);
    if let Some(parent) = to.parent() {
        adoption::sync_dir(parent)?;
    }
    hash_file(to_root, file)
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
    seal::write_staged_manifest(&path, &bytes)?;
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
    catalog: &Catalog,
    plan_id: &str,
    files: &[ManifestFile],
) -> Result<Option<String>> {
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
