//! Publish one granted revision. A directory rename without
//! `modelprepper-bundle.json` is not a seal, and the review decision stays pending.
use super::{adoption, check_path, grant, open_catalog, report, review};
use crate::{
    Config, Error, Result,
    inventory::{Source, SourceFile, UpstreamHash},
    transfer::{KernelSocketBuffers, RevisionStage, measure_kernel_socket_buffers},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const BUNDLE_NAME: &str = "modelprepper-bundle.json";
const BUNDLE_CAP: u64 = 1024 * 1024;
const RESERVATION_CAP: u64 = 64 * 1024;

pub const PROVENANCE_RANGED_HTTPS: &str = "upstream-verified (ranged https)";

/// Where a seal test stops. Neither point writes a custody row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealStop {
    BeforeRename,
    BeforeCatalog,
}

/// One published revision. `transfer_authorized` stays false and `workspace_bytes` stays null.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Seal {
    pub schema_version: u32,
    pub status: &'static str,
    pub plan_id: String,
    pub repo_id: String,
    pub resolved_revision: String,
    pub provenance: &'static str,
    pub path: PathBuf,
    pub file_count: usize,
    pub size_bytes: u64,
    pub license_id: String,
    pub transfer_authorized: bool,
    pub workspace_bytes: Option<u64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reservation {
    schema_version: u32,
    plan_id: String,
    policy_digest: String,
    volume_id: String,
    payload_bytes: u64,
    file_count: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleDocument {
    schema_version: u32,
    bundle_id: String,
    source: Source,
    profile: String,
    rights: BundleRights,
    files: Vec<BundleFile>,
    excluded: Vec<String>,
    dependencies: Vec<String>,
    /// Absent on a seal written before provenance was stored in the bundle.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    provenance: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleRights {
    license_id: String,
    status: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleFile {
    path: String,
    size_bytes: u64,
    sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upstream_hash: Option<UpstreamHash>,
}

pub(crate) struct PublishedBundle {
    pub bundle_id: String,
    pub repo_id: String,
    pub resolved_revision: String,
    pub provenance: String,
    pub files: Vec<PublishedFile>,
}

pub(crate) struct PublishedFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

struct Prepared {
    plan_id: String,
    repo: String,
    revision: String,
    license_id: String,
    source: Source,
    selected: Vec<SourceFile>,
    excluded: Vec<String>,
    payload_bytes: u64,
    min_free_bytes: u64,
    policy_digest: String,
    volume_id: String,
    volume_label: String,
    available: u64,
    published: PathBuf,
    copy_root: PathBuf,
    reservation: PathBuf,
    layout: String,
}

/// Fetch one granted revision and seal it. The payload socket stays closed until the grant checks.
pub fn fetch(config: &Config, plan_id: &str, stage: &Path) -> Result<Seal> {
    let kernel = measure_kernel_socket_buffers()?;
    fetch_with(
        config,
        plan_id,
        stage,
        &crate::http::Http::default(),
        "https://huggingface.co",
        kernel,
        None,
    )
}

pub(crate) fn fetch_with(
    config: &Config,
    plan_id: &str,
    stage: &Path,
    http: &crate::http::Http,
    base: &str,
    kernel: KernelSocketBuffers,
    stop: Option<SealStop>,
) -> Result<Seal> {
    let prepared = prepare(config, plan_id, kernel)?;
    if let Some(seal) = recover(&prepared, config)? {
        return Ok(seal);
    }
    let stage_path = checked_stage(config, stage)?;
    let files = prepared.selected.clone();
    let mut session = if stage_path.exists() {
        RevisionStage::open(&stage_path, &prepared.source, &files)?
    } else {
        RevisionStage::create(&stage_path, &prepared.source, &files)?
    };
    let allowance = files
        .iter()
        .map(|file| file.size_bytes)
        .max()
        .unwrap_or(1)
        .max(1);
    let cap = files.len().saturating_mul(8).max(1);
    let mut steps = 0usize;
    while !session.is_staged()? {
        if steps >= cap {
            return Err(Error::new(
                "transfer_interrupted",
                "the staged revision did not finish within its request bound",
            ));
        }
        steps += 1;
        let segment = session.stage_next_with(http, base, allowance)?;
        if segment.staged {
            break;
        }
    }
    copy_and_publish(config, plan_id, kernel, &session, stop)
}

/// Seal a frozen set that is already staged. This does not open a socket.
pub fn seal_staged(config: &Config, plan_id: &str, stage: &RevisionStage) -> Result<Seal> {
    seal_staged_with(
        config,
        plan_id,
        stage,
        measure_kernel_socket_buffers()?,
        None,
    )
}

pub(crate) fn seal_staged_with(
    config: &Config,
    plan_id: &str,
    stage: &RevisionStage,
    kernel: KernelSocketBuffers,
    stop: Option<SealStop>,
) -> Result<Seal> {
    copy_and_publish(config, plan_id, kernel, stage, stop)
}

/// Payload plus the configured free reserve must fit. This is not `workspace_bytes`.
pub(crate) fn reservation_fits(available: u64, payload: u64, reserve: u64) -> Result<()> {
    let need = payload.checked_add(reserve).ok_or_else(|| {
        Error::new(
            "size_overflow",
            "payload and free-space reserve exceed u64 capacity",
        )
    })?;
    if available < need {
        return Err(Error::new(
            "insufficient_space",
            "the destination does not have the payload plus the configured free reserve",
        ));
    }
    Ok(())
}

fn prepare(config: &Config, plan_id: &str, kernel: KernelSocketBuffers) -> Result<Prepared> {
    let _grant = grant::current_grant_with(config, plan_id, kernel)?;
    let catalog = open_catalog(config, false)?;
    let proposal = review::load(&catalog, config, plan_id)?;
    if proposal.decision != review::Decision::Pending {
        return Err(Error::new(
            "proposal_not_pending",
            "fetch requires a pending source review",
        ));
    }
    let status = report(&catalog, config)?;
    let volume = status
        .volumes
        .iter()
        .find(|volume| volume.volume_id == proposal.review.volume_id)
        .ok_or_else(|| Error::new("invalid_proposal", "review volume is not in this vault"))?;
    if volume.state != "mounted" {
        return Err(Error::new(
            "volume_unavailable",
            "connect the configured destination disk with its original identity",
        ));
    }
    crate::filesystem::admit(&volume.path, crate::filesystem::Purpose::Payload)?;
    let available = volume.available_bytes.ok_or_else(|| {
        Error::new(
            "volume_unavailable",
            "connect the configured destination disk with its original identity",
        )
    })?;
    let volume_path = fs::canonicalize(&volume.path).map_err(super::io)?;
    let repo = proposal.review.inventory.source.repo_id.clone();
    let revision = proposal.review.inventory.source.resolved_revision.clone();
    let published = adoption::publish_dir(&volume_path, &repo, &revision);
    let copy_root = volume_path.join(".modelprepper-adopt").join(plan_id);
    let reservation = volume_path
        .join(".modelprepper-adopt")
        .join(format!("{plan_id}.reservation.json"));
    check_path(&published)?;
    check_path(&copy_root)?;
    check_path(&reservation)?;
    let mut selected = proposal.review.selected_files.clone();
    selected.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Prepared {
        plan_id: proposal.plan_id,
        repo,
        revision,
        license_id: proposal.review.inventory.license.license_id.clone(),
        source: proposal.review.inventory.source.clone(),
        selected,
        excluded: proposal.review.excluded_files.clone(),
        payload_bytes: proposal.review.payload_bytes,
        min_free_bytes: proposal.review.min_free_bytes,
        policy_digest: proposal.review.policy_digest.clone(),
        volume_id: proposal.review.volume_id.clone(),
        volume_label: volume.label.clone(),
        available,
        published,
        copy_root,
        reservation,
        layout: proposal.review.inventory.layout.clone(),
    })
}

fn recover(prepared: &Prepared, config: &Config) -> Result<Option<Seal>> {
    if !directory_exists(&prepared.published)? {
        return Ok(None);
    }
    let bundle_path = prepared.published.join(BUNDLE_NAME);
    if read_bundle(&bundle_path)?.is_none() {
        return Err(Error::new(
            "not_sealed",
            "a renamed directory is not a seal",
        ));
    }
    let hashes = hash_selected(&prepared.published, &prepared.selected)?;
    bundle_agrees(&bundle_path, prepared, &hashes)?;
    finish_catalog(config, prepared, &hashes)?;
    Ok(Some(view(prepared, prepared.published.clone())))
}

fn copy_and_publish(
    config: &Config,
    plan_id: &str,
    kernel: KernelSocketBuffers,
    stage: &RevisionStage,
    stop: Option<SealStop>,
) -> Result<Seal> {
    let prepared = prepare(config, plan_id, kernel)?;
    if let Some(seal) = recover(&prepared, config)? {
        return Ok(seal);
    }
    if !stage.is_staged()? {
        return Err(Error::new(
            "revision_not_staged",
            "the frozen set is not fully staged",
        ));
    }
    if !same_set(stage.files(), &prepared.selected) {
        return Err(Error::new(
            "revision_set_changed",
            "the frozen set does not match this staging directory",
        ));
    }
    if prepared
        .selected
        .iter()
        .any(|file| file.path == BUNDLE_NAME || file.path.ends_with(&format!("/{BUNDLE_NAME}")))
    {
        return Err(Error::new(
            "destination_conflict",
            "the revision names the bundle manifest path",
        ));
    }
    reservation_fits(
        prepared.available,
        prepared.payload_bytes,
        prepared.min_free_bytes,
    )?;
    write_reservation(&prepared)?;
    if let Some(parent) = prepared.copy_root.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    let mut hashes = Vec::with_capacity(prepared.selected.len());
    for (index, file) in prepared.selected.iter().enumerate() {
        crate::verify::metadata_nofollow(&prepared.copy_root, &file.path)?;
        let sha = copy_verified(
            &stage.payload_path(index),
            &crate::verify::local_path(&prepared.copy_root, &file.path),
            file,
        )?;
        hashes.push((file.path.clone(), file.size_bytes, sha));
    }
    write_bundle(&prepared, &prepared.copy_root, &hashes)?;
    if stop == Some(SealStop::BeforeRename) {
        return Err(Error::new("seal_interrupted", "stopped before publication"));
    }
    if let Some(parent) = prepared.published.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    fs::rename(&prepared.copy_root, &prepared.published).map_err(super::io)?;
    if let Some(parent) = prepared.published.parent() {
        adoption::sync_dir(parent)?;
    }
    bundle_agrees(&prepared.published.join(BUNDLE_NAME), &prepared, &hashes)?;
    if stop == Some(SealStop::BeforeCatalog) {
        return Err(Error::new(
            "seal_interrupted",
            "stopped before the catalog record",
        ));
    }
    // The grant can expire while bytes are in flight. Recheck before the catalog write.
    grant::current_grant_with(config, plan_id, kernel)?;
    finish_catalog(config, &prepared, &hashes)?;
    Ok(view(&prepared, prepared.published.clone()))
}

fn finish_catalog(
    config: &Config,
    prepared: &Prepared,
    hashes: &[(String, u64, String)],
) -> Result<()> {
    let manifest = adoption::manifest_from_tree(&prepared.published, &prepared.selected)?;
    let fresh = hash_selected(&prepared.published, &prepared.selected)?;
    if fresh != hashes {
        return Err(conflict());
    }
    let catalog = open_catalog(config, false)?;
    match adoption::recorded_manifest(&catalog, &prepared.plan_id)? {
        Some(stored) if stored != manifest => return Err(conflict()),
        Some(_) => {}
        None => {
            let relative = adoption::relative_dir(&prepared.repo, &prepared.revision);
            adoption::record_custody(
                &catalog,
                &adoption::CustodyRecord {
                    plan_id: &prepared.plan_id,
                    repo: &prepared.repo,
                    revision: &prepared.revision,
                    provenance: PROVENANCE_RANGED_HTTPS,
                    volume_label: &prepared.volume_label,
                    relative: &relative,
                    manifest: &manifest,
                },
            )?;
        }
    }
    drop(catalog);
    remove_file_if_present(&prepared.reservation)?;
    if let Some(parent) = prepared.reservation.parent() {
        adoption::sync_dir(parent)?;
    }
    Ok(())
}

fn view(prepared: &Prepared, path: PathBuf) -> Seal {
    Seal {
        schema_version: 1,
        status: "sealed",
        plan_id: prepared.plan_id.clone(),
        repo_id: prepared.repo.clone(),
        resolved_revision: prepared.revision.clone(),
        provenance: PROVENANCE_RANGED_HTTPS,
        path,
        file_count: prepared.selected.len(),
        size_bytes: prepared.payload_bytes,
        license_id: prepared.license_id.clone(),
        transfer_authorized: false,
        workspace_bytes: None,
    }
}

fn write_reservation(prepared: &Prepared) -> Result<()> {
    let body = Reservation {
        schema_version: 1,
        plan_id: prepared.plan_id.clone(),
        policy_digest: prepared.policy_digest.clone(),
        volume_id: prepared.volume_id.clone(),
        payload_bytes: prepared.payload_bytes,
        file_count: u64::try_from(prepared.selected.len())
            .map_err(|_| Error::new("size_overflow", "selected file count exceeds u64"))?,
    };
    let bytes = serde_json::to_vec(&body)
        .map_err(|error| Error::new("output_failed", error.to_string()))?;
    if prepared.reservation.exists() {
        let stored = read_capped(&prepared.reservation, RESERVATION_CAP)?;
        if stored != bytes {
            return Err(Error::new(
                "reservation_conflict",
                "the reservation does not match this grant",
            ));
        }
        return Ok(());
    }
    if let Some(parent) = prepared.reservation.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
    }
    let temporary = prepared.reservation.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(super::io)?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(super::io)?;
    drop(file);
    fs::rename(&temporary, &prepared.reservation).map_err(super::io)?;
    if let Some(parent) = prepared.reservation.parent() {
        adoption::sync_dir(parent)?;
    }
    Ok(())
}

