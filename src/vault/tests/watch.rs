use super::*;
use crate::{
    fixture::{Server, response},
    hub::{CapturedEvidence, Resolution},
    inventory::{
        EvidenceStatus, HashAlgorithm, Inventory, LicenseEvidence, Role, SourceFile, UpstreamHash,
    },
    plan::digest,
    transfer::KernelSocketBuffers,
    vault::initialize,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const NOW: u64 = 1_794_139_200;
const WEEK: &str = "2026-11-02";
const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
const NEXT: &str = "fedcba98fedcba98fedcba98fedcba98fedcba98";
const LICENSE: &[u8] = b"\
MIT License\n\
\n\
Copyright (c) 2026 Example\n\
\n\
Permission is hereby granted, free of charge, to any person obtaining a copy of this software \
and associated documentation files (the \"Software\"), to deal in the Software without \
restriction, including without limitation the rights to use, copy, modify, merge, publish, \
distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the \
Software is furnished to do so, subject to the following conditions:\n\
\n\
The above copyright notice and this permission notice shall be included in all copies or \
substantial portions of the Software.\n\
\n\
THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING \
BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND \
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, \
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, \
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.\n";

fn setup() -> (tempfile::TempDir, crate::Config, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let config = crate::Config::default_for(root.join("catalog"), root.join("primary"));
    initialize(&config).unwrap();
    (dir, config, root.join("stage"))
}

fn kernel() -> KernelSocketBuffers {
    KernelSocketBuffers {
        receive_bytes: 16_384,
        send_bytes: 32_768,
    }
}

fn watch(repo: &str) -> crate::config::Watch {
    crate::config::Watch {
        source: "huggingface".into(),
        repo_type: "model".into(),
        repo_id: repo.into(),
        revision: "main".into(),
        update_policy: "propose".into(),
    }
}

fn source(revision: &str) -> crate::inventory::Source {
    crate::inventory::Source {
        endpoint: "https://huggingface.co".into(),
        repo_type: "model".into(),
        repo_id: "example/model".into(),
        requested_revision: "main".into(),
        resolved_revision: revision.into(),
        gated: false,
        private: false,
    }
}

fn bodies() -> Vec<(&'static str, &'static [u8], Role)> {
    vec![
        ("LICENSE", LICENSE, Role::License),
        (
            "chat_template.jinja",
            include_bytes!("../../../examples/readiness/chat_template.jinja"),
            Role::Template,
        ),
        (
            "config.json",
            include_bytes!("../../../examples/readiness/config.json"),
            Role::Config,
        ),
        (
            "model.safetensors",
            include_bytes!("../../../examples/readiness/model.safetensors"),
            Role::Weight,
        ),
        (
            "tokenizer.json",
            include_bytes!("../../../examples/readiness/tokenizer.json"),
            Role::Tokenizer,
        ),
    ]
}

fn inventory(revision: &str) -> Inventory {
    Inventory {
        schema_version: 1,
        source: source(revision),
        complete: true,
        layout: "transformers-safetensors-v1".into(),
        license: LicenseEvidence {
            license_id: "mit".into(),
            status: EvidenceStatus::Matched,
            paths: vec!["LICENSE".into()],
        },
        files: bodies()
            .into_iter()
            .map(|(path, bytes, role)| SourceFile {
                path: path.into(),
                size_bytes: bytes.len() as u64,
                upstream_hash: Some(UpstreamHash {
                    algorithm: HashAlgorithm::Sha256,
                    value: digest(bytes),
                }),
                role,
                required: true,
            })
            .collect(),
        base: None,
    }
}

fn resolution(revision: &str) -> Resolution {
    Resolution {
        schema_version: 1,
        mode: "resolved_inventory",
        transfer_authorized: false,
        inventory: inventory(revision),
        evidence: vec![CapturedEvidence {
            path: "LICENSE".into(),
            sha256: digest(LICENSE),
            text: String::from_utf8(LICENSE.to_vec()).unwrap(),
        }],
    }
}

fn closed() -> crate::http::Http {
    crate::http::Http::fixture("http://127.0.0.1:1")
}

fn io<'a>(
    http: &'a crate::http::Http,
    base: &'a str,
    now: u64,
    stop: Option<crate::vault::SealStop>,
) -> WatchIo<'a> {
    WatchIo {
        now,
        http,
        base,
        kernel: kernel(),
        stop,
    }
}

fn ranged(body: &[u8]) -> Vec<u8> {
    let headers = format!(
        "Content-Range: bytes 0-{}/{}\r\n",
        body.len() - 1,
        body.len()
    );
    response(206, &headers, body)
}

