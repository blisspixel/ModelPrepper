//! Data-only category list. The tool never fills a slot, recommends a model, or writes a vault.
use crate::{
    Config, Error, Inventory, Result,
    budget::{self, WORKSPACE_NOTE},
    validation::normalize_repo,
};
use serde::Serialize;
use std::collections::BTreeMap;

const SLOTS: &[(&str, &str)] = &[
    ("text", "text/chat"),
    ("coding", "coding"),
    ("vision", "vision (image-to-text)"),
    ("image_generation", "image generation"),
    ("video_generation", "video generation"),
    ("speech_to_text", "speech-to-text"),
    ("text_to_speech", "text-to-speech"),
    ("embeddings", "embeddings"),
    ("reference", "reference"),
];

#[derive(Debug, Serialize)]
pub struct KitReport {
    pub schema_version: u32,
    pub mode: &'static str,
    pub transfer_authorized: bool,
    pub network_contact: bool,
    pub vault_mutated: bool,
    pub rewritten: bool,
    pub supported: bool,
    pub hardware_tier_note: Option<String>,
    pub max_stored_bytes: u64,
    pub min_free_bytes: u64,
    pub workspace_bytes: Option<u64>,
    pub workspace_note: &'static str,
    pub slots: Vec<SlotReport>,
    pub physical_required_bytes: u64,
    pub physical_size_complete: bool,
    pub physical_remaining_bytes: Option<i64>,
    pub physical_shortfall_bytes: Option<u64>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SlotReport {
    pub id: String,
    pub display: String,
    pub known: bool,
    pub picks: Vec<PickReport>,
    /// Category attribution. The same model in two slots is counted in each slot.
    pub attributed_bytes: u64,
    pub attributed_complete: bool,
}

#[derive(Debug, Serialize)]
pub struct PickReport {
    pub repo_id: String,
    pub revision: String,
    pub representation: String,
    pub note: String,
    pub layout: String,
    pub license_id: String,
    pub license_status: String,
    pub required_bytes: Option<u64>,
    pub size_known: bool,
    pub verified_local: Option<bool>,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct KitDocument {
    pub hardware_tier: Option<String>,
    pub slots: Vec<RawSlot>,
}

#[derive(Clone, Debug)]
pub struct RawSlot {
    pub id: String,
    pub display: String,
    pub known: bool,
    pub picks: Vec<RawPick>,
}

#[derive(Clone, Debug)]
pub struct RawPick {
    pub repo_id: String,
    pub revision: String,
    pub representation: String,
    pub note: String,
    pub select: Vec<String>,
    pub evidence_path: Option<String>,
    pub invalid: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SummaryEvidence {
    pub source_kind: String,
    pub layout: String,
    pub license_id: String,
    pub gated: bool,
    pub private: bool,
    pub required_bytes: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct LoadedEvidence {
    pub schema_ok: bool,
    pub verified_local: bool,
    pub inventory: Option<Inventory>,
    pub summary: Option<SummaryEvidence>,
    pub invalid: Option<String>,
}

pub enum EvidenceLoad {
    Missing,
    Unreadable(String),
    Loaded(Box<LoadedEvidence>),
}

pub enum ParsedKit {
    Unsupported,
    Document(KitDocument),
}

pub fn parse(text: &str) -> Result<ParsedKit> {
    let value: toml::Value =
        toml::from_str(text).map_err(|error| Error::new("invalid_kit", error.to_string()))?;
    let Some(table) = value.as_table() else {
        return Ok(ParsedKit::Unsupported);
    };
    if table
        .get("schema_version")
        .and_then(toml::Value::as_integer)
        != Some(1)
    {
        return Ok(ParsedKit::Unsupported);
    }
    let hardware_tier = match table.get("hardware_tier") {
        None => None,
        Some(toml::Value::String(note)) => Some(note.clone()),
        Some(_) => return Ok(ParsedKit::Unsupported),
    };
    let mut slots = Vec::new();
    for (id, display) in SLOTS {
        let picks = match table.get(*id) {
            None => Vec::new(),
            Some(value) => parse_picks(value),
        };
        slots.push(RawSlot {
            id: (*id).into(),
            display: (*display).into(),
            known: true,
            picks,
        });
    }
    let mut unknown: Vec<_> = table
        .keys()
        .filter(|key| {
            *key != "schema_version"
                && *key != "hardware_tier"
                && !SLOTS.iter().any(|(id, _)| id == key)
        })
        .cloned()
        .collect();
    unknown.sort();
    for id in unknown {
        let picks = parse_picks(&table[&id]);
        slots.push(RawSlot {
            id: id.clone(),
            display: id,
            known: false,
            picks,
        });
    }
    Ok(ParsedKit::Document(KitDocument {
        hardware_tier,
        slots,
    }))
}

fn parse_picks(value: &toml::Value) -> Vec<RawPick> {
    let Some(items) = value.as_array() else {
        return vec![invalid_pick("slot must be an array of picks")];
    };
    if items.is_empty() {
        return Vec::new();
    }
    items.iter().map(parse_pick).collect()
}

fn parse_pick(value: &toml::Value) -> RawPick {
    let Some(table) = value.as_table() else {
        return invalid_pick("pick must be a table");
    };
    let mut unknown: Vec<_> = table
        .keys()
        .filter(|key| {
            !matches!(
                key.as_str(),
                "repo_id" | "revision" | "representation" | "note" | "select" | "evidence"
            )
        })
        .cloned()
        .collect();
    unknown.sort();
    let mut pick = RawPick {
        repo_id: string_field(table, "repo_id"),
        revision: string_field(table, "revision"),
        representation: string_field(table, "representation"),
        note: match table.get("note") {
            None => String::new(),
            Some(toml::Value::String(note)) => note.clone(),
            Some(_) => String::new(),
        },
        select: string_list(table.get("select")),
        evidence_path: match table.get("evidence") {
            Some(toml::Value::String(path)) if !path.is_empty() => Some(path.clone()),
            _ => None,
        },
        invalid: None,
    };
    if !unknown.is_empty() {
        pick.invalid = Some(format!("unknown pick field {}", unknown.join(", ")));
    } else if table.get("note").is_some() && !table.get("note").is_some_and(toml::Value::is_str) {
        pick.invalid = Some("note must be a string".into());
    } else if table.get("select").is_some() && pick.select.is_empty() && !empty_select(table) {
        pick.invalid = Some("select must be an array of paths".into());
    } else if table.get("evidence").is_some() && pick.evidence_path.is_none() {
        pick.invalid = Some("evidence must be a path".into());
    } else if pick.repo_id.is_empty() || pick.revision.is_empty() || pick.representation.is_empty()
    {
        pick.invalid = Some("pick needs repo_id, revision, and representation".into());
    }
    pick
}

fn empty_select(table: &toml::map::Map<String, toml::Value>) -> bool {
    table
        .get("select")
        .and_then(toml::Value::as_array)
        .is_some_and(Vec::is_empty)
}

fn string_field(table: &toml::map::Map<String, toml::Value>, key: &str) -> String {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn string_list(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn invalid_pick(reason: &str) -> RawPick {
    RawPick {
        repo_id: String::new(),
        revision: String::new(),
        representation: String::new(),
        note: String::new(),
        select: Vec::new(),
        evidence_path: None,
        invalid: Some(reason.into()),
    }
}

pub fn load_evidence_text(text: &str) -> LoadedEvidence {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return LoadedEvidence {
            schema_ok: false,
            verified_local: false,
            inventory: None,
            summary: None,
            invalid: Some("evidence is not JSON".into()),
        };
    };
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return LoadedEvidence {
            schema_ok: false,
            verified_local: false,
            inventory: None,
            summary: None,
            invalid: Some("unsupported evidence schema".into()),
        };
    }
    let verified_local = value
        .get("verified_local")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let inventory = value.get("inventory").and_then(|inventory| {
        serde_json::from_value::<Inventory>(inventory.clone())
            .ok()
            .filter(|inventory| inventory.validate().is_ok())
    });
    let inventory_invalid = value.get("inventory").is_some() && inventory.is_none();
    let summary = value.get("source_kind").and_then(|kind| {
        Some(SummaryEvidence {
            source_kind: kind.as_str()?.to_owned(),
            layout: value
                .get("layout")
                .and_then(|item| item.as_str())?
                .to_owned(),
            license_id: value
                .get("license_id")
                .and_then(|item| item.as_str())
                .unwrap_or("unknown")
                .to_owned(),
            gated: value
                .get("gated")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            private: value
                .get("private")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            required_bytes: match value.get("required_bytes") {
                None | Some(serde_json::Value::Null) => None,
                Some(number) => number.as_u64(),
            },
        })
    });
    LoadedEvidence {
        schema_ok: !inventory_invalid,
        verified_local,
        inventory,
        summary,
        invalid: inventory_invalid.then(|| "evidence inventory is invalid".into()),
    }
}

pub fn report(
    config: &Config,
    parsed: &ParsedKit,
    evidence_for: &dyn Fn(&str) -> EvidenceLoad,
) -> Result<KitReport> {
    let ParsedKit::Document(document) = parsed else {
        return Ok(empty_report(
            config,
            None,
            vec!["unsupported_schema".into()],
            false,
        ));
    };
    let mut slots = Vec::new();
    let mut physical: BTreeMap<String, u64> = BTreeMap::new();
    let mut complete = true;
    let mut conflict = false;
    for slot in &document.slots {
        let mut picks = Vec::new();
        let mut attributed = 0_u64;
        let mut slot_complete = true;
        for pick in &slot.picks {
            let mut status = assess_pick(config, pick, evidence_for)?;
            if !status.size_known {
                slot_complete = false;
                complete = false;
            } else if let Some(bytes) = status.required_bytes {
                attributed = attributed.saturating_add(bytes);
                let select_key = if status.representation == "selected-derivative" {
                    pick.select.join("+")
                } else {
                    String::new()
                };
                let key = format!(
                    "{}|{}|{}|{select_key}",
                    status.repo_id, status.revision, status.representation
                );
                match physical.get(&key) {
                    Some(previous) if *previous != bytes => conflict = true,
                    Some(_) => {}
                    None => {
                        physical.insert(key, bytes);
                    }
                }
            }
            if !slot.known {
                push_reason(&mut status.reasons, "unsupported_slot");
                status.reasons.sort();
                status.reasons.dedup();
            }
            picks.push(status);
        }
        slots.push(SlotReport {
            id: slot.id.clone(),
            display: slot.display.clone(),
            known: slot.known,
            picks,
            attributed_bytes: attributed,
            attributed_complete: slot_complete,
        });
    }
    let mut blockers = Vec::new();
    if conflict {
        blockers.push("conflicting_candidate_size".into());
        complete = false;
    }
    let physical_required_bytes = physical
        .values()
        .try_fold(0_u64, |sum, size| sum.checked_add(*size));
    if physical_required_bytes.is_none() {
        blockers.push("size_overflow".into());
        complete = false;
    }
    let physical_required_bytes = physical_required_bytes.unwrap_or(u64::MAX);
    let max = config.storage.max_stored_bytes;
    let (physical_remaining_bytes, physical_shortfall_bytes) = if physical_required_bytes > max {
        blockers.push("over_budget".into());
        (
            i64::try_from(i128::from(max) - i128::from(physical_required_bytes)).ok(),
            Some(physical_required_bytes - max),
        )
    } else if complete {
        (
            i64::try_from(i128::from(max) - i128::from(physical_required_bytes)).ok(),
            Some(0),
        )
    } else {
        (None, None)
    };
    blockers.sort();
    blockers.dedup();
    Ok(KitReport {
        schema_version: 1,
        mode: "offline_kit",
        transfer_authorized: false,
        network_contact: false,
        vault_mutated: false,
        rewritten: false,
        supported: true,
        hardware_tier_note: document.hardware_tier.clone(),
        max_stored_bytes: max,
        min_free_bytes: config.storage.min_free_bytes,
        workspace_bytes: None,
        workspace_note: WORKSPACE_NOTE,
        slots,
        physical_required_bytes,
        physical_size_complete: complete,
        physical_remaining_bytes,
        physical_shortfall_bytes,
        blockers,
    })
}

fn empty_report(
    config: &Config,
    hardware_tier_note: Option<String>,
    blockers: Vec<String>,
    supported: bool,
) -> KitReport {
    KitReport {
        schema_version: 1,
        mode: "offline_kit",
        transfer_authorized: false,
        network_contact: false,
        vault_mutated: false,
        rewritten: false,
        supported,
        hardware_tier_note,
        max_stored_bytes: config.storage.max_stored_bytes,
        min_free_bytes: config.storage.min_free_bytes,
        workspace_bytes: None,
        workspace_note: WORKSPACE_NOTE,
        slots: Vec::new(),
        physical_required_bytes: 0,
        physical_size_complete: false,
        physical_remaining_bytes: None,
        physical_shortfall_bytes: None,
        blockers,
    }
}

fn assess_pick(
    config: &Config,
    pick: &RawPick,
    evidence_for: &dyn Fn(&str) -> EvidenceLoad,
) -> Result<PickReport> {
    if let Some(reason) = &pick.invalid {
        return Ok(blank_pick(
            pick,
            vec![reason.clone(), "invalid_pick".into()],
        ));
    }
    let Ok(repo_id) = normalize_repo(&pick.repo_id) else {
        return Ok(blank_pick(pick, vec!["invalid_pick".into()]));
    };
    if pick.revision != "unpinned" && crate::validation::validate_hex(&pick.revision, 40).is_err() {
        return Ok(blank_pick(pick, vec!["invalid_pick".into()]));
    }
    let representation = pick.representation.as_str();
    if !matches!(representation, "original" | "selected-derivative") {
        return Ok(blank_pick(pick, vec!["invalid_pick".into()]));
    }
    let mut reasons = Vec::new();
    if pick.revision == "unpinned" {
        reasons.push("unpinned".into());
    }
    let Some(path) = &pick.evidence_path else {
        reasons.push("no_saved_review".into());
        reasons.push("unknown_size".into());
        reasons.sort();
        return Ok(PickReport {
            repo_id,
            revision: pick.revision.clone(),
            representation: representation.into(),
            note: pick.note.clone(),
            layout: "unknown".into(),
            license_id: "unknown".into(),
            license_status: "unreviewed".into(),
            required_bytes: None,
            size_known: false,
            verified_local: None,
            reasons,
        });
    };
    let loaded = match evidence_for(path) {
        EvidenceLoad::Missing => {
            reasons.push("no_saved_review".into());
            reasons.push("unknown_size".into());
            reasons.sort();
            return Ok(PickReport {
                repo_id,
                revision: pick.revision.clone(),
                representation: representation.into(),
                note: pick.note.clone(),
                layout: "unknown".into(),
                license_id: "unknown".into(),
                license_status: "unreviewed".into(),
                required_bytes: None,
                size_known: false,
                verified_local: None,
                reasons,
            });
        }
        EvidenceLoad::Unreadable(message) => {
            reasons.push("no_saved_review".into());
            reasons.push(message);
            reasons.sort();
            return Ok(blank_with(&repo_id, pick, None, reasons));
        }
        EvidenceLoad::Loaded(loaded) => loaded,
    };
    if !loaded.schema_ok {
        reasons.push("unsupported_schema".into());
        reasons.push("unknown_size".into());
        reasons.sort();
        return Ok(blank_with(
            &repo_id,
            pick,
            Some(loaded.verified_local),
            reasons,
        ));
    }
    if let Some(inventory) = &loaded.inventory {
        if normalize_repo(&inventory.source.repo_id).ok().as_deref() != Some(repo_id.as_str()) {
            reasons.push("evidence_repo_mismatch".into());
            reasons.push("unknown_size".into());
            reasons.sort();
            return Ok(blank_with(
                &repo_id,
                pick,
                Some(loaded.verified_local),
                reasons,
            ));
        }
        if pick.revision != "unpinned" && inventory.source.resolved_revision != pick.revision {
            reasons.push("revision_mismatch".into());
            reasons.push("unknown_size".into());
            reasons.sort();
            return Ok(blank_with(
                &repo_id,
                pick,
                Some(loaded.verified_local),
                reasons,
            ));
        }
        let profile_representation = if representation == "selected-derivative" {
            "derivative"
        } else {
            "original"
        };
        let assessment =
            budget::assess_inventory(config, profile_representation, inventory, &pick.select)?;
        reasons.extend(assessment.blockers);
        let size_known = assessment.required_bytes.is_some();
        if !size_known {
            push_reason(&mut reasons, "unknown_size");
        }
        reasons.sort();
        reasons.dedup();
        return Ok(PickReport {
            repo_id,
            revision: pick.revision.clone(),
            representation: representation.into(),
            note: pick.note.clone(),
            layout: inventory.layout.clone(),
            license_id: assessment.license_id,
            license_status: assessment.license_status,
            required_bytes: assessment.required_bytes,
            size_known,
            verified_local: Some(loaded.verified_local),
            reasons,
        });
    }
    let Some(summary) = &loaded.summary else {
        reasons.push("no_saved_review".into());
        reasons.push("unknown_size".into());
        reasons.sort();
        return Ok(blank_with(
            &repo_id,
            pick,
            Some(loaded.verified_local),
            reasons,
        ));
    };
    if summary.source_kind != "huggingface" {
        reasons.push("unsupported_source".into());
    }
    let supported = match representation {
        "original" => summary.layout == "transformers-safetensors-v1",
        _ => summary.layout == "gguf-v1",
    };
    if !supported {
        reasons.push(if summary.source_kind == "huggingface" {
            "unsupported_layout".into()
        } else {
            "unsupported_source".into()
        });
    }
    if summary.gated || summary.private {
        reasons.push("access_restricted".into());
    }
    if !config
        .licenses
        .allow
        .iter()
        .any(|id| id == &summary.license_id)
    {
        reasons.push("license_not_allowed".into());
    }
    reasons.push("license_needs_review".into());
    let size_known = summary.required_bytes.is_some();
    if !size_known {
        reasons.push("unknown_size".into());
    }
    reasons.sort();
    reasons.dedup();
    Ok(PickReport {
        repo_id,
        revision: pick.revision.clone(),
        representation: representation.into(),
        note: pick.note.clone(),
        layout: summary.layout.clone(),
        license_id: summary.license_id.clone(),
        license_status: "unreviewed".into(),
        required_bytes: summary.required_bytes,
        size_known,
        verified_local: Some(loaded.verified_local),
        reasons,
    })
}

fn blank_pick(pick: &RawPick, mut reasons: Vec<String>) -> PickReport {
    reasons.sort();
    reasons.dedup();
    PickReport {
        repo_id: pick.repo_id.clone(),
        revision: pick.revision.clone(),
        representation: pick.representation.clone(),
        note: pick.note.clone(),
        layout: "unknown".into(),
        license_id: "unknown".into(),
        license_status: "unreviewed".into(),
        required_bytes: None,
        size_known: false,
        verified_local: None,
        reasons,
    }
}

fn blank_with(
    repo_id: &str,
    pick: &RawPick,
    verified_local: Option<bool>,
    mut reasons: Vec<String>,
) -> PickReport {
    reasons.sort();
    reasons.dedup();
    PickReport {
        repo_id: repo_id.into(),
        revision: pick.revision.clone(),
        representation: pick.representation.clone(),
        note: pick.note.clone(),
        layout: "unknown".into(),
        license_id: "unknown".into(),
        license_status: "unreviewed".into(),
        required_bytes: None,
        size_known: false,
        verified_local,
        reasons,
    }
}

fn push_reason(reasons: &mut Vec<String>, reason: &str) {
    let reason = reason.to_owned();
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{
        BaseDependency, EvidenceStatus, HashAlgorithm, LicenseEvidence, Role, Source, SourceFile,
        UpstreamHash,
    };

    const PIN: &str = "0123456789abcdef0123456789abcdef01234567";

    fn config() -> Config {
        let mut config = Config::default_for("catalog".into(), "models".into());
        config.storage.max_stored_bytes = 100;
        config.storage.min_free_bytes = 1;
        config
    }

    fn file(path: &str, size: u64, role: Role) -> SourceFile {
        SourceFile {
            path: path.into(),
            size_bytes: size,
            upstream_hash: Some(UpstreamHash {
                algorithm: HashAlgorithm::Sha256,
                value: "cd".repeat(32),
            }),
            role,
            required: true,
        }
    }

    fn inventory(
        repo: &str,
        layout: &str,
        license_id: &str,
        status: EvidenceStatus,
        files: Vec<SourceFile>,
        gated: bool,
        base: Option<BaseDependency>,
    ) -> Inventory {
        Inventory {
            schema_version: 1,
            source: Source {
                endpoint: "https://huggingface.co".into(),
                repo_type: "model".into(),
                repo_id: repo.into(),
                requested_revision: "main".into(),
                resolved_revision: PIN.into(),
                gated,
                private: false,
            },
            complete: true,
            layout: layout.into(),
            license: LicenseEvidence {
                license_id: license_id.into(),
                status,
                paths: if matches!(status, EvidenceStatus::Matched) {
                    vec!["LICENSE".into()]
                } else {
                    Vec::new()
                },
            },
            files,
            base,
        }
    }

    fn safetensors(repo: &str, license_id: &str, weight: u64, gated: bool) -> Inventory {
        inventory(
            repo,
            "transformers-safetensors-v1",
            license_id,
            EvidenceStatus::Matched,
            vec![
                file("model.safetensors", weight, Role::Weight),
                file("config.json", 0, Role::Config),
                file("tokenizer.json", 0, Role::Tokenizer),
                file("LICENSE", 0, Role::License),
            ],
            gated,
            None,
        )
    }

    fn evidence(inventory: &Inventory, verified_local: bool) -> String {
        serde_json::json!({
            "schema_version": 1,
            "verified_local": verified_local,
            "inventory": inventory,
        })
        .to_string()
    }

    fn slot<'a>(report: &'a KitReport, id: &str) -> &'a SlotReport {
        report.slots.iter().find(|slot| slot.id == id).unwrap()
    }

