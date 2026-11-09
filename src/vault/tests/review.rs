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
fn every_supported_prior_schema_opens_at_7_and_keeps_its_rows() {
    let (_dir, config) = setup();
    let catalog = open_catalog(&config, false).unwrap();
    let (vault_id, state): (String, String) = catalog
        .db
        .query_row("SELECT id, state FROM vault", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(state, "ready");
    let (label, volume_id, volume_path): (String, String, String) = catalog
        .db
        .query_row("SELECT label, id, path FROM volumes", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    drop(catalog);
    let weight = config.volumes[0].path.join("sealed-weight.bin");
    let sealed = b"sealed-bytes";
    fs::write(&weight, sealed).unwrap();

    for version in 1..=6 {
        write_prior_catalog(
            &config,
            version,
            &vault_id,
            &label,
            &volume_id,
            &volume_path,
        );
        let report = crate::vault::status(&config).unwrap();
        assert_eq!(report.vault_id, vault_id);
        assert_eq!(fs::read(&weight).unwrap(), sealed);
        let catalog = open_catalog(&config, false).unwrap();
        assert_eq!(user_version(&catalog.db), 7);
        assert_eq!(text(&catalog.db, "SELECT id FROM vault"), vault_id);
        assert_eq!(text(&catalog.db, "SELECT id FROM volumes"), volume_id);
        match version {
            1 => {
                assert_eq!(rows(&catalog.db, "proposals"), 0);
                assert_eq!(rows(&catalog.db, "custody"), 0);
                assert_eq!(rows(&catalog.db, "grants"), 0);
                assert_eq!(rows(&catalog.db, "replicas"), 0);
                assert_eq!(rows(&catalog.db, "period_charges"), 0);
                assert_eq!(rows(&catalog.db, "collections"), 0);
            }
            2 => assert_eq!(
                text(&catalog.db, "SELECT document FROM proposals"),
                r#"{"kept":"proposal"}"#
            ),
            3 => assert_eq!(
                text(&catalog.db, "SELECT provenance FROM custody"),
                "kept-provenance"
            ),
            4 => assert_eq!(
                text(&catalog.db, "SELECT document FROM grants"),
                r#"{"kept":"grant"}"#
            ),
            5 => assert_eq!(
                text(&catalog.db, "SELECT relative_dir FROM replicas"),
                "kept-copy"
            ),
            6 => assert_eq!(
                i64_cell(&catalog.db, "SELECT payload_bytes FROM period_charges"),
                10
            ),
            _ => unreachable!(),
        }
        drop(catalog);
    }

    for version in [0u32, 8] {
        write_prior_catalog(
            &config,
            version,
            &vault_id,
            &label,
            &volume_id,
            &volume_path,
        );
        assert_eq!(
            crate::vault::status(&config).unwrap_err().code,
            "unsupported_catalog_schema"
        );
        assert_eq!(fs::read(&weight).unwrap(), sealed);
        let connection = rusqlite::Connection::open(database(&config)).unwrap();
        assert_eq!(user_version(&connection), version);
        assert!(!table_exists(
            &connection,
            if version == 0 {
                "proposals"
            } else {
                "collections"
            }
        ));
    }

    write_prior_catalog(&config, 6, &vault_id, &label, &volume_id, &volume_path);
    crate::vault::status(&config).unwrap();
    let catalog = open_catalog(&config, false).unwrap();
    catalog
        .db
        .execute(
            "INSERT INTO collections (name, replica) VALUES ('kept', NULL)",
            [],
        )
        .unwrap();
    let tables = rows_matching(
        &catalog.db,
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE 'collection%'",
    );
    drop(catalog);
    crate::vault::status(&config).unwrap();
    assert_eq!(fs::read(&weight).unwrap(), sealed);
    let catalog = open_catalog(&config, false).unwrap();
    assert_eq!(user_version(&catalog.db), 7);
    assert_eq!(text(&catalog.db, "SELECT name FROM collections"), "kept");
    assert_eq!(
        rows_matching(
            &catalog.db,
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name LIKE 'collection%'"
        ),
        tables
    );
}

fn database(config: &Config) -> std::path::PathBuf {
    config.catalog_path.join("catalog.sqlite3")
}

fn write_prior_catalog(
    config: &Config,
    version: u32,
    vault_id: &str,
    label: &str,
    volume_id: &str,
    volume_path: &str,
) {
    let path = database(config);
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(path.with_file_name("catalog.sqlite3-wal"));
    let _ = fs::remove_file(path.with_file_name("catalog.sqlite3-shm"));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE vault (id TEXT PRIMARY KEY, state TEXT NOT NULL);
             CREATE TABLE volumes (
                label TEXT PRIMARY KEY,
                id TEXT NOT NULL UNIQUE,
                path TEXT NOT NULL UNIQUE
             );",
        )
        .unwrap();
    connection
        .execute("INSERT INTO vault VALUES (?1, 'ready')", [vault_id])
        .unwrap();
    connection
        .execute(
            "INSERT INTO volumes VALUES (?1, ?2, ?3)",
            rusqlite::params![label, volume_id, volume_path],
        )
        .unwrap();
    if matches!(version, 2..=7) {
        connection
            .execute_batch(
                "CREATE TABLE proposals (
                    id TEXT PRIMARY KEY CHECK(length(id)=64),
                    document TEXT NOT NULL,
                    observed_available_bytes TEXT NOT NULL,
                    decision TEXT NOT NULL DEFAULT 'pending' CHECK(decision IN ('pending','approved','rejected')),
                    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                    decided_at INTEGER,
                    CHECK((decision='pending' AND decided_at IS NULL) OR (decision!='pending' AND decided_at IS NOT NULL))
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO proposals (id, document, observed_available_bytes, decision)
                 VALUES (?1, '{\"kept\":\"proposal\"}', '1', 'pending')",
                [&"ab".repeat(32)],
            )
            .unwrap();
    }
    if matches!(version, 3..=7) {
        connection
            .execute_batch(
                "CREATE TABLE custody (
                    plan_id TEXT PRIMARY KEY CHECK(length(plan_id)=64),
                    repo_id TEXT NOT NULL,
                    resolved_revision TEXT NOT NULL CHECK(length(resolved_revision)=40),
                    provenance TEXT NOT NULL,
                    volume_label TEXT NOT NULL,
                    relative_dir TEXT NOT NULL,
                    manifest TEXT NOT NULL
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO custody VALUES (?1, 'example/model', ?2, 'kept-provenance', ?3, 'model', '{}')",
                rusqlite::params!["cd".repeat(32), "12".repeat(20), label],
            )
            .unwrap();
    }
    if matches!(version, 4..=7) {
        connection
            .execute_batch(
                "CREATE TABLE grants (
                    plan_id TEXT PRIMARY KEY CHECK(length(plan_id)=64),
                    document TEXT NOT NULL,
                    recorded_at INTEGER NOT NULL DEFAULT (unixepoch())
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO grants (plan_id, document) VALUES (?1, '{\"kept\":\"grant\"}')",
                [&"ef".repeat(32)],
            )
            .unwrap();
    }
    if matches!(version, 5..=7) {
        connection
            .execute_batch(
                "CREATE TABLE replicas (
                    plan_id TEXT NOT NULL CHECK(length(plan_id)=64),
                    volume_label TEXT NOT NULL,
                    relative_dir TEXT NOT NULL,
                    manifest TEXT NOT NULL,
                    PRIMARY KEY (plan_id, volume_label)
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO replicas VALUES (?1, ?2, 'kept-copy', '{}')",
                rusqlite::params!["12".repeat(32), label],
            )
            .unwrap();
    }
    if matches!(version, 6..=7) {
        connection
            .execute_batch(
                "CREATE TABLE period_charges (
                    period_start TEXT NOT NULL,
                    plan_id TEXT NOT NULL CHECK(length(plan_id)=64),
                    payload_bytes INTEGER NOT NULL CHECK(payload_bytes >= 0),
                    PRIMARY KEY (period_start, plan_id)
                );",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO period_charges VALUES ('2026-11-02', ?1, 10)",
                [&"34".repeat(32)],
            )
            .unwrap();
    }
    connection
        .execute_batch(&format!("PRAGMA user_version={version};"))
        .unwrap();
}