fn responses() -> Vec<Vec<u8>> {
    bodies()
        .into_iter()
        .map(|(_, body, _)| ranged(body))
        .collect()
}

fn count(config: &crate::Config, sql: &str) -> i64 {
    let catalog = super::super::open_catalog(config, false).unwrap();
    catalog.db.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn seed_charge(config: &crate::Config, period: &str, plan: &str, bytes: i64) {
    let catalog = super::super::open_catalog(config, false).unwrap();
    catalog
        .db
        .execute(
            "INSERT INTO period_charges (period_start, plan_id, payload_bytes) VALUES (?1,?2,?3)",
            rusqlite::params![period, plan, bytes],
        )
        .unwrap();
}

fn reserved(config: &crate::Config, plan: &str) -> PathBuf {
    config.volumes[0]
        .path
        .join(".modelprepper-adopt")
        .join(format!("{plan}.reservation.json"))
}

fn published(config: &crate::Config, revision: &str) -> PathBuf {
    config.volumes[0]
        .path
        .join("preserved")
        .join("example")
        .join("model")
        .join(revision)
}

fn assert_closed(report: &WatchReport) {
    assert_eq!(report.schema_version, 1);
    assert_eq!(report.status, "watched");
    assert!(!report.transfer_authorized);
    assert_eq!(report.workspace_bytes, None);
}

fn grant(config: &crate::Config, plan: &str) {
    super::super::grant::record_grant_with(config, plan, kernel()).unwrap();
}

#[test]
fn an_empty_list_names_the_week_and_does_not_resolve_or_require_a_stage() {
    let (_dir, config, _stage) = setup();
    let mut called = false;
    let http = closed();
    let report = run_with(
        &config,
        Path::new("relative"),
        |_, _| {
            called = true;
            Err(Error::new("revision_not_found", "absent"))
        },
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert!(!called);
    assert_closed(&report);
    assert_eq!(report.period_start, WEEK);
    assert_eq!(report.charged_bytes, 0);
    assert!(report.entries.is_empty());

    let far = run_with(
        &config,
        Path::new("relative"),
        |_, _| unreachable!("empty list"),
        &io(&http, "http://127.0.0.1:1", u64::MAX, None),
    )
    .unwrap();
    assert_eq!(far.charged_bytes, 0);
    assert!(far.period_start.len() >= 10);
}

#[test]
fn weeks_start_on_monday_including_dates_before_the_epoch() {
    assert_eq!(week_start(1_794_139_200), "2026-11-02");
    assert_eq!(week_start(0), "1969-12-29");
    assert_eq!(week_start(345_600), "1970-01-05");
    assert_eq!(week_start(5_184_000), "1970-03-02");
    let ancient = week_start(-800_000 * 86_400);
    assert!(ancient.contains('-'), "{ancient}");
}

#[test]
fn a_bad_stage_fails_before_any_watch_is_resolved() {
    let (_dir, mut config, _stage) = setup();
    config.watches = vec![watch("example/model")];
    let mut called = false;
    let http = closed();
    let error = run_with(
        &config,
        Path::new("relative"),
        |_, _| {
            called = true;
            Ok(resolution(REVISION))
        },
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap_err();
    assert!(!called);
    assert_eq!(error.code, "unsafe_staging_path");
}

#[test]
fn a_missing_destination_fails_closed_before_a_fetch() {
    let (_dir, mut config, stage) = setup();
    config.watches = vec![watch("example/model")];
    let marker = config.volumes[0].path.join(".modelprepper-volume.json");
    fs::remove_file(&marker).unwrap();
    let http = closed();
    let error = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap_err();
    assert_eq!(error.code, "volume_unavailable");
    assert_eq!(count(&config, "SELECT COUNT(*) FROM proposals"), 0);
}

#[test]
fn source_failures_stay_on_one_entry_and_an_unknown_code_aborts() {
    let (_dir, mut config, stage) = setup();
    let cases = [
        ("example/missing", "revision_not_found", "source_missing"),
        (
            "example/private",
            "repository_not_found_or_private",
            "access_failed",
        ),
        ("example/gated", "access_restricted", "access_failed"),
        ("example/status", "source_status", "access_failed"),
        ("example/metadata", "invalid_source_metadata", "partial"),
        ("example/identity", "source_identity_changed", "partial"),
        ("example/layout", "invalid_layout", "partial"),
        ("example/license", "invalid_license_encoding", "partial"),
    ];
    config.watches = cases.iter().map(|(repo, _, _)| watch(repo)).collect();
    let http = closed();
    let report = run_with(
        &config,
        &stage,
        |repo, revision| {
            assert_eq!(revision, "main");
            let code = cases.iter().find(|(name, _, _)| *name == repo).unwrap().1;
            Err(Error::new(code, format!("{repo} failed")))
        },
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert_closed(&report);
    assert_eq!(report.entries.len(), cases.len());
    for (entry, (repo, _, action)) in report.entries.iter().zip(cases) {
        assert_eq!(entry.repo_id, repo);
        assert_eq!(entry.action, action);
        assert_eq!(entry.resolved_revision, None);
        assert_eq!(entry.plan_id, None);
        assert!(entry.detail.contains(repo));
    }
    assert_eq!(count(&config, "SELECT COUNT(*) FROM proposals"), 0);

    config.watches = vec![watch("example/model"), watch("example/other")];
    let mut second = false;
    let error = run_with(
        &config,
        &stage,
        |repo, _| {
            if repo == "example/other" {
                second = true;
            }
            Err(Error::new("destination_changed", "the source moved"))
        },
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap_err();
    assert_eq!(error.code, "destination_changed");
    assert!(!second);
}

#[test]
fn an_incomplete_listing_is_not_saved_and_a_missing_grant_does_not_fetch() {
    let (_dir, mut config, stage) = setup();
    config.watches = vec![watch("example/model")];
    let http = closed();
    let report = run_with(
        &config,
        &stage,
        |_, _| {
            let mut partial = resolution(REVISION);
            partial.inventory.complete = false;
            Ok(partial)
        },
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert_eq!(report.entries[0].action, "partial");
    assert_eq!(report.entries[0].plan_id, None);
    assert_eq!(
        report.entries[0].resolved_revision.as_deref(),
        Some(REVISION)
    );
    assert_eq!(count(&config, "SELECT COUNT(*) FROM proposals"), 0);

    let proposed = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert_closed(&proposed);
    assert_eq!(proposed.entries[0].action, "proposed");
    assert!(proposed.entries[0].plan_id.is_some());
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 0);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM period_charges"), 0);
    assert!(!published(&config, REVISION).exists());
}

#[test]
fn a_changed_kernel_drops_the_grant_and_still_does_not_fetch() {
    let (_dir, mut config, stage) = setup();
    config.watches = vec![watch("example/model")];
    let http = closed();
    let proposed = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    let plan = proposed.entries[0].plan_id.clone().unwrap();
    grant(&config, &plan);
    let mut dropped = io(&http, "http://127.0.0.1:1", NOW, None);
    dropped.kernel.receive_bytes = 1;
    let report = run_with(&config, &stage, |_, _| Ok(resolution(REVISION)), &dropped).unwrap();
    assert_eq!(report.entries[0].action, "proposed");
    assert_eq!(report.entries[0].plan_id.as_deref(), Some(plan.as_str()));
    assert_eq!(count(&config, "SELECT COUNT(*) FROM grants"), 0);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 0);
}

#[test]
fn a_grant_fetches_once_keeps_the_older_revision_and_leaves_a_deleted_source() {
    let (_dir, mut config, stage) = setup();
    config.watches = vec![watch("example/model"), watch("example/other")];
    let http = closed();
    let proposed = run_with(
        &config,
        &stage,
        |repo, _| {
            if repo == "example/other" {
                Err(Error::new("access_restricted", "gated"))
            } else {
                Ok(resolution(REVISION))
            }
        },
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert_eq!(proposed.entries[0].action, "proposed");
    assert_eq!(proposed.entries[1].action, "access_failed");
    let plan = proposed.entries[0].plan_id.clone().unwrap();
    grant(&config, &plan);

    let server = Server::new(responses());
    let origin = server.origin.clone();
    let live = crate::http::Http::fixture(&origin);
    let fetched = run_with(
        &config,
        &stage,
        |repo, _| {
            if repo == "example/other" {
                Err(Error::new("revision_not_found", "removed"))
            } else {
                Ok(resolution(REVISION))
            }
        },
        &io(&live, &origin, NOW, None),
    )
    .unwrap();
    assert_eq!(
        fetched.entries[0].action, "fetched",
        "{}",
        fetched.entries[0].detail
    );
    assert_eq!(server.finish().len(), 5);
    assert_closed(&fetched);
    assert_eq!(fetched.period_start, WEEK);
    assert_eq!(fetched.entries[0].action, "fetched");
    assert_eq!(fetched.entries[0].plan_id.as_deref(), Some(plan.as_str()));
    assert_eq!(fetched.entries[1].action, "source_missing");
    assert!(fetched.charged_bytes > 0);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 1);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM period_charges"), 1);
    assert!(
        published(&config, REVISION)
            .join("modelprepper-bundle.json")
            .is_file()
    );
    assert!(!reserved(&config, &plan).exists());

    let mut repeat = io(&http, "http://127.0.0.1:1", NOW, None);
    repeat.kernel.receive_bytes = 1;
    let again = run_with(
        &config,
        &stage,
        |repo, _| {
            if repo == "example/other" {
                Err(Error::new("revision_not_found", "removed"))
            } else {
                Ok(resolution(REVISION))
            }
        },
        &repeat,
    )
    .unwrap();
    assert_eq!(again.entries[0].action, "unchanged");
    assert_eq!(again.charged_bytes, fetched.charged_bytes);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM grants"), 1);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 1);

    let proposed_next = run_with(
        &config,
        &stage,
        |repo, _| {
            if repo == "example/other" {
                Err(Error::new("revision_not_found", "removed"))
            } else {
                Ok(resolution(NEXT))
            }
        },
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    let next_plan = proposed_next.entries[0].plan_id.clone().unwrap();
    assert_ne!(next_plan, plan);
    assert_eq!(proposed_next.entries[0].action, "proposed");
    assert!(published(&config, REVISION).is_dir());
    grant(&config, &next_plan);
    fs::remove_dir_all(&stage).unwrap();
    let server = Server::new(responses());
    let origin = server.origin.clone();
    let live = crate::http::Http::fixture(&origin);
    let second = run_with(
        &config,
        &stage,
        |repo, _| {
            if repo == "example/other" {
                Err(Error::new("revision_not_found", "removed"))
            } else {
                Ok(resolution(NEXT))
            }
        },
        &io(&live, &origin, NOW, None),
    )
    .unwrap();
    assert_eq!(server.finish().len(), 5);
    assert_eq!(second.entries[0].action, "fetched");
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 2);
    assert!(published(&config, REVISION).is_dir());
    assert!(published(&config, NEXT).is_dir());

    let missing = run_with(
        &config,
        &stage,
        |_, _| {
            Err(Error::new(
                "revision_not_found",
                "publisher removed the commit",
            ))
        },
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert!(
        missing
            .entries
            .iter()
            .all(|entry| entry.action == "source_missing")
    );
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 2);
    assert!(published(&config, REVISION).is_dir());
    assert!(published(&config, NEXT).is_dir());
}

#[test]
fn the_weekly_ceiling_pauses_until_the_next_week_and_a_full_ledger_does_not_fetch() {
    let (_dir, mut config, stage) = setup();
    config.watches = vec![watch("example/model")];
    let http = closed();
    let proposed = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    let plan = proposed.entries[0].plan_id.clone().unwrap();
    let payload = crate::vault::show_proposal(&config, &plan)
        .unwrap()
        .review
        .payload_bytes;
    assert!(payload > 0);
    grant(&config, &plan);
    seed_charge(
        &config,
        WEEK,
        &"ab".repeat(32),
        i64::try_from(config.transfer.max_transfer_bytes_per_period - 1).unwrap(),
    );
    let paused = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert_eq!(paused.entries[0].action, "paused");
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 0);
    assert!(!published(&config, REVISION).exists());

    let later = NOW + 7 * 86_400;
    let server = Server::new(responses());
    let origin = server.origin.clone();
    let live = crate::http::Http::fixture(&origin);
    let fetched = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&live, &origin, later, None),
    )
    .unwrap();
    assert_eq!(
        fetched.entries[0].action, "fetched",
        "{}",
        fetched.entries[0].detail
    );
    assert_eq!(server.finish().len(), 5);
    assert_eq!(fetched.period_start, "2026-11-09");
    assert_eq!(fetched.entries[0].action, "fetched");
    assert_eq!(fetched.charged_bytes, payload);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 1);

    let (_dir, mut config, stage) = setup();
    config.watches = vec![watch("example/model")];
    let proposed = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    let plan = proposed.entries[0].plan_id.clone().unwrap();
    grant(&config, &plan);
    seed_charge(&config, WEEK, &"cd".repeat(32), i64::MAX);
    let paused = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert_eq!(paused.entries[0].action, "paused");
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 0);
    assert!(charge_fits(1, 1, 2).unwrap());
    assert!(!charge_fits(2, 1, 2).unwrap());
    assert_eq!(
        charge_fits(u64::MAX, 1, u64::MAX).unwrap_err().code,
        "size_overflow"
    );
    assert_eq!(
        record_charge(&config, WEEK, &"ef".repeat(32), u64::MAX)
            .unwrap_err()
            .code,
        "size_overflow"
    );
}