fn write_bundle(
    prepared: &Prepared,
    directory: &Path,
    hashes: &[(String, u64, String)],
) -> Result<()> {
    let mut files = Vec::with_capacity(hashes.len());
    for ((path, size, sha), file) in hashes.iter().zip(&prepared.selected) {
        let upstream_hash = file.upstream_hash.clone().ok_or_else(|| {
            Error::new(
                "missing_hash",
                format!("{} has no upstream identity", file.path),
            )
        })?;
        files.push(BundleFile {
            path: path.clone(),
            size_bytes: *size,
            sha256: sha.clone(),
            upstream_hash: Some(upstream_hash),
        });
    }
    let document = BundleDocument {
        schema_version: 1,
        bundle_id: prepared.plan_id.clone(),
        source: prepared.source.clone(),
        profile: prepared.layout.clone(),
        rights: BundleRights {
            license_id: prepared.license_id.clone(),
            status: "matched".into(),
        },
        files,
        excluded: prepared.excluded.clone(),
        dependencies: Vec::new(),
        provenance: PROVENANCE_RANGED_HTTPS.into(),
    };
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
    adoption::sync_dir(directory)?;
    Ok(())
}

fn copy_verified(from: &Path, to: &Path, file: &SourceFile) -> Result<String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(super::io)?;
        adoption::sync_dir(parent)?;
    }
    if let Some(sha) = adoption::matched_sha256(to, file)? {
        return Ok(sha);
    }
    if to.exists() {
        fs::remove_file(to).map_err(super::io)?;
    }
    let meta = fs::symlink_metadata(from).map_err(super::io)?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() != file.size_bytes {
        return Err(Error::new(
            "hash_mismatch",
            format!("{} hash_mismatch", file.path),
        ));
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
    adoption::matched_sha256(to, file)?.ok_or_else(|| {
        let _ = fs::remove_file(to);
        Error::new("hash_mismatch", format!("{} hash_mismatch", file.path))
    })
}

