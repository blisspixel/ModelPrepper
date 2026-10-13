//! Read-only comparison of a local folder with a resolved inventory.
use crate::{
    Error, Inventory, Result,
    integrity::hash_exact,
    inventory::{HashAlgorithm, SourceFile},
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    path::Path,
};

#[derive(Debug, Serialize)]
pub struct Verification {
    pub schema_version: u32,
    pub mode: &'static str,
    pub ok: bool,
    pub repo_id: String,
    pub resolved_revision: String,
    pub files: Vec<VerifiedFile>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct VerifiedFile {
    pub path: String,
    pub status: &'static str,
}

pub fn verify_tree(
    inventory: &Inventory,
    root: &Path,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<Verification> {
    inventory.validate()?;
    let root_meta = fs::symlink_metadata(root)
        .map_err(|error| Error::new("path_not_found", format!("{}: {error}", root.display())))?;
    if root_meta.file_type().is_symlink() || !root_meta.is_dir() {
        return Err(Error::new(
            "unsafe_path",
            "local model path must be a real directory",
        ));
    }
    let mut files = Vec::new();
    let mut problems = Vec::new();
    let mut known = BTreeSet::new();
    for file in &inventory.files {
        known.insert(file.path.to_ascii_lowercase());
        let status = file_status(root, file, progress)?;
        if status != "match" && (file.required || status != "missing") {
            problems.push(format!("{} {status}", file.path));
        }
        files.push(VerifiedFile {
            path: file.path.clone(),
            status,
        });
    }
    let mut extras = Vec::new();
    walk(root, root, &mut extras)?;
    extras.sort();
    for path in extras {
        if known.contains(&path.to_ascii_lowercase()) {
            continue;
        }
        files.push(VerifiedFile {
            path,
            status: "extra",
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Verification {
        schema_version: 1,
        mode: "local_verification",
        ok: problems.is_empty(),
        repo_id: inventory.source.repo_id.clone(),
        resolved_revision: inventory.source.resolved_revision.clone(),
        message: problems.join("; "),
        files,
    })
}

fn file_status(
    root: &Path,
    file: &SourceFile,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<&'static str> {
    let Some(meta) = metadata_nofollow(root, &file.path)? else {
        return Ok("missing");
    };
    if !meta.is_file() {
        return Ok("hash_mismatch");
    }
    if meta.len() != file.size_bytes {
        return Ok("size_mismatch");
    }
    let Some(expected) = &file.upstream_hash else {
        return Ok("hash_mismatch");
    };
    let path = local_path(root, &file.path);
    let handle = File::open(&path)
        .map_err(|error| Error::new("input_read_failed", format!("{}: {error}", file.path)))?;
    let streamed = hash_exact(handle, file.size_bytes, |done| {
        progress(&file.path, done, file.size_bytes);
    })?;
    let actual = match expected.algorithm {
        HashAlgorithm::Sha256 => streamed.sha256,
        HashAlgorithm::GitSha1 => streamed.git_sha1,
    };
    Ok(if actual == expected.value {
        "match"
    } else {
        "hash_mismatch"
    })
}

pub(crate) fn local_path(root: &Path, rel: &str) -> std::path::PathBuf {
    rel.split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part))
}

fn metadata_nofollow(root: &Path, rel: &str) -> Result<Option<fs::Metadata>> {
    let mut current = root.to_path_buf();
    let parts: Vec<&str> = rel.split('/').filter(|part| !part.is_empty()).collect();
    for (index, part) in parts.iter().enumerate() {
        current.push(part);
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(Error::new("input_read_failed", format!("{rel}: {error}")));
            }
        };
        if meta.file_type().is_symlink() {
            return Err(Error::new(
                "unsafe_path",
                format!("{rel} is a symlink and was not followed"),
            ));
        }
        let last = index + 1 == parts.len();
        if last {
            return Ok(Some(meta));
        }
        if !meta.is_dir() {
            return Ok(Some(meta));
        }
    }
    Ok(None)
}

