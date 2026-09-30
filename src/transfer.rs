//! Restart-focused transport experiment. No approval, accounting, or publication.
use crate::{
    Error, Result,
    http::Http,
    integrity::verify_reader,
    inventory::{HashAlgorithm, Source, SourceFile},
    plan::digest,
    validation::{normalize_repo, validate_file_path, validate_hex},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};
use url::Url;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub schema_version: u32,
    pub identity: String,
    pub bytes: u64,
    pub prefix_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct Segment {
    pub checkpoint: Checkpoint,
    pub received_bytes: u64,
    pub verified_sha256: Option<String>,
}

fn disk(error: std::io::Error) -> Error {
    Error::new("staging_io_failed", error.to_string())
}

/// Caller owns the open staging file exclusively and durably journals the returned
/// checkpoint. This library primitive does not create a preserved model or seal.
pub fn stage_segment(
    source: &Source,
    expected: &SourceFile,
    file: &mut File,
    checkpoint: Option<&Checkpoint>,
    max_new_bytes: u64,
) -> Result<Segment> {
    stage_with(
        &Http::default(),
        "https://huggingface.co",
        source,
        expected,
        file,
        checkpoint,
        max_new_bytes,
    )
}

fn prefix(file: &mut File, length: u64) -> Result<String> {
    file.seek(SeekFrom::Start(0)).map_err(disk)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut left = length;
    while left > 0 {
        let requested = left.min(buffer.len() as u64) as usize;
        let count = file.read(&mut buffer[..requested]).map_err(disk)?;
        if count == 0 {
            return Err(Error::new(
                "invalid_checkpoint",
                "staging file is shorter than its checkpoint",
            ));
        }
        hash.update(&buffer[..count]);
        left -= count as u64;
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn stage_with(
    http: &Http,
    base: &str,
    source: &Source,
    expected: &SourceFile,
    file: &mut File,
    checkpoint: Option<&Checkpoint>,
    max_new_bytes: u64,
) -> Result<Segment> {
    if source.endpoint != "https://huggingface.co"
        || source.repo_type != "model"
        || source.gated
        || source.private
    {
        return Err(Error::new(
            "unsupported_source",
            "only public ungated official model sources are supported",
        ));
    }
    let repo = normalize_repo(&source.repo_id)?;
    validate_hex(&source.resolved_revision, 40)?;
    validate_file_path(&expected.path)?;
    let hash = expected
        .upstream_hash
        .as_ref()
        .ok_or_else(|| Error::new("missing_hash", "upstream identity is required"))?;
    validate_hex(
        &hash.value,
        if hash.algorithm == HashAlgorithm::Sha256 {
            64
        } else {
            40
        },
    )?;
    if max_new_bytes == 0 {
        return Err(Error::new(
            "invalid_limit",
            "segment allowance must be positive",
        ));
    }
    let identity = digest(
        &serde_json::to_vec(&("modelprepper.transfer-file.v1", source, expected))
            .map_err(|e| Error::new("invalid_inventory", e.to_string()))?,
    );
    let offset = file.metadata().map_err(disk)?.len();
    if offset > expected.size_bytes {
        return Err(Error::new(
            "invalid_checkpoint",
            "staging file exceeds expected size",
        ));
    }
    let prior_hash = prefix(file, offset)?;
    if let Some(checkpoint) = checkpoint {
        if checkpoint.schema_version != 1
            || checkpoint.identity != identity
            || checkpoint.bytes != offset
            || checkpoint.prefix_sha256 != prior_hash
        {
            return Err(Error::new(
                "invalid_checkpoint",
                "checkpoint identity or partial bytes changed",
            ));
        }
    } else if offset != 0 {
        return Err(Error::new(
            "invalid_checkpoint",
            "nonempty staging files require an identity-bound checkpoint",
        ));
    }
    let remaining = expected.size_bytes - offset;
    let count = remaining.min(max_new_bytes);
    if count > 0 {
        let end = offset + count - 1;
        let mut url = Url::parse(base).expect("fixed or fixture HTTP origin");
        url.path_segments_mut()
            .expect("HTTP origin")
            .extend(repo.split('/'))
            .extend(["resolve", &source.resolved_revision])
            .extend(expected.path.split('/'));
        let mut response = http.open(&url, Some((offset, end)))?;
        let status = response.status().as_u16();
        if status == 206 {
            let range = format!("bytes {offset}-{end}/{}", expected.size_bytes);
            if response
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                != Some(range.as_str())
            {
                return Err(Error::new(
                    "invalid_range",
                    "response range differs from the pinned segment",
                ));
            }
        } else if status != 200 || offset != 0 || count != expected.size_bytes {
            return Err(Error::new(
                "range_not_honored",
                "source did not return the requested segment",
            ));
        }
        if response
            .headers()
            .get("content-length")
            .is_some_and(|v| v.to_str().ok().and_then(|s| s.parse::<u64>().ok()) != Some(count))
        {
            return Err(Error::new(
                "invalid_range",
                "response length differs from requested segment",
            ));
        }
        file.seek(SeekFrom::Start(offset)).map_err(disk)?;
        let result = (|| {
            let mut reader = response.body_mut().as_reader();
            let mut left = count;
            let mut buffer = [0; 64 * 1024];
            while left > 0 {
                let requested = left.min(buffer.len() as u64) as usize;
                let bytes = reader.read(&mut buffer[..requested]).map_err(|_| {
                    Error::new("source_read_failed", "segment body was interrupted")
                })?;
                if bytes == 0 {
                    return Err(Error::new("size_mismatch", "segment body ended early"));
                }
                file.write_all(&buffer[..bytes]).map_err(disk)?;
                left -= bytes as u64;
            }
            let mut extra = [0];
            if reader
                .read(&mut extra)
                .map_err(|_| Error::new("source_read_failed", "segment body was interrupted"))?
                != 0
            {
                return Err(Error::new(
                    "size_mismatch",
                    "segment body exceeds requested length",
                ));
            }
            Ok(())
        })();
        if let Err(error) = result {
            file.set_len(offset)
                .and_then(|()| file.sync_all())
                .map_err(disk)?;
            return Err(error);
        }
        file.sync_all().map_err(disk)?;
    }
    let length = offset + count;
    let local = prefix(file, length)?;
    let verified = if length == expected.size_bytes {
        file.seek(SeekFrom::Start(0)).map_err(disk)?;
        match verify_reader(&mut *file, expected) {
            Ok(hash) => Some(hash),
            Err(error) => {
                file.set_len(offset)
                    .and_then(|()| file.sync_all())
                    .map_err(disk)?;
                return Err(error);
            }
        }
    } else {
        None
    };
    Ok(Segment {
        checkpoint: Checkpoint {
            schema_version: 1,
            identity,
            bytes: length,
            prefix_sha256: local,
        },
        received_bytes: count,
        verified_sha256: verified,
    })
}

#[cfg(test)]
mod tests;
