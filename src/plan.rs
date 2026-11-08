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
    AmbiguousGgufSelection,
    MissingGgufSelection,
    UnknownGgufSelection,
    IncompleteGgufSet,
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
    /// `original` or `derivative`. Omitted for the publisher-safetensors profile so
    /// existing preview JSON stays stable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_repo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantization_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision_note: Option<String>,
    /// `repository`, `inherited`, or `absent`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_evidence_source: Option<String>,
    /// License id that the derivative preview judged. The inventory status stays the
    /// derivative repository's own status, which is not upgraded to matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub gguf_candidates: Vec<GgufCandidate>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GgufCandidate {
    pub path: String,
    pub size_bytes: u64,
    pub role: Role,
    pub quantization_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub split_count: Option<u32>,
}

pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A local inventory is untrusted input. This preview can never authorize a transfer.
pub fn build_plan(config: &Config, inventory: &Inventory, available_bytes: u64) -> Result<Plan> {
    build_plan_selecting(config, inventory, available_bytes, &[])
}

/// `selection` names repository paths. It is honored only for `selected-gguf-v1`.
/// An empty selection is omitted from preview identity, so existing plan ids stay put.
pub fn build_plan_selecting(
    config: &Config,
    inventory: &Inventory,
    available_bytes: u64,
    selection: &[String],
) -> Result<Plan> {
    if config.files.profile == "selected-gguf-v1" {
        return build_selected_gguf(config, inventory, available_bytes, selection);
    }
    if !selection.is_empty() {
        return Err(Error::new(
            "invalid_usage",
            "naming files with --select requires profile selected-gguf-v1",
        ));
    }
    build_safetensors_plan(config, inventory, available_bytes)
}

fn build_safetensors_plan(
    config: &Config,
    inventory: &Inventory,
    available_bytes: u64,
) -> Result<Plan> {
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

    let mut normalized_inventory = inventory.clone();
    normalized_inventory.source = source.clone();
    normalized_inventory
        .files
        .sort_by(|a, b| a.path.cmp(&b.path));
    normalized_inventory.license.paths.sort();
    let plan_id = preview_id(&policy_digest, &source, &normalized_inventory, &[])?;

    Ok(Plan {
        schema_version: 1,
        plan_id,
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
        representation: None,
        base_repo_id: None,
        base_revision: None,
        quantization_label: None,
        precision_note: None,
        license_evidence_source: None,
        license_id: None,
        gguf_candidates: Vec::new(),
    })
}

const PRECISION_NOTE: &str = "a quantization cannot restore the original weights' precision";