fn walk(root: &Path, dir: &Path, files: &mut Vec<String>) -> Result<()> {
    for entry in fs::read_dir(dir)
        .map_err(|error| Error::new("input_read_failed", format!("{}: {error}", dir.display())))?
    {
        let entry = entry.map_err(|error| Error::new("input_read_failed", error.to_string()))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)
            .map_err(|error| Error::new("input_read_failed", error.to_string()))?;
        let rel = path
            .strip_prefix(root)
            .map_err(|_| Error::new("unsafe_path", "walked outside the local model directory"))?;
        let rel = rel
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if meta.file_type().is_symlink() || meta.is_file() {
            files.push(rel);
        } else if meta.is_dir() {
            walk(root, &path, files)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{EvidenceStatus, LicenseEvidence, Source, UpstreamHash};
    use std::io::Read;
    use tempfile::TempDir;

    fn inventory(files: Vec<SourceFile>) -> Inventory {
        Inventory {
            schema_version: 1,
            source: Source {
                endpoint: "https://huggingface.co".into(),
                repo_type: "model".into(),
                repo_id: "owner/model".into(),
                requested_revision: "main".into(),
                resolved_revision: "0123456789abcdef0123456789abcdef01234567".into(),
                gated: false,
                private: false,
            },
            complete: true,
            layout: "transformers-safetensors-v1".into(),
            license: LicenseEvidence {
                license_id: "mit".into(),
                status: EvidenceStatus::Matched,
                paths: vec!["LICENSE".into()],
            },
            files,
        }
    }

    fn file(path: &str, bytes: &[u8], algorithm: HashAlgorithm, required: bool) -> SourceFile {
        let streamed = hash_exact(bytes, bytes.len() as u64, |_| {}).unwrap();
        let value = match algorithm {
            HashAlgorithm::Sha256 => streamed.sha256,
            HashAlgorithm::GitSha1 => streamed.git_sha1,
        };
        SourceFile {
            path: path.into(),
            size_bytes: bytes.len() as u64,
            upstream_hash: Some(UpstreamHash { algorithm, value }),
            role: if path.ends_with(".safetensors") {
                crate::inventory::Role::Weight
            } else if path == "LICENSE" {
                crate::inventory::Role::License
            } else {
                crate::inventory::Role::Config
            },
            required,
        }
    }

    #[test]
    fn matches_required_files_and_names_damage_or_absence() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("model.safetensors"), b"weights").unwrap();
        fs::write(dir.path().join("LICENSE"), b"license").unwrap();
        fs::write(dir.path().join("notes.txt"), b"extra").unwrap();
        let before = fs::read(dir.path().join("model.safetensors")).unwrap();
        let good = inventory(vec![
            file("model.safetensors", b"weights", HashAlgorithm::Sha256, true),
            file("LICENSE", b"license", HashAlgorithm::GitSha1, true),
            file("README.md", b"optional", HashAlgorithm::GitSha1, false),
        ]);
        let mut updates = 0_u32;
        let report = verify_tree(&good, dir.path(), &mut |_, _, _| updates += 1).unwrap();
        assert!(report.ok, "{report:?}");
        assert!(report.message.is_empty());
        assert!(updates >= 2);
        assert!(
            report
                .files
                .iter()
                .any(|file| file.path == "notes.txt" && file.status == "extra")
        );
        assert!(
            report
                .files
                .iter()
                .any(|file| file.path == "README.md" && file.status == "missing")
        );
        assert_eq!(
            fs::read(dir.path().join("model.safetensors")).unwrap(),
            before
        );

        let mut flipped = b"weights".to_vec();
        flipped[0] = b'W';
        fs::write(dir.path().join("model.safetensors"), &flipped).unwrap();
        let damaged = verify_tree(&good, dir.path(), &mut |_, _, _| {}).unwrap();
        assert!(!damaged.ok);
        assert!(damaged.message.contains("model.safetensors"));
        assert!(damaged.message.contains("hash_mismatch"));

        fs::remove_file(dir.path().join("model.safetensors")).unwrap();
        let missing = verify_tree(&good, dir.path(), &mut |_, _, _| {}).unwrap();
        assert!(!missing.ok);
        assert!(missing.message.contains("model.safetensors missing"));
    }

    #[test]
    fn hashing_streams_in_bounded_chunks_and_reports_progress() {
        let data = vec![7_u8; 70_000];
        let mut reads = Vec::new();
        let mut reader = ChunkWatch {
            inner: data.as_slice(),
            reads: &mut reads,
        };
        let mut seen = Vec::new();
        let hashed = hash_exact(&mut reader, data.len() as u64, |done| seen.push(done)).unwrap();
        assert!(reads.iter().all(|size| *size <= 64 * 1024));
        assert!(reads.len() >= 2);
        assert!(seen.len() >= 2);
        assert_eq!(*seen.last().unwrap(), data.len() as u64);
        assert_eq!(hashed.sha256.len(), 64);
    }

    struct ChunkWatch<'a> {
        inner: &'a [u8],
        reads: &'a mut Vec<usize>,
    }

    impl Read for ChunkWatch<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reads.push(buf.len());
            self.inner.read(buf)
        }
    }
}
