//! Durable source reviews. Only the internal online resolver can create one.
use super::{Catalog, db, identities, open_catalog, report};
use crate::{
    Config, Error, Inventory, Result, build_plan,
    hub::{CapturedEvidence, Resolution},
    inventory::SourceFile,
    plan::{Blocker, digest},
    validation::validate_hex,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

const DOMAIN: &str = "modelprepper.source-review.v1";
const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Pending,
    Approved,
    Rejected,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
        }
    }
    fn parse(text: &str) -> Result<Self> {
        match text {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            _ => Err(invalid()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceReview {
    pub schema_version: u32,
    pub domain: String,
    pub vault_id: String,
    pub volume_id: String,
    pub volume_label: String,
    pub policy_digest: String,
    pub inventory: Inventory,
    pub evidence: Vec<CapturedEvidence>,
    pub selected_files: Vec<SourceFile>,
    pub excluded_files: Vec<String>,
    pub payload_bytes: u64,
    pub min_free_bytes: u64,
    pub workspace_bytes: Option<u64>,
    pub blockers: Vec<Blocker>,
}

#[derive(Debug, Serialize)]
pub struct Proposal {
    pub schema_version: u32,
    pub plan_id: String,
    pub decision: Decision,
    pub created_at: i64,
    pub observed_available_bytes: u64,
    pub decided_at: Option<i64>,
    pub policy_current: bool,
    pub transfer_authorized: bool,
    pub review: SourceReview,
}

#[derive(Debug, Serialize)]
pub struct ProposalSummary {
    pub plan_id: String,
    pub repo_id: String,
    pub resolved_revision: String,
    pub payload_bytes: u64,
    pub decision: Decision,
    pub policy_current: bool,
    pub blocker_count: usize,
}

#[derive(Debug, Serialize)]
pub struct ProposalPage {
    pub schema_version: u32,
    pub proposals: Vec<ProposalSummary>,
    pub next_after: Option<String>,
}

fn invalid() -> Error {
    Error::new(
        "invalid_proposal",
        "stored source review is invalid; retain the catalog and inspect it before retrying",
    )
}

fn destination(catalog: &Catalog, config: &Config) -> Result<(String, String, u64)> {
    let status = report(catalog, config)?;
    if status.state != "ready" {
        return Err(Error::new(
            "vault_not_ready",
            "finish vault initialization before proposing a source",
        ));
    }
    let volume = status
        .volumes
        .iter()
        .find(|v| v.label == config.storage.default_volume)
        .expect("validated config and catalog");
    let available = volume
        .available_bytes
        .filter(|_| volume.state == "mounted")
        .ok_or_else(|| {
            Error::new(
                "volume_unavailable",
                "connect the configured destination disk with its original identity",
            )
        })?;
    Ok((status.vault_id, volume.volume_id.clone(), available))
}

/// Resolves inside the trust boundary; accepts no imported resolution or plan JSON.
pub fn propose(config: &Config, repo: &str, revision: &str) -> Result<Proposal> {
    let original = {
        let catalog = open_catalog(config, false)?;
        destination(&catalog, config)?
    }; // Never hold the writer lock during publisher requests.
    let resolved = crate::hub::resolve(repo, revision)?;
    let catalog = open_catalog(config, false)?;
    let current = destination(&catalog, config)?;
    if original.0 != current.0 || original.1 != current.1 {
        return Err(Error::new(
            "destination_changed",
            "vault or destination identity changed during source inspection",
        ));
    }
    save(&catalog, config, resolved, current)
}

fn save(
    catalog: &Catalog,
    config: &Config,
    mut resolved: Resolution,
    destination: (String, String, u64),
) -> Result<Proposal> {
    let preview = build_plan(config, &resolved.inventory, destination.2)?;
    let mut blockers = preview.blockers;
    blockers.retain(|b| *b != Blocker::LocalInventoryUntrusted);
    blockers.push(Blocker::TransferBackendUnqualified);
    blockers.sort();
    resolved.inventory.files.sort_by(|a, b| a.path.cmp(&b.path));
    resolved.inventory.license.paths.sort();
    resolved.evidence.sort_by(|a, b| a.path.cmp(&b.path));
    let review = SourceReview {
        schema_version: 1,
        domain: DOMAIN.into(),
        vault_id: destination.0,
        volume_id: destination.1,
        volume_label: preview.volume_label,
        policy_digest: preview.policy_digest,
        inventory: resolved.inventory,
        evidence: resolved.evidence,
        selected_files: preview.selected_files,
        excluded_files: preview.excluded_files,
        payload_bytes: preview.payload_bytes,
        min_free_bytes: preview.min_free_bytes,
        workspace_bytes: None,
        blockers,
    };
    let document = serde_json::to_string(&review).map_err(|_| invalid())?;
    if document.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::new(
            "proposal_too_large",
            "source review exceeds the 8 MiB catalog record limit",
        ));
    }
    let id = digest(document.as_bytes());
    // Repeated inspection retains the original decision and observation time.
    catalog
        .db
        .execute(
            "INSERT INTO proposals (id,document,observed_available_bytes) VALUES (?1,?2,?3) ON CONFLICT(id) DO NOTHING",
            params![id, document, destination.2.to_string()],
        )
        .map_err(db)?;
    load(catalog, config, &id)
}

fn load(catalog: &Catalog, config: &Config, id: &str) -> Result<Proposal> {
    validate_hex(id, 64)?;
    let row: Option<(String, String, i64, Option<i64>, String)> = catalog.db.query_row(
        "SELECT document,decision,created_at,decided_at,observed_available_bytes FROM proposals WHERE id=?1 AND length(CAST(document AS BLOB))<=?2 AND length(observed_available_bytes)<=20", params![id, MAX_DOCUMENT_BYTES as i64],
        |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(db)?;
    let Some((document, decision, created_at, decided_at, available)) = row else {
        let exists: bool = catalog
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM proposals WHERE id=?1)",
                [id],
                |r| r.get(0),
            )
            .map_err(db)?;
        return Err(if exists {
            invalid()
        } else {
            Error::new(
                "proposal_not_found",
                "no saved source review has this exact plan id",
            )
        });
    };
    if digest(document.as_bytes()) != id {
        return Err(invalid());
    }
    let review: SourceReview = serde_json::from_str(&document).map_err(|_| invalid())?;
    let identity = identities(catalog, config)?;
    if review.schema_version != 1
        || review.domain != DOMAIN
        || review.vault_id != identity.vault_id
        || !identity
            .volumes
            .iter()
            .any(|(v, _)| v.volume_id == review.volume_id && v.label == review.volume_label)
    {
        return Err(invalid());
    }
    let decision = Decision::parse(&decision)?;
    if (decision == Decision::Pending) != decided_at.is_none() {
        return Err(invalid());
    }
    Ok(Proposal {
        schema_version: 1,
        plan_id: id.into(),
        decision,
        created_at,
        observed_available_bytes: available.parse().map_err(|_| invalid())?,
        decided_at,
        policy_current: review.policy_digest == digest(&config.canonical_bytes()?),
        transfer_authorized: false,
        review,
    })
}