#[test]
fn an_interrupted_seal_resumes_without_a_second_charge_or_an_approval() {
    let (_dir, mut config, stage) = setup();
    config.watches = vec![watch("example/model")];
    let http = closed();
    let proposed = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    let plan = proposed.entries[0].plan_id.clone().unwrap();
    grant(&config, &plan);
    let server = Server::new(responses());
    let origin = server.origin.clone();
    let live = crate::http::Http::fixture(&origin);
    let interrupted = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(
            &live,
            &origin,
            NOW,
            Some(crate::vault::SealStop::BeforeCatalog),
        ),
    )
    .unwrap();
    assert_eq!(
        interrupted.entries[0].action, "fetch_failed",
        "{}",
        interrupted.entries[0].detail
    );
    assert_eq!(server.finish().len(), 5);
    assert_eq!(interrupted.entries[0].action, "fetch_failed");
    assert!(interrupted.entries[0].detail.contains("catalog"));
    assert_eq!(interrupted.charged_bytes, 0);
    assert!(reserved(&config, &plan).is_file());
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 0);
    assert!(
        published(&config, REVISION)
            .join("modelprepper-bundle.json")
            .is_file()
    );

    let resumed = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW, None),
    )
    .unwrap();
    assert_eq!(resumed.entries[0].action, "fetched");
    assert!(resumed.charged_bytes > 0);
    assert!(!reserved(&config, &plan).exists());
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 1);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM period_charges"), 1);

    let later = run_with(
        &config,
        &stage,
        |_, _| Ok(resolution(REVISION)),
        &io(&http, "http://127.0.0.1:1", NOW + 7 * 86_400, None),
    )
    .unwrap();
    assert_eq!(later.entries[0].action, "unchanged");
    assert_eq!(later.period_start, "2026-11-09");
    assert_eq!(later.charged_bytes, 0);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), 1);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM period_charges"), 1);

    let error = crate::vault::decide(&config, &plan, crate::vault::Decision::Approved).unwrap_err();
    assert_eq!(error.code, "transfer_backend_unqualified");
    assert_eq!(
        crate::vault::show_proposal(&config, &plan)
            .unwrap()
            .decision,
        crate::vault::Decision::Pending
    );
}

