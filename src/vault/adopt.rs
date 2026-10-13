//! Copy or move a local folder that matches a pending, upstream-resolved review.
use super::{
    check_path, db, io, open_catalog, report,
    review::{Decision, load},
};
use crate::{
    Config, Error, Result,
    integrity::hash_exact,
    inventory::{EvidenceStatus, HashAlgorithm, SourceFile},
    verify::{self, Verification},
};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const PROVENANCE_LOCAL_UPSTREAM: &str = "upstream-verified (local source)";

#[derive(Debug, Serialize)]
pub struct Adoption {
    pub schema_version: u32,
    pub status: &'static str,
    pub repo_id: String,
    pub resolved_revision: String,
    pub provenance: &'static str,
    pub path: PathBuf,
    pub moved: bool,
}

#[derive(Debug)]
pub enum Outcome {
    Recorded(Adoption),
    Mismatch(Verification),
}

pub fn adopt(
    config: &Config,
    plan_id: &str,
    source: &Path,
    move_source: bool,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<Outcome> {
    adopt_stopping(config, plan_id, source, move_source, progress, None)
}

pub(crate) fn adopt_stopping(
    config: &Config,
    plan_id: &str,
    source: &Path,
    move_source: bool,
    progress: &mut dyn FnMut(&str, u64, u64),
    stop_after_files: Option<usize>,
) -> Result<Outcome> {
    let catalog = open_catalog(config, false)?;
    let proposal = load(&catalog, config, plan_id)?;
    if proposal.decision != Decision::Pending {
        return Err(Error::new(
            "proposal_not_pending",
            "adoption requires a pending source review",
        ));
    }
    let inventory = proposal.review.inventory.clone();
    let evidence = proposal.review.evidence.clone();
    if inventory.license.status != EvidenceStatus::Matched || inventory.license.paths.is_empty() {
        return Err(Error::new(
            "license_needs_review",
            "adoption cannot use a resolved commit that lacks matched license evidence",
        ));
    }
    if !inventory.complete {
        return Err(Error::new(
            "incomplete_inventory",
            "adoption needs the complete resolved inventory",
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
    let volume_path = fs::canonicalize(&volume.path).map_err(io)?;
    let volume_label = volume.label.clone();
    let repo = inventory.source.repo_id.clone();
    let revision = inventory.source.resolved_revision.clone();
    let published = publish_dir(&volume_path, &repo, &revision);
    let staging_root = volume_path.join(".modelprepper-adopt");
    let staging = staging_root.join(plan_id);
    let journal = staging_root.join(format!("{plan_id}.json"));
    let source = checked_source(config, source)?;

    if published.exists() {
        let check = verify::verify_tree(&inventory, &published, progress)?;
        if !check.ok {
            return Err(Error::new(
                "destination_conflict",
                format!(
                    "published copy does not match the resolved commit: {}",
                    check.message
                ),
            ));
        }
        let manifest = manifest_from_tree(&published, &inventory.files)?;
        record(
            &catalog,
            plan_id,
            &repo,
            &revision,
            &volume_label,
            &relative_dir(&repo, &revision),
            &manifest,
        )?;
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_file(&journal);
        if move_source {
            remove_sources(&source, &published, &inventory.files)?;
        }
        return Ok(recorded(&repo, &revision, published, move_source));
    }

    for item in &evidence {
        let mismatch = || {
            Error::new(
                "evidence_mismatch",
                format!("{} does not match captured license evidence", item.path),
            )
        };
        let size = item.text.len() as u64;
        let meta = verify::metadata_nofollow(&source, &item.path)?.ok_or_else(mismatch)?;
        if !meta.is_file() || meta.len() != size {
            return Err(mismatch());
        }
        let input = File::open(verify::local_path(&source, &item.path)).map_err(|_| mismatch())?;
        let hashed = hash_exact(input, size, |_| {}).map_err(|_| mismatch())?;
        if hashed.sha256 != item.sha256 {
            return Err(Error::new(
                "evidence_mismatch",
                format!("{} does not match captured license evidence", item.path),
            ));
        }
    }

    let verification = verify::verify_tree(&inventory, &source, progress)?;
    if !verification.ok {
        rollback(&staging, &journal);
        return Ok(Outcome::Mismatch(verification));
    }

    if let Err(error) = stage_files(
        &source,
        &staging,
        &journal,
        plan_id,
        &inventory.files,
        progress,
        stop_after_files,
    ) {
        if error.code != "adopt_interrupted" {
            rollback(&staging, &journal);
        }
        return Err(error);
    }
    let staged = verify::verify_tree(&inventory, &staging, progress)?;
    if !staged.ok {
        rollback(&staging, &journal);
        return Ok(Outcome::Mismatch(staged));
    }
    if let Some(parent) = published.parent() {
        fs::create_dir_all(parent).map_err(io)?;
        sync_dir(parent)?;
    }
    fs::rename(&staging, &published).map_err(io)?;
    if let Some(parent) = published.parent() {
        sync_dir(parent)?;
    }
    // The published tree is already durable. A later read failure must not delete it.
    let manifest = manifest_from_tree(&published, &inventory.files)?;
    record(
        &catalog,
        plan_id,
        &repo,
        &revision,
        &volume_label,
        &relative_dir(&repo, &revision),
        &manifest,
    )?;
    let _ = fs::remove_file(&journal);
    if move_source {
        remove_sources(&source, &published, &inventory.files)?;
    }
    Ok(recorded(&repo, &revision, published, move_source))
}

fn recorded(repo: &str, revision: &str, path: PathBuf, moved: bool) -> Outcome {
    Outcome::Recorded(Adoption {
        schema_version: 1,
        status: "adopted",
        repo_id: repo.to_owned(),
        resolved_revision: revision.to_owned(),
        provenance: PROVENANCE_LOCAL_UPSTREAM,
        path,
        moved,
    })
}

fn stage_files(
    source: &Path,
    staging: &Path,
    journal: &Path,
    plan_id: &str,
    files: &[SourceFile],
    progress: &mut dyn FnMut(&str, u64, u64),
    stop_after_files: Option<usize>,
) -> Result<()> {
    fs::create_dir_all(staging).map_err(io)?;
    let mut done = Vec::new();
    for (index, file) in files.iter().enumerate() {
        if stop_after_files == Some(index) {
            write_journal(journal, plan_id, &done)?;
            return Err(Error::new(
                "adopt_interrupted",
                "stopped before publication",
            ));
        }
        copy_one(source, staging, file, progress)?;
        done.push(file.path.clone());
        write_journal(journal, plan_id, &done)?;
    }
    Ok(())
}

fn copy_one(
    source: &Path,
    staging: &Path,
    file: &SourceFile,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<()> {
    let from = verify::local_path(source, &file.path);
    let to = verify::local_path(staging, &file.path);
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    if destination_matches(&to, file)?.is_some() {
        progress(&file.path, file.size_bytes, file.size_bytes);
        return Ok(());
    }
    let partial = partial_path(&to);
    if partial.exists() {
        fs::remove_file(&partial).map_err(io)?;
    }
    let mut input = File::open(&from).map_err(io)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .map_err(io)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut count = 0_u64;
    loop {
        let length = input.read(&mut buffer).map_err(io)?;
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
        output.write_all(&buffer[..length]).map_err(io)?;
        progress(&file.path, count, file.size_bytes);
    }
    if count != file.size_bytes {
        drop(output);
        let _ = fs::remove_file(&partial);
        return Err(Error::new(
            "size_mismatch",
            format!("{} size_mismatch", file.path),
        ));
    }
    output.sync_all().map_err(io)?;
    drop(output);
    fs::rename(&partial, &to).map_err(io)?;
    if let Some(parent) = to.parent() {
        sync_dir(parent)?;
    }
    if destination_matches(&to, file)?.is_none() {
        let _ = fs::remove_file(&to);
        return Err(Error::new(
            "hash_mismatch",
            format!("{} hash_mismatch", file.path),
        ));
    }
    Ok(())
}

fn destination_matches(path: &Path, file: &SourceFile) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let meta = fs::symlink_metadata(path).map_err(io)?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() != file.size_bytes {
        fs::remove_file(path).map_err(io)?;
        return Ok(None);
    }
    let expected = file.upstream_hash.as_ref().ok_or_else(|| {
        Error::new(
            "missing_hash",
            format!("{} has no upstream identity", file.path),
        )
    })?;
    let hashed = hash_exact(File::open(path).map_err(io)?, file.size_bytes, |_| {})?;
    let actual = match expected.algorithm {
        HashAlgorithm::Sha256 => hashed.sha256.clone(),
        HashAlgorithm::GitSha1 => hashed.git_sha1.clone(),
    };
    if actual != expected.value {
        fs::remove_file(path).map_err(io)?;
        return Ok(None);
    }
    Ok(Some(hashed.sha256))
}

fn manifest_from_tree(root: &Path, files: &[SourceFile]) -> Result<String> {
    let mut entries = Vec::new();
    for file in files {
        let sha256 = destination_matches(&verify::local_path(root, &file.path), file)?
            .ok_or_else(|| Error::new("hash_mismatch", format!("{} hash_mismatch", file.path)))?;
        entries.push(serde_json::json!({
            "path": file.path,
            "sha256": sha256,
            "size_bytes": file.size_bytes,
        }));
    }
    serde_json::to_string(&entries).map_err(|error| Error::new("output_failed", error.to_string()))
}

fn record(
    catalog: &super::Catalog,
    plan_id: &str,
    repo: &str,
    revision: &str,
    volume_label: &str,
    relative: &str,
    manifest: &str,
) -> Result<()> {
    catalog
        .db
        .execute(
            "INSERT INTO custody (plan_id, repo_id, resolved_revision, provenance, volume_label, relative_dir, manifest) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(plan_id) DO NOTHING",
            rusqlite::params![
                plan_id,
                repo,
                revision,
                PROVENANCE_LOCAL_UPSTREAM,
                volume_label,
                relative,
                manifest
            ],
        )
        .map_err(db)?;
    Ok(())
}

fn checked_source(config: &Config, source: &Path) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(source)
        .map_err(|error| Error::new("path_not_found", format!("{}: {error}", source.display())))?;
    let linked = meta.file_type().is_symlink();
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        linked || meta.file_attributes() & 0x400 != 0
    };
    if linked || !meta.is_dir() {
        return Err(Error::new(
            "unsafe_path",
            "local source must be a real directory",
        ));
    }
    let source = fs::canonicalize(source)
        .map_err(|error| Error::new("path_not_found", format!("{}: {error}", source.display())))?;
    check_path(&source)?;
    let source_alias = PathBuf::from(source.to_string_lossy().to_lowercase());
    for managed in
        std::iter::once(&config.catalog_path).chain(config.volumes.iter().map(|v| &v.path))
    {
        // Offline volumes still reserve their configured paths.
        let managed = match fs::canonicalize(managed) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => managed.clone(),
            Err(error) => return Err(io(error)),
        };
        let managed_alias = PathBuf::from(managed.to_string_lossy().to_lowercase());
        if source_alias.starts_with(&managed_alias) || managed_alias.starts_with(&source_alias) {
            return Err(Error::new(
                "overlapping_vault_paths",
                "local source must be outside the catalog and every vault volume",
            ));
        }
    }
    Ok(source)
}

fn removable(source: &Path, published: &Path, file: &SourceFile) -> Result<bool> {
    if verify::metadata_nofollow(source, &file.path)?.is_none() {
        return Ok(false); // A prior successful cleanup may have removed this entry.
    }
    // Check the source last so a potentially large destination read does not
    // separate source verification from removal.
    if verify::file_status(published, file, &mut |_, _, _| {})? != "match"
        || verify::file_status(source, file, &mut |_, _, _| {})? != "match"
    {
        return Err(Error::new(
            "source_remove_failed",
            format!(
                "{} must match its preserved upstream copy before removal",
                file.path
            ),
        ));
    }
    Ok(true)
}

fn remove_sources(source: &Path, published: &Path, files: &[SourceFile]) -> Result<()> {
    // Preflight every remaining file before deleting any; preserve partial retries.
    for file in files {
        removable(source, published, file)?;
    }
    for file in files {
        if removable(source, published, file)? {
            let path = verify::local_path(source, &file.path);
            fs::remove_file(&path).map_err(|error| {
                Error::new("source_remove_failed", format!("{}: {error}", file.path))
            })?;
        }
    }
    Ok(())
}

fn write_journal(path: &Path, plan_id: &str, files: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    let body = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "plan_id": plan_id,
        "files": files,
    }))
    .map_err(|error| Error::new("output_failed", error.to_string()))?;
    let temporary = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(io)?;
    file.write_all(&body)
        .and_then(|()| file.sync_all())
        .map_err(io)?;
    fs::rename(&temporary, path).map_err(io)?;
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

fn rollback(staging: &Path, journal: &Path) {
    let _ = fs::remove_dir_all(staging);
    let _ = fs::remove_file(journal);
}

fn publish_dir(volume: &Path, repo: &str, revision: &str) -> PathBuf {
    let mut path = volume.join("preserved");
    for part in repo.split('/') {
        path.push(part);
    }
    path.push(revision);
    path
}

fn relative_dir(repo: &str, revision: &str) -> String {
    format!("preserved/{repo}/{revision}")
}

fn partial_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.partial"))
}

fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path).map_err(io)?.sync_all().map_err(io)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
#[path = "tests/adopt.rs"]
mod tests;
