use super::super::review::save;
use super::{Outcome, PROVENANCE_LOCAL_UPSTREAM, adopt, adopt_stopping, publish_dir};
use crate::{
    Config, Inventory,
    hub::{CapturedEvidence, Resolution},
    integrity::hash_exact,
    inventory::{
        EvidenceStatus, HashAlgorithm, LicenseEvidence, Role, Source, SourceFile, UpstreamHash,
    },
    plan::digest,
    vault::{initialize, preserved_digests, status},
};
use std::fs;
use tempfile::TempDir;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn setup() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let config = Config::default_for(root.join("catalog"), root.join("models"));
    initialize(&config).unwrap();
    (dir, config)
}

fn identity(bytes: &[u8], algorithm: HashAlgorithm) -> UpstreamHash {
    let hashed = hash_exact(bytes, bytes.len() as u64, |_| {}).unwrap();
    UpstreamHash {
        algorithm,
        value: match algorithm {
            HashAlgorithm::Sha256 => hashed.sha256,
            HashAlgorithm::GitSha1 => hashed.git_sha1,
        },
    }
}

fn source_file(path: &str, bytes: &[u8], role: Role, algorithm: HashAlgorithm) -> SourceFile {
    SourceFile {
        path: path.into(),
        size_bytes: bytes.len() as u64,
        upstream_hash: Some(identity(bytes, algorithm)),
        role,
        required: true,
    }
}

struct Sample {
    _dir: TempDir,
    download: std::path::PathBuf,
    inventory: Inventory,
    evidence: Vec<CapturedEvidence>,
}

fn sample(matched: bool) -> Sample {
    let dir = TempDir::new().unwrap();
    let download = dir.path().join("download");
    fs::create_dir(&download).unwrap();
    let license = b"upstream license text";
    let config = br#"{"model_type":"fixture"}"#;
    let weights = b"weights";
    fs::write(download.join("LICENSE"), license).unwrap();
    fs::write(download.join("config.json"), config).unwrap();
    fs::write(download.join("model.safetensors"), weights).unwrap();
    let inventory = Inventory {
        schema_version: 1,
        source: Source {
            endpoint: "https://huggingface.co".into(),
            repo_type: "model".into(),
            repo_id: "example/model".into(),
            requested_revision: "main".into(),
            resolved_revision: COMMIT.into(),
            gated: false,
            private: false,
        },
        complete: true,
        layout: "transformers-safetensors-v1".into(),
        license: LicenseEvidence {
            license_id: "mit".into(),
            status: if matched {
                EvidenceStatus::Matched
            } else {
                EvidenceStatus::Unreviewed
            },
            paths: vec!["LICENSE".into()],
        },
        files: vec![
            source_file("LICENSE", license, Role::License, HashAlgorithm::GitSha1),
            source_file("config.json", config, Role::Config, HashAlgorithm::GitSha1),
            source_file(
                "model.safetensors",
                weights,
                Role::Weight,
                HashAlgorithm::Sha256,
            ),
        ],
    };
    let evidence = vec![CapturedEvidence {
        path: "LICENSE".into(),
        sha256: digest(license),
        text: String::from_utf8(license.to_vec()).unwrap(),
    }];
    Sample {
        _dir: dir,
        download,
        inventory,
        evidence,
    }
}

fn stored(config: &Config, sample: &Sample) -> String {
    let catalog = super::super::open_catalog(config, false).unwrap();
    let current = super::super::report(&catalog, config).unwrap();
    let proposal = save(
        &catalog,
        config,
        Resolution {
            schema_version: 1,
            mode: "resolved_inventory",
            transfer_authorized: false,
            inventory: sample.inventory.clone(),
            evidence: sample.evidence.clone(),
        },
        (
            current.vault_id,
            current.volumes[0].volume_id.clone(),
            current.volumes[0].available_bytes.unwrap(),
        ),
    )
    .unwrap();
    drop(catalog);
    proposal.plan_id
}

