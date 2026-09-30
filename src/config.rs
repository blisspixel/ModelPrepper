use crate::{
    Error, Result,
    validation::{normalize_repo, validate_revision},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub catalog_path: PathBuf,
    pub licenses: Licenses,
    pub storage: Storage,
    pub transfer: Transfer,
    pub approval: Approval,
    pub files: FilePolicy,
    pub volumes: Vec<Volume>,
    #[serde(default)]
    pub watches: Vec<Watch>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Licenses {
    pub allow: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Storage {
    pub default_volume: String,
    pub max_stored_bytes: u64,
    pub min_free_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transfer {
    pub max_transfer_bytes_per_period: u64,
    pub period: String,
    pub max_parallel_files: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub mode: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FilePolicy {
    pub profile: String,
    pub include_repo_code: bool,
    pub include_gguf: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Volume {
    pub label: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Watch {
    pub source: String,
    pub repo_type: String,
    pub repo_id: String,
    pub revision: String,
    pub update_policy: String,
}

impl Config {
    pub fn default_for(catalog: PathBuf, volume: PathBuf) -> Self {
        Self {
            schema_version: 1,
            catalog_path: catalog,
            licenses: Licenses {
                allow: vec!["apache-2.0".into(), "mit".into()],
            },
            storage: Storage {
                default_volume: "primary".into(),
                max_stored_bytes: 500_000_000_000,
                min_free_bytes: 20_000_000_000,
            },
            transfer: Transfer {
                max_transfer_bytes_per_period: 80_000_000_000,
                period: "utc-week".into(),
                max_parallel_files: 2,
            },
            approval: Approval {
                mode: "manual".into(),
            },
            files: FilePolicy {
                profile: "publisher-safetensors-v1".into(),
                include_repo_code: false,
                include_gguf: false,
            },
            volumes: vec![Volume {
                label: "primary".into(),
                path: volume,
            }],
            watches: vec![],
        }
    }

    pub fn parse(input: &str) -> Result<Self> {
        let value: Self =
            toml::from_str(input).map_err(|e| Error::new("invalid_config", e.message()))?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(Error::new(
                "unsupported_schema",
                "config schema_version must be 1",
            ));
        }
        if self.catalog_path.as_os_str().is_empty() {
            return Err(Error::new("invalid_config", "catalog_path cannot be empty"));
        }
        if self.licenses.allow.is_empty()
            || self
                .licenses
                .allow
                .iter()
                .any(|id| !matches!(id.as_str(), "mit" | "apache-2.0"))
            || self.licenses.allow.iter().collect::<BTreeSet<_>>().len()
                != self.licenses.allow.len()
        {
            return Err(Error::new(
                "invalid_config",
                "allow unique license ids: mit, apache-2.0",
            ));
        }
        if self.storage.max_stored_bytes == 0
            || self.transfer.max_transfer_bytes_per_period == 0
            || !(1..=8).contains(&self.transfer.max_parallel_files)
        {
            return Err(Error::new(
                "invalid_config",
                "budgets must be positive and max_parallel_files must be 1..8",
            ));
        }
        if self.transfer.period != "utc-week" || self.approval.mode != "manual" {
            return Err(Error::new(
                "unsupported_policy",
                "only utc-week periods and manual approval are implemented",
            ));
        }
        if self.files.profile != "publisher-safetensors-v1"
            || self.files.include_repo_code
            || self.files.include_gguf
        {
            return Err(Error::new(
                "unsupported_policy",
                "only publisher-safetensors-v1 without code or GGUF is implemented",
            ));
        }
        let mut labels = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for volume in &self.volumes {
            if volume.label.is_empty()
                || !volume
                    .label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                || volume.path.as_os_str().is_empty()
                || !labels.insert(&volume.label)
                || !paths.insert(&volume.path)
            {
                return Err(Error::new(
                    "invalid_config",
                    "volumes need unique labels and paths; labels use letters, digits, '-' or '_'",
                ));
            }
        }
        if !labels.contains(&self.storage.default_volume) {
            return Err(Error::new(
                "invalid_config",
                "default_volume must name a configured volume",
            ));
        }
        let mut watches = BTreeSet::new();
        for watch in &self.watches {
            if watch.source != "huggingface"
                || watch.repo_type != "model"
                || watch.update_policy != "propose"
            {
                return Err(Error::new(
                    "unsupported_policy",
                    "only Hugging Face models with propose updates are implemented",
                ));
            }
            let id = normalize_repo(&watch.repo_id)?;
            validate_revision(&watch.revision)?;
            if !watches.insert((id, &watch.revision)) {
                return Err(Error::new("invalid_config", "duplicate watch"));
            }
        }
        Ok(())
    }

    /// Deterministic encoding for policy identity. Set-like lists ignore input order.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut normalized = self.clone();
        normalized.licenses.allow.sort();
        normalized.volumes.sort_by(|a, b| a.label.cmp(&b.label));
        for watch in &mut normalized.watches {
            watch.repo_id = normalize_repo(&watch.repo_id)?;
        }
        normalized
            .watches
            .sort_by(|a, b| (&a.repo_id, &a.revision).cmp(&(&b.repo_id, &b.revision)));
        serde_json::to_vec(&normalized).map_err(|e| Error::new("invalid_config", e.to_string()))
    }
}