fn hash_selected(root: &Path, files: &[SourceFile]) -> Result<Vec<(String, u64, String)>> {
    let mut hashes = Vec::with_capacity(files.len());
    for file in files {
        crate::verify::metadata_nofollow(root, &file.path)?;
        let path = crate::verify::local_path(root, &file.path);
        let Some(sha) = adoption::matched_sha256(&path, file)? else {
            return Err(conflict());
        };
        hashes.push((file.path.clone(), file.size_bytes, sha));
    }
    Ok(hashes)
}

fn bundle_agrees(path: &Path, prepared: &Prepared, hashes: &[(String, u64, String)]) -> Result<()> {
    let Some(document) = read_bundle(path)? else {
        return Err(Error::new(
            "not_sealed",
            "a renamed directory is not a seal",
        ));
    };
    if document.schema_version != 1
        || document.bundle_id != prepared.plan_id
        || document.source.repo_id != prepared.source.repo_id
        || document.source.resolved_revision != prepared.source.resolved_revision
        || document.files.len() != hashes.len()
        || !(document.provenance.is_empty() || document.provenance == PROVENANCE_RANGED_HTTPS)
    {
        return Err(conflict());
    }
    let mut files = document.files;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    for (file, (path, size, sha)) in files.iter().zip(hashes) {
        if file.path != *path || file.size_bytes != *size || file.sha256 != *sha {
            return Err(conflict());
        }
    }
    Ok(())
}