    #[test]
    fn slots_keep_reasons_and_charge_one_physical_copy() {
        let original = safetensors("example/original", "apache-2.0", 60, false);
        let diffusers = inventory(
            "example/diffusers",
            "diffusers",
            "apache-2.0",
            EvidenceStatus::Matched,
            vec![
                file("transformer.safetensors", 50, Role::Weight),
                file("LICENSE", 0, Role::License),
            ],
            false,
            None,
        );
        let gated = safetensors("example/gated", "apache-2.0", 10, true);
        let refused = safetensors("example/cc", "cc-by-4.0", 10, false);
        let derivative = inventory(
            "example/gguf",
            "gguf-v1",
            "",
            EvidenceStatus::Unreviewed,
            vec![file("weights-Q8_0.gguf", 7, Role::Weight)],
            false,
            Some(BaseDependency {
                repo_id: "Qwen/Qwen3.8-27B".into(),
                revision: Some(PIN.into()),
                license_id: "apache-2.0".into(),
                license_status: Some(EvidenceStatus::Matched),
                license_paths: vec!["LICENSE".into()],
            }),
        );
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        files.insert("original.json".into(), evidence(&original, true));
        files.insert("diffusers.json".into(), evidence(&diffusers, false));
        files.insert("gated.json".into(), evidence(&gated, false));
        files.insert("cc.json".into(), evidence(&refused, false));
        files.insert("gguf.json".into(), evidence(&derivative, true));
        files.insert(
            "zim.json".into(),
            serde_json::json!({
                "schema_version": 1,
                "source_kind": "other",
                "layout": "zim",
                "license_id": "cc-by-sa-4.0",
                "required_bytes": 10
            })
            .to_string(),
        );
        let document = r#"
schema_version = 1
hardware_tier = "desk note, not a recommendation"

[[text]]
repo_id = "example/original"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "original.json"

[[coding]]
repo_id = "example/original"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "original.json"

[[vision]]
repo_id = "example/diffusers"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "diffusers.json"

[[image_generation]]
repo_id = "example/gated"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "gated.json"

[[speech_to_text]]
repo_id = "example/cc"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "cc.json"

[[text_to_speech]]
repo_id = "example/speech"
revision = "unpinned"
representation = "original"
note = "named by the operator"

[[embeddings]]
repo_id = "example/gguf"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "selected-derivative"
select = ["weights-Q8_0.gguf"]
evidence = "gguf.json"

[[reference]]
repo_id = "example/reference"
revision = "unpinned"
representation = "original"
evidence = "zim.json"

[[extra]]
repo_id = "example/original"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "original.json"
"#;
        let parsed = parse(document).unwrap();
        let report = report(&config(), &parsed, &|path| {
            files
                .get(path)
                .map(|text| EvidenceLoad::Loaded(Box::new(load_evidence_text(text))))
                .unwrap_or(EvidenceLoad::Missing)
        })
        .unwrap();
        assert_eq!(
            report.hardware_tier_note.as_deref(),
            Some("desk note, not a recommendation")
        );
        assert!(!report.transfer_authorized);
        assert!(!report.network_contact);
        assert!(!report.vault_mutated);
        assert!(!report.rewritten);
        assert!(report.workspace_bytes.is_none());
        assert!(slot(&report, "video_generation").picks.is_empty());
        assert_eq!(slot(&report, "text").attributed_bytes, 60);
        assert_eq!(slot(&report, "coding").attributed_bytes, 60);
        assert_eq!(slot(&report, "text").picks[0].verified_local, Some(true));
        let vision = &slot(&report, "vision").picks[0];
        assert_eq!(vision.required_bytes, Some(50));
        assert!(
            vision
                .reasons
                .iter()
                .any(|reason| reason == "unsupported_layout")
        );
        let gated_pick = &slot(&report, "image_generation").picks[0];
        assert!(
            gated_pick
                .reasons
                .iter()
                .any(|reason| reason == "access_restricted")
        );
        assert_eq!(gated_pick.required_bytes, Some(10));
        let refused_pick = &slot(&report, "speech_to_text").picks[0];
        assert!(
            refused_pick
                .reasons
                .iter()
                .any(|reason| reason == "license_not_allowed")
        );
        assert!(
            !refused_pick
                .reasons
                .iter()
                .any(|reason| reason == "license_needs_review")
        );
        let unpinned = &slot(&report, "text_to_speech").picks[0];
        assert!(unpinned.reasons.iter().any(|reason| reason == "unpinned"));
        assert!(
            unpinned
                .reasons
                .iter()
                .any(|reason| reason == "no_saved_review")
        );
        assert!(!unpinned.size_known);
        let inherited = &slot(&report, "embeddings").picks[0];
        assert_eq!(inherited.license_status, "inherited");
        assert_eq!(inherited.license_id, "apache-2.0");
        assert_eq!(inherited.required_bytes, Some(7));
        assert!(
            !inherited
                .reasons
                .iter()
                .any(|reason| reason == "license_needs_review")
        );
        let reference = &slot(&report, "reference").picks[0];
        assert!(
            reference
                .reasons
                .iter()
                .any(|reason| reason == "unsupported_source")
        );
        assert_eq!(reference.required_bytes, Some(10));
        let extra = &slot(&report, "extra").picks[0];
        assert!(!slot(&report, "extra").known);
        assert!(
            extra
                .reasons
                .iter()
                .any(|reason| reason == "unsupported_slot")
        );
        assert_eq!(report.physical_required_bytes, 147);
        assert!(!report.physical_size_complete);
        assert_eq!(report.physical_shortfall_bytes, Some(47));
        assert!(
            report
                .blockers
                .iter()
                .any(|blocker| blocker == "over_budget")
        );
        let text = crate::present::render_kit(&report, false, 200);
        assert!(text.contains("empty"), "{text}");
        assert!(text.contains("recommendation"), "{text}");
        assert!(text.contains("desk note, not a recommendation"), "{text}");
        assert!(text.contains("verified-local"), "{text}");
    }