fn build_selected_gguf(
    config: &Config,
    inventory: &Inventory,
    available_bytes: u64,
    selection: &[String],
) -> Result<Plan> {
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
    if inventory.layout != "gguf-v1" {
        blockers.push(Blocker::UnsupportedLayout);
    }
    if inventory.source.gated || inventory.source.private {
        blockers.push(Blocker::AccessRestricted);
    }
    let (license_id, license_source) = apply_license(config, inventory, &mut blockers);

    let mut requested = Vec::new();
    for path in selection {
        if path.is_empty() {
            return Err(Error::new(
                "invalid_usage",
                "a selected path cannot be empty",
            ));
        }
        if !requested.iter().any(|existing: &String| existing == path) {
            requested.push(path.clone());
        }
    }
    let candidates = gguf_candidates(inventory);
    let resolved = resolve_gguf_selection(inventory, &requested);
    let selection_ok = resolved.blockers.is_empty();
    blockers.extend(resolved.blockers);
    if !requested.is_empty() && !resolved.files.iter().any(|file| file.role == Role::Weight) {
        blockers.push(Blocker::MissingRequiredRole);
    }
    let mut selected_files = resolved.files;
    if selection_ok && selected_files.iter().any(|file| file.role == Role::Weight) {
        for file in &inventory.files {
            if is_carried_rights(file) && !selected_files.iter().any(|have| have.path == file.path)
            {
                selected_files.push(file.clone());
            }
        }
    }
    for file in &selected_files {
        if file.upstream_hash.is_none() {
            blockers.push(Blocker::MissingUpstreamHash);
        }
    }
    selected_files.sort_by(|a, b| a.path.cmp(&b.path));
    let selected_paths: Vec<String> = selected_files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    let mut excluded_files = inventory
        .files
        .iter()
        .filter(|file| !selected_paths.iter().any(|path| path == &file.path))
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    excluded_files.sort();
    let payload_bytes = sum_sizes(selected_files.iter().map(|file| file.size_bytes))?;
    push_budget_blockers(
        &mut blockers,
        payload_bytes,
        config.storage.max_stored_bytes,
        config.transfer.max_transfer_bytes_per_period,
        config.storage.min_free_bytes,
        available_bytes,
    )?;
    blockers.sort();
    blockers.dedup();
    let mut source = inventory.source.clone();
    source.repo_id = normalize_repo(&source.repo_id)?;
    let mut normalized_inventory = inventory.clone();
    normalized_inventory.source = source.clone();
    normalized_inventory
        .files
        .sort_by(|a, b| a.path.cmp(&b.path));
    normalized_inventory.license.paths.sort();
    if let Some(base) = normalized_inventory.base.as_mut() {
        base.repo_id = normalize_repo(&base.repo_id)?;
        base.license_paths.sort();
    }
    let identity_selection = if selected_paths.is_empty() {
        let mut names = requested.clone();
        names.sort();
        names
    } else {
        selected_paths
    };
    let plan_id = preview_id(
        &policy_digest,
        &source,
        &normalized_inventory,
        &identity_selection,
    )?;
    let quant_labels: Vec<String> = selected_files
        .iter()
        .filter(|file| is_gguf_candidate(file))
        .map(|file| quantization_label(&file.path))
        .collect();
    let quantization_label = if quant_labels.is_empty() {
        None
    } else {
        Some(quant_labels.join(", "))
    };
    let base_repo_id = normalized_inventory
        .base
        .as_ref()
        .map(|base| base.repo_id.clone())
        .unwrap_or_else(|| "not declared".into());
    let base_revision = normalized_inventory
        .base
        .as_ref()
        .and_then(|base| base.revision.clone())
        .unwrap_or_else(|| "not established".into());
    Ok(Plan {
        schema_version: 1,
        plan_id,
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
        representation: Some("derivative".into()),
        base_repo_id: Some(base_repo_id),
        base_revision: Some(base_revision),
        quantization_label,
        precision_note: Some(PRECISION_NOTE.into()),
        license_evidence_source: Some(license_source),
        license_id: Some(license_id),
        gguf_candidates: candidates,
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
        Role::Other | Role::Projector => false,
    }
}

fn preview_id(
    policy_digest: &str,
    source: &Source,
    inventory: &Inventory,
    selection: &[String],
) -> Result<String> {
    #[derive(Serialize)]
    struct Identity<'a> {
        domain: &'static str,
        policy_digest: &'a str,
        source: &'a Source,
        inventory: &'a Inventory,
        #[serde(skip_serializing_if = "selection_empty")]
        selection: &'a [String],
    }
    let bytes = serde_json::to_vec(&Identity {
        domain: "modelprepper.offline-plan.v1",
        policy_digest,
        source,
        inventory,
        selection,
    })
    .map_err(|e| Error::new("invalid_inventory", e.to_string()))?;
    Ok(digest(&bytes))
}

fn selection_empty(selection: &&[String]) -> bool {
    selection.is_empty()
}

fn sum_sizes(mut sizes: impl Iterator<Item = u64>) -> Result<u64> {
    sizes
        .try_fold(0_u64, |sum, size| sum.checked_add(size))
        .ok_or_else(|| Error::new("size_overflow", "selected payload exceeds u64 capacity"))
}

fn push_budget_blockers(
    blockers: &mut Vec<Blocker>,
    payload_bytes: u64,
    max_stored_bytes: u64,
    max_transfer_bytes: u64,
    min_free_bytes: u64,
    available_bytes: u64,
) -> Result<()> {
    if payload_bytes > max_stored_bytes {
        blockers.push(Blocker::RevisionTooLarge);
    }
    if payload_bytes > max_transfer_bytes {
        blockers.push(Blocker::TransferBudgetExceeded);
    }
    let minimum = payload_bytes.checked_add(min_free_bytes).ok_or_else(|| {
        Error::new(
            "size_overflow",
            "payload and free-space reserve exceed u64 capacity",
        )
    })?;
    if minimum > available_bytes {
        blockers.push(Blocker::InsufficientSpace);
    }
    Ok(())
}

