//! Stage every file of one frozen set. Each file keeps the single-file
//! checkpoint rules. This does not seal a revision and it is not a fetch command.
use super::{Segment, disk};
use crate::{
    Error, Result,
    http::Http,
    inventory::{Source, SourceFile},
    transfer::session::StagingSession,
    vault::check_path,
    writer_lock::WriterLock,
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

const MAX_FILES: usize = 1024;
const MANIFEST_CAP: u64 = 1024 * 1024;
const OWNED: &[&str] = &["writer.lock", "members.json", "members"];

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberSet {
    schema_version: u32,
    source: Source,
    files: Vec<SourceFile>,
}

/// One segment of one file in a frozen set. `staged` is true only when every
/// member checkpoint already covers that file. The part files stay part files.
#[derive(Debug)]
pub struct RevisionSegment {
    pub path: String,
    pub segment: Segment,
    pub staged: bool,
}

/// Exclusive staging for every file of one frozen set, including license and
/// notice text when those files are members. Not a seal and not a command.
pub struct RevisionStage {
    members: Vec<StagingSession>,
    _writer: WriterLock,
    files: Vec<SourceFile>,
}

impl RevisionStage {
    /// Create a directory of single-file sessions. Interrupted initialization
    /// is left in place and is not adopted.
    pub fn create(root: &Path, source: &Source, files: &[SourceFile]) -> Result<Self> {
        let files = prepare(source, files)?;
        set_location(root)?;
        fs::create_dir(root).map_err(disk)?;
        let writer = lock(root, true)?;
        fs::create_dir(root.join("members")).map_err(disk)?;
        let mut members = Vec::with_capacity(files.len());
        for (index, file) in files.iter().enumerate() {
            members.push(StagingSession::create(
                &member_path(root, index),
                source,
                file,
            )?);
        }
        write_manifest(root, source, &files)?;
        sync_directory(root)?;
        sync_directory(root.parent().ok_or_else(invalid)?)?;
        Ok(Self {
            members,
            _writer: writer,
            files,
        })
    }

    /// Reopen the same frozen set and reconcile each uncommitted tail before
    /// any request. A changed file list is rejected.
    pub fn open(root: &Path, source: &Source, files: &[SourceFile]) -> Result<Self> {
        let files = prepare(source, files)?;
        set_location(root)?;
        let writer = lock(root, false)?;
        let stored = read_manifest(root)?;
        if stored != canonical(source, &files)? {
            return Err(Error::new(
                "revision_set_changed",
                "the frozen set does not match this staging directory",
            ));
        }
        check_member_dirs(root, files.len())?;
        let mut members = Vec::with_capacity(files.len());
        for (index, file) in files.iter().enumerate() {
            members.push(StagingSession::open(
                &member_path(root, index),
                source,
                file,
            )?);
        }
        Ok(Self {
            members,
            _writer: writer,
            files,
        })
    }

    /// Allowances charged before requests, summed across the set. Recovery does
    /// not reduce the sum.
    pub fn spent_bytes(&self) -> Result<u64> {
        self.members.iter().try_fold(0u64, |sum, member| {
            sum.checked_add(member.spent_bytes()?).ok_or_else(|| {
                Error::new(
                    "size_overflow",
                    "staged transfer spending exceeds u64 capacity",
                )
            })
        })
    }

    /// Bytes discarded from uncommitted tails. This is not a refund.
    pub fn recovered_bytes(&self) -> Result<u64> {
        self.members.iter().try_fold(0u64, |sum, member| {
            sum.checked_add(member.recovered_bytes())
                .ok_or_else(|| Error::new("size_overflow", "staged recovery exceeds u64 capacity"))
        })
    }

    /// Every member checkpoint covers its file. The directory is still staging.
    pub fn is_staged(&self) -> Result<bool> {
        for (file, member) in self.files.iter().zip(&self.members) {
            let bytes = member.checkpoint()?.bytes;
            if bytes > file.size_bytes {
                return Err(invalid());
            }
            if bytes != file.size_bytes {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Stage the next incomplete member. A finished set makes no request.
    pub fn stage_next(&mut self, allowance: u64) -> Result<RevisionSegment> {
        self.stage_next_with(&Http::default(), "https://huggingface.co", allowance)
    }

    pub(crate) fn stage_next_with(
        &mut self,
        http: &Http,
        base: &str,
        allowance: u64,
    ) -> Result<RevisionSegment> {
        if allowance == 0 {
            return Err(Error::new(
                "invalid_limit",
                "segment allowance must be positive",
            ));
        }
        let mut next = None;
        for (index, file) in self.files.iter().enumerate() {
            let bytes = self.members[index].checkpoint()?.bytes;
            if bytes > file.size_bytes {
                return Err(invalid());
            }
            if bytes < file.size_bytes {
                next = Some(index);
                break;
            }
        }
        let Some(index) = next else {
            return Err(Error::new(
                "revision_already_staged",
                "every file in the frozen set is already staged",
            ));
        };
        let segment = self.members[index].stage_with(http, base, allowance)?;
        let path = self.files[index].path.clone();
        let staged = self.is_staged()?;
        Ok(RevisionSegment {
            path,
            segment,
            staged,
        })
    }
}

fn prepare(source: &Source, files: &[SourceFile]) -> Result<Vec<SourceFile>> {
    if files.is_empty() {
        return Err(Error::new(
            "empty_revision",
            "a frozen set names at least one file",
        ));
    }
    if files.len() > MAX_FILES {
        return Err(Error::new(
            "too_many_files",
            "a staged revision is limited to 1024 files",
        ));
    }
    let mut files = files.to_vec();
    files.sort_by(|left, right| left.path.cmp(&right.path));
    if files.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(Error::new(
            "duplicate_path",
            "a frozen set names each path once",
        ));
    }
    for file in &files {
        super::file_identity(source, file)?;
    }
    Ok(files)
}

fn member_path(root: &Path, index: usize) -> std::path::PathBuf {
    root.join("members").join(format!("{index:04}"))
}

fn canonical(source: &Source, files: &[SourceFile]) -> Result<Vec<u8>> {
    serde_json::to_vec(&MemberSet {
        schema_version: 1,
        source: source.clone(),
        files: files.to_vec(),
    })
    .map_err(|_| invalid())
}

fn write_manifest(root: &Path, source: &Source, files: &[SourceFile]) -> Result<()> {
    let bytes = canonical(source, files)?;
    let path = root.join("members.json");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(disk)?;
    file.write_all(&bytes).map_err(disk)?;
    file.sync_all().map_err(disk)?;
    Ok(())
}

fn read_manifest(root: &Path) -> Result<Vec<u8>> {
    let path = root.join("members.json");
    let len = fs::metadata(&path).map_err(disk)?.len();
    if len > MANIFEST_CAP {
        return Err(invalid());
    }
    let bytes = fs::read(&path).map_err(disk)?;
    serde_json::from_slice::<MemberSet>(&bytes).map_err(|_| invalid())?;
    Ok(bytes)
}

fn check_member_dirs(root: &Path, count: usize) -> Result<()> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root.join("members")).map_err(disk)? {
        let entry = entry.map_err(disk)?;
        if !entry.file_type().map_err(disk)?.is_dir() {
            return Err(invalid());
        }
        names.push(entry.file_name());
    }
    names.sort();
    let expected: Vec<_> = (0..count)
        .map(|index| std::ffi::OsString::from(format!("{index:04}")))
        .collect();
    if names != expected {
        return Err(invalid());
    }
    Ok(())
}

fn lock(root: &Path, create: bool) -> Result<WriterLock> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if create {
        options.create_new(true);
    }
    let file = options.open(root.join("writer.lock")).map_err(disk)?;
    WriterLock::acquire(file)
        .map_err(|_| Error::new("writer_busy", "another process owns this staging session"))
}