    #[test]
    fn unknown_schema_is_reported_and_not_rewritten() {
        let parsed = parse("schema_version = 2\nhardware_tier = 1\n").unwrap();
        assert!(matches!(parsed, ParsedKit::Unsupported));
        let report = report(&config(), &parsed, &|_| panic!("evidence is not read")).unwrap();
        assert!(!report.supported);
        assert!(report.slots.is_empty());
        assert_eq!(report.blockers, ["unsupported_schema"]);
        assert!(!report.rewritten);
        assert!(!report.network_contact);
        assert!(!report.vault_mutated);
        let text = crate::present::render_kit(&report, false, 80);
        assert!(text.contains("unsupported"), "{text}");
        assert!(matches!(
            parse("schema_version = 1\nhardware_tier = 1\n").unwrap(),
            ParsedKit::Unsupported
        ));
    }

    #[test]
    fn bad_picks_and_disagreeing_evidence_stay_visible() {
        let small = safetensors("example/model", "apache-2.0", 4, false);
        let large = safetensors("example/model", "apache-2.0", 9, false);
        let other = safetensors("example/other", "apache-2.0", 4, false);
        let mut moved = small.clone();
        moved.source.resolved_revision = "fedcba9876543210fedcba9876543210fedcba98".into();
        let mut files: BTreeMap<String, String> = BTreeMap::new();
        files.insert("small.json".into(), evidence(&small, false));
        files.insert("large.json".into(), evidence(&large, false));
        files.insert("other.json".into(), evidence(&other, false));
        files.insert("moved.json".into(), evidence(&moved, false));
        files.insert("old.json".into(), "{\"schema_version\":2}".into());
        files.insert(
            "summary.json".into(),
            serde_json::json!({
                "schema_version": 1,
                "source_kind": "huggingface",
                "layout": "diffusers",
                "license_id": "apache-2.0",
                "required_bytes": 3
            })
            .to_string(),
        );
        let document = r#"
schema_version = 1
embeddings = "nope"
reference = ["nope"]

[[text]]
surprise = 1
repo_id = "example/model"
revision = "unpinned"
representation = "original"

[[text]]
repo_id = "example/model"
revision = "unpinned"
representation = "original"
note = 1

[[text]]
repo_id = "not a repo"
revision = "unpinned"
representation = "original"

[[text]]
repo_id = "example/model"
revision = "zz"
representation = "original"

[[text]]
repo_id = "example/model"
revision = "unpinned"
representation = "quant"

[[text]]
repo_id = "example/model"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "absent.json"

[[text]]
repo_id = "example/model"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "locked.json"

[[text]]
repo_id = "example/model"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "old.json"

[[text]]
repo_id = "example/model"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "other.json"

[[text]]
repo_id = "example/model"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "moved.json"

[[coding]]
repo_id = "example/model"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "small.json"

[[vision]]
repo_id = "example/model"
revision = "0123456789abcdef0123456789abcdef01234567"
representation = "original"
evidence = "large.json"

[[image_generation]]
repo_id = "example/diffusers"
revision = "unpinned"
representation = "original"
evidence = "summary.json"
"#;
        let report = report(&config(), &parse(document).unwrap(), &|path| match path {
            "locked.json" => EvidenceLoad::Unreadable("input_read_failed".into()),
            other => files
                .get(other)
                .map(|text| EvidenceLoad::Loaded(Box::new(load_evidence_text(text))))
                .unwrap_or(EvidenceLoad::Missing),
        })
        .unwrap();
        let text = &slot(&report, "text").picks;
        assert!(text.iter().any(|pick| {
            pick.reasons
                .iter()
                .any(|reason| reason.contains("unknown pick field"))
        }));
        assert!(
            text.iter()
                .any(|pick| pick.reasons.iter().any(|reason| reason == "invalid_pick"))
        );
        assert!(text.iter().any(|pick| {
            pick.reasons
                .iter()
                .any(|reason| reason == "no_saved_review")
        }));
        assert!(text.iter().any(|pick| {
            pick.reasons
                .iter()
                .any(|reason| reason == "input_read_failed")
        }));
        assert!(text.iter().any(|pick| {
            pick.reasons
                .iter()
                .any(|reason| reason == "unsupported_schema")
        }));
        assert!(text.iter().any(|pick| {
            pick.reasons
                .iter()
                .any(|reason| reason == "evidence_repo_mismatch")
        }));
        assert!(text.iter().any(|pick| {
            pick.reasons
                .iter()
                .any(|reason| reason == "revision_mismatch")
        }));
        assert!(
            slot(&report, "embeddings").picks[0]
                .reasons
                .iter()
                .any(|reason| reason == "invalid_pick")
        );
        assert!(
            slot(&report, "reference").picks[0]
                .reasons
                .iter()
                .any(|reason| reason == "invalid_pick")
        );
        let summary = &slot(&report, "image_generation").picks[0];
        assert!(
            summary
                .reasons
                .iter()
                .any(|reason| reason == "unsupported_layout")
        );
        assert_eq!(summary.required_bytes, Some(3));
        assert!(
            report
                .blockers
                .iter()
                .any(|blocker| blocker == "conflicting_candidate_size")
        );
        assert!(!report.physical_size_complete);
    }
}
