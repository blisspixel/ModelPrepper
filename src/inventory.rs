use crate::{
    Error, Result,
    validation::{normalize_repo, unique_paths, validate_hex, validate_revision},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub schema_version: u32,
    pub source: Source,
    pub complete: bool,
    pub layout: String,
    pub license: LicenseEvidence,
    pub files: Vec<SourceFile>,
    /// Present only for a derivative inventory. Omitted from identity bytes when absent,
    /// so publisher-safetensors previews keep their existing plan id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<BaseDependency>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub endpoint: String,
    pub repo_type: String,
    pub repo_id: String,
    pub requested_revision: String,
    pub resolved_revision: String,
    pub gated: bool,
    pub private: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LicenseEvidence {
    pub license_id: String,
    pub status: EvidenceStatus,
    pub paths: Vec<String>,
}

/// Declared base of a derivative. A missing revision means the pin is not established.
/// License fields are evidence already captured for that base. They do not change the
/// derivative repository's own license status.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BaseDependency {
    pub repo_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_status: Option<EvidenceStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub license_paths: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStatus {
    Unreviewed,
    Matched,
    Conflicting,
    /// License text matched and a NOTICE file was captured. The notice prose
    /// is not interpreted, so this is not a match and not a conflict.
    NoticePresent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceFile {
    pub path: String,
    pub size_bytes: u64,
    pub upstream_hash: Option<UpstreamHash>,
    pub role: Role,
    pub required: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamHash {
    pub algorithm: HashAlgorithm,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HashAlgorithm {
    Sha256,
    GitSha1,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Weight,
    Config,
    Tokenizer,
    License,
    Notice,
    Template,
    Documentation,
    Other,
    /// Vision projector named beside a GGUF quant. It is not a main weight.
    Projector,
}

impl Inventory {
    pub fn parse(input: &str) -> Result<Self> {
        let inventory: Self = serde_json::from_str(input)
            .map_err(|e| Error::new("invalid_inventory", e.to_string()))?;
        inventory.validate()?;
        Ok(inventory)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::new(
                "unsupported_schema",
                "inventory schema_version must be 1",
            ));
        }
        if self.source.endpoint != "https://huggingface.co" || self.source.repo_type != "model" {
            return Err(Error::new(
                "unsupported_source",
                "only official Hugging Face model inventories are supported",
            ));
        }
        normalize_repo(&self.source.repo_id)?;
        validate_revision(&self.source.requested_revision)?;
        validate_hex(&self.source.resolved_revision, 40)?;
        unique_paths(self.files.iter().map(|file| file.path.as_str()))?;
        unique_paths(self.license.paths.iter().map(String::as_str))?;
        for file in &self.files {
            if let Some(hash) = &file.upstream_hash {
                let length = match hash.algorithm {
                    HashAlgorithm::Sha256 => 64,
                    HashAlgorithm::GitSha1 => 40,
                };
                validate_hex(&hash.value, length)?;
            }
        }
        for path in &self.license.paths {
            if !self
                .files
                .iter()
                .any(|file| &file.path == path && matches!(file.role, Role::License | Role::Notice))
            {
                return Err(Error::new(
                    "invalid_inventory",
                    format!("license evidence path is not a license or notice file: {path:?}"),
                ));
            }
        }
        if let Some(base) = &self.base {
            normalize_repo(&base.repo_id)?;
            if let Some(revision) = &base.revision {
                validate_hex(revision, 40)?;
            }
            unique_paths(base.license_paths.iter().map(String::as_str))?;
            if matches!(
                base.license_status,
                Some(EvidenceStatus::Matched | EvidenceStatus::NoticePresent)
            ) && (base.license_id.is_empty() || base.license_paths.is_empty())
            {
                return Err(Error::new(
                    "invalid_inventory",
                    "a captured base license needs an id and a path",
                ));
            }
        }
        Ok(())
    }
}
