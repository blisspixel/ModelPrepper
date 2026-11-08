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