fn apply_license(
    config: &Config,
    inventory: &Inventory,
    blockers: &mut Vec<Blocker>,
) -> (String, String) {
    let own_matched =
        inventory.license.status == EvidenceStatus::Matched && !inventory.license.paths.is_empty();
    if own_matched {
        if !config
            .licenses
            .allow
            .contains(&inventory.license.license_id)
        {
            blockers.push(Blocker::LicenseNotAllowed);
        }
        return (inventory.license.license_id.clone(), "repository".into());
    }
    if let Some(base) = &inventory.base {
        let inherited = base.revision.is_some()
            && base.license_status == Some(EvidenceStatus::Matched)
            && !base.license_id.is_empty()
            && !base.license_paths.is_empty();
        if inherited {
            if !config.licenses.allow.contains(&base.license_id) {
                blockers.push(Blocker::LicenseNotAllowed);
            }
            return (base.license_id.clone(), "inherited".into());
        }
    }
    blockers.push(Blocker::LicenseNeedsReview);
    if !inventory.license.license_id.is_empty()
        && !config
            .licenses
            .allow
            .contains(&inventory.license.license_id)
    {
        blockers.push(Blocker::LicenseNotAllowed);
    }
    (inventory.license.license_id.clone(), "absent".into())
}

struct ResolvedSelection {
    files: Vec<SourceFile>,
    blockers: Vec<Blocker>,
}

fn resolve_gguf_selection(inventory: &Inventory, requested: &[String]) -> ResolvedSelection {
    if requested.is_empty() {
        let blocker = if gguf_candidates(inventory).len() > 1 {
            Blocker::AmbiguousGgufSelection
        } else {
            Blocker::MissingGgufSelection
        };
        return ResolvedSelection {
            files: Vec::new(),
            blockers: vec![blocker],
        };
    }
    let mut blockers = Vec::new();
    let mut chosen = Vec::new();
    let mut groups = std::collections::BTreeSet::new();
    for path in requested {
        let Some(file) = inventory.files.iter().find(|file| file.path == *path) else {
            blockers.push(Blocker::UnknownGgufSelection);
            continue;
        };
        if !is_gguf_candidate(file) {
            blockers.push(Blocker::UnknownGgufSelection);
            continue;
        }
        if let Some(split) = split_parts(&file.path) {
            groups.insert((split.prefix, split.count));
        } else if !chosen
            .iter()
            .any(|have: &SourceFile| have.path == file.path)
        {
            chosen.push(file.clone());
        }
    }
    for (prefix, count) in groups {
        let members: Vec<&SourceFile> = inventory
            .files
            .iter()
            .filter(|file| {
                split_parts(&file.path)
                    .is_some_and(|split| split.prefix == prefix && split.count == count)
            })
            .collect();
        let mut indexes: Vec<u32> = members
            .iter()
            .filter_map(|file| split_parts(&file.path).map(|split| split.index))
            .collect();
        indexes.sort_unstable();
        indexes.dedup();
        let complete =
            indexes.len() == count as usize && (1..=count).all(|index| indexes.contains(&index));
        if !complete {
            blockers.push(Blocker::IncompleteGgufSet);
            continue;
        }
        for file in members {
            if !chosen.iter().any(|have| have.path == file.path) {
                chosen.push(file.clone());
            }
        }
    }
    ResolvedSelection {
        files: chosen,
        blockers,
    }
}

fn is_gguf_candidate(file: &SourceFile) -> bool {
    !file.path.contains('/')
        && file.path.to_ascii_lowercase().ends_with(".gguf")
        && matches!(file.role, Role::Weight | Role::Projector)
}

/// License and notice bytes travel with a successful GGUF selection. The
/// operator cannot name them with `--select`. A root `NOTICE` kept as `other`
/// still counts. Nested paths and `..` do not.
fn is_carried_rights(file: &SourceFile) -> bool {
    if file.path.starts_with('/') || file.path.split(['/', '\\']).any(|part| part == "..") {
        return false;
    }
    if matches!(file.role, Role::License | Role::Notice) {
        return true;
    }
    let name = file
        .path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(file.path.as_str())
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "license" | "license.txt" | "license.md" | "notice" | "notice.txt" | "notice.md"
    )
}

