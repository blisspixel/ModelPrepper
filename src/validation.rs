use crate::{Error, Result};
use std::collections::BTreeSet;

/// Only plain, portable ASCII paths are admitted in the initial file profile.
pub fn validate_file_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 240 || !path.is_ascii() {
        return Err(Error::new(
            "unsafe_path",
            "file paths must be 1..240 ASCII bytes",
        ));
    }
    for component in path.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
            || component
                .bytes()
                .any(|b| b < 32 || b == 127 || b"\\:<>\"|?*".contains(&b))
        {
            return Err(Error::new(
                "unsafe_path",
                format!("unsafe file path: {path:?}"),
            ));
        }
        let stem = component
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(Error::new(
                "unsafe_path",
                format!("reserved filename: {path:?}"),
            ));
        }
    }
    Ok(())
}

pub fn unique_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Result<()> {
    let mut seen = BTreeSet::new();
    for path in paths {
        validate_file_path(path)?;
        if !seen.insert(path.to_ascii_lowercase()) {
            return Err(Error::new(
                "path_collision",
                format!("duplicate or case-colliding path: {path:?}"),
            ));
        }
    }
    for path in &seen {
        for (index, _) in path.match_indices('/') {
            if seen.contains(&path[..index]) {
                return Err(Error::new(
                    "path_collision",
                    "a file is also used as a parent directory",
                ));
            }
        }
    }
    Ok(())
}

pub fn normalize_repo(input: &str) -> Result<String> {
    let id = input
        .strip_prefix("https://huggingface.co/")
        .unwrap_or(input)
        .trim_end_matches('/');
    let parts: Vec<_> = id.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || part.len() > 96
                || part.starts_with('.')
                || part.ends_with('.')
                || part.contains("..")
                || !part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err(Error::new(
            "invalid_repo",
            "use owner/model or its official https://huggingface.co/owner/model URL",
        ));
    }
    Ok(id.to_owned())
}

pub fn validate_revision(revision: &str) -> Result<()> {
    if revision.is_empty()
        || revision.len() > 128
        || revision
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !revision
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./".contains(&b))
    {
        return Err(Error::new(
            "invalid_revision",
            "revision must be a bounded branch, tag, or commit reference",
        ));
    }
    Ok(())
}

pub fn validate_hex(value: &str, length: usize) -> Result<()> {
    if value.len() != length
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::new(
            "invalid_hash",
            format!("expected {length} lowercase hexadecimal characters"),
        ));
    }
    Ok(())
}