pub fn show_proposal(config: &Config, id: &str) -> Result<Proposal> {
    load(&open_catalog(config, false)?, config, id)
}

/// Stable keyset pagination avoids loading whole evidence documents into a list.
pub fn proposals(config: &Config, after: Option<&str>, limit: u32) -> Result<ProposalPage> {
    if !(1..=100).contains(&limit) {
        return Err(Error::new(
            "invalid_page_limit",
            "proposal page limit must be 1 through 100",
        ));
    }
    if let Some(after) = after {
        validate_hex(after, 64)?;
    }
    let catalog = open_catalog(config, false)?;
    identities(&catalog, config)?;
    let ids = catalog
        .db
        .prepare("SELECT id FROM proposals WHERE id>?1 ORDER BY id LIMIT ?2")
        .map_err(db)?
        .query_map(params![after.unwrap_or(""), limit + 1], |r| {
            r.get::<_, String>(0)
        })
        .map_err(db)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db)?;
    let next_after = if ids.len() > limit as usize {
        Some(ids[limit as usize - 1].clone())
    } else {
        None
    };
    let mut proposals = Vec::new();
    for id in ids.iter().take(limit as usize) {
        let p = load(&catalog, config, id)?;
        proposals.push(ProposalSummary {
            plan_id: p.plan_id,
            repo_id: p.review.inventory.source.repo_id,
            resolved_revision: p.review.inventory.source.resolved_revision,
            payload_bytes: p.review.payload_bytes,
            decision: p.decision,
            policy_current: p.policy_current,
            blocker_count: p.review.blockers.len(),
        });
    }
    Ok(ProposalPage {
        schema_version: 1,
        proposals,
        next_after,
    })
}

pub fn decide(config: &Config, id: &str, decision: Decision) -> Result<Proposal> {
    if decision == Decision::Pending {
        return Err(Error::new(
            "invalid_decision",
            "choose approved or rejected; decisions cannot be reset to pending",
        ));
    }
    let catalog = open_catalog(config, false)?;
    let proposal = load(&catalog, config, id)?;
    if proposal.decision == decision {
        return Ok(proposal);
    }
    if proposal.decision != Decision::Pending {
        return Err(Error::new(
            "decision_conflict",
            "this exact source review already has a different final decision",
        ));
    }
    if decision == Decision::Approved {
        if !proposal.policy_current {
            return Err(Error::new(
                "stale_policy",
                "configuration changed; propose a new review before approval",
            ));
        }
        destination(&catalog, config)?;
        // No production transfer backend has qualified bounds yet. This gate is
        // independent of stored JSON, including its asserted blocker list.
        return Err(Error::new(
            "transfer_backend_unqualified",
            "approval is unavailable until durable transfer, workspace bounds, and accounting are qualified; the review remains pending",
        ));
    }
    catalog.db.execute("UPDATE proposals SET decision=?1,decided_at=unixepoch() WHERE id=?2 AND decision='pending'", params![decision.as_str(),id]).map_err(db)?;
    load(&catalog, config, id)
}

#[cfg(test)]
#[path = "tests/review.rs"]
mod tests;