fn gguf_candidates(inventory: &Inventory) -> Vec<GgufCandidate> {
    let mut candidates: Vec<_> = inventory
        .files
        .iter()
        .filter(|file| is_gguf_candidate(file))
        .map(|file| {
            let split = split_parts(&file.path);
            GgufCandidate {
                path: file.path.clone(),
                size_bytes: file.size_bytes,
                role: file.role,
                quantization_label: quantization_label(&file.path),
                split_index: split.as_ref().map(|split| split.index),
                split_count: split.as_ref().map(|split| split.count),
            }
        })
        .collect();
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates
}

struct SplitParts {
    prefix: String,
    index: u32,
    count: u32,
}

fn split_parts(path: &str) -> Option<SplitParts> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();
    let stem = lower.strip_suffix(".gguf")?;
    let of = stem.rfind("-of-")?;
    let count_text = &stem[of + 4..];
    if count_text.is_empty() || !count_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let head = &stem[..of];
    let dash = head.rfind('-')?;
    let index_text = &head[dash + 1..];
    if index_text.is_empty() || !index_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let prefix = &head[..dash];
    if prefix.is_empty() {
        return None;
    }
    let index = index_text.parse::<u32>().ok()?;
    let count = count_text.parse::<u32>().ok()?;
    if index == 0 || count == 0 || index > count {
        return None;
    }
    Some(SplitParts {
        prefix: prefix.to_owned(),
        index,
        count,
    })
}

pub(crate) fn quantization_label(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let lower_name = name.to_ascii_lowercase();
    let stem_len = if lower_name.ends_with(".gguf") {
        name.len() - 5
    } else {
        name.len()
    };
    let mut stem = &name[..stem_len];
    let lower_stem = stem.to_ascii_lowercase();
    if let Some(of) = lower_stem.rfind("-of-")
        && let Some(dash) = lower_stem[..of].rfind('-')
    {
        let count_ok = lower_stem[of + 4..]
            .bytes()
            .all(|byte| byte.is_ascii_digit());
        let index_ok = lower_stem[dash + 1..of]
            .bytes()
            .all(|byte| byte.is_ascii_digit());
        if count_ok && index_ok && dash > 0 {
            stem = &stem[..dash];
        }
    }
    find_quant(stem).unwrap_or_else(|| "unlabeled".into())
}

fn find_quant(stem: &str) -> Option<String> {
    let lower = stem.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut best = None;
    for start in 0..bytes.len() {
        let Some(len) = quant_at(&lower[start..]) else {
            continue;
        };
        if start > 0 && !matches!(bytes[start - 1], b'-' | b'_') {
            continue;
        }
        let end = start + len;
        if end < bytes.len() && !matches!(bytes[end], b'-' | b'_' | b'.') {
            continue;
        }
        match best {
            Some((_, best_len, best_end))
                if end < best_end || (end == best_end && len <= best_len) => {}
            _ => best = Some((start, len, end)),
        }
    }
    best.map(|(start, len, _)| stem[start..start + len].to_owned())
}

fn quant_at(text: &str) -> Option<usize> {
    if let Some(rest) = text.strip_prefix("ud-")
        && let Some(len) = q_family(rest)
    {
        return Some(3 + len);
    }
    for prefix in ["ptq", "pq", "iq", "q"] {
        if let Some(rest) = text.strip_prefix(prefix)
            && let Some(len) = numeric_quant(rest, prefix == "ptq" || prefix == "pq")
        {
            return Some(prefix.len() + len);
        }
    }
    for word in ["bf16", "fp16", "fp32", "f16", "f32"] {
        if text.starts_with(word) {
            return Some(word.len());
        }
    }
    None
}

fn q_family(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('q')?;
    numeric_quant(rest, false).map(|len| 1 + len)
}

fn numeric_quant(text: &str, require_underscore: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.first().is_none_or(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    let mut index = 1;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if require_underscore {
        if index >= bytes.len() || bytes[index] != b'_' {
            return None;
        }
        index += 1;
        let start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == start {
            return None;
        }
        return Some(index);
    }
    while index < bytes.len() && bytes[index] == b'_' {
        let start = index + 1;
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
            end += 1;
        }
        if end == start {
            break;
        }
        index = end;
    }
    Some(index)
}

