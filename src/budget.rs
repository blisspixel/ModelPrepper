//! Offline comparison of pinned candidate sizes. Nothing is reserved or transferred.
use crate::{
    Config, Error, Inventory, Result,
    inventory::EvidenceStatus,
    plan::{self, build_plan, build_plan_selecting},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WORKSPACE_NOTE: &str =
    "workspace bytes are not measured; this comparison does not reserve space or check a disk";

#[derive(Debug, Serialize)]
pub struct BudgetReport {
    pub schema_version: u32,
    pub mode: &'static str,
    pub transfer_authorized: bool,
    pub rewritten: bool,
    pub supported: bool,
    pub max_stored_bytes: u64,
    pub min_free_bytes: u64,
    pub workspace_bytes: Option<u64>,
    pub workspace_note: &'static str,
    pub rows: Vec<BudgetRow>,
    /// Sum of distinct known artifacts. Unknown rows are left out, not added as zero.
    pub combined_required_bytes: u64,
    pub combined_size_complete: bool,
    pub combined_remaining_bytes: Option<i64>,
    pub combined_shortfall_bytes: Option<u64>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BudgetRow {
    pub candidate: String,
    pub representation: String,
    pub identity: String,
    pub required_bytes: Option<u64>,
    pub size_known: bool,
    pub remaining_bytes: Option<i64>,
    pub shortfall_bytes: Option<u64>,
    pub at_storage_ceiling: bool,
    pub fits_with_reserve: Option<bool>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetRequest {
    schema_version: u32,
    candidates: Vec<BudgetCandidate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetCandidate {
    name: String,
    #[serde(default)]
    inventory: Option<Inventory>,
    #[serde(default)]
    select: Vec<String>,
    #[serde(default)]
    representation: Option<String>,
    #[serde(default)]
    size_known: Option<bool>,
}

pub fn report_json(config: &Config, text: &str) -> Result<BudgetReport> {
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|error| Error::new("invalid_budget", error.to_string()))?;
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
    {
        return Ok(unsupported(config));
    }
    let request: BudgetRequest = serde_json::from_value(value)
        .map_err(|error| Error::new("invalid_budget", error.to_string()))?;
    let _ = request.schema_version;
    let mut names = BTreeMap::new();
    for candidate in &request.candidates {
        if candidate.name.is_empty() || candidate.name.contains(['\n', '\r']) {
            return Err(Error::new(
                "invalid_budget",
                "each candidate needs a one-line name",
            ));
        }
        if names.insert(candidate.name.clone(), ()).is_some() {
            return Err(Error::new(
                "invalid_budget",
                format!("duplicate candidate name {}", candidate.name),
            ));
        }
    }
    let mut rows = Vec::new();
    for candidate in &request.candidates {
        rows.push(row(config, candidate)?);
    }
    Ok(finish(config, rows, Vec::new(), true))
}

fn unsupported(config: &Config) -> BudgetReport {
    finish(config, Vec::new(), vec!["unsupported_schema".into()], false)
}

fn finish(
    config: &Config,
    rows: Vec<BudgetRow>,
    mut blockers: Vec<String>,
    supported: bool,
) -> BudgetReport {
    let mut seen = BTreeMap::<String, u64>::new();
    let mut complete = supported;
    for row in &rows {
        if !row.size_known {
            complete = false;
            continue;
        }
        let bytes = row.required_bytes.unwrap_or(0);
        if let Some(previous) = seen.get(&row.identity) {
            if *previous != bytes {
                blockers.push("conflicting_candidate_size".into());
                complete = false;
            }
        } else {
            seen.insert(row.identity.clone(), bytes);
        }
    }
    let combined = seen
        .values()
        .try_fold(0_u64, |sum, size| sum.checked_add(*size));
    let combined_required_bytes = combined.unwrap_or(u64::MAX);
    if combined.is_none() {
        blockers.push("size_overflow".into());
        complete = false;
    }
    blockers.sort();
    blockers.dedup();
    let (combined_remaining_bytes, combined_shortfall_bytes) = combined_bounds(
        config.storage.max_stored_bytes,
        combined_required_bytes,
        complete,
    );
    BudgetReport {
        schema_version: 1,
        mode: "offline_budget",
        transfer_authorized: false,
        rewritten: false,
        supported,
        max_stored_bytes: config.storage.max_stored_bytes,
        min_free_bytes: config.storage.min_free_bytes,
        workspace_bytes: None,
        workspace_note: WORKSPACE_NOTE,
        rows,
        combined_required_bytes,
        combined_size_complete: complete,
        combined_remaining_bytes,
        combined_shortfall_bytes,
        blockers,
    }
}

fn combined_bounds(max_stored: u64, known: u64, complete: bool) -> (Option<i64>, Option<u64>) {
    if known > max_stored {
        return (
            signed_diff(max_stored, known).ok(),
            Some(known - max_stored),
        );
    }
    if !complete {
        return (None, None);
    }
    (signed_diff(max_stored, known).ok(), Some(0))
}

fn row(config: &Config, candidate: &BudgetCandidate) -> Result<BudgetRow> {
    if candidate.size_known == Some(false) {
        return Ok(unknown_row(
            &candidate.name,
            candidate.representation.as_deref().unwrap_or("unknown"),
            vec!["unknown_size".into()],
        ));
    }
    let Some(inventory) = &candidate.inventory else {
        return Ok(unknown_row(
            &candidate.name,
            candidate.representation.as_deref().unwrap_or("unknown"),
            vec!["unknown_size".into()],
        ));
    };
    if let Err(error) = inventory.validate() {
        return Ok(unknown_row(
            &candidate.name,
            candidate.representation.as_deref().unwrap_or("unknown"),
            vec![error.code.into()],
        ));
    }
    let representation = match candidate.representation.as_deref() {
        Some("original") => "original",
        Some("derivative") => "derivative",
        Some(_) => {
            return Ok(unknown_row(
                &candidate.name,
                "unknown",
                vec!["invalid_representation".into()],
            ));
        }
        None if inventory.layout == "gguf-v1" || !candidate.select.is_empty() => "derivative",
        None => "original",
    };
    let select_key = if representation == "derivative" {
        candidate.select.join("+")
    } else {
        String::new()
    };
    let identity = format!(
        "{}@{}:{representation}:{select_key}",
        inventory.source.repo_id, inventory.source.resolved_revision
    );
    if !inventory.complete {
        let mut blockers = rights_blockers(config, inventory);
        blockers.push("incomplete_inventory".into());
        blockers.push("unknown_size".into());
        blockers.sort();
        blockers.dedup();
        return Ok(BudgetRow {
            candidate: candidate.name.clone(),
            representation: representation.into(),
            identity,
            required_bytes: None,
            size_known: false,
            remaining_bytes: None,
            shortfall_bytes: None,
            at_storage_ceiling: false,
            fits_with_reserve: None,
            blockers,
        });
    }
    let assessment = assess_inventory(config, representation, inventory, &candidate.select)?;
    let required = assessment.required_bytes;
    let mut blockers = assessment.blockers;
    let size_known = required.is_some();
    if !size_known {
        blockers.push("unknown_size".into());
    }
    let measured = measure(config, required)?;
    blockers.extend(measured.blockers);
    blockers.sort();
    blockers.dedup();
    Ok(BudgetRow {
        candidate: candidate.name.clone(),
        representation: representation.into(),
        identity,
        required_bytes: required,
        size_known,
        remaining_bytes: measured.remaining,
        shortfall_bytes: measured.shortfall,
        at_storage_ceiling: measured.at_ceiling,
        fits_with_reserve: measured.fits_with_reserve,
        blockers,
    })
}

fn unknown_row(name: &str, representation: &str, mut blockers: Vec<String>) -> BudgetRow {
    blockers.sort();
    blockers.dedup();
    BudgetRow {
        candidate: name.into(),
        representation: representation.into(),
        identity: format!("unknown:{name}"),
        required_bytes: None,
        size_known: false,
        remaining_bytes: None,
        shortfall_bytes: None,
        at_storage_ceiling: false,
        fits_with_reserve: None,
        blockers,
    }
}

struct Measurement {
    remaining: Option<i64>,
    shortfall: Option<u64>,
    at_ceiling: bool,
    fits_with_reserve: Option<bool>,
    blockers: Vec<String>,
}

fn measure(config: &Config, required: Option<u64>) -> Result<Measurement> {
    let Some(required) = required else {
        return Ok(Measurement {
            remaining: None,
            shortfall: None,
            at_ceiling: false,
            fits_with_reserve: None,
            blockers: Vec::new(),
        });
    };
    let max = config.storage.max_stored_bytes;
    let mut blockers = Vec::new();
    let shortfall = if required > max {
        blockers.push("revision_too_large".into());
        Some(required - max)
    } else {
        Some(0)
    };
    let fits_with_reserve = match required.checked_add(config.storage.min_free_bytes) {
        Some(total) => Some(total <= max),
        None => {
            blockers.push("size_overflow".into());
            Some(false)
        }
    };
    Ok(Measurement {
        remaining: Some(signed_diff(max, required)?),
        shortfall,
        at_ceiling: required == max,
        fits_with_reserve,
        blockers,
    })
}

pub(crate) struct Assessment {
    pub required_bytes: Option<u64>,
    pub blockers: Vec<String>,
    pub license_id: String,
    pub license_status: String,
}

pub(crate) fn assess_inventory(
    config: &Config,
    representation: &str,
    inventory: &Inventory,
    select: &[String],
) -> Result<Assessment> {
    if representation == "derivative" && inventory.layout == "gguf-v1" {
        return derivative_bytes(config, inventory, select);
    }
    if representation == "original" && inventory.layout == "transformers-safetensors-v1" {
        return original_bytes(config, inventory);
    }
    let mut blockers = rights_blockers(config, inventory);
    blockers.push("unsupported_layout".into());
    Ok(Assessment {
        required_bytes: Some(pinned_bytes(inventory)?),
        blockers,
        license_id: inventory.license.license_id.clone(),
        license_status: status_name(inventory.license.status),
    })
}

fn original_bytes(config: &Config, inventory: &Inventory) -> Result<Assessment> {
    let plan = build_plan(
        &profile(config, "publisher-safetensors-v1"),
        inventory,
        u64::MAX,
    )?;
    Ok(Assessment {
        required_bytes: Some(plan.payload_bytes),
        blockers: plan_blockers(&plan.blockers),
        license_id: inventory.license.license_id.clone(),
        license_status: status_name(inventory.license.status),
    })
}

fn derivative_bytes(
    config: &Config,
    inventory: &Inventory,
    select: &[String],
) -> Result<Assessment> {
    let plan = build_plan_selecting(
        &profile(config, "selected-gguf-v1"),
        inventory,
        u64::MAX,
        select,
    )?;
    let blockers = plan_blockers(&plan.blockers);
    let unresolved = blockers.iter().any(|blocker| {
        matches!(
            blocker.as_str(),
            "ambiguous_gguf_selection"
                | "missing_gguf_selection"
                | "unknown_gguf_selection"
                | "incomplete_gguf_set"
                | "unsupported_layout"
        )
    });
    let license_status = if plan.license_evidence_source.as_deref() == Some("inherited") {
        "inherited".into()
    } else {
        status_name(inventory.license.status)
    };
    Ok(Assessment {
        required_bytes: if unresolved {
            None
        } else {
            Some(plan.payload_bytes)
        },
        blockers,
        license_id: plan
            .license_id
            .unwrap_or_else(|| inventory.license.license_id.clone()),
        license_status,
    })
}

fn status_name(status: EvidenceStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unreviewed".into())
}

fn plan_blockers(blockers: &[plan::Blocker]) -> Vec<String> {
    blockers
        .iter()
        .filter(|blocker| **blocker != plan::Blocker::LocalInventoryUntrusted)
        .map(plan::blocker_name)
        .collect()
}

fn rights_blockers(config: &Config, inventory: &Inventory) -> Vec<String> {
    let mut blockers = Vec::new();
    if inventory.source.gated || inventory.source.private {
        blockers.push("access_restricted".into());
    }
    if inventory.license.status != EvidenceStatus::Matched || inventory.license.paths.is_empty() {
        blockers.push("license_needs_review".into());
    }
    if !config
        .licenses
        .allow
        .contains(&inventory.license.license_id)
    {
        blockers.push("license_not_allowed".into());
    }
    blockers
}

fn pinned_bytes(inventory: &Inventory) -> Result<u64> {
    let required: Vec<u64> = inventory
        .files
        .iter()
        .filter(|file| file.required)
        .map(|file| file.size_bytes)
        .collect();
    let sizes = if required.is_empty() {
        inventory.files.iter().map(|file| file.size_bytes).collect()
    } else {
        required
    };
    sizes
        .into_iter()
        .try_fold(0_u64, |sum, size| sum.checked_add(size))
        .ok_or_else(|| Error::new("size_overflow", "candidate size exceeds u64 capacity"))
}

fn profile(config: &Config, profile: &str) -> Config {
    let mut copy = config.clone();
    copy.files.profile = profile.into();
    copy.files.include_repo_code = false;
    copy.files.include_gguf = false;
    copy
}

fn signed_diff(max_stored: u64, required: u64) -> Result<i64> {
    let difference = i128::from(max_stored) - i128::from(required);
    i64::try_from(difference)
        .map_err(|_| Error::new("size_overflow", "budget remainder exceeds i64"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{
        EvidenceStatus, HashAlgorithm, LicenseEvidence, Role, Source, SourceFile, UpstreamHash,
    };

    fn config_with(max_stored: u64, min_free: u64) -> Config {
        let mut config = Config::default_for("catalog".into(), "models".into());
        config.storage.max_stored_bytes = max_stored;
        config.storage.min_free_bytes = min_free;
        config.transfer.max_transfer_bytes_per_period = u64::MAX;
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

    fn source(repo: &str) -> Source {
        Source {
            endpoint: "https://huggingface.co".into(),
            repo_type: "model".into(),
            repo_id: repo.into(),
            requested_revision: "main".into(),
            resolved_revision: "0123456789abcdef0123456789abcdef01234567".into(),
            gated: false,
            private: false,
        }
    }

    fn safetensors(repo: &str, weight: u64) -> Inventory {
        Inventory {
            schema_version: 1,
            source: source(repo),
            complete: true,
            layout: "transformers-safetensors-v1".into(),
            license: LicenseEvidence {
                license_id: "apache-2.0".into(),
                status: EvidenceStatus::Matched,
                paths: vec!["LICENSE".into()],
            },
            files: vec![
                file("model.safetensors", weight, Role::Weight),
                file("config.json", 0, Role::Config),
                file("tokenizer.json", 0, Role::Tokenizer),
                file("LICENSE", 0, Role::License),
            ],
            base: None,
        }
    }

    fn request(candidates: serde_json::Value) -> String {
        serde_json::json!({"schema_version": 1, "candidates": candidates}).to_string()
    }

    #[test]
    fn rows_flag_over_budget_the_ceiling_and_unknown_or_unsupported_sizes() {
        let over = config_with(80, 10);
        let inventory = safetensors("example/over", 100);
        let report = report_json(
            &over,
            &request(serde_json::json!([
                {"name": "over", "inventory": inventory, "representation": "original"},
                {"name": "unknown", "size_known": false, "representation": "original"},
                {"name": "diffusers", "representation": "original", "inventory": Inventory {
                    layout: "diffusers".into(),
                    files: vec![
                        file("transformer.safetensors", 15, Role::Weight),
                        file("LICENSE", 0, Role::License),
                    ],
                    ..safetensors("example/diffusers", 0)
                }}
            ])),
        )
        .unwrap();
        let over_row = &report.rows[0];
        assert_eq!(over_row.required_bytes, Some(100));
        assert_eq!(over_row.shortfall_bytes, Some(20));
        assert_eq!(over_row.remaining_bytes, Some(-20));
        assert!(!over_row.at_storage_ceiling);
        assert_eq!(over_row.fits_with_reserve, Some(false));
        assert!(
            over_row
                .blockers
                .iter()
                .any(|blocker| blocker == "revision_too_large")
        );
        let unknown = &report.rows[1];
        assert!(!unknown.size_known);
        assert_eq!(unknown.required_bytes, None);
        assert!(
            unknown
                .blockers
                .iter()
                .any(|blocker| blocker == "unknown_size")
        );
        let unsupported = &report.rows[2];
        assert_eq!(unsupported.required_bytes, Some(15));
        assert!(
            unsupported
                .blockers
                .iter()
                .any(|blocker| blocker == "unsupported_layout")
        );
        assert_eq!(report.combined_required_bytes, 115);
        assert!(!report.combined_size_complete);
        assert_eq!(report.combined_remaining_bytes, Some(-35));
        assert_eq!(report.combined_shortfall_bytes, Some(35));
        let text = crate::present::render_budget(&report, false, 200);
        assert!(text.contains("over"), "{text}");
        assert!(text.contains("100"), "{text}");
        assert!(text.contains("shortfall 20"), "{text}");
        assert!(text.contains("unknown"), "{text}");
        assert!(text.contains("unsupported_layout"), "{text}");
        assert!(text.contains("15"), "{text}");
        assert!(!report.transfer_authorized);
        assert!(report.workspace_bytes.is_none());

        let exact = report_json(
            &config_with(50, 1),
            &request(serde_json::json!([
                {"name": "exact", "representation": "original", "inventory": safetensors("example/exact", 50)}
            ])),
        )
        .unwrap();
        assert_eq!(exact.rows[0].required_bytes, Some(50));
        assert_eq!(exact.rows[0].shortfall_bytes, Some(0));
        assert_eq!(exact.rows[0].remaining_bytes, Some(0));
        assert!(exact.rows[0].at_storage_ceiling);
        assert_eq!(exact.rows[0].fits_with_reserve, Some(false));
        assert!(
            !exact.rows[0]
                .blockers
                .iter()
                .any(|blocker| blocker == "revision_too_large")
        );
        let exact_text = crate::present::render_budget(&exact, false, 120);
        assert!(exact_text.contains("at-ceiling"), "{exact_text}");
        assert!(exact_text.contains("shortfall 0"), "{exact_text}");
    }

    #[test]
    fn published_original_and_derivative_sizes_stay_distinct_from_the_whole_repo() {
        let quant = 16_464_440_224_u64;
        let projector = 931_146_432_u64;
        let original = 55_586_038_316_u64;
        let mut files = vec![
            SourceFile {
                path: "qwen-UD-Q4_K_M.gguf".into(),
                size_bytes: quant,
                upstream_hash: Some(UpstreamHash {
                    algorithm: HashAlgorithm::Sha256,
                    value: "cd".repeat(32),
                }),
                role: Role::Weight,
                required: true,
            },
            SourceFile {
                path: "mmproj-BF16.gguf".into(),
                size_bytes: projector,
                upstream_hash: Some(UpstreamHash {
                    algorithm: HashAlgorithm::Sha256,
                    value: "ce".repeat(32),
                }),
                role: Role::Projector,
                required: false,
            },
        ];
        for index in 0..25 {
            files.push(SourceFile {
                path: format!("other-{index}.gguf"),
                size_bytes: 1,
                upstream_hash: Some(UpstreamHash {
                    algorithm: HashAlgorithm::Sha256,
                    value: "cf".repeat(32),
                }),
                role: Role::Weight,
                required: true,
            });
        }
        let derivative = Inventory {
            schema_version: 1,
            source: source("unsloth/Qwen3.8-27B-GGUF"),
            complete: true,
            layout: "gguf-v1".into(),
            license: LicenseEvidence {
                license_id: "apache-2.0".into(),
                status: EvidenceStatus::Unreviewed,
                paths: Vec::new(),
            },
            files,
            base: None,
        };
        let config = Config::default_for("catalog".into(), "models".into());
        let report = report_json(
            &config,
            &request(serde_json::json!([
                {"name": "original", "representation": "original", "inventory": safetensors("Qwen/Qwen3.8-27B", original)},
                {"name": "derivative", "representation": "derivative", "select": ["qwen-UD-Q4_K_M.gguf", "mmproj-BF16.gguf"], "inventory": derivative.clone()},
                {"name": "whole-repo", "representation": "derivative", "inventory": derivative}
            ])),
        )
        .unwrap();
        assert_eq!(report.rows[0].required_bytes, Some(original));
        assert_eq!(report.rows[0].remaining_bytes, Some(444_413_961_684));
        assert_eq!(report.rows[1].required_bytes, Some(quant + projector));
        assert_eq!(report.rows[1].remaining_bytes, Some(482_604_413_344));
        assert_eq!(report.rows[1].representation, "derivative");
        assert!(!report.rows[2].size_known);
        assert_eq!(report.combined_required_bytes, original + quant + projector);
        assert!(!report.combined_size_complete);
        let duplicate = report_json(
            &config_with(1_000, 0),
            &request(serde_json::json!([
                {"name": "first", "inventory": safetensors("example/model", 40)},
                {"name": "second", "inventory": safetensors("example/model", 40)}
            ])),
        )
        .unwrap();
        assert_eq!(duplicate.combined_required_bytes, 40);
        assert!(duplicate.combined_size_complete);
        assert_eq!(
            report_json(&config, "{\"schema_version\":2}")
                .unwrap()
                .blockers,
            ["unsupported_schema"]
        );
    }

    #[test]
    fn malformed_rows_stay_unknown_and_conflicts_are_named() {
        let config = config_with(100, 0);
        assert_eq!(
            report_json(&config, "not json").unwrap_err().code,
            "invalid_budget"
        );
        assert_eq!(
            report_json(&config, &request(serde_json::json!([{"name": ""}])))
                .unwrap_err()
                .code,
            "invalid_budget"
        );
        assert_eq!(
            report_json(&config, &request(serde_json::json!([{"name": "a\nb"}])))
                .unwrap_err()
                .code,
            "invalid_budget"
        );
        assert_eq!(
            report_json(
                &config,
                &request(serde_json::json!([{"name": "same"}, {"name": "same"}]))
            )
            .unwrap_err()
            .code,
            "invalid_budget"
        );
        let mut incomplete = safetensors("example/partial", 8);
        incomplete.complete = false;
        let mut gguf = safetensors("example/gguf", 8);
        gguf.layout = "gguf-v1".into();
        gguf.license.status = EvidenceStatus::Unreviewed;
        gguf.license.paths.clear();
        gguf.files = vec![file("only-Q4_K_M.gguf", 8, Role::Weight)];
        let report = report_json(
            &config,
            &request(serde_json::json!([
                {"name": "bare"},
                {"name": "weird", "representation": "both", "inventory": safetensors("example/weird", 1)},
                {"name": "partial", "inventory": incomplete},
                {"name": "inferred", "inventory": gguf},
                {"name": "low", "inventory": safetensors("example/model", 4)},
                {"name": "high", "inventory": safetensors("example/model", 9)}
            ])),
        )
        .unwrap();
        assert!(!report.rows[0].size_known);
        assert!(
            report.rows[1]
                .blockers
                .iter()
                .any(|blocker| blocker == "invalid_representation")
        );
        assert!(
            report.rows[2]
                .blockers
                .iter()
                .any(|blocker| blocker == "incomplete_inventory")
        );
        assert_eq!(report.rows[3].representation, "derivative");
        assert!(!report.rows[3].size_known);
        assert!(
            report
                .blockers
                .iter()
                .any(|blocker| blocker == "conflicting_candidate_size")
        );
        assert!(!report.combined_size_complete);
    }
}
