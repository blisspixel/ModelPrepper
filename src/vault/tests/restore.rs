use super::*;
use crate::{
    fixture::{Server, response},
    hub::{CapturedEvidence, Resolution},
    inventory::{
        EvidenceStatus, HashAlgorithm, Inventory, LicenseEvidence, Role, SourceFile, UpstreamHash,
    },
    plan::{Blocker, digest},
    transfer::KernelSocketBuffers,
    vault::{Decision, PROVENANCE_RANGED_HTTPS, initialize},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";
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
    let mut config = crate::Config::default_for(root.join("catalog"), root.join("primary"));
    config.volumes.push(crate::config::Volume {
        label: "backup".into(),
        path: root.join("backup"),
    });
    initialize(&config).unwrap();
    (dir, config, root.join("stage"))
}

fn kernel() -> KernelSocketBuffers {
    KernelSocketBuffers {
        receive_bytes: 16_384,
        send_bytes: 32_768,
    }
}

fn source() -> crate::inventory::Source {
    crate::inventory::Source {
        endpoint: "https://huggingface.co".into(),
        repo_type: "model".into(),
        repo_id: "example/model".into(),
        requested_revision: "main".into(),
        resolved_revision: REVISION.into(),
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

fn files() -> Vec<SourceFile> {
    bodies()
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
        .collect()
}

fn inventory() -> Inventory {
    Inventory {
        schema_version: 1,
        source: source(),
        complete: true,
        layout: "transformers-safetensors-v1".into(),
        license: LicenseEvidence {
            license_id: "mit".into(),
            status: EvidenceStatus::Matched,
            paths: vec!["LICENSE".into()],
        },
        files: files(),
        base: None,
    }
}

fn granted(config: &crate::Config) -> crate::vault::Proposal {
    let catalog = super::super::open_catalog(config, false).unwrap();
    let current = super::super::report(&catalog, config).unwrap();
    let volume = current
        .volumes
        .iter()
        .find(|volume| volume.label == config.storage.default_volume)
        .unwrap();
    let proposal = super::super::review::save(
        &catalog,
        config,
        Resolution {
            schema_version: 1,
            mode: "resolved_inventory",
            transfer_authorized: false,
            inventory: inventory(),
            evidence: vec![CapturedEvidence {
                path: "LICENSE".into(),
                sha256: digest(LICENSE),
                text: String::from_utf8(LICENSE.to_vec()).unwrap(),
            }],
        },
        (
            current.vault_id,
            volume.volume_id.clone(),
            volume.available_bytes.unwrap(),
        ),
    )
    .unwrap();
    drop(catalog);
    assert_eq!(
        proposal.review.blockers,
        vec![Blocker::TransferBackendUnqualified]
    );
    super::super::grant::record_grant_with(config, &proposal.plan_id, kernel()).unwrap();
    proposal
}

fn ranged(body: &[u8]) -> Vec<u8> {
    let headers = format!(
        "Content-Range: bytes 0-{}/{}\r\n",
        body.len() - 1,
        body.len()
    );
    response(206, &headers, body)
}

fn seal(config: &crate::Config, stage: &Path) -> crate::vault::Proposal {
    let proposal = granted(config);
    let order: Vec<&[u8]> = bodies().into_iter().map(|(_, body, _)| body).collect();
    let server = Server::new(order.iter().copied().map(ranged).collect());
    super::super::seal::fetch_with(
        config,
        &proposal.plan_id,
        stage,
        &crate::http::Http::fixture(&server.origin),
        &server.origin,
        kernel(),
        None,
    )
    .unwrap();
    let _ = server.finish();
    proposal
}

fn count(config: &crate::Config, sql: &str) -> i64 {
    let catalog = super::super::open_catalog(config, false).unwrap();
    catalog.db.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn delete_catalog(config: &crate::Config) {
    for name in [
        "catalog.sqlite3",
        "catalog.sqlite3-wal",
        "catalog.sqlite3-shm",
        "catalog.sqlite3-journal",
    ] {
        let path = config.catalog_path.join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
}

fn plant(volume: &Path, revision: &str, plan: &str, body: &[u8], provenance: Option<&str>) {
    let dir = volume
        .join("preserved")
        .join("example")
        .join("model")
        .join(revision);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("weights.bin"), body).unwrap();
    let mut document = serde_json::json!({
        "schema_version": 1,
        "bundle_id": plan,
        "source": {
            "endpoint": "local",
            "repo_type": "model",
            "repo_id": "example/model",
            "requested_revision": revision,
            "resolved_revision": revision,
            "gated": false,
            "private": false
        },
        "profile": "local-baseline",
        "rights": {"license_id": "unreviewed", "status": "unreviewed"},
        "files": [{
            "path": "weights.bin",
            "size_bytes": body.len(),
            "sha256": digest(body)
        }],
        "excluded": [],
        "dependencies": []
    });
    if let Some(provenance) = provenance {
        document["provenance"] = serde_json::Value::String(provenance.into());
    }
    fs::write(
        dir.join("modelprepper-bundle.json"),
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();
}

#[test]
fn replica_and_rebuild_restore_one_revision_with_the_hub_unreachable() {
    let (_dir, config, stage) = setup();
    let proposal = seal(&config, &stage);
    let marker: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(config.volumes[0].path.join(".modelprepper-volume.json")).unwrap(),
    )
    .unwrap();
    let copied = replica(&config, &proposal.plan_id, "backup").unwrap();
    assert!(!copied.transfer_authorized);
    assert!(copied.workspace_bytes.is_none());
    assert_eq!(copied.provenance, PROVENANCE_RANGED_HTTPS);
    assert_eq!(copied.failure_domain, "unlabeled");
    assert_eq!(copied.file_count, 5);
    assert!(copied.path.join("model.safetensors").is_file());
    assert_eq!(
        fs::read(copied.path.join("model.safetensors")).unwrap(),
        include_bytes!("../../../examples/readiness/model.safetensors")
    );
    assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 1);
    let again = replica(&config, &proposal.plan_id, "backup").unwrap();
    assert_eq!(again.path, copied.path);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 1);
    assert_eq!(
        replica(&config, &proposal.plan_id, "primary")
            .err()
            .unwrap()
            .code,
        "replica_same_volume"
    );
    assert_eq!(
        crate::vault::decide(&config, &proposal.plan_id, Decision::Approved)
            .err()
            .unwrap()
            .code,
        "transfer_backend_unqualified"
    );
    delete_catalog(&config);
    let rebuilt = rebuild(&config).unwrap();
    assert!(!rebuilt.approvals_transferred);
    assert!(!rebuilt.transfer_authorized);
    assert!(rebuilt.workspace_bytes.is_none());
    assert_eq!(rebuilt.vault_id, marker["vault_id"].as_str().unwrap());
    assert_eq!(rebuilt.custody_count, 1);
    assert_eq!(rebuilt.replica_count, 1);
    assert!(rebuilt.conflicts.is_empty());
    assert!(rebuilt.damaged.is_empty());
    assert_eq!(count(&config, "SELECT COUNT(*) FROM grants"), 0);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM proposals"), 0);
    let provenance: String = {
        let catalog = super::super::open_catalog(&config, false).unwrap();
        catalog
            .db
            .query_row("SELECT provenance FROM custody", [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(provenance, PROVENANCE_RANGED_HTTPS);
    let shown = crate::vault::status(&config).unwrap();
    assert_eq!(shown.replicas.len(), 1);
    assert_eq!(shown.replicas[0].volume_label, "backup");
}

#[test]
fn a_corrupt_file_and_an_interrupted_replica_are_not_healthy() {
    let (_dir, config, stage) = setup();
    let proposal = seal(&config, &stage);
    let published = config.volumes[0]
        .path
        .join("preserved")
        .join("example")
        .join("model")
        .join(REVISION)
        .join("model.safetensors");
    let mut bytes = fs::read(&published).unwrap();
    bytes[0] ^= 0xff;
    fs::write(&published, &bytes).unwrap();
    assert_eq!(
        replica(&config, &proposal.plan_id, "backup")
            .err()
            .unwrap()
            .code,
        "hash_mismatch"
    );
    assert!(
        !config.volumes[1]
            .path
            .join("preserved")
            .join("example")
            .exists()
    );
    assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 0);

    let (_dir, config, stage) = setup();
    let proposal = seal(&config, &stage);
    assert_eq!(
        replica_with(
            &config,
            &proposal.plan_id,
            "backup",
            Some(ReplicaStop::BeforeRename)
        )
        .err()
        .unwrap()
        .code,
        "replica_interrupted"
    );
    assert!(
        !config.volumes[1]
            .path
            .join("preserved")
            .join("example")
            .exists()
    );
    assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 0);
    replica(&config, &proposal.plan_id, "backup").unwrap();
    assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 1);

    let (_dir, config, stage) = setup();
    let proposal = seal(&config, &stage);
    assert_eq!(
        replica_with(
            &config,
            &proposal.plan_id,
            "backup",
            Some(ReplicaStop::BeforeCatalog)
        )
        .err()
        .unwrap()
        .code,
        "replica_interrupted"
    );
    assert!(
        config.volumes[1]
            .path
            .join("preserved")
            .join("example")
            .join("model")
            .join(REVISION)
            .join("modelprepper-bundle.json")
            .is_file()
    );
    assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 0);
    replica(&config, &proposal.plan_id, "backup").unwrap();
    replica(&config, &proposal.plan_id, "backup").unwrap();
    assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 1);
}

