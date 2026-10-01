use crate::{
    Config, Error, Inventory, Result,
    inventory::{EvidenceStatus, Role, Source, SourceFile},
    validation::normalize_repo,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocker {
    LocalInventoryUntrusted,
    IncompleteInventory,
    UnsupportedLayout,
    AccessRestricted,
    LicenseNeedsReview,
    LicenseNotAllowed,
    MissingRequiredRole,
    RequiredFileExcluded,
    MissingUpstreamHash,
    RevisionTooLarge,
    TransferBudgetExceeded,
    InsufficientSpace,
    TransferBackendUnqualified,
}

#[derive(Debug, Serialize)]
pub struct Plan {
    pub schema_version: u32,
    pub plan_id: String,
    pub policy_digest: String,
    pub mode: &'static str,
    pub transfer_authorized: bool,
    pub source: Source,
    pub volume_label: String,
    pub volume_path: std::path::PathBuf,
    pub selected_files: Vec<SourceFile>,
    pub excluded_files: Vec<String>,
    pub payload_bytes: u64,
    pub min_free_bytes: u64,
    pub available_bytes: u64,
    pub workspace_bytes: Option<u64>,
    pub blockers: Vec<Blocker>,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A local inventory is untrusted input. This preview can never authorize a transfer.
pub fn build_plan(config: &Config, inventory: &Inventory, available_bytes: u64) -> Result<Plan> {
    let policy_digest = digest(&config.canonical_bytes()?);
    inventory.validate()?;
    let volume = config
        .volumes
        .iter()
        .find(|v| v.label == config.storage.default_volume)
        .ok_or_else(|| Error::new("invalid_config", "default volume not found"))?;
    let mut blockers = vec![Blocker::LocalInventoryUntrusted];
    if !inventory.complete {
        blockers.push(Blocker::IncompleteInventory);
    }
    if inventory.layout != "transformers-safetensors-v1" {
        blockers.push(Blocker::UnsupportedLayout);
    }
    if inventory.source.gated || inventory.source.private {
        blockers.push(Blocker::AccessRestricted);
    }
    if inventory.license.status != EvidenceStatus::Matched || inventory.license.paths.is_empty() {
        blockers.push(Blocker::LicenseNeedsReview);
    }
    if !config
        .licenses
        .allow
        .contains(&inventory.license.license_id)
    {
        blockers.push(Blocker::LicenseNotAllowed);
    }

    let mut selected_files = Vec::new();
    let mut excluded_files = Vec::new();
    for file in &inventory.files {
        if admitted(file) {
            if file.upstream_hash.is_none() {
                blockers.push(Blocker::MissingUpstreamHash);
            }
            selected_files.push(file.clone());
        } else {
            if file.required {
                blockers.push(Blocker::RequiredFileExcluded);
            }
            excluded_files.push(file.path.clone());
        }
    }
    for role in [Role::Weight, Role::Config, Role::Tokenizer, Role::License] {
        if !selected_files.iter().any(|file| file.role == role) {
            blockers.push(Blocker::MissingRequiredRole);
        }
    }
    selected_files.sort_by(|a, b| a.path.cmp(&b.path));
    excluded_files.sort();
    let payload_bytes = selected_files
        .iter()
        .try_fold(0_u64, |sum, file| sum.checked_add(file.size_bytes))
        .ok_or_else(|| Error::new("size_overflow", "selected payload exceeds u64 capacity"))?;
    if payload_bytes > config.storage.max_stored_bytes {
        blockers.push(Blocker::RevisionTooLarge);
    }
    if payload_bytes > config.transfer.max_transfer_bytes_per_period {
        blockers.push(Blocker::TransferBudgetExceeded);
    }
    let minimum = payload_bytes
        .checked_add(config.storage.min_free_bytes)
        .ok_or_else(|| {
            Error::new(
                "size_overflow",
                "payload and free-space reserve exceed u64 capacity",
            )
        })?;
    if minimum > available_bytes {
        blockers.push(Blocker::InsufficientSpace);
    }
    blockers.sort();
    blockers.dedup();
    let mut source = inventory.source.clone();
    source.repo_id = normalize_repo(&source.repo_id)?;

    // Versioned struct field order and sorted arrays define the canonical identity.
    #[derive(Serialize)]
    struct Identity<'a> {
        domain: &'static str,
        policy_digest: &'a str,
        source: &'a Source,
        inventory: &'a Inventory,
    }
    let mut normalized_inventory = inventory.clone();
    normalized_inventory.source = source.clone();
    normalized_inventory
        .files
        .sort_by(|a, b| a.path.cmp(&b.path));
    normalized_inventory.license.paths.sort();
    let identity = serde_json::to_vec(&Identity {
        domain: "modelprepper.offline-plan.v1",
        policy_digest: &policy_digest,
        source: &source,
        inventory: &normalized_inventory,
    })
    .map_err(|e| Error::new("invalid_inventory", e.to_string()))?;

    Ok(Plan {
        schema_version: 1,
        plan_id: digest(&identity),
        policy_digest,
        mode: "offline_preview",
        transfer_authorized: false,
        source,
        volume_label: volume.label.clone(),
        volume_path: volume.path.clone(),
        selected_files,
        excluded_files,
        payload_bytes,
        min_free_bytes: config.storage.min_free_bytes,
        available_bytes,
        workspace_bytes: None,
        blockers,
    })
}

fn admitted(file: &SourceFile) -> bool {
    let path = file.path.to_ascii_lowercase();
    let name = path.rsplit('/').next().unwrap_or_default();
    let extension = name.rsplit('.').next().unwrap_or_default();
    if matches!(
        extension,
        "py" | "bin"
            | "pt"
            | "pth"
            | "pkl"
            | "pickle"
            | "gguf"
            | "onnx"
            | "h5"
            | "msgpack"
            | "ot"
            | "exe"
            | "dll"
            | "sh"
            | "bat"
            | "ps1"
    ) {
        return false;
    }
    match file.role {
        Role::Weight => extension == "safetensors",
        Role::Config => extension == "json",
        Role::Tokenizer => matches!(extension, "json" | "model" | "txt"),
        Role::Template => matches!(extension, "jinja" | "json"),
        Role::License | Role::Notice => {
            name == "license" || name == "notice" || matches!(extension, "txt" | "md")
        }
        Role::Documentation => extension == "md",
        Role::Other => false,
    }
}