pub(crate) fn blocker_name(blocker: &Blocker) -> String {
    serde_json::to_value(blocker)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "blocker".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{
        BaseDependency, EvidenceStatus, HashAlgorithm, LicenseEvidence, Source, UpstreamHash,
    };

    fn config(profile: &str) -> Config {
        let mut config = Config::default_for("catalog".into(), "models".into());
        config.files.profile = profile.into();
        config
    }

    fn hash() -> UpstreamHash {
        UpstreamHash {
            algorithm: HashAlgorithm::Sha256,
            value: "ab".repeat(32),
        }
    }

    fn gguf(path: &str, size: u64, role: Role) -> SourceFile {
        SourceFile {
            path: path.into(),
            size_bytes: size,
            upstream_hash: Some(hash()),
            role,
            required: role == Role::Weight,
        }
    }

    fn inventory(files: Vec<SourceFile>, base: Option<BaseDependency>) -> Inventory {
        Inventory {
            schema_version: 1,
            source: Source {
                endpoint: "https://huggingface.co".into(),
                repo_type: "model".into(),
                repo_id: "unsloth/Qwen3.8-27B-GGUF".into(),
                requested_revision: "main".into(),
                resolved_revision: "0123456789abcdef0123456789abcdef01234567".into(),
                gated: false,
                private: false,
            },
            complete: true,
            layout: "gguf-v1".into(),
            license: LicenseEvidence {
                license_id: "apache-2.0".into(),
                status: EvidenceStatus::Unreviewed,
                paths: Vec::new(),
            },
            files,
            base,
        }
    }

    fn has(plan: &Plan, blocker: Blocker) -> bool {
        plan.blockers.contains(&blocker)
    }

    #[test]
    fn an_explicit_quant_and_projector_are_the_only_payload() {
        let quant = 16_464_440_224;
        let projector = 931_146_432;
        let mut files: Vec<_> = (0..25)
            .map(|index| gguf(&format!("other-{index}.gguf"), 1, Role::Weight))
            .collect();
        files.push(gguf("qwen-UD-Q4_K_M.gguf", quant, Role::Weight));
        files.push(gguf("mmproj-BF16.gguf", projector, Role::Projector));
        assert_eq!(files.len(), 27);
        let inventory = inventory(files, None);
        let repo_total = inventory
            .files
            .iter()
            .map(|file| file.size_bytes)
            .sum::<u64>();
        let plan = build_plan_selecting(
            &config("selected-gguf-v1"),
            &inventory,
            u64::MAX,
            &["qwen-UD-Q4_K_M.gguf".into(), "mmproj-BF16.gguf".into()],
        )
        .unwrap();
        assert_eq!(plan.payload_bytes, quant + projector);
        assert_ne!(plan.payload_bytes, repo_total);
        assert_eq!(plan.selected_files.len(), 2);
        assert_eq!(
            plan.selected_files
                .iter()
                .find(|file| file.path == "mmproj-BF16.gguf")
                .unwrap()
                .role,
            Role::Projector
        );
        assert_eq!(plan.representation.as_deref(), Some("derivative"));
        assert_eq!(plan.base_repo_id.as_deref(), Some("not declared"));
        assert_eq!(plan.base_revision.as_deref(), Some("not established"));
        assert_eq!(plan.quantization_label.as_deref(), Some("BF16, UD-Q4_K_M"));
        assert_eq!(plan.precision_note.as_deref(), Some(PRECISION_NOTE));
        assert!(!plan.transfer_authorized);
        assert!(has(&plan, Blocker::LocalInventoryUntrusted));
        assert!(!has(&plan, Blocker::AmbiguousGgufSelection));
        assert_eq!(plan.gguf_candidates.len(), 27);
        let text = crate::present::render_plan(&plan, false, 200);
        assert!(text.contains("derivative"), "{text}");
        assert!(text.contains("BF16, UD-Q4_K_M"), "{text}");
        assert!(text.contains(PRECISION_NOTE), "{text}");
        assert!(text.contains("projector"), "{text}");
    }

    #[test]
    fn naming_nothing_in_a_multi_file_repo_does_not_select_the_repo() {
        let files = vec![
            gguf("a-Q4_K_M.gguf", 10, Role::Weight),
            gguf("b-Q5_K_M.gguf", 20, Role::Weight),
        ];
        let plan = build_plan(
            &config("selected-gguf-v1"),
            &inventory(files, None),
            u64::MAX,
        )
        .unwrap();
        assert!(has(&plan, Blocker::AmbiguousGgufSelection));
        assert!(plan.selected_files.is_empty());
        assert_eq!(plan.payload_bytes, 0);
        assert_eq!(plan.gguf_candidates.len(), 2);
        assert!(
            plan.gguf_candidates
                .iter()
                .any(|candidate| candidate.size_bytes == 20)
        );
    }

    #[test]
    fn one_unnamed_file_is_not_chosen_silently_and_a_named_file_is_the_payload() {
        let files = vec![gguf("only-Q8_0.gguf", 42, Role::Weight)];
        let inventory = inventory(files, None);
        let silent = build_plan(&config("selected-gguf-v1"), &inventory, u64::MAX).unwrap();
        assert!(has(&silent, Blocker::MissingGgufSelection));
        assert_eq!(silent.payload_bytes, 0);
        let named = build_plan_selecting(
            &config("selected-gguf-v1"),
            &inventory,
            u64::MAX,
            &["only-Q8_0.gguf".into()],
        )
        .unwrap();
        assert_eq!(named.payload_bytes, 42);
        assert!(!has(&named, Blocker::MissingGgufSelection));
        assert_eq!(named.quantization_label.as_deref(), Some("Q8_0"));
    }

    #[test]
    fn a_missing_split_shard_blocks_and_a_complete_set_is_kept_together() {
        let partial = inventory(
            vec![
                gguf("model-q4_k_m-00001-of-00003.gguf", 10, Role::Weight),
                gguf("model-q4_k_m-00002-of-00003.gguf", 20, Role::Weight),
            ],
            None,
        );
        let blocked = build_plan_selecting(
            &config("selected-gguf-v1"),
            &partial,
            u64::MAX,
            &["model-q4_k_m-00001-of-00003.gguf".into()],
        )
        .unwrap();
        assert!(has(&blocked, Blocker::IncompleteGgufSet));
        assert!(blocked.selected_files.is_empty());
        let mut files = partial.files.clone();
        files.push(gguf("model-q4_k_m-00003-of-00003.gguf", 30, Role::Weight));
        let complete = inventory(files, None);
        let one = build_plan_selecting(
            &config("selected-gguf-v1"),
            &complete,
            u64::MAX,
            &["model-q4_k_m-00001-of-00003.gguf".into()],
        )
        .unwrap();
        let all = build_plan_selecting(
            &config("selected-gguf-v1"),
            &complete,
            u64::MAX,
            &[
                "model-q4_k_m-00001-of-00003.gguf".into(),
                "model-q4_k_m-00002-of-00003.gguf".into(),
                "model-q4_k_m-00003-of-00003.gguf".into(),
            ],
        )
        .unwrap();
        assert_eq!(one.payload_bytes, 60);
        assert_eq!(one.selected_files.len(), 3);
        assert_eq!(one.plan_id, all.plan_id);
        assert_eq!(
            one.quantization_label.as_deref(),
            Some("q4_k_m, q4_k_m, q4_k_m")
        );
        assert!(!has(&one, Blocker::IncompleteGgufSet));
    }

    #[test]
    fn a_gguf_selection_carries_license_and_notice_without_labeling_them() {
        let mut license = gguf("LICENSE", 20, Role::License);
        license.required = true;
        license.upstream_hash = Some(UpstreamHash {
            algorithm: HashAlgorithm::GitSha1,
            value: "0123456789abcdef0123456789abcdef01234567".into(),
        });
        let mut notice = gguf("NOTICE", 5, Role::Other);
        notice.required = false;
        let files = vec![
            gguf("m-Q4_K_M-00001-of-00003.gguf", 10, Role::Weight),
            gguf("m-Q4_K_M-00002-of-00003.gguf", 11, Role::Weight),
            gguf("m-Q4_K_M-00003-of-00003.gguf", 12, Role::Weight),
            gguf("mmproj-f16.gguf", 7, Role::Projector),
            license,
            notice,
            gguf("README.md", 3, Role::Documentation),
        ];
        let mut inventory = inventory(files, None);
        inventory.license = LicenseEvidence {
            license_id: "apache-2.0".into(),
            status: EvidenceStatus::Matched,
            paths: vec!["LICENSE".into()],
        };
        let plan = build_plan_selecting(
            &config("selected-gguf-v1"),
            &inventory,
            u64::MAX,
            &["m-Q4_K_M-00002-of-00003.gguf".into()],
        )
        .unwrap();
        let selected: Vec<&str> = plan
            .selected_files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert_eq!(
            selected,
            vec![
                "LICENSE",
                "NOTICE",
                "m-Q4_K_M-00001-of-00003.gguf",
                "m-Q4_K_M-00002-of-00003.gguf",
                "m-Q4_K_M-00003-of-00003.gguf",
            ]
        );
        assert_eq!(plan.payload_bytes, 10 + 11 + 12 + 20 + 5);
        assert_eq!(
            plan.quantization_label.as_deref(),
            Some("Q4_K_M, Q4_K_M, Q4_K_M")
        );
        assert!(plan.excluded_files.contains(&"README.md".into()));
        assert!(plan.excluded_files.contains(&"mmproj-f16.gguf".into()));
        assert!(!plan.excluded_files.iter().any(|path| path == "LICENSE"));
        assert!(!has(&plan, Blocker::RequiredFileExcluded));
        assert!(!has(&plan, Blocker::MissingUpstreamHash));
        assert_eq!(plan.blockers, vec![Blocker::LocalInventoryUntrusted]);
        let rejected = build_plan_selecting(
            &config("selected-gguf-v1"),
            &inventory,
            u64::MAX,
            &["LICENSE".into()],
        )
        .unwrap();
        assert!(has(&rejected, Blocker::UnknownGgufSelection));
        assert!(rejected.selected_files.is_empty());
    }

    #[test]
    fn a_license_less_derivative_stays_unreviewed_unless_the_base_pin_is_captured() {
        let files = vec![gguf("only-Q4_K_M.gguf", 8, Role::Weight)];
        let bare = inventory(files.clone(), None);
        let plan = build_plan_selecting(
            &config("selected-gguf-v1"),
            &bare,
            u64::MAX,
            &["only-Q4_K_M.gguf".into()],
        )
        .unwrap();
        assert!(has(&plan, Blocker::LicenseNeedsReview));
        assert_eq!(plan.license_evidence_source.as_deref(), Some("absent"));
        assert_ne!(bare.license.status, EvidenceStatus::Matched);
        let unpinned = inventory(
            files.clone(),
            Some(BaseDependency {
                repo_id: "Qwen/Qwen3.8-27B".into(),
                revision: None,
                license_id: "apache-2.0".into(),
                license_status: Some(EvidenceStatus::Matched),
                license_paths: vec!["LICENSE".into()],
            }),
        );
        let waiting = build_plan_selecting(
            &config("selected-gguf-v1"),
            &unpinned,
            u64::MAX,
            &["only-Q4_K_M.gguf".into()],
        )
        .unwrap();
        assert!(has(&waiting, Blocker::LicenseNeedsReview));
        assert_eq!(waiting.base_repo_id.as_deref(), Some("Qwen/Qwen3.8-27B"));
        assert_eq!(waiting.base_revision.as_deref(), Some("not established"));
        let inherited = inventory(
            files,
            Some(BaseDependency {
                repo_id: "https://huggingface.co/Qwen/Qwen3.8-27B".into(),
                revision: Some("fedcba9876543210fedcba9876543210fedcba98".into()),
                license_id: "apache-2.0".into(),
                license_status: Some(EvidenceStatus::Matched),
                license_paths: vec!["LICENSE".into()],
            }),
        );
        assert_eq!(inherited.license.status, EvidenceStatus::Unreviewed);
        let allowed = build_plan_selecting(
            &config("selected-gguf-v1"),
            &inherited,
            u64::MAX,
            &["only-Q4_K_M.gguf".into()],
        )
        .unwrap();
        assert!(!has(&allowed, Blocker::LicenseNeedsReview));
        assert!(!has(&allowed, Blocker::LicenseNotAllowed));
        assert_eq!(
            allowed.license_evidence_source.as_deref(),
            Some("inherited")
        );
        assert_eq!(allowed.license_id.as_deref(), Some("apache-2.0"));
        assert_eq!(allowed.base_repo_id.as_deref(), Some("Qwen/Qwen3.8-27B"));
        assert_eq!(
            allowed.base_revision.as_deref(),
            Some("fedcba9876543210fedcba9876543210fedcba98")
        );
        assert_eq!(inherited.license.status, EvidenceStatus::Unreviewed);
        let mut refused = inherited.clone();
        refused.base.as_mut().unwrap().license_id = "cc-by-4.0".into();
        let refused = build_plan_selecting(
            &config("selected-gguf-v1"),
            &refused,
            u64::MAX,
            &["only-Q4_K_M.gguf".into()],
        )
        .unwrap();
        assert!(has(&refused, Blocker::LicenseNotAllowed));
        assert_eq!(
            refused.license_evidence_source.as_deref(),
            Some("inherited")
        );
        assert!(!has(&refused, Blocker::LicenseNeedsReview));
    }

    #[test]
    fn the_default_profile_rejects_selection_and_keeps_its_identity_shape() {
        let files = vec![gguf("only-Q4_K_M.gguf", 8, Role::Weight)];
        let error = build_plan_selecting(
            &config("publisher-safetensors-v1"),
            &inventory(files, None),
            u64::MAX,
            &["only-Q4_K_M.gguf".into()],
        )
        .unwrap_err();
        assert_eq!(error.code, "invalid_usage");
        let safetensors =
            crate::Inventory::parse(include_str!("../tests/fixtures/inventory.json")).unwrap();
        let plan = build_plan(&config("publisher-safetensors-v1"), &safetensors, u64::MAX).unwrap();
        let json = serde_json::to_value(&plan).unwrap();
        assert!(json.get("representation").is_none());
        assert!(json.get("gguf_candidates").is_none());
        assert!(json.get("license_evidence_source").is_none());
        let mut selected = config("selected-gguf-v1");
        selected.files.include_gguf = true;
        assert_eq!(selected.validate().unwrap_err().code, "unsupported_policy");
        selected.files.include_gguf = false;
        selected.validate().unwrap();
        assert_ne!(
            config("publisher-safetensors-v1")
                .canonical_bytes()
                .unwrap(),
            selected.canonical_bytes().unwrap()
        );
    }

    #[test]
    fn declared_quantization_labels_come_from_the_file_name() {
        for (path, label) in [
            ("Qwen3.8-27B-UD-Q4_K_M.gguf", "UD-Q4_K_M"),
            ("mmproj-BF16.gguf", "BF16"),
            ("model-q4_k_m-00001-of-00003.gguf", "q4_k_m"),
            ("PQ2_0.gguf", "PQ2_0"),
            ("PTQ1_0.gguf", "PTQ1_0"),
            ("readme.gguf", "unlabeled"),
        ] {
            assert_eq!(quantization_label(path), label, "{path}");
        }
    }

    #[test]
    fn unknown_paths_and_a_projector_alone_do_not_invent_a_weight() {
        let files = vec![
            gguf("weights-Q4_0.gguf", 5, Role::Weight),
            gguf("mmproj-F16.gguf", 1, Role::Projector),
        ];
        let inventory = inventory(files, None);
        let unknown = build_plan_selecting(
            &config("selected-gguf-v1"),
            &inventory,
            u64::MAX,
            &["missing.gguf".into()],
        )
        .unwrap();
        assert!(has(&unknown, Blocker::UnknownGgufSelection));
        assert_eq!(unknown.payload_bytes, 0);
        let projector = build_plan_selecting(
            &config("selected-gguf-v1"),
            &inventory,
            u64::MAX,
            &["mmproj-F16.gguf".into()],
        )
        .unwrap();
        assert!(has(&projector, Blocker::MissingRequiredRole));
        assert_eq!(projector.payload_bytes, 1);
        assert_eq!(projector.selected_files[0].role, Role::Projector);
    }
}
