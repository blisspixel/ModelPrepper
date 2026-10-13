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
    let download = fs::canonicalize(dir.path()).unwrap().join("download");
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

#[test]
fn move_retry_preserves_changed_source_and_all_other_files() {
    let (_dir, config) = setup();
    let sample = sample(true);
    let plan = stored(&config, &sample);
    adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {}).unwrap();
    fs::write(sample.download.join("model.safetensors"), b"CHANGED").unwrap();
    assert!(adopt(&config, &plan, &sample.download, true, &mut |_, _, _| {}).is_err());
    assert_eq!(
        fs::read(sample.download.join("model.safetensors")).unwrap(),
        b"CHANGED"
    );
    assert!(sample.download.join("LICENSE").exists());
}

#[test]
fn move_retry_rejects_the_published_tree_as_source() {
    let (_dir, config) = setup();
    let sample = sample(true);
    let plan = stored(&config, &sample);
    let Outcome::Recorded(copy) =
        adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {}).unwrap()
    else {
        panic!("copy");
    };
    assert!(adopt(&config, &plan, &copy.path, true, &mut |_, _, _| {}).is_err());
    assert_eq!(
        fs::read(copy.path.join("model.safetensors")).unwrap(),
        b"weights"
    );
}

#[cfg(unix)]
#[test]
fn license_fifo_is_rejected_before_opening() {
    let (_dir, config) = setup();
    let sample = sample(true);
    let plan = stored(&config, &sample);
    let license = sample.download.join("LICENSE");
    fs::remove_file(&license).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&license)
            .status()
            .unwrap()
            .success()
    );
    assert!(adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {}).is_err());
    assert!(status(&config).unwrap().preserved.is_empty());
}

#[test]
fn move_retry_resumes_partial_cleanup_without_touching_extra_files() {
    let (_dir, config) = setup();
    let sample = sample(true);
    let plan = stored(&config, &sample);
    adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {}).unwrap();
    fs::remove_file(sample.download.join("LICENSE")).unwrap();
    fs::write(sample.download.join("personal.txt"), b"keep").unwrap();
    for _ in 0..2 {
        assert!(matches!(
            adopt(&config, &plan, &sample.download, true, &mut |_, _, _| {}).unwrap(),
            Outcome::Recorded(_)
        ));
    }
    assert!(!sample.download.join("model.safetensors").exists());
    assert_eq!(
        fs::read(sample.download.join("personal.txt")).unwrap(),
        b"keep"
    );
}

#[test]
fn move_preserves_source_changed_after_staging_started() {
    let (_dir, config) = setup();
    let sample = sample(true);
    let plan = stored(&config, &sample);
    let mut weight_updates = 0;
    let result = adopt(&config, &plan, &sample.download, true, &mut |path, _, _| {
        if path == "model.safetensors" {
            weight_updates += 1;
            if weight_updates == 3 {
                fs::write(sample.download.join(path), b"CHANGED").unwrap();
            }
        }
    });
    assert_eq!(result.unwrap_err().code, "source_remove_failed");
    assert_eq!(
        fs::read(sample.download.join("model.safetensors")).unwrap(),
        b"CHANGED"
    );
    assert!(sample.download.join("LICENSE").exists());
    assert_eq!(
        fs::read(
            publish_dir(&config.volumes[0].path, "example/model", COMMIT).join("model.safetensors")
        )
        .unwrap(),
        b"weights"
    );
}

#[test]
fn source_must_not_overlap_any_managed_root() {
    let (dir, mut config) = setup();
    let other = fs::canonicalize(dir.path()).unwrap().join("other-volume");
    fs::create_dir(&other).unwrap();
    config.volumes.push(crate::config::Volume {
        label: "other".into(),
        path: other.clone(),
    });
    for path in [
        &config.catalog_path,
        &config.volumes[0].path,
        &other,
        &fs::canonicalize(dir.path()).unwrap(),
    ] {
        assert_eq!(
            super::checked_source(&config, path).unwrap_err().code,
            "overlapping_vault_paths"
        );
    }
    let file = dir.path().join("regular-file");
    fs::write(&file, b"x").unwrap();
    assert_eq!(
        super::checked_source(&config, &file).unwrap_err().code,
        "unsafe_path"
    );
}

