use super::*;
use crate::{hub::CapturedEvidence, vault::initialize};
use std::fs;
use tempfile::TempDir;

fn setup() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let config = Config::default_for(root.join("catalog"), root.join("models"));
    initialize(&config).unwrap();
    (dir, config)
}

// Injection is private test-only plumbing, never an application import path.
fn fixture(config: &Config) -> Proposal {
    let catalog = open_catalog(config, false).unwrap();
    let inventory =
        Inventory::parse(include_str!("../../../tests/fixtures/inventory.json")).unwrap();
    let evidence = vec![CapturedEvidence {
        path: "LICENSE".into(),
        sha256: digest(b"fixture evidence"),
        text: "fixture evidence".into(),
    }];
    save(
        &catalog,
        config,
        Resolution {
            schema_version: 1,
            mode: "resolved_inventory",
            transfer_authorized: false,
            inventory,
            evidence,
        },
        destination(&catalog, config).unwrap(),
    )
    .unwrap()
}

#[test]
fn reviews_survive_restarts_bind_identity_and_retain_final_decisions() {
    let (_dir, config) = setup();
    let first = fixture(&config);
    assert_eq!(first.decision, Decision::Pending);
    assert!(!first.transfer_authorized);
    assert!(first.policy_current);
    assert!(
        first
            .review
            .blockers
            .contains(&Blocker::TransferBackendUnqualified)
    );
    assert!(
        !first
            .review
            .blockers
            .contains(&Blocker::LocalInventoryUntrusted)
    );
    assert_eq!(first.review.evidence[0].text, "fixture evidence");
    let again = fixture(&config);
    assert_eq!(again.plan_id, first.plan_id);
    assert_eq!(again.created_at, first.created_at);
    assert_eq!(
        show_proposal(&config, &first.plan_id)
            .unwrap()
            .review
            .volume_id,
        initialize(&config).unwrap().volumes[0].volume_id
    );
    assert_eq!(
        decide(&config, &first.plan_id, Decision::Approved)
            .unwrap_err()
            .code,
        "transfer_backend_unqualified"
    );
    assert_eq!(
        show_proposal(&config, &first.plan_id).unwrap().decision,
        Decision::Pending
    );
    let rejected = decide(&config, &first.plan_id, Decision::Rejected).unwrap();
    assert!(rejected.decided_at.is_some());
    assert_eq!(
        decide(&config, &first.plan_id, Decision::Rejected)
            .unwrap()
            .decided_at,
        rejected.decided_at
    );
    assert_eq!(fixture(&config).decision, Decision::Rejected);
    assert_eq!(
        decide(&config, &first.plan_id, Decision::Approved)
            .unwrap_err()
            .code,
        "decision_conflict"
    );
    assert_eq!(
        decide(&config, &first.plan_id, Decision::Pending)
            .unwrap_err()
            .code,
        "invalid_decision"
    );
}

#[test]
fn changed_policy_creates_a_new_review_and_stale_approval_is_blocked() {
    let (_dir, config) = setup();
    let first = fixture(&config);
    let mut changed = config.clone();
    changed.transfer.max_transfer_bytes_per_period += 1;
    assert!(
        !show_proposal(&changed, &first.plan_id)
            .unwrap()
            .policy_current
    );
    assert_eq!(
        decide(&changed, &first.plan_id, Decision::Approved)
            .unwrap_err()
            .code,
        "stale_policy"
    );
    let second = fixture(&changed);
    assert_ne!(first.plan_id, second.plan_id);
    assert!(second.policy_current);
    let page = proposals(&changed, None, 1).unwrap();
    assert_eq!(page.proposals.len(), 1);
    let cursor = page.next_after.unwrap();
    let last = proposals(&changed, Some(&cursor), 1).unwrap();
    assert_eq!(last.proposals.len(), 1);
    assert_ne!(page.proposals[0].plan_id, last.proposals[0].plan_id);
    assert!(last.next_after.is_none());
    assert_eq!(proposals(&changed, None, 100).unwrap().proposals.len(), 2);
    for limit in [0, 101] {
        assert_eq!(
            proposals(&config, None, limit).unwrap_err().code,
            "invalid_page_limit"
        );
    }
    assert!(proposals(&config, Some("bad"), 20).is_err());
    assert!(show_proposal(&config, "bad").is_err());
    assert_eq!(
        show_proposal(&config, &"0".repeat(64)).unwrap_err().code,
        "proposal_not_found"
    );
}