fn user_version(connection: &rusqlite::Connection) -> u32 {
    connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

fn text(connection: &rusqlite::Connection, sql: &str) -> String {
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn i64_cell(connection: &rusqlite::Connection, sql: &str) -> i64 {
    connection.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn rows(connection: &rusqlite::Connection, table: &str) -> i64 {
    i64_cell(connection, &format!("SELECT COUNT(*) FROM {table}"))
}

fn rows_matching(connection: &rusqlite::Connection, sql: &str) -> i64 {
    i64_cell(connection, sql)
}

fn table_exists(connection: &rusqlite::Connection, name: &str) -> bool {
    connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
        == 1
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
    use crate::cli::{Cli, CommandOutput, execute};
    use clap::Parser;
    let (dir, config) = setup();
    let proposal = fixture(&config);
    let path = dir.path().join("config.toml");
    fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
    let CommandOutput::Json(shown) = execute(
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
    .unwrap() else {
        panic!("expected json");
    };
    assert_eq!(shown["review"]["evidence"][0]["text"], "fixture evidence");
    let CommandOutput::Json(rejected) = execute(
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
    .unwrap() else {
        panic!("expected json");
    };
    assert_eq!(rejected["decision"], "rejected");
    assert_eq!(rejected["transfer_authorized"], false);
}
