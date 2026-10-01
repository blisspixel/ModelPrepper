//! Resolve public repositories twice: a moving ref first, then its exact commit.
use crate::{
    Error, Inventory, Result,
    http::Http,
    integrity::verify_reader,
    inventory::{
        EvidenceStatus, HashAlgorithm, LicenseEvidence, Role, Source, SourceFile, UpstreamHash,
    },
    licenses,
    plan::digest,
    validation::{normalize_repo, validate_hex, validate_revision},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use url::Url;

const METADATA_LIMIT: u64 = 2 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Resolution {
    pub schema_version: u32,
    pub mode: &'static str,
    pub transfer_authorized: bool,
    pub inventory: Inventory,
    pub evidence: Vec<CapturedEvidence>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedEvidence {
    pub path: String,
    pub sha256: String,
    pub text: String,
}

#[derive(Deserialize)]
struct Metadata {
    id: String,
    sha: String,
    private: bool,
    gated: Value,
    #[serde(rename = "cardData")]
    card_data: Option<Value>,
    siblings: Vec<Sibling>,
}

#[derive(Deserialize)]
struct Sibling {
    rfilename: String,
    size: u64,
    #[serde(rename = "blobId")]
    blob_id: String,
    lfs: Option<Lfs>,
}

#[derive(Deserialize)]
struct Lfs {
    sha256: String,
    size: u64,
}

pub fn resolve(repo: &str, revision: &str) -> Result<Resolution> {
    resolve_with(&Http::default(), "https://huggingface.co", repo, revision)
}

fn endpoint(base: &str, segments: &[&str]) -> Result<Url> {
    let mut url =
        Url::parse(base).map_err(|_| Error::new("invalid_source", "invalid source origin"))?;
    url.path_segments_mut()
        .map_err(|_| Error::new("invalid_source", "source cannot contain paths"))?
        .extend(segments);
    Ok(url)
}

fn metadata(http: &Http, base: &str, repo: &str, revision: &str) -> Result<Metadata> {
    let mut url = endpoint(base, &["api", "models", repo, "revision", revision])?;
    // Repository owner/name are distinct path segments; ref slashes stay encoded.
    let (owner, name) = repo.split_once('/').expect("validated repository");
    url.set_path("");
    url.path_segments_mut()
        .expect("HTTP origin")
        .extend(["api", "models", owner, name, "revision", revision]);
    url.set_query(Some("blobs=true"));
    let value: Metadata =
        serde_json::from_slice(&http.get(&url, METADATA_LIMIT)?).map_err(|_| {
            Error::new(
                "invalid_source_metadata",
                "source lacks complete file sizes and identities",
            )
        })?;
    validate_hex(&value.sha, 40)?;
    if value.id != repo {
        return Err(Error::new(
            "source_identity_changed",
            "source repository identity differs from request",
        ));
    }
    if value.private || value.gated != Value::Bool(false) {
        return Err(Error::new(
            "access_restricted",
            "only public, ungated repositories are supported",
        ));
    }
    Ok(value)
}

fn resolve_with(http: &Http, base: &str, repo: &str, revision: &str) -> Result<Resolution> {
    let repo = normalize_repo(repo)?;
    validate_revision(revision)?;
    let moving = metadata(http, base, &repo, revision)?;
    let pinned = metadata(http, base, &repo, &moving.sha)?;
    if pinned.sha != moving.sha {
        return Err(Error::new(
            "source_identity_changed",
            "pinned revision differs from resolved commit",
        ));
    }
    let license_id = pinned
        .card_data
        .as_ref()
        .and_then(|v| v.get("license"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let mut files = Vec::new();
    for sibling in pinned.siblings {
        validate_hex(&sibling.blob_id, 40)?;
        let hash = if let Some(lfs) = sibling.lfs {
            if lfs.size != sibling.size {
                return Err(Error::new(
                    "invalid_source_metadata",
                    "LFS size disagrees with repository inventory",
                ));
            }
            validate_hex(&lfs.sha256, 64)?;
            UpstreamHash {
                algorithm: HashAlgorithm::Sha256,
                value: lfs.sha256,
            }
        } else {
            UpstreamHash {
                algorithm: HashAlgorithm::GitSha1,
                value: sibling.blob_id,
            }
        };
        let role = classify(&sibling.rfilename);
        files.push(SourceFile {
            path: sibling.rfilename,
            size_bytes: sibling.size,
            upstream_hash: Some(hash),
            role,
            required: matches!(
                role,
                Role::Weight | Role::Config | Role::Tokenizer | Role::License | Role::Notice
            ),
        });
    }
    let paths = files
        .iter()
        .filter(|f| f.role == Role::License)
        .map(|f| f.path.clone())
        .collect::<Vec<_>>();
    let mut inventory = Inventory {
        schema_version: 1,
        source: Source {
            endpoint: "https://huggingface.co".into(),
            repo_type: "model".into(),
            repo_id: repo,
            requested_revision: revision.into(),
            resolved_revision: pinned.sha,
            gated: false,
            private: false,
        },
        complete: true,
        layout: "unsupported".into(),
        license: LicenseEvidence {
            license_id,
            status: EvidenceStatus::Unreviewed,
            paths,
        },
        files,
    };
    inventory.validate()?;
    if inventory
        .files
        .iter()
        .filter(|f| matches!(f.role, Role::License | Role::Notice))
        .count()
        > 16
    {
        return Err(Error::new(
            "too_many_evidence_files",
            "source exceeds the 16-file license evidence limit",
        ));
    }
    let mut evidence = Vec::new();
    let mut evidence_bytes = 0_u64;
    for file in inventory
        .files
        .iter()
        .filter(|f| matches!(f.role, Role::License | Role::Notice))
    {
        evidence_bytes = evidence_bytes.checked_add(file.size_bytes).ok_or_else(|| {
            Error::new(
                "metadata_too_large",
                "license evidence exceeds metadata limit",
            )
        })?;
        if evidence_bytes > METADATA_LIMIT {
            return Err(Error::new(
                "metadata_too_large",
                "license evidence exceeds metadata limit",
            ));
        }
        let bytes = fetch_file(http, base, &inventory.source, file)?;
        let text = String::from_utf8(bytes).map_err(|_| {
            Error::new("invalid_license_encoding", "license evidence must be UTF-8")
        })?;
        evidence.push(CapturedEvidence {
            path: file.path.clone(),
            sha256: digest(text.as_bytes()),
            text,
        });
    }
    let license_texts = evidence
        .iter()
        .filter(|e| inventory.license.paths.contains(&e.path))
        .collect::<Vec<_>>();
    if !license_texts.is_empty() {
        inventory.license.status = if license_texts
            .iter()
            .all(|e| licenses::matches(&inventory.license.license_id, &e.text))
            && !evidence.iter().any(|e| classify(&e.path) == Role::Notice)
        {
            EvidenceStatus::Matched
        } else {
            EvidenceStatus::Conflicting
        };
    }
    if supported_layout(http, base, &inventory)? {
        inventory.layout = "transformers-safetensors-v1".into();
    }
    inventory.files.sort_by(|a, b| a.path.cmp(&b.path));
    inventory.license.paths.sort();
    evidence.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Resolution {
        schema_version: 1,
        mode: "resolved_inventory",
        transfer_authorized: false,
        inventory,
        evidence,
    })
}

fn fetch_file(http: &Http, base: &str, source: &Source, file: &SourceFile) -> Result<Vec<u8>> {
    if file.size_bytes > METADATA_LIMIT {
        return Err(Error::new(
            "metadata_too_large",
            "metadata file exceeds 2 MiB limit",
        ));
    }
    let mut segments = source.repo_id.split('/').collect::<Vec<_>>();
    segments.extend(["resolve", &source.resolved_revision]);
    segments.extend(file.path.split('/'));
    let bytes = http.get(&endpoint(base, &segments)?, file.size_bytes)?;
    verify_reader(bytes.as_slice(), file)?;
    Ok(bytes)
}

fn classify(path: &str) -> Role {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "license" | "license.txt" | "license.md" | "copying" | "copying.txt"
    ) {
        return Role::License;
    }
    if matches!(name.as_str(), "notice" | "notice.txt" | "notice.md") {
        return Role::Notice;
    }
    if path.contains('/') {
        return Role::Other;
    }
    match name.as_str() {
        "config.json" | "generation_config.json" | "model.safetensors.index.json" => Role::Config,
        "tokenizer.json"
        | "tokenizer_config.json"
        | "special_tokens_map.json"
        | "added_tokens.json"
        | "vocab.json"
        | "vocab.txt"
        | "merges.txt"
        | "tokenizer.model"
        | "spiece.model" => Role::Tokenizer,
        "chat_template.jinja" | "chat_template.json" => Role::Template,
        "readme.md" => Role::Documentation,
        "model.safetensors" => Role::Weight,
        _ if name.starts_with("model-") && name.ends_with(".safetensors") => Role::Weight,
        _ => Role::Other,
    }
}

fn supported_layout(http: &Http, base: &str, inventory: &Inventory) -> Result<bool> {
    let Some(config) = inventory.files.iter().find(|f| f.path == "config.json") else {
        return Ok(false);
    };
    let bytes = fetch_file(http, base, &inventory.source, config)?;
    let config: Value = serde_json::from_slice(&bytes)
        .map_err(|_| Error::new("invalid_layout", "config.json is invalid JSON"))?;
    if !config.is_object()
        || config.get("auto_map").is_some()
        || config.get("trust_remote_code") == Some(&Value::Bool(true))
        || config.get("model_type").and_then(Value::as_str).is_none()
    {
        return Ok(false);
    }
    if let Some(tokenizer_config) = inventory
        .files
        .iter()
        .find(|f| f.path == "tokenizer_config.json")
    {
        let bytes = fetch_file(http, base, &inventory.source, tokenizer_config)?;
        let tokenizer_config: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new("invalid_layout", "tokenizer_config.json is invalid JSON"))?;
        if !tokenizer_config.is_object()
            || tokenizer_config.get("auto_map").is_some()
            || tokenizer_config.get("trust_remote_code") == Some(&Value::Bool(true))
        {
            return Ok(false);
        }
    }
    let tokenizer = inventory.files.iter().any(|f| {
        matches!(
            f.path.as_str(),
            "tokenizer.json" | "tokenizer.model" | "spiece.model"
        )
    });
    if !tokenizer {
        return Ok(false);
    }
    let weights: BTreeSet<_> = inventory
        .files
        .iter()
        .filter(|f| f.role == Role::Weight)
        .map(|f| f.path.as_str())
        .collect();
    let index = inventory
        .files
        .iter()
        .find(|f| f.path == "model.safetensors.index.json");
    if let Some(index) = index {
        let bytes = fetch_file(http, base, &inventory.source, index)?;
        let index: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::new("invalid_layout", "weight index is invalid JSON"))?;
        let Some(map) = index.get("weight_map").and_then(Value::as_object) else {
            return Ok(false);
        };
        let Some(shards) = map
            .values()
            .map(Value::as_str)
            .collect::<Option<BTreeSet<_>>>()
        else {
            return Ok(false);
        };
        Ok(!shards.is_empty() && shards == weights && !weights.contains("model.safetensors"))
    } else {
        Ok(weights == BTreeSet::from(["model.safetensors"]))
    }
}

#[cfg(test)]
mod tests;
