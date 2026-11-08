//! One grant for one source review. It names that review, the measured
//! connection bounds, the per-file staging bound, the volume UUID, and the
//! policy digest. It is deleted when any of those differ. It does not
//! authorize a transfer and it is not a fetch command.
use super::{db, open_catalog, report, review};
use crate::{
    Error, Result,
    plan::Blocker,
    transfer::{
        KernelSocketBuffers, measure_kernel_socket_buffers, recorded_transfer_buffers,
        revision_disk_bound,
    },
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

const DOMAIN: &str = "modelprepper.grant.v1";
const DOCUMENT_CAP: i64 = 64 * 1024;

/// A recorded grant. `transfer_authorized` is always false. `workspace_bytes`
/// on the review stays null; the bounds here are this record, not that field.
#[derive(Debug, PartialEq, Eq)]
pub struct Grant {
    pub schema_version: u32,
    pub plan_id: String,
    pub policy_digest: String,
    pub volume_id: String,
    pub file_count: u64,
    pub payload_bytes: u64,
    pub side_file_bytes: u64,
    pub manifest_bytes: u64,
    pub application_read_bytes: u64,
    pub tls_input_bytes: u64,
    pub tls_output_bytes: u64,
    pub tls_received_plaintext_bytes: u64,
    pub tls_send_buffer_bytes: u64,
    pub kernel_receive_bytes: u64,
    pub kernel_send_bytes: u64,
    pub max_single_get_bytes: u64,
    pub transfer_authorized: bool,
    pub recorded_at: i64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct StoredGrant {
    schema_version: u32,
    domain: String,
    plan_id: String,
    policy_digest: String,
    volume_id: String,
    file_count: u64,
    payload_bytes: u64,
    side_file_bytes: u64,
    manifest_bytes: u64,
    application_read_bytes: u64,
    tls_input_bytes: u64,
    tls_output_bytes: u64,
    tls_received_plaintext_bytes: u64,
    tls_send_buffer_bytes: u64,
    kernel_receive_bytes: u64,
    kernel_send_bytes: u64,
    max_single_get_bytes: u64,
}

/// Record the current grant for one saved review. The destination disk must be
/// mounted. A later change of plan, bounds, volume, or policy drops it.
pub fn record_grant(config: &crate::Config, plan_id: &str) -> Result<Grant> {
    record_grant_with(config, plan_id, measure_kernel_socket_buffers()?)
}

/// Return the grant only while every binding still matches. A mismatch deletes it.
pub fn current_grant(config: &crate::Config, plan_id: &str) -> Result<Grant> {
    current_grant_with(config, plan_id, measure_kernel_socket_buffers()?)
}

pub(crate) fn record_grant_with(
    config: &crate::Config,
    plan_id: &str,
    kernel: KernelSocketBuffers,
) -> Result<Grant> {
    let catalog = open_catalog(config, false)?;
    let proposal = review::load(&catalog, config, plan_id)?;
    if !proposal.policy_current {
        return Err(Error::new(
            "stale_policy",
            "configuration changed; propose a new review before approval",
        ));
    }
    // A file outside the ranged path is refused before other blockers, so the
    // refusal does not depend on how much free space the volume reports.
    let document = document(&proposal, kernel)?;
    if proposal
        .review
        .blockers
        .iter()
        .any(|blocker| *blocker != Blocker::TransferBackendUnqualified)
    {
        return Err(Error::new(
            "grant_blocked",
            "resolve the review blockers before recording a grant",
        ));
    }
    let status = report(&catalog, config)?;
    if status.state != "ready" {
        return Err(Error::new(
            "vault_not_ready",
            "finish vault initialization before proposing a source",
        ));
    }
    let volume = status
        .volumes
        .iter()
        .find(|volume| volume.label == config.storage.default_volume)
        .ok_or_else(|| Error::new("invalid_config", "default volume not found"))?;
    if volume.state != "mounted" || volume.volume_id != proposal.review.volume_id {
        return Err(Error::new(
            "volume_unavailable",
            "connect the configured destination disk with its original identity",
        ));
    }
    let recorded_at = store(&catalog, plan_id, &document)?;
    view(&document, recorded_at)
}

pub(crate) fn current_grant_with(
    config: &crate::Config,
    plan_id: &str,
    kernel: KernelSocketBuffers,
) -> Result<Grant> {
    let catalog = open_catalog(config, false)?;
    let Some(stored) = read(&catalog, plan_id)? else {
        return Err(Error::new(
            "grant_not_recorded",
            "no grant is recorded for this review",
        ));
    };
    let proposal = match review::load(&catalog, config, plan_id) {
        Ok(proposal) => proposal,
        Err(error) if error.code == "invalid_proposal" || error.code == "proposal_not_found" => {
            return drop_grant(&catalog, plan_id);
        }
        Err(error) => return Err(error),
    };
    if !proposal.policy_current || document(&proposal, kernel)? != stored {
        return drop_grant(&catalog, plan_id);
    }
    let recorded_at = recorded_at(&catalog, plan_id)?;
    view(&stored, recorded_at)
}

fn document(proposal: &review::Proposal, kernel: KernelSocketBuffers) -> Result<String> {
    let review = &proposal.review;
    let bound = revision_disk_bound(review.selected_files.len())?;
    let buffers = recorded_transfer_buffers();
    let max_single_get_bytes = buffers.max_single_get_bytes;
    if review
        .selected_files
        .iter()
        .any(|file| file.size_bytes > max_single_get_bytes)
    {
        return Err(Error::new(
            "revision_outside_bounds",
            "a selected file is larger than the ranged GET path allows",
        ));
    }
    let stored = StoredGrant {
        schema_version: 1,
        domain: DOMAIN.into(),
        plan_id: proposal.plan_id.clone(),
        policy_digest: review.policy_digest.clone(),
        volume_id: review.volume_id.clone(),
        file_count: u64::try_from(review.selected_files.len())
            .map_err(|_| Error::new("size_overflow", "selected file count exceeds u64"))?,
        payload_bytes: review.payload_bytes,
        side_file_bytes: bound.side_file_bytes,
        manifest_bytes: bound.manifest_bytes,
        application_read_bytes: widen(buffers.application_read_bytes)?,
        tls_input_bytes: widen(buffers.tls_input_bytes)?,
        tls_output_bytes: widen(buffers.tls_output_bytes)?,
        tls_received_plaintext_bytes: widen(buffers.tls_received_plaintext_bytes)?,
        tls_send_buffer_bytes: widen(buffers.tls_send_buffer_bytes)?,
        kernel_receive_bytes: kernel.receive_bytes,
        kernel_send_bytes: kernel.send_bytes,
        max_single_get_bytes,
    };
    serde_json::to_string(&stored).map_err(|_| {
        Error::new(
            "invalid_grant",
            "the grant could not be recorded; inspect the review and try again",
        )
    })
}

fn view(document: &str, recorded_at: i64) -> Result<Grant> {
    let stored: StoredGrant = serde_json::from_str(document).map_err(|_| {
        Error::new(
            "invalid_grant",
            "the recorded grant cannot be read; record it again",
        )
    })?;
    Ok(Grant {
        schema_version: stored.schema_version,
        plan_id: stored.plan_id,
        policy_digest: stored.policy_digest,
        volume_id: stored.volume_id,
        file_count: stored.file_count,
        payload_bytes: stored.payload_bytes,
        side_file_bytes: stored.side_file_bytes,
        manifest_bytes: stored.manifest_bytes,
        application_read_bytes: stored.application_read_bytes,
        tls_input_bytes: stored.tls_input_bytes,
        tls_output_bytes: stored.tls_output_bytes,
        tls_received_plaintext_bytes: stored.tls_received_plaintext_bytes,
        tls_send_buffer_bytes: stored.tls_send_buffer_bytes,
        kernel_receive_bytes: stored.kernel_receive_bytes,
        kernel_send_bytes: stored.kernel_send_bytes,
        max_single_get_bytes: stored.max_single_get_bytes,
        transfer_authorized: false,
        recorded_at,
    })
}

fn widen(value: usize) -> Result<u64> {
    u64::try_from(value)
        .map_err(|_| Error::new("size_overflow", "a buffer bound does not fit in u64"))
}

fn read(catalog: &super::Catalog, plan_id: &str) -> Result<Option<String>> {
    catalog
        .db
        .query_row(
            "SELECT document FROM grants WHERE plan_id=?1 AND length(CAST(document AS BLOB))<=?2",
            params![plan_id, DOCUMENT_CAP],
            |row| row.get(0),
        )
        .optional()
        .map_err(db)
}

fn recorded_at(catalog: &super::Catalog, plan_id: &str) -> Result<i64> {
    catalog
        .db
        .query_row(
            "SELECT recorded_at FROM grants WHERE plan_id=?1",
            [plan_id],
            |row| row.get(0),
        )
        .map_err(db)
}

fn store(catalog: &super::Catalog, plan_id: &str, document: &str) -> Result<i64> {
    if let Some(existing) = read(catalog, plan_id)? {
        if existing == document {
            return recorded_at(catalog, plan_id);
        }
        catalog
            .db
            .execute(
                "UPDATE grants SET document=?1, recorded_at=unixepoch() WHERE plan_id=?2",
                params![document, plan_id],
            )
            .map_err(db)?;
    } else {
        catalog
            .db
            .execute(
                "INSERT INTO grants (plan_id, document) VALUES (?1, ?2)",
                params![plan_id, document],
            )
            .map_err(db)?;
    }
    recorded_at(catalog, plan_id)
}

fn drop_grant(catalog: &super::Catalog, plan_id: &str) -> Result<Grant> {
    catalog
        .db
        .execute("DELETE FROM grants WHERE plan_id=?1", [plan_id])
        .map_err(db)?;
    Err(Error::new(
        "grant_dropped",
        "the grant was dropped because the plan, measured bounds, volume, or policy no longer match",
    ))
}

#[cfg(test)]
#[path = "tests/grant.rs"]
mod tests;