#[test]
fn text_names_each_action_and_keeps_approval_blocked() {
    let empty = WatchReport {
        schema_version: 1,
        status: "watched",
        period_start: WEEK.into(),
        charged_bytes: 0,
        transfer_authorized: false,
        workspace_bytes: None,
        entries: Vec::new(),
    };
    let plain = crate::present::render_watch(&empty, false, 80);
    assert!(plain.contains("watched"));
    assert!(plain.contains("nothing fetched"));
    assert!(plain.contains("still blocked"));
    assert!(!plain.contains('\u{1b}'));
    let narrow = crate::present::render_watch(&empty, true, 20);
    assert!(narrow.contains('\u{1b}'));
    for line in narrow.lines() {
        let visible = line
            .replace("\u{1b}[32m", "")
            .replace("\u{1b}[36m", "")
            .replace("\u{1b}[0m", "");
        assert!(visible.chars().count() <= 20, "{visible}");
    }

    let rich = WatchReport {
        entries: vec![
            WatchEntry {
                repo_id: "example/model".into(),
                requested_revision: "main".into(),
                resolved_revision: Some(REVISION.into()),
                plan_id: Some("abcdef0123456789".repeat(4)),
                action: "fetched",
                detail: "sealed inside the current grant".into(),
            },
            WatchEntry {
                repo_id: "example/other".into(),
                requested_revision: "main".into(),
                resolved_revision: None,
                plan_id: None,
                action: "source_missing",
                detail: "removed".into(),
            },
        ],
        ..empty
    };
    let text = crate::present::render_watch(&rich, true, 80);
    assert!(text.contains("2 entries"));
    assert!(text.contains("fetched"));
    assert!(text.contains("example/model"));
    assert!(text.contains("01234567"));
    assert!(text.contains("abcdef01"));
    assert!(text.contains("source_missing"));
    assert!(text.contains("no plan"));
    assert!(text.contains("still blocked"));
    let one = WatchReport {
        schema_version: 1,
        status: "watched",
        period_start: WEEK.into(),
        charged_bytes: 0,
        transfer_authorized: false,
        workspace_bytes: None,
        entries: vec![WatchEntry {
            repo_id: "example/other".into(),
            requested_revision: "main".into(),
            resolved_revision: None,
            plan_id: None,
            action: "source_missing",
            detail: "removed".into(),
        }],
    };
    let singular = crate::present::render_watch(&one, false, 80);
    assert!(singular.contains("1 entry"));
    assert!(singular.contains("no plan"));
    assert!(!singular.contains("1 entries"));
}