pub(crate) fn read_published_bundle(path: &Path) -> Result<PublishedBundle> {
    let bytes = read_capped(path, BUNDLE_CAP)?;
    let document: BundleDocument = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("not_sealed", "the bundle manifest is missing or unreadable"))?;
    if document.schema_version != 1 || !hex_len(&document.bundle_id, 64) {
        return Err(Error::new(
            "not_sealed",
            "the bundle manifest is missing or unreadable",
        ));
    }
    if !hex_len(&document.source.resolved_revision, 40) || document.source.repo_id.is_empty() {
        return Err(Error::new(
            "not_sealed",
            "the bundle manifest is missing or unreadable",
        ));
    }
    if document.files.is_empty() || document.files.len() > 1024 {
        return Err(Error::new(
            "not_sealed",
            "the bundle manifest is missing or unreadable",
        ));
    }
    let mut files = Vec::with_capacity(document.files.len());
    for file in document.files {
        if !safe_relative(&file.path) || !hex_len(&file.sha256, 64) {
            return Err(Error::new(
                "not_sealed",
                "the bundle manifest is missing or unreadable",
            ));
        }
        files.push(PublishedFile {
            path: file.path,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(Error::new(
            "not_sealed",
            "the bundle manifest is missing or unreadable",
        ));
    }
    Ok(PublishedBundle {
        bundle_id: document.bundle_id,
        repo_id: document.source.repo_id,
        resolved_revision: document.source.resolved_revision,
        provenance: document.provenance,
        files,
    })
}

fn hex_len(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn read_bundle(path: &Path) -> Result<Option<BundleDocument>> {
    let Ok(bytes) = read_capped(path, BUNDLE_CAP) else {
        return Ok(None);
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    Ok(serde_json::from_slice(&bytes).ok())
}

fn read_capped(path: &Path, cap: u64) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).map_err(super::io)?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > cap {
        return Err(Error::new(
            "not_sealed",
            "a renamed directory is not a seal",
        ));
    }
    fs::read(path).map_err(super::io)
}

fn same_set(stage: &[SourceFile], selected: &[SourceFile]) -> bool {
    stage.len() == selected.len()
        && stage.iter().zip(selected).all(|(left, right)| {
            left.path == right.path
                && left.size_bytes == right.size_bytes
                && same_hash(&left.upstream_hash, &right.upstream_hash)
        })
}

fn same_hash(left: &Option<UpstreamHash>, right: &Option<UpstreamHash>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.algorithm == right.algorithm && left.value == right.value,
        (None, None) => true,
        _ => false,
    }
}

