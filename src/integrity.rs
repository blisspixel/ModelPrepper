//! Upstream Git blob identities are hashes of an object header and its bytes.
use crate::{
    Error, Result,
    inventory::{HashAlgorithm, SourceFile},
};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::io::Read;

/// Streams without buffering a model in memory; always computes a local SHA-256.
pub fn verify_reader(mut reader: impl Read, file: &SourceFile) -> Result<String> {
    let expected = file
        .upstream_hash
        .as_ref()
        .ok_or_else(|| Error::new("missing_hash", "upstream file identity is required"))?;
    let mut sha256 = Sha256::new();
    let mut git = Sha1::new();
    git.update(format!("blob {}\0", file.size_bytes).as_bytes());
    let mut count = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let length = reader
            .read(&mut buffer)
            .map_err(|_| Error::new("read_failed", "could not read file bytes"))?;
        if length == 0 {
            break;
        }
        count = count
            .checked_add(length as u64)
            .ok_or_else(|| Error::new("size_mismatch", "file exceeds expected length"))?;
        if count > file.size_bytes {
            return Err(Error::new("size_mismatch", "file exceeds expected length"));
        }
        sha256.update(&buffer[..length]);
        git.update(&buffer[..length]);
    }
    if count != file.size_bytes {
        return Err(Error::new("size_mismatch", "file is shorter than expected"));
    }
    let local = format!("{:x}", sha256.finalize());
    let identity = match expected.algorithm {
        HashAlgorithm::Sha256 => local.clone(),
        HashAlgorithm::GitSha1 => format!("{:x}", git.finalize()),
    };
    if identity != expected.value {
        return Err(Error::new(
            "hash_mismatch",
            "file bytes do not match upstream identity",
        ));
    }
    Ok(local)
}