fn invalid() -> Error {
    Error::new(
        "invalid_revision_stage",
        "retain the staging directory; its frozen set cannot be trusted",
    )
}

fn set_location(root: &Path) -> Result<()> {
    if !root.is_absolute()
        || root.to_str().is_none()
        || root
            .to_string_lossy()
            .split(['/', '\\'])
            .any(|part| matches!(part, "." | ".."))
    {
        return Err(Error::new(
            "unsafe_staging_path",
            "use an absolute staging directory without dot segments",
        ));
    }
    #[cfg(windows)]
    if root.components().any(|component| {
        matches!(component, std::path::Component::Prefix(prefix)
            if matches!(prefix.kind(), std::path::Prefix::UNC(_, _) | std::path::Prefix::VerbatimUNC(_, _)))
    }) {
        return Err(Error::new(
            "unsupported_staging_filesystem",
            "the checkpoint SQLite journal requires a local filesystem",
        ));
    }
    check_path(root)?;
    for name in OWNED {
        check_path(&root.join(name))?;
    }
    if root.exists() {
        for entry in fs::read_dir(root).map_err(disk)? {
            let entry = entry.map_err(disk)?;
            if !OWNED.iter().any(|name| entry.file_name() == *name) {
                return Err(Error::new(
                    "staging_directory_not_owned",
                    "staging never adopts unrelated entries",
                ));
            }
        }
        if root.join("members").exists() && !root.join("members").is_dir() {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Disk beyond the payload parts for every member of one frozen set.
/// Each member keeps its own checkpoint database and rollback journal, and the
/// set keeps one manifest. This is not `workspace_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RevisionDiskBound {
    pub side_file_bytes: u64,
    pub manifest_bytes: u64,
}

pub(crate) fn revision_disk_bound(file_count: usize) -> Result<RevisionDiskBound> {
    if file_count == 0 {
        return Err(Error::new(
            "empty_revision",
            "a frozen set names at least one file",
        ));
    }
    if file_count > MAX_FILES {
        return Err(Error::new(
            "too_many_files",
            "a staged revision is limited to 1024 files",
        ));
    }
    let side_file_bytes = super::staging_workspace_bound()
        .checked_mul(file_count as u64)
        .ok_or_else(|| Error::new("size_overflow", "staged side files exceed u64 capacity"))?;
    Ok(RevisionDiskBound {
        side_file_bytes,
        manifest_bytes: MANIFEST_CAP,
    })
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)
        .map_err(disk)?
        .sync_all()
        .map_err(disk)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
#[path = "tests/revision.rs"]
mod tests;