fn directory_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(Error::new(
            "unsafe_path",
            "symlinks and reparse points cannot own vault paths",
        )),
        Ok(meta) if meta.is_dir() => Ok(true),
        Ok(_) => Err(conflict()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(super::io(error)),
    }
}

fn remove_file_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(super::io(error)),
    }
}

pub(crate) fn checked_stage(config: &Config, stage: &Path) -> Result<PathBuf> {
    if !stage.is_absolute()
        || stage.to_str().is_none()
        || stage.components().any(|component| {
            matches!(
                component,
                std::path::Component::CurDir | std::path::Component::ParentDir
            )
        })
    {
        return Err(Error::new(
            "unsafe_staging_path",
            "use an absolute staging directory without dot segments",
        ));
    }
    let stage = if stage.exists() {
        let meta = fs::symlink_metadata(stage).map_err(super::io)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(Error::new(
                "unsafe_path",
                "staging directory must be a real directory",
            ));
        }
        fs::canonicalize(stage).map_err(super::io)?
    } else {
        let parent = stage
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .ok_or_else(|| {
                Error::new(
                    "unsafe_staging_path",
                    "use an absolute staging directory without dot segments",
                )
            })?;
        let name = stage.file_name().ok_or_else(|| {
            Error::new(
                "unsafe_staging_path",
                "use an absolute staging directory without dot segments",
            )
        })?;
        if !parent.exists() {
            return Err(Error::new(
                "path_not_found",
                format!("{}: parent is missing", stage.display()),
            ));
        }
        fs::canonicalize(parent).map_err(super::io)?.join(name)
    };
    check_path(&stage)?;
    for managed in std::iter::once(&config.catalog_path)
        .chain(config.volumes.iter().map(|volume| &volume.path))
    {
        let managed = match fs::canonicalize(managed) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => managed.clone(),
            Err(error) => return Err(super::io(error)),
        };
        if crate::config::paths_overlap(&stage, &managed) {
            return Err(Error::new(
                "overlapping_vault_paths",
                "staging directory must be outside the catalog and every vault volume",
            ));
        }
    }
    Ok(stage)
}

fn conflict() -> Error {
    Error::new(
        "destination_conflict",
        "published copy does not match the granted revision",
    )
}

#[cfg(test)]
#[path = "tests/seal.rs"]
mod tests;