#[test]
fn oversized_and_corrupt_license_evidence_is_rejected() {
    let (_dir, config) = setup();
    let sample = sample(true);
    let plan = stored(&config, &sample);
    let license = sample.download.join("LICENSE");
    let file = fs::OpenOptions::new().write(true).open(&license).unwrap();
    file.set_len(8 * 1024 * 1024 * 1024).unwrap();
    drop(file);
    assert_eq!(
        adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {})
            .unwrap_err()
            .code,
        "evidence_mismatch"
    );
    fs::write(&license, b"UPSTREAM LICENSE TEXT").unwrap();
    assert_eq!(
        adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {})
            .unwrap_err()
            .code,
        "evidence_mismatch"
    );
    assert!(status(&config).unwrap().preserved.is_empty());
}

#[cfg(unix)]
#[test]
fn linked_evidence_and_retry_ancestors_are_not_followed() {
    use std::os::unix::fs::symlink;
    let (_dir, config) = setup();
    let mut sample = sample(true);
    // A reviewed nested path lets the retry exercise an ancestor link.
    fs::create_dir(sample.download.join("nested")).unwrap();
    fs::rename(
        sample.download.join("model.safetensors"),
        sample.download.join("nested/model.safetensors"),
    )
    .unwrap();
    sample.inventory.files[2].path = "nested/model.safetensors".into();
    let plan = stored(&config, &sample);
    adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {}).unwrap();
    let outside = sample._dir.path().join("outside");
    fs::rename(sample.download.join("nested"), &outside).unwrap();
    symlink(&outside, sample.download.join("nested")).unwrap();
    assert_eq!(
        adopt(&config, &plan, &sample.download, true, &mut |_, _, _| {})
            .unwrap_err()
            .code,
        "unsafe_path"
    );
    assert_eq!(
        fs::read(outside.join("model.safetensors")).unwrap(),
        b"weights"
    );
    assert!(sample.download.join("LICENSE").exists());

    let sample = self::sample(true);
    let license = sample.download.join("LICENSE");
    let original = sample._dir.path().join("original-license");
    fs::rename(&license, &original).unwrap();
    symlink(&original, &license).unwrap();
    // Use a fresh vault so the first-publication evidence path is exercised.
    let (_fresh, config) = setup();
    let plan = stored(&config, &sample);
    assert_eq!(
        adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {})
            .unwrap_err()
            .code,
        "unsafe_path"
    );
}

#[cfg(windows)]
#[test]
fn retry_does_not_follow_windows_junctions() {
    let (_dir, config) = setup();
    let mut sample = sample(true);
    let nested = sample.download.join("nested");
    fs::create_dir(&nested).unwrap();
    fs::rename(
        sample.download.join("model.safetensors"),
        nested.join("model.safetensors"),
    )
    .unwrap();
    sample.inventory.files[2].path = "nested/model.safetensors".into();
    let plan = stored(&config, &sample);
    adopt(&config, &plan, &sample.download, false, &mut |_, _, _| {}).unwrap();
    let outside = sample._dir.path().join("outside");
    fs::rename(&nested, &outside).unwrap();
    assert!(
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&nested)
            .arg(&outside)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        adopt(&config, &plan, &sample.download, true, &mut |_, _, _| {})
            .unwrap_err()
            .code,
        "unsafe_path"
    );
    assert_eq!(
        fs::read(outside.join("model.safetensors")).unwrap(),
        b"weights"
    );
    assert!(sample.download.join("LICENSE").exists());
    fs::remove_dir(&nested).unwrap();
}