#[test]
fn copy_and_move_record_upstream_verified_local_provenance() {
    let (_dir, config) = setup();
    let sample = sample(true);
    let download = sample.download.clone();
    let plan = stored(&config, &sample);
    let mut progress = 0_u32;
    let Outcome::Recorded(adopted) = adopt(&config, &plan, &download, false, &mut |_, _, _| {
        progress += 1
    })
    .unwrap() else {
        panic!("expected adoption");
    };
    assert!(progress > 0);
    assert_eq!(adopted.provenance, PROVENANCE_LOCAL_UPSTREAM);
    assert_eq!(adopted.resolved_revision, COMMIT);
    assert!(!adopted.moved);
    assert!(download.join("model.safetensors").exists());
    assert_eq!(
        fs::read(adopted.path.join("model.safetensors")).unwrap(),
        b"weights"
    );
    let reported = status(&config).unwrap();
    assert_eq!(reported.preserved.len(), 1);
    assert_eq!(reported.preserved[0].repo_id, "example/model");
    assert_eq!(reported.preserved[0].resolved_revision, COMMIT);
    assert_eq!(reported.preserved[0].provenance, PROVENANCE_LOCAL_UPSTREAM);
    assert!(
        preserved_digests(&config)
            .unwrap()
            .contains(&digest(b"weights"))
    );
    let again = adopt(&config, &plan, &download, true, &mut |_, _, _| {}).unwrap();
    let Outcome::Recorded(moved) = again else {
        panic!("expected idempotent adoption");
    };
    assert!(moved.moved);
    assert!(!download.join("LICENSE").exists());
    assert!(adopted.path.join("LICENSE").exists());
}

#[test]
fn mismatch_names_the_file_and_interruption_rolls_forward() {
    let (dir, config) = setup();
    let sample = sample(true);
    let download = sample.download.clone();
    let plan = stored(&config, &sample);
    let mut flipped = fs::read(download.join("model.safetensors")).unwrap();
    flipped[0] = b'W';
    fs::write(download.join("model.safetensors"), &flipped).unwrap();
    let mismatch = adopt(&config, &plan, &download, false, &mut |_, _, _| {}).unwrap();
    let Outcome::Mismatch(report) = mismatch else {
        panic!("expected a verification failure");
    };
    assert!(!report.ok);
    assert!(report.message.contains("model.safetensors"));
    assert!(status(&config).unwrap().preserved.is_empty());
    let volume = config.volumes[0].path.clone();
    assert!(!volume.join(".modelprepper-adopt").exists());
    assert!(!volume.join("preserved").exists());

    fs::write(download.join("model.safetensors"), b"weights").unwrap();
    let error =
        adopt_stopping(&config, &plan, &download, false, &mut |_, _, _| {}, Some(1)).unwrap_err();
    assert_eq!(error.code, "adopt_interrupted");
    assert!(volume.join(".modelprepper-adopt").join(&plan).exists());
    assert!(!publish_dir(&volume, "example/model", COMMIT).exists());
    let Outcome::Recorded(_) = adopt(&config, &plan, &download, false, &mut |_, _, _| {}).unwrap()
    else {
        panic!("resume should publish");
    };
    assert!(!volume.join(".modelprepper-adopt").join(&plan).exists());
    assert_eq!(
        fs::read(publish_dir(&volume, "example/model", COMMIT).join("config.json")).unwrap(),
        br#"{"model_type":"fixture"}"#
    );
    let _ = dir;
}

#[test]
fn missing_license_evidence_is_not_adopted() {
    let (_dir, config) = setup();
    let unreviewed = sample(false);
    let download = unreviewed.download.clone();
    let plan = stored(&config, &unreviewed);
    assert_eq!(
        adopt(&config, &plan, &download, false, &mut |_, _, _| {})
            .unwrap_err()
            .code,
        "license_needs_review"
    );
    assert!(status(&config).unwrap().preserved.is_empty());
    fs::remove_file(download.join("config.json")).unwrap();
    let matched = sample(true);
    let download = matched.download.clone();
    let plan = stored(&config, &matched);
    fs::remove_file(download.join("config.json")).unwrap();
    let Outcome::Mismatch(report) =
        adopt(&config, &plan, &download, false, &mut |_, _, _| {}).unwrap()
    else {
        panic!("missing shard should be reported");
    };
    assert!(report.message.contains("config.json missing"));
}

#[test]
fn schema_upgrade_keeps_custody_available() {
    let (_dir, config) = setup();
    let catalog = super::super::open_catalog(&config, false).unwrap();
    catalog
        .db
        .execute_batch("DROP TABLE custody; PRAGMA user_version=2;")
        .unwrap();
    drop(catalog);
    assert!(status(&config).unwrap().preserved.is_empty());
}
