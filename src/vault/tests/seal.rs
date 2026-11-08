use super::*;
use crate::{
    fixture::{Server, response},
    hub::{CapturedEvidence, Resolution},
    inventory::{
        EvidenceStatus, HashAlgorithm, Inventory, LicenseEvidence, Role, Source, SourceFile,
        UpstreamHash,
    },
    plan::{Blocker, digest},
    transfer::KernelSocketBuffers,
    vault::{Decision, initialize},
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
    let config = crate::Config::default_for(root.join("catalog"), root.join("models"));
    initialize(&config).unwrap();
    let stage = root.join("stage");
    (dir, config, stage)
}

fn kernel() -> KernelSocketBuffers {
    KernelSocketBuffers {
        receive_bytes: 16_384,
        send_bytes: 32_768,
    }
}

fn source() -> Source {
    Source {
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

fn save(config: &crate::Config) -> crate::vault::Proposal {
    let catalog = super::super::open_catalog(config, false).unwrap();
    let status = super::super::report(&catalog, config).unwrap();
    let volume = status
        .volumes
        .iter()
        .find(|volume| volume.label == config.storage.default_volume)
        .unwrap();
    let text = String::from_utf8(LICENSE.to_vec()).unwrap();
    super::super::review::save(
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
                text,
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

fn granted(config: &crate::Config) -> crate::vault::Proposal {
    let proposal = save(config);
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

fn stage_all(path: &Path, files: &[SourceFile]) -> RevisionStage {
    let mut session = RevisionStage::create(path, &source(), files).unwrap();
    for (_, body, _) in bodies() {
        let server = Server::new(vec![ranged(body)]);
        let segment = session
            .stage_next_with(
                &crate::http::Http::fixture(&server.origin),
                &server.origin,
                body.len() as u64,
            )
            .unwrap();
        assert!(
            server
                .finish()
                .remove(0)
                .to_ascii_lowercase()
                .contains("bytes=")
        );
        assert!(!segment.path.is_empty());
    }
    assert!(session.is_staged().unwrap());
    session
}

fn custody_count(config: &crate::Config) -> i64 {
    let catalog = super::super::open_catalog(config, false).unwrap();
    catalog
        .db
        .query_row("SELECT COUNT(*) FROM custody", [], |row| row.get(0))
        .unwrap()
}

fn published(config: &crate::Config) -> PathBuf {
    let catalog = super::super::open_catalog(config, false).unwrap();
    let status = super::super::report(&catalog, config).unwrap();
    status.volumes[0]
        .path
        .join("preserved")
        .join("example")
        .join("model")
        .join(REVISION)
}

#[test]
fn reservation_space_rejects_a_reserve_that_does_not_fit() {
    assert!(reservation_fits(11, 6, 5).is_ok());
    assert_eq!(
        reservation_fits(10, 6, 5).err().unwrap().code,
        "insufficient_space"
    );
    assert_eq!(
        reservation_fits(1, u64::MAX, 1).err().unwrap().code,
        "size_overflow"
    );
}

#[test]
fn fetch_seals_one_granted_revision_and_does_not_claim_a_load() {
    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    assert!(proposal.review.workspace_bytes.is_none());
    assert!(!proposal.transfer_authorized);
    let order: Vec<&[u8]> = bodies().into_iter().map(|(_, body, _)| body).collect();
    assert_eq!(
        digest(include_bytes!("../../../examples/readiness/config.json")),
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
    assert_eq!(
        digest(include_bytes!("../../../examples/readiness/tokenizer.json")),
        "afbf9d0f3560b0fd7795e81c42a0a79ee6b6fc67e064f77826aee642cad28d91"
    );
    assert_eq!(
        digest(include_bytes!(
            "../../../examples/readiness/model.safetensors"
        )),
        "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c"
    );
    assert_eq!(
        digest(include_bytes!(
            "../../../examples/readiness/chat_template.jinja"
        )),
        "52ed09e8448c395a40c52b4d91dc1818070d481a6afd2354b2653956dbf2dfc5"
    );
    let server = Server::new(order.iter().copied().map(ranged).collect());
    let seal = fetch_with(
        &config,
        &proposal.plan_id,
        &stage,
        &crate::http::Http::fixture(&server.origin),
        &server.origin,
        kernel(),
        None,
    )
    .unwrap();
    let requests = server.finish();
    assert_eq!(requests.len(), order.len());
    for request in &requests {
        let text = request.to_ascii_lowercase();
        assert!(text.contains("bytes="), "{text}");
        assert!(!text.contains("accept-encoding"), "{text}");
    }
    assert!(!seal.transfer_authorized);
    assert!(seal.workspace_bytes.is_none());
    assert_eq!(seal.provenance, PROVENANCE_RANGED_HTTPS);
    assert_eq!(seal.file_count, 5);
    assert!(seal.path.join("modelprepper-bundle.json").is_file());
    let bundle: serde_json::Value =
        serde_json::from_slice(&fs::read(seal.path.join("modelprepper-bundle.json")).unwrap())
            .unwrap();
    assert_eq!(bundle["schema_version"], 1);
    assert_eq!(bundle["bundle_id"], proposal.plan_id);
    assert!(bundle.get("sealed_at").is_none());
    assert_eq!(bundle["files"].as_array().unwrap().len(), 5);
    assert!(stage.join("members/0003/payload.part").is_file());
    assert_eq!(
        fs::read(stage.join("members/0003/payload.part")).unwrap(),
        include_bytes!("../../../examples/readiness/model.safetensors")
    );
    assert_eq!(custody_count(&config), 1);
    let catalog = super::super::open_catalog(&config, false).unwrap();
    let provenance: String = catalog
        .db
        .query_row("SELECT provenance FROM custody", [], |row| row.get(0))
        .unwrap();
    drop(catalog);
    assert_eq!(provenance, PROVENANCE_RANGED_HTTPS);
    let again = fetch_with(
        &config,
        &proposal.plan_id,
        &stage,
        &crate::http::Http::fixture("http://127.0.0.1:9"),
        "http://127.0.0.1:9",
        kernel(),
        None,
    )
    .unwrap();
    assert_eq!(again.path, seal.path);
    assert_eq!(custody_count(&config), 1);
    let shown = crate::vault::show_proposal(&config, &proposal.plan_id).unwrap();
    assert_eq!(shown.decision, Decision::Pending);
    assert!(!shown.transfer_authorized);
    assert!(shown.review.workspace_bytes.is_none());
    assert_eq!(
        crate::vault::decide(&config, &proposal.plan_id, Decision::Approved)
            .err()
            .unwrap()
            .code,
        "transfer_backend_unqualified"
    );
    let recipe =
        crate::readiness::Recipe::parse(include_str!("../../../examples/recipe.json")).unwrap();
    let report = crate::readiness::preflight(&recipe, &seal.path).unwrap();
    assert!(!report.network_contact);
    assert!(!report.executed);
    assert!(!report.vault_mutated);
    assert!(!report.transfer_authorized);
    assert_eq!(report.integrity, "matched");
    assert_eq!(report.runtime_retention, "named");
    assert_eq!(report.offline_readiness, "untested");
    assert_ne!(report.offline_readiness, "load_passed");
    assert!(report.next.contains("named or remote"), "{}", report.next);
}

#[test]
fn fetch_without_a_grant_does_not_open_a_stage() {
    let (_dir, config, stage) = setup();
    let proposal = save(&config);
    let error = fetch_with(
        &config,
        &proposal.plan_id,
        &stage,
        &crate::http::Http::default(),
        "https://huggingface.co",
        kernel(),
        None,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "grant_not_recorded");
    assert!(!stage.exists());
    assert_eq!(custody_count(&config), 0);
}

#[test]
fn fetch_refuses_a_stage_inside_the_volume_and_an_ignored_range() {
    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    let inside = config.volumes[0].path.join("inside");
    let error = fetch_with(
        &config,
        &proposal.plan_id,
        &inside,
        &crate::http::Http::default(),
        "https://huggingface.co",
        kernel(),
        None,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "overlapping_vault_paths");
    assert!(!inside.exists());
    let mut session = RevisionStage::create(&stage, &source(), &files()).unwrap();
    let server = Server::new(vec![response(
        206,
        &format!("Content-Range: bytes 0-0/{}\r\n", LICENSE.len()),
        &LICENSE[..1],
    )]);
    session
        .stage_next_with(
            &crate::http::Http::fixture(&server.origin),
            &server.origin,
            1,
        )
        .unwrap();
    server.finish();
    drop(session);
    let server = Server::new(vec![response(200, "", &LICENSE[1..])]);
    let error = fetch_with(
        &config,
        &proposal.plan_id,
        &stage,
        &crate::http::Http::fixture(&server.origin),
        &server.origin,
        kernel(),
        None,
    )
    .err()
    .unwrap();
    server.finish();
    assert_eq!(error.code, "range_not_honored");
    assert!(!published(&config).exists());
    assert_eq!(custody_count(&config), 0);
}

#[test]
fn a_renamed_directory_without_a_bundle_is_not_a_seal() {
    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    let session = stage_all(&stage, &files());
    let published = published(&config);
    fs::create_dir_all(&published).unwrap();
    for (index, (name, bytes, _)) in bodies().iter().enumerate() {
        fs::write(published.join(name), bytes).unwrap();
        assert_eq!(fs::read(session.payload_path(index)).unwrap(), *bytes);
    }
    drop(session);
    let error = fetch_with(
        &config,
        &proposal.plan_id,
        &stage,
        &crate::http::Http::default(),
        "https://huggingface.co",
        kernel(),
        None,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "not_sealed");
    assert!(published.join("model.safetensors").is_file());
    assert!(!published.join("modelprepper-bundle.json").exists());
    assert_eq!(custody_count(&config), 0);
}

#[test]
fn a_corrupt_part_and_an_incomplete_stage_do_not_seal() {
    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    let session = stage_all(&stage, &files());
    let mut tampered = fs::read(session.payload_path(3)).unwrap();
    tampered[0] ^= 0xff;
    fs::write(session.payload_path(3), &tampered).unwrap();
    let error = seal_staged_with(&config, &proposal.plan_id, &session, kernel(), None)
        .err()
        .unwrap();
    assert_eq!(error.code, "hash_mismatch");
    assert!(!published(&config).exists());
    assert_eq!(custody_count(&config), 0);
    drop(session);
    let error = RevisionStage::open(&stage, &source(), &files())
        .err()
        .unwrap();
    assert_eq!(error.code, "invalid_checkpoint");

    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    let session = RevisionStage::create(&stage, &source(), &files()).unwrap();
    let error = seal_staged_with(&config, &proposal.plan_id, &session, kernel(), None)
        .err()
        .unwrap();
    assert_eq!(error.code, "revision_not_staged");
    assert_eq!(custody_count(&config), 0);
    assert!(!published(&config).exists());
    assert!(
        !config.volumes[0]
            .path
            .join(".modelprepper-adopt")
            .join(format!("{}.reservation.json", proposal.plan_id))
            .exists()
    );
}

#[test]
fn a_changed_policy_drops_the_grant_before_custody() {
    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    let session = stage_all(&stage, &files());
    let mut changed = config.clone();
    changed.transfer.max_transfer_bytes_per_period += 1;
    let error = seal_staged_with(&changed, &proposal.plan_id, &session, kernel(), None)
        .err()
        .unwrap();
    assert_eq!(error.code, "grant_dropped");
    assert!(!published(&config).exists());
    assert_eq!(custody_count(&config), 0);
    assert_eq!(
        seal_staged_with(&config, &proposal.plan_id, &session, kernel(), None)
            .err()
            .unwrap()
            .code,
        "grant_not_recorded"
    );
}

#[test]
fn interrupted_rename_and_catalog_retries_leave_one_custody_row() {
    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    let session = stage_all(&stage, &files());
    let error = seal_staged_with(
        &config,
        &proposal.plan_id,
        &session,
        kernel(),
        Some(SealStop::BeforeRename),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "seal_interrupted");
    assert!(!published(&config).exists());
    let reservation = config.volumes[0]
        .path
        .join(".modelprepper-adopt")
        .join(format!("{}.reservation.json", proposal.plan_id));
    assert!(reservation.is_file());
    let sealed = seal_staged_with(&config, &proposal.plan_id, &session, kernel(), None).unwrap();
    assert!(sealed.path.join("modelprepper-bundle.json").is_file());
    assert_eq!(custody_count(&config), 1);
    assert!(!reservation.exists());

    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    let session = stage_all(&stage, &files());
    let error = seal_staged_with(
        &config,
        &proposal.plan_id,
        &session,
        kernel(),
        Some(SealStop::BeforeCatalog),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "seal_interrupted");
    let folder = published(&config);
    assert!(folder.join("modelprepper-bundle.json").is_file());
    assert_eq!(custody_count(&config), 0);
    seal_staged_with(&config, &proposal.plan_id, &session, kernel(), None).unwrap();
    seal_staged_with(&config, &proposal.plan_id, &session, kernel(), None).unwrap();
    assert_eq!(custody_count(&config), 1);
    let mut bad = fs::read(folder.join("model.safetensors")).unwrap();
    bad[0] ^= 0xff;
    fs::write(folder.join("model.safetensors"), &bad).unwrap();
    let error = seal_staged_with(&config, &proposal.plan_id, &session, kernel(), None)
        .err()
        .unwrap();
    assert_eq!(error.code, "destination_conflict");
    assert!(folder.join("model.safetensors").is_file());
    assert_eq!(custody_count(&config), 1);

    let (_dir, config, stage) = setup();
    let proposal = granted(&config);
    let session = stage_all(&stage, &files());
    let error = seal_staged_with(
        &config,
        &proposal.plan_id,
        &session,
        kernel(),
        Some(SealStop::BeforeCatalog),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "seal_interrupted");
    let mut changed = config.clone();
    changed.transfer.max_transfer_bytes_per_period += 1;
    let error = seal_staged_with(&changed, &proposal.plan_id, &session, kernel(), None)
        .err()
        .unwrap();
    assert_eq!(error.code, "grant_dropped");
    assert!(
        published(&config)
            .join("modelprepper-bundle.json")
            .is_file()
    );
    assert_eq!(custody_count(&config), 0);
}
