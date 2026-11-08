use super::*;
use crate::{
    hub::{CapturedEvidence, Resolution},
    inventory::{EvidenceStatus, Inventory},
    plan::{Blocker, digest},
    transfer::{KernelSocketBuffers, recorded_transfer_buffers, staging_workspace_bound},
    vault::{Decision, initialize},
};
use std::fs;

fn setup() -> (tempfile::TempDir, crate::Config) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let config = crate::Config::default_for(root.join("catalog"), root.join("models"));
    initialize(&config).unwrap();
    (dir, config)
}

fn kernel() -> KernelSocketBuffers {
    KernelSocketBuffers {
        receive_bytes: 16_384,
        send_bytes: 32_768,
    }
}

fn save(config: &crate::Config, inventory: Inventory) -> crate::vault::Proposal {
    let catalog = super::super::open_catalog(config, false).unwrap();
    let status = super::super::report(&catalog, config).unwrap();
    let volume = status
        .volumes
        .iter()
        .find(|volume| volume.label == config.storage.default_volume)
        .unwrap();
    super::super::review::save(
        &catalog,
        config,
        Resolution {
            schema_version: 1,
            mode: "resolved_inventory",
            transfer_authorized: false,
            inventory,
            evidence: vec![CapturedEvidence {
                path: "LICENSE".into(),
                sha256: digest(b"mit"),
                text: "mit".into(),
            }],
        },
        (
            status.vault_id,
            volume.volume_id.clone(),
            volume.available_bytes.unwrap(),
        ),
    )
    .unwrap()
}

fn matched(config: &crate::Config) -> crate::vault::Proposal {
    let mut inventory =
        Inventory::parse(include_str!("../../../tests/fixtures/inventory.json")).unwrap();
    inventory.license.status = EvidenceStatus::Matched;
    save(config, inventory)
}

#[test]
fn a_grant_names_the_review_bounds_volume_and_policy_and_drops_when_they_change() {
    let (_dir, config) = setup();
    let proposal = matched(&config);
    assert_eq!(
        proposal.review.blockers,
        vec![Blocker::TransferBackendUnqualified]
    );
    assert!(proposal.review.workspace_bytes.is_none());
    assert!(
        proposal
            .review
            .selected_files
            .iter()
            .any(|file| file.path == "LICENSE")
    );
    let version: u32 = super::super::open_catalog(&config, false)
        .unwrap()
        .db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 4);

    let granted = record_grant(&config, &proposal.plan_id).unwrap();
    let current = current_grant(&config, &proposal.plan_id).unwrap();
    assert_eq!(granted.plan_id, proposal.plan_id);
    assert_eq!(current.kernel_receive_bytes, granted.kernel_receive_bytes);
    assert_eq!(current.kernel_send_bytes, granted.kernel_send_bytes);
    assert!(!granted.transfer_authorized);
    assert!(!current.transfer_authorized);
    assert_eq!(granted.policy_digest, proposal.review.policy_digest);
    assert_eq!(granted.volume_id, proposal.review.volume_id);
    assert_eq!(
        granted.file_count,
        proposal.review.selected_files.len() as u64
    );
    assert!(granted.file_count > 1);
    assert_eq!(granted.payload_bytes, proposal.review.payload_bytes);
    assert_eq!(
        granted.side_file_bytes,
        staging_workspace_bound() * granted.file_count
    );
    assert_eq!(granted.manifest_bytes, 1024 * 1024);
    assert_eq!(
        granted.max_single_get_bytes,
        recorded_transfer_buffers().max_single_get_bytes
    );
    assert_eq!(
        crate::vault::decide(&config, &proposal.plan_id, Decision::Approved)
            .err()
            .unwrap()
            .code,
        "transfer_backend_unqualified"
    );
    assert_eq!(
        crate::vault::show_proposal(&config, &proposal.plan_id)
            .unwrap()
            .decision,
        Decision::Pending
    );

    let again = record_grant_with(&config, &proposal.plan_id, kernel()).unwrap();
    assert_eq!(
        record_grant_with(&config, &proposal.plan_id, kernel())
            .unwrap()
            .recorded_at,
        again.recorded_at
    );
    let mut changed_kernel = kernel();
    changed_kernel.receive_bytes += 1;
    assert_eq!(
        current_grant_with(&config, &proposal.plan_id, changed_kernel)
            .err()
            .unwrap()
            .code,
        "grant_dropped"
    );
    assert_eq!(
        current_grant_with(&config, &proposal.plan_id, kernel())
            .err()
            .unwrap()
            .code,
        "grant_not_recorded"
    );

    let granted = record_grant_with(&config, &proposal.plan_id, kernel()).unwrap();
    let mut changed = config.clone();
    changed.transfer.max_transfer_bytes_per_period += 1;
    assert_eq!(
        current_grant_with(&changed, &proposal.plan_id, kernel())
            .err()
            .unwrap()
            .code,
        "grant_dropped"
    );
    assert_eq!(
        record_grant_with(&changed, &proposal.plan_id, kernel())
            .err()
            .unwrap()
            .code,
        "stale_policy"
    );
    let _ = granted;

    let granted = record_grant_with(&config, &proposal.plan_id, kernel()).unwrap();
    {
        let catalog = super::super::open_catalog(&config, false).unwrap();
        catalog
            .db
            .execute(
                "UPDATE volumes SET id='00000000-0000-0000-0000-000000000001'",
                [],
            )
            .unwrap();
    }
    assert_eq!(
        current_grant_with(&config, &granted.plan_id, kernel())
            .err()
            .unwrap()
            .code,
        "grant_dropped"
    );
    assert_eq!(
        current_grant_with(&config, &granted.plan_id, kernel())
            .err()
            .unwrap()
            .code,
        "grant_not_recorded"
    );
}

#[test]
fn a_grant_is_refused_for_a_blocked_review_or_a_file_outside_the_ranged_path() {
    let (_dir, config) = setup();
    let blocked = save(
        &config,
        Inventory::parse(include_str!("../../../tests/fixtures/inventory.json")).unwrap(),
    );
    assert_eq!(
        record_grant_with(&config, &blocked.plan_id, kernel())
            .err()
            .unwrap()
            .code,
        "grant_blocked"
    );
    assert_eq!(
        current_grant_with(&config, &blocked.plan_id, kernel())
            .err()
            .unwrap()
            .code,
        "grant_not_recorded"
    );

    let mut inventory =
        Inventory::parse(include_str!("../../../tests/fixtures/inventory.json")).unwrap();
    inventory.license.status = EvidenceStatus::Matched;
    inventory.files[0].size_bytes = recorded_transfer_buffers().max_single_get_bytes + 1;
    let outside = save(&config, inventory);
    assert_eq!(
        record_grant_with(&config, &outside.plan_id, kernel())
            .err()
            .unwrap()
            .code,
        "revision_outside_bounds"
    );
}