#[test]
fn conflicting_manifests_and_unrecorded_provenance_stay_honest() {
    let (_dir, config, _stage) = setup();
    let plan = digest(b"same-bundle");
    let other = "abcdefabcdefabcdefabcdefabcdefabcdefabcd";
    plant(
        &config.volumes[0].path,
        other,
        &plan,
        b"alpha",
        Some(PROVENANCE_RANGED_HTTPS),
    );
    plant(
        &config.volumes[1].path,
        other,
        &plan,
        b"beta",
        Some(PROVENANCE_RANGED_HTTPS),
    );
    let rebuilt = rebuild(&config).unwrap();
    assert_eq!(rebuilt.conflicts, vec![plan.clone()]);
    assert_eq!(rebuilt.custody_count, 0);
    assert_eq!(rebuilt.replica_count, 0);
    assert!(
        config.volumes[0]
            .path
            .join("preserved")
            .join("example")
            .join("model")
            .join(other)
            .join("weights.bin")
            .is_file()
    );
    assert_eq!(count(&config, "SELECT COUNT(*) FROM grants"), 0);

    let (_dir, config, _stage) = setup();
    let plan = digest(b"unrecorded");
    let revision = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    plant(&config.volumes[0].path, revision, &plan, b"gamma", None);
    let misplaced = config.volumes[0].path.join("preserved").join("cache");
    fs::create_dir_all(&misplaced).unwrap();
    fs::write(misplaced.join("modelprepper-bundle.json"), b"{}\n").unwrap();
    let rebuilt = rebuild(&config).unwrap();
    assert!(rebuilt.conflicts.is_empty());
    assert_eq!(rebuilt.custody_count, 1);
    assert!(!rebuilt.damaged.is_empty());
    let provenance: String = {
        let catalog = super::super::open_catalog(&config, false).unwrap();
        catalog
            .db
            .query_row("SELECT provenance FROM custody", [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(provenance, PROVENANCE_UNRECORDED);
    assert_ne!(provenance, PROVENANCE_RANGED_HTTPS);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM proposals"), 0);
    assert_eq!(count(&config, "SELECT COUNT(*) FROM grants"), 0);
}

#[test]
fn import_keeps_local_baseline_provenance_when_bytes_match() {
    let (_dir, config, stage) = setup();
    let proposal = seal(&config, &stage);
    let outside = stage.parent().unwrap().join("outside");
    let published = config.volumes[0]
        .path
        .join("preserved")
        .join("example")
        .join("model")
        .join(REVISION);
    fs::create_dir_all(&outside).unwrap();
    for (name, _, _) in bodies() {
        fs::copy(published.join(name), outside.join(name)).unwrap();
    }
    fs::write(
        outside.join("modelprepper-bundle.json"),
        br#"{"provenance":"upstream-verified (ranged https)"}"#,
    )
    .unwrap();
    let revision = "cccccccccccccccccccccccccccccccccccccccc";
    let imported =
        import_baseline(&config, &outside, "local/baseline", revision, "backup").unwrap();
    assert_eq!(imported.provenance, PROVENANCE_LOCAL_BASELINE);
    assert_eq!(
        imported.consistent_with.as_deref(),
        Some(proposal.plan_id.as_str())
    );
    assert!(!imported.transfer_authorized);
    assert!(imported.workspace_bytes.is_none());
    let bundle: serde_json::Value =
        serde_json::from_slice(&fs::read(imported.path.join("modelprepper-bundle.json")).unwrap())
            .unwrap();
    assert_eq!(bundle["provenance"], PROVENANCE_LOCAL_BASELINE);
    assert_eq!(bundle["source"]["endpoint"], "local");
    let sealed: String = {
        let catalog = super::super::open_catalog(&config, false).unwrap();
        catalog
            .db
            .query_row(
                "SELECT provenance FROM custody WHERE plan_id = ?1",
                [proposal.plan_id.as_str()],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert_eq!(sealed, PROVENANCE_RANGED_HTTPS);
    assert_eq!(
        count(&config, "SELECT COUNT(*) FROM grants WHERE plan_id != ''"),
        1
    );
    let baseline_grants: i64 = {
        let catalog = super::super::open_catalog(&config, false).unwrap();
        catalog
            .db
            .query_row(
                "SELECT COUNT(*) FROM grants WHERE plan_id = ?1",
                [imported.plan_id.as_str()],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert_eq!(baseline_grants, 0);
    assert_eq!(
        import_baseline(
            &config,
            &config.volumes[0].path,
            "local/baseline",
            revision,
            "backup"
        )
        .err()
        .unwrap()
        .code,
        "overlapping_vault_paths"
    );
    let empty = stage.parent().unwrap().join("empty");
    fs::create_dir(&empty).unwrap();
    assert_eq!(
        import_baseline(&config, &empty, "local/baseline", revision, "backup")
            .err()
            .unwrap()
            .code,
        "empty_revision"
    );
}

fn failure<T: std::fmt::Debug>(result: crate::Result<T>, expected: &str) {
    match result {
        Ok(value) => panic!("expected {expected}, got {value:?}"),
        Err(error) => assert_eq!(error.code, expected, "{}", error.message),
    }
}

fn bundle_file(volume: &Path, revision: &str) -> PathBuf {
    volume
        .join("preserved")
        .join("example")
        .join("model")
        .join(revision)
        .join("modelprepper-bundle.json")
}

fn rewrite_bundle(path: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut document: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    edit(&mut document);
    fs::write(path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
}

fn cell(config: &crate::Config, sql: &str) -> String {
    let catalog = super::super::open_catalog(config, false).unwrap();
    catalog.db.query_row(sql, [], |row| row.get(0)).unwrap()
}

fn exec(config: &crate::Config, sql: &str, params: impl rusqlite::Params) {
    let catalog = super::super::open_catalog(config, false).unwrap();
    catalog.db.execute(sql, params).unwrap();
}

#[cfg(unix)]
fn link(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn replica_refuses_a_tree_that_is_not_the_sealed_revision() {
    let (_dir, config, stage) = setup();
    let proposal = seal(&config, &stage);
    let source = bundle_file(&config.volumes[0].path, REVISION);
    let original = fs::read(&source).unwrap();
    let model = source.parent().unwrap().join("model.safetensors");
    let model_bytes = fs::read(&model).unwrap();

    failure(
        replica(&config, &digest(b"missing-revision"), "backup"),
        "revision_not_sealed",
    );
    failure(
        replica(&config, &proposal.plan_id, "other"),
        "unknown_volume",
    );
    let mut tight = config.clone();
    // Larger than any mounted disk, so the result does not depend on which volume was measured.
    tight.storage.min_free_bytes = 1 << 62;
    failure(
        replica(&tight, &proposal.plan_id, "backup"),
        "insufficient_space",
    );

    for edit in [
        Box::new(|document: &mut serde_json::Value| {
            document["bundle_id"] = serde_json::Value::String(digest(b"other-bundle"));
        }) as Box<dyn Fn(&mut serde_json::Value)>,
        Box::new(|document: &mut serde_json::Value| {
            document["source"]["repo_id"] = serde_json::json!("other/repo");
        }),
        Box::new(|document: &mut serde_json::Value| {
            document["source"]["resolved_revision"] =
                serde_json::json!("dddddddddddddddddddddddddddddddddddddddd");
        }),
        Box::new(|document: &mut serde_json::Value| {
            let first = document["files"][0].clone();
            document["files"] = serde_json::Value::Array(vec![first]);
        }),
        Box::new(|document: &mut serde_json::Value| {
            document["files"][0]["path"] = serde_json::json!("other.bin");
        }),
        Box::new(|document: &mut serde_json::Value| {
            document["files"][0]["sha256"] = serde_json::Value::String(digest(b"different"));
        }),
    ] {
        rewrite_bundle(&source, edit);
        failure(
            replica(&config, &proposal.plan_id, "backup"),
            "destination_conflict",
        );
        fs::write(&source, &original).unwrap();
    }
    assert!(
        !config.volumes[1]
            .path
            .join("preserved")
            .join("example")
            .exists()
    );

    let mut shortened = model_bytes.clone();
    shortened.pop();
    fs::write(&model, &shortened).unwrap();
    failure(
        replica(&config, &proposal.plan_id, "backup"),
        "hash_mismatch",
    );
    fs::remove_file(&model).unwrap();
    fs::create_dir(&model).unwrap();
    failure(
        replica(&config, &proposal.plan_id, "backup"),
        "hash_mismatch",
    );
    fs::remove_dir(&model).unwrap();
    #[cfg(unix)]
    {
        link(model.parent().unwrap().join("LICENSE").as_path(), &model);
        failure(
            replica(&config, &proposal.plan_id, "backup"),
            "hash_mismatch",
        );
        fs::remove_file(&model).unwrap();
    }
    fs::write(&model, &model_bytes).unwrap();

    let staging = config.volumes[1]
        .path
        .join(".modelprepper-adopt")
        .join(&proposal.plan_id);
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("LICENSE"), b"not-the-license").unwrap();
    fs::write(staging.join("config.partial"), b"stale").unwrap();
    let copied = replica(&config, &proposal.plan_id, "backup").unwrap();
    assert_eq!(fs::read(copied.path.join("LICENSE")).unwrap(), LICENSE);
    assert!(!copied.path.join("config.partial").exists());
    assert!(!copied.transfer_authorized);
    assert!(copied.workspace_bytes.is_none());
    let shown = crate::vault::status(&config).unwrap();
    let status_text = crate::present::render_status(&shown, false, 80);
    assert!(status_text.contains("replica"), "{status_text}");
    assert!(status_text.contains("local list"), "{status_text}");
    let replica_text = crate::present::render_replica(&copied, true, 40);
    assert!(replica_text.contains("replicated"), "{replica_text}");
    assert!(replica_text.contains("unlabeled"), "{replica_text}");
    assert!(replica_text.contains("still blocked"), "{replica_text}");

    let dest = copied.path.join("modelprepper-bundle.json");
    let saved = fs::read(&dest).unwrap();
    rewrite_bundle(&dest, |document| {
        document["bundle_id"] = serde_json::Value::String(digest(b"dest-bundle"));
    });
    failure(
        replica(&config, &proposal.plan_id, "backup"),
        "destination_conflict",
    );
    fs::write(&dest, &saved).unwrap();
    rewrite_bundle(&dest, |document| {
        document["files"][0]["sha256"] = serde_json::Value::String(digest(b"dest-sha"));
    });
    failure(
        replica(&config, &proposal.plan_id, "backup"),
        "destination_conflict",
    );
    fs::write(&dest, &saved).unwrap();
    assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 1);

    let rebuilt = rebuild(&config).unwrap();
    assert_eq!(rebuilt.custody_count, 1);
    assert_eq!(rebuilt.replica_count, 1);
    assert!(rebuilt.conflicts.is_empty());
    let quiet = crate::present::render_rebuild(&rebuilt, false, 80);
    assert!(quiet.contains("none"), "{quiet}");
    assert!(quiet.contains("not transferred"), "{quiet}");
    assert!(!quiet.contains("offline"), "{quiet}");

    let backup = config.volumes[1].path.clone();
    let parked = backup.parent().unwrap().join("backup-unplugged");
    fs::rename(&backup, &parked).unwrap();
    failure(
        replica(&config, &proposal.plan_id, "backup"),
        "volume_unavailable",
    );
    let offline = rebuild(&config).unwrap();
    assert_eq!(offline.offline_volumes, vec!["backup".to_string()]);
    assert_eq!(offline.replica_count, 1);
    let offline_text = crate::present::render_rebuild(&offline, true, 40);
    assert!(offline_text.contains("offline"), "{offline_text}");
    assert!(offline_text.contains("backup"), "{offline_text}");
    fs::rename(&parked, &backup).unwrap();

    exec(
        &config,
        "UPDATE replicas SET manifest = '[]' WHERE volume_label = 'backup'",
        [],
    );
    failure(
        replica(&config, &proposal.plan_id, "backup"),
        "destination_conflict",
    );
    assert_eq!(cell(&config, "SELECT manifest FROM replicas"), "[]");

    let before = cell(&config, "SELECT manifest FROM custody WHERE plan_id != ''");
    let flipped = {
        let mut bytes = model_bytes.clone();
        bytes[0] ^= 0xff;
        bytes
    };
    let flipped_hash = digest(&flipped);
    for volume in [&config.volumes[0].path, &config.volumes[1].path] {
        let folder = volume
            .join("preserved")
            .join("example")
            .join("model")
            .join(REVISION);
        fs::write(folder.join("model.safetensors"), &flipped).unwrap();
        rewrite_bundle(&folder.join("modelprepper-bundle.json"), |document| {
            let files = document["files"].as_array_mut().unwrap();
            let file = files
                .iter_mut()
                .find(|file| file["path"] == "model.safetensors")
                .unwrap();
            file["sha256"] = serde_json::Value::String(flipped_hash.clone());
        });
    }
    let conflicted = rebuild(&config).unwrap();
    assert_eq!(conflicted.conflicts, vec![proposal.plan_id.clone()]);
    assert_eq!(
        cell(&config, "SELECT manifest FROM custody WHERE plan_id != ''"),
        before
    );
    assert_eq!(
        fs::read(
            config.volumes[0]
                .path
                .join("preserved")
                .join("example")
                .join("model")
                .join(REVISION)
                .join("model.safetensors")
        )
        .unwrap(),
        flipped
    );
}

#[test]
fn import_keeps_a_baseline_only_when_the_tree_and_the_row_agree() {
    let (_dir, config, stage) = setup();
    let outside = stage.parent().unwrap().join("outside");
    fs::create_dir_all(outside.join("notes")).unwrap();
    fs::write(outside.join("weights.bin"), b"baseline-weights").unwrap();
    fs::write(outside.join("notes").join("readme.txt"), b"local note").unwrap();
    let digits = "12".repeat(20);
    let other = "ab".repeat(20);
    let different = "cd".repeat(20);
    failure(
        import_baseline(
            &config,
            Path::new("missing"),
            "local/baseline",
            "main",
            "backup",
        ),
        "invalid_revision",
    );
    let bad_hex = "g".repeat(40);
    failure(
        import_baseline(
            &config,
            Path::new("missing"),
            "local/baseline",
            &bad_hex,
            "backup",
        ),
        "invalid_revision",
    );
    failure(
        import_baseline(&config, &outside, "local/baseline", &digits, "other"),
        "unknown_volume",
    );

    let imported = import_baseline(&config, &outside, "local/baseline", &digits, "backup").unwrap();
    assert!(imported.consistent_with.is_none());
    assert_eq!(imported.provenance, PROVENANCE_LOCAL_BASELINE);
    let again = import_baseline(&config, &outside, "local/baseline", &digits, "backup").unwrap();
    assert_eq!(again.plan_id, imported.plan_id);
    assert_eq!(again.path, imported.path);
    let none_text = crate::present::render_import(&imported, false, 80);
    assert!(none_text.contains("imported"), "{none_text}");
    assert!(!none_text.contains("bytes match"), "{none_text}");
    assert!(none_text.contains("still blocked"), "{none_text}");

    let matched = import_baseline(&config, &outside, "local/baseline", &other, "primary").unwrap();
    assert_eq!(
        matched.consistent_with.as_deref(),
        Some(imported.plan_id.as_str())
    );
    let match_text = crate::present::render_import(&matched, true, 40);
    assert!(match_text.contains("bytes match"), "{match_text}");
    let elsewhere = stage.parent().unwrap().join("different");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("weights.bin"), b"other-bytes").unwrap();
    let unmatched =
        import_baseline(&config, &elsewhere, "local/baseline", &different, "backup").unwrap();
    assert!(unmatched.consistent_with.is_none());

    let bundle = imported.path.join("modelprepper-bundle.json");
    let saved = fs::read(&bundle).unwrap();
    rewrite_bundle(&bundle, |document| {
        document["provenance"] = serde_json::json!("upstream-verified (ranged https)");
    });
    failure(
        import_baseline(&config, &outside, "local/baseline", &digits, "backup"),
        "destination_conflict",
    );
    fs::write(&bundle, &saved).unwrap();
    rewrite_bundle(&bundle, |document| {
        document["bundle_id"] = serde_json::Value::String(digest(b"not-this-baseline"));
    });
    failure(
        import_baseline(&config, &outside, "local/baseline", &digits, "backup"),
        "destination_conflict",
    );
    fs::write(&bundle, &saved).unwrap();
    rewrite_bundle(&bundle, |document| {
        document["files"][0]["sha256"] = serde_json::Value::String(digest(b"edited"));
    });
    failure(
        import_baseline(&config, &outside, "local/baseline", &digits, "backup"),
        "destination_conflict",
    );
    fs::write(&bundle, &saved).unwrap();
    assert_eq!(
        cell(
            &config,
            "SELECT provenance FROM custody WHERE provenance = 'local-baseline' LIMIT 1",
        ),
        PROVENANCE_LOCAL_BASELINE
    );

    exec(
        &config,
        "UPDATE custody SET provenance = 'rewritten' WHERE plan_id = ?1",
        [imported.plan_id.as_str()],
    );
    failure(
        import_baseline(&config, &outside, "local/baseline", &digits, "backup"),
        "destination_conflict",
    );
    assert_eq!(
        cell(
            &config,
            &format!(
                "SELECT provenance FROM custody WHERE plan_id = '{}'",
                imported.plan_id
            ),
        ),
        "rewritten"
    );
    exec(
        &config,
        "UPDATE custody SET provenance = 'local-baseline', manifest = '[]' WHERE plan_id = ?1",
        [imported.plan_id.as_str()],
    );
    failure(
        import_baseline(&config, &outside, "local/baseline", &digits, "backup"),
        "destination_conflict",
    );
    assert_eq!(
        cell(
            &config,
            &format!(
                "SELECT manifest FROM custody WHERE plan_id = '{}'",
                imported.plan_id
            ),
        ),
        "[]"
    );

    let crowded = stage.parent().unwrap().join("crowded");
    fs::create_dir(&crowded).unwrap();
    for index in 0..1025 {
        fs::write(crowded.join(format!("f{index}.txt")), b"x").unwrap();
    }
    failure(
        import_baseline(&config, &crowded, "local/baseline", &digits, "backup"),
        "too_many_files",
    );

    let blocked = stage.parent().unwrap().join("blocked");
    fs::create_dir(&blocked).unwrap();
    fs::write(blocked.join("weights.bin"), b"x").unwrap();
    let file_dest = config.volumes[1]
        .path
        .join("preserved")
        .join("local")
        .join("baseline")
        .join("ef".repeat(20));
    fs::create_dir_all(file_dest.parent().unwrap()).unwrap();
    fs::write(&file_dest, b"not-a-directory").unwrap();
    failure(
        import_baseline(
            &config,
            &blocked,
            "local/baseline",
            &"ef".repeat(20),
            "backup",
        ),
        "unsafe_path",
    );

    #[cfg(unix)]
    {
        let linked = stage.parent().unwrap().join("linked");
        fs::create_dir(&linked).unwrap();
        fs::write(linked.join("weights.bin"), b"x").unwrap();
        link(&outside, &linked.join("alias"));
        failure(
            import_baseline(&config, &linked, "local/baseline", &digits, "backup"),
            "unsafe_path",
        );
        let odd = stage.parent().unwrap().join("odd");
        fs::create_dir(&odd).unwrap();
        fs::write(odd.join("a\\b"), b"x").unwrap();
        failure(
            import_baseline(&config, &odd, "local/baseline", &digits, "backup"),
            "unsafe_path",
        );
        // APFS rejects a non-UTF-8 name. Linux accepts the byte and import refuses it.
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::ffi::OsStringExt;
            let raw = stage.parent().unwrap().join("raw");
            fs::create_dir(&raw).unwrap();
            let name = std::ffi::OsString::from_vec(vec![0xff]);
            fs::write(raw.join(name), b"x").unwrap();
            failure(
                import_baseline(&config, &raw, "local/baseline", &digits, "backup"),
                "unsafe_path",
            );
        }
        let piped = stage.parent().unwrap().join("piped");
        fs::create_dir(&piped).unwrap();
        let fifo = piped.join("pipe");
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        failure(
            import_baseline(&config, &piped, "local/baseline", &digits, "backup"),
            "unsafe_path",
        );
        let leaf_revision = "98".repeat(20);
        let leaf = config.volumes[0]
            .path
            .join("preserved")
            .join("local")
            .join("baseline")
            .join(&leaf_revision);
        fs::create_dir_all(leaf.parent().unwrap()).unwrap();
        link(&outside, &leaf);
        failure(
            import_baseline(
                &config,
                &blocked,
                "local/baseline",
                &leaf_revision,
                "primary",
            ),
            "unsafe_vault_path",
        );
    }
}

#[test]
fn rebuild_names_damage_offline_disks_and_marker_loss_without_inventing_rows() {
    let (_dir, config, _stage) = setup();
    let plan = digest(b"placed");
    let revision = "ab".repeat(20);
    plant(
        &config.volumes[0].path,
        &revision,
        &plan,
        b"placed-bytes",
        Some(PROVENANCE_RANGED_HTTPS),
    );
    let preserved = config.volumes[0].path.join("preserved");
    let notes = preserved.join("notes");
    fs::create_dir_all(&notes).unwrap();
    fs::copy(
        preserved
            .join("example")
            .join("model")
            .join(&revision)
            .join("modelprepper-bundle.json"),
        notes.join("modelprepper-bundle.json"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        link(
            &preserved
                .join("example")
                .join("model")
                .join(&revision)
                .join("weights.bin"),
            &preserved.join("alias"),
        );
        assert!(
            std::process::Command::new("mkfifo")
                .arg(preserved.join("pipe"))
                .status()
                .unwrap()
                .success()
        );
        link(&preserved, &config.volumes[1].path.join("preserved"));
    }
    let rebuilt = rebuild(&config).unwrap();
    assert_eq!(rebuilt.custody_count, 1);
    assert_eq!(rebuilt.replica_count, 0);
    assert!(rebuilt.conflicts.is_empty());
    assert!(rebuilt.damaged.iter().any(|item| item.contains("notes")));
    assert_eq!(
        cell(&config, "SELECT provenance FROM custody"),
        PROVENANCE_RANGED_HTTPS
    );
    #[cfg(unix)]
    {
        assert!(rebuilt.damaged.iter().any(|item| item.contains("alias")));
        assert!(
            rebuilt
                .damaged
                .iter()
                .any(|item| item.starts_with("backup:") && item.ends_with("preserved"))
        );
        assert_eq!(count(&config, "SELECT COUNT(*) FROM replicas"), 0);
    }
    let rich = Rebuild {
        schema_version: 1,
        status: "rebuilt",
        vault_id: rebuilt.vault_id.clone(),
        custody_count: 1,
        replica_count: 0,
        damaged: rebuilt.damaged.clone(),
        conflicts: vec![plan.clone()],
        offline_volumes: vec!["backup".into()],
        approvals_transferred: false,
        transfer_authorized: false,
        workspace_bytes: None,
    };
    let rich_text = crate::present::render_rebuild(&rich, false, 80);
    assert!(rich_text.contains("notes"), "{rich_text}");
    assert!(rich_text.contains(&plan[..12]), "{rich_text}");
    assert!(rich_text.contains("offline"), "{rich_text}");
    let quiet = Rebuild {
        damaged: Vec::new(),
        conflicts: Vec::new(),
        offline_volumes: Vec::new(),
        ..rich
    };
    let quiet_text = crate::present::render_rebuild(&quiet, true, 40);
    assert!(quiet_text.contains("none"), "{quiet_text}");
    assert!(!quiet_text.contains("offline"), "{quiet_text}");

    let (_dir, config, _stage) = setup();
    let plan = digest(b"backup-only");
    let revision = "cd".repeat(20);
    plant(
        &config.volumes[1].path,
        &revision,
        &plan,
        b"backup-bytes",
        Some(PROVENANCE_UNRECORDED),
    );
    delete_catalog(&config);
    let rebuilt = rebuild(&config).unwrap();
    assert_eq!(rebuilt.custody_count, 1);
    assert_eq!(rebuilt.replica_count, 0);
    assert_eq!(cell(&config, "SELECT volume_label FROM custody"), "backup");
    assert_eq!(
        cell(&config, "SELECT provenance FROM custody"),
        PROVENANCE_UNRECORDED
    );
    assert_eq!(count(&config, "SELECT COUNT(*) FROM grants"), 0);

    let (_dir, config, _stage) = setup();
    delete_catalog(&config);
    fs::remove_file(config.volumes[1].path.join(".modelprepper-volume.json")).unwrap();
    failure(rebuild(&config), "volume_identity_missing");

    let (_dir, config, _stage) = setup();
    let marker = config.volumes[1].path.join(".modelprepper-volume.json");
    let mut identity: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    identity["label"] = serde_json::json!("other");
    fs::write(&marker, serde_json::to_vec_pretty(&identity).unwrap()).unwrap();
    delete_catalog(&config);
    failure(rebuild(&config), "volume_identity_mismatch");

    let (_dir, config, _stage) = setup();
    let marker = config.volumes[0].path.join(".modelprepper-volume.json");
    let mut identity: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    identity["vault_id"] = serde_json::json!("00000000-0000-4000-8000-000000000001");
    fs::write(&marker, serde_json::to_vec_pretty(&identity).unwrap()).unwrap();
    delete_catalog(&config);
    failure(rebuild(&config), "volume_identity_mismatch");

    let (_dir, config, _stage) = setup();
    fs::write(config.volumes[0].path.join("notes.txt"), b"keep").unwrap();
    delete_catalog(&config);
    failure(rebuild(&config), "directory_not_empty");
    assert_eq!(
        fs::read(config.volumes[0].path.join("notes.txt")).unwrap(),
        b"keep"
    );
}

#[test]
fn one_sealed_plan_is_hashed_and_custody_is_not_rewritten() {
    let (_dir, config, _stage) = setup();
    let plan = "ab".repeat(32);
    let other = "cd".repeat(32);
    let bytes = b"sealed weights";
    let hashed = crate::integrity::hash_exact(&bytes[..], bytes.len() as u64, |_| {}).unwrap();
    let revision = REVISION;
    let directory = config.volumes[0]
        .path
        .join("preserved")
        .join("example")
        .join("model")
        .join(revision);
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("weights.bin"), bytes).unwrap();
    let manifest = format!(
        r#"[{{"path":"weights.bin","sha256":"{}","size_bytes":{}}}]"#,
        hashed.sha256,
        bytes.len()
    );
    let connection =
        rusqlite::Connection::open(config.catalog_path.join("catalog.sqlite3")).unwrap();
    connection
        .execute(
            "INSERT INTO custody (plan_id, repo_id, resolved_revision, provenance, volume_label, relative_dir, manifest) VALUES (?1, 'example/model', ?2, 'upstream-verified (ranged https)', 'primary', ?3, ?4)",
            rusqlite::params![plan, revision, format!("preserved/example/model/{revision}"), manifest],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO custody (plan_id, repo_id, resolved_revision, provenance, volume_label, relative_dir, manifest) VALUES (?1, 'example/other', ?2, 'upstream-verified (ranged https)', 'primary', 'preserved/example/other/rev', '[{\"path\":\"other.bin\",\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"size_bytes\":1}]')",
            rusqlite::params![other, revision],
        )
        .unwrap();
    drop(connection);
    let before = count(&config, "SELECT COUNT(*) FROM custody");
    let report = verify_sealed(&config, &plan).unwrap();
    assert_eq!(report.action, "verified");
    assert!(report.matched);
    assert!(!report.vault_mutated);
    assert!(!report.network_contact);
    assert!(report.workspace_bytes.is_none());
    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].status, "match");
    assert_eq!(count(&config, "SELECT COUNT(*) FROM custody"), before);
    fs::write(directory.join("weights.bin"), b"changed weights").unwrap();
    let changed = verify_sealed(&config, &plan).unwrap();
    assert!(!changed.matched);
    assert_eq!(changed.files[0].status, "mismatch");
    fs::remove_file(directory.join("weights.bin")).unwrap();
    let missing = verify_sealed(&config, &plan).unwrap();
    assert_eq!(missing.files[0].status, "missing");
    assert_eq!(
        verify_sealed(&config, &other).unwrap().files[0].status,
        "missing"
    );
    assert_eq!(
        verify_sealed(&config, "nope").unwrap_err().code,
        "invalid_plan"
    );
    assert_eq!(
        verify_sealed(&config, &"ef".repeat(32)).unwrap_err().code,
        "plan_not_stored"
    );
    let text = crate::present::render_sealed(&report, false, 80);
    assert!(text.contains("matched"), "{text}");
    assert!(text.contains("not a collection custody rehash"), "{text}");
    assert!(
        text.lines().all(|line| line.chars().count() <= 80),
        "{text}"
    );
}