#[test]
fn missing_destination_blocks_proposal_before_network_and_retains_offline_review() {
    let (dir, config) = setup();
    let review = fixture(&config);
    fs::rename(&config.volumes[0].path, dir.path().join("unplugged")).unwrap();
    assert_eq!(
        propose(&config, "invalid", "main").unwrap_err().code,
        "volume_unavailable"
    );
    assert_eq!(
        decide(&config, &review.plan_id, Decision::Approved)
            .unwrap_err()
            .code,
        "volume_unavailable"
    );
    assert_eq!(
        show_proposal(&config, &review.plan_id).unwrap().plan_id,
        review.plan_id
    );
    assert_eq!(
        decide(&config, &review.plan_id, Decision::Rejected)
            .unwrap()
            .decision,
        Decision::Rejected
    );
    let catalog = open_catalog(&config, false).unwrap();
    catalog
        .db
        .execute("UPDATE vault SET state='initializing'", [])
        .unwrap();
    drop(catalog);
    assert_eq!(
        propose(&config, "invalid", "main").unwrap_err().code,
        "vault_not_ready"
    );
}

#[test]
fn catalog_migration_preserves_ownership_and_bad_reviews_fail_closed() {
    let (_dir, config) = setup();
    let before = initialize(&config).unwrap();
    let catalog = open_catalog(&config, false).unwrap();
    catalog
        .db
        .execute_batch("DROP TABLE proposals; PRAGMA user_version=1;")
        .unwrap();
    drop(catalog);
    assert!(proposals(&config, None, 20).unwrap().proposals.is_empty());
    let after = initialize(&config).unwrap();
    assert_eq!(before.vault_id, after.vault_id);
    assert_eq!(before.volumes[0].volume_id, after.volumes[0].volume_id);
    let proposal = fixture(&config);
    let catalog = open_catalog(&config, false).unwrap();
    let original = serde_json::to_string(&proposal.review).unwrap();
    catalog
        .db
        .execute(
            "UPDATE proposals SET document='{}' WHERE id=?1",
            [&proposal.plan_id],
        )
        .unwrap();
    drop(catalog);
    assert_eq!(
        show_proposal(&config, &proposal.plan_id).unwrap_err().code,
        "invalid_proposal"
    );
    assert_eq!(
        proposals(&config, None, 20).unwrap_err().code,
        "invalid_proposal"
    );
    let catalog = open_catalog(&config, false).unwrap();
    catalog
        .db
        .execute(
            "UPDATE proposals SET document=?1 WHERE id=?2",
            params![original, proposal.plan_id],
        )
        .unwrap();
    assert!(
        catalog
            .db
            .execute("UPDATE proposals SET decision='unknown'", [])
            .is_err()
    );
    assert!(
        catalog
            .db
            .execute("UPDATE proposals SET decision='approved'", [])
            .is_err()
    );
    drop(catalog);
    assert_eq!(
        show_proposal(&config, &proposal.plan_id).unwrap().decision,
        Decision::Pending
    );
}

#[test]
fn invalid_source_fails_without_saving_a_review() {
    let (_dir, config) = setup();
    assert_eq!(
        propose(&config, "bad", "main").unwrap_err().code,
        "invalid_repo"
    );
    assert!(proposals(&config, None, 20).unwrap().proposals.is_empty());
}

#[test]
fn cli_shows_saved_evidence_and_records_only_the_exact_rejection() {
    use crate::cli::{Cli, execute};
    use clap::Parser;
    let (dir, config) = setup();
    let proposal = fixture(&config);
    let path = dir.path().join("config.toml");
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    let shown = execute(
        Cli::try_parse_from([
            "modelprepper",
            "proposals",
            "--config",
            path.to_str().unwrap(),
            "show",
            "--plan",
            &proposal.plan_id,
        ])
        .unwrap(),
    )
    .unwrap();
    assert_eq!(shown["review"]["evidence"][0]["text"], "fixture evidence");
    let rejected = execute(
        Cli::try_parse_from([
            "modelprepper",
            "decide",
            "--config",
            path.to_str().unwrap(),
            "--plan",
            &proposal.plan_id,
            "--decision",
            "reject",
        ])
        .unwrap(),
    )
    .unwrap();
    assert_eq!(rejected["decision"], "rejected");
    assert_eq!(rejected["transfer_authorized"], false);
}
