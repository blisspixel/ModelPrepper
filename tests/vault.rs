use modelprepper::{
    Config,
    config::Volume,
    vault::{initialize, locate, status},
};
use rusqlite::Connection;
use std::{fs, fs::OpenOptions};
use tempfile::TempDir;

fn configured() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    // macOS temporary paths can contain the OS-owned /var symlink.
    let root = fs::canonicalize(dir.path()).unwrap();
    let config = Config::default_for(root.join("catalog"), root.join("models"));
    (dir, config)
}

fn edit(config: &Config, sql: &str) {
    Connection::open(config.catalog_path.join("catalog.sqlite3"))
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

#[test]
fn creates_stable_catalog_and_volume_identity_and_holds_a_kernel_lock() {
    let (_dir, config) = configured();
    let first = initialize(&config).unwrap();
    assert_eq!(first.state, "ready");
    assert_eq!(first.volumes[0].state, "mounted");
    assert!(first.volumes[0].available_bytes.unwrap() > 0);
    let again = initialize(&config).unwrap();
    assert_eq!(first.vault_id, again.vault_id);
    assert_eq!(first.volumes[0].volume_id, again.volumes[0].volume_id);
    fs::write(config.volumes[0].path.join("user-copy.txt"), b"preserve").unwrap();
    assert_eq!(initialize(&config).unwrap().state, "ready");
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(config.catalog_path.join("writer.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert_eq!(status(&config).unwrap_err().code, "writer_busy");
    drop(lock);
    assert_eq!(status(&config).unwrap().state, "ready");
}

#[test]
fn config_relative_paths_are_resolved_against_the_config_file() {
    let dir = TempDir::new().unwrap();
    let config = Config::default_for("catalog".into(), "models".into());
    let file = dir.path().join("config.toml");
    fs::write(&file, toml::to_string(&config).unwrap()).unwrap();
    let located = locate(&config, &file).unwrap();
    assert_eq!(
        located.catalog_path,
        fs::canonicalize(dir.path()).unwrap().join("catalog")
    );
    assert_eq!(initialize(&located).unwrap().state, "ready");
    assert!(locate(&config, &dir.path().join("missing.toml")).is_err());
    let mut dotted = config.clone();
    dotted.volumes[0].path = "../models".into();
    assert_eq!(
        locate(&dotted, &file).unwrap_err().code,
        "unsafe_vault_path"
    );
    dotted = config.clone();
    dotted.catalog_path = "./catalog".into();
    assert_eq!(
        locate(&dotted, &file).unwrap_err().code,
        "unsafe_vault_path"
    );
    assert_eq!(initialize(&config).unwrap_err().code, "unsafe_vault_path");
}

#[test]
fn empty_setup_never_adopts_user_data_or_overlapping_paths() {
    let (_dir, config) = configured();
    assert_eq!(status(&config).unwrap_err().code, "vault_not_initialized");
    assert!(!config.catalog_path.exists());
    let mut invalid = config.clone();
    invalid.volumes[0].path = config.catalog_path.join("models");
    assert_eq!(
        initialize(&invalid).unwrap_err().code,
        "overlapping_vault_paths"
    );
    invalid.volumes[0].path =
        std::path::PathBuf::from(format!("{}/../models", config.volumes[0].path.display()));
    assert_eq!(initialize(&invalid).unwrap_err().code, "unsafe_vault_path");
    invalid.volumes[0].path =
        std::path::PathBuf::from(format!("{}/./models", config.volumes[0].path.display()));
    assert_eq!(initialize(&invalid).unwrap_err().code, "unsafe_vault_path");
    fs::create_dir(&config.volumes[0].path).unwrap();
    let valuable = config.volumes[0].path.join("keep.txt");
    fs::write(&valuable, b"valuable").unwrap();
    assert_eq!(initialize(&config).unwrap_err().code, "directory_not_empty");
    assert_eq!(fs::read(&valuable).unwrap(), b"valuable");
    assert!(!config.catalog_path.exists());
    fs::create_dir(&config.catalog_path).unwrap();
    fs::write(config.catalog_path.join("unrelated.txt"), b"other").unwrap();
    assert_eq!(initialize(&config).unwrap_err().code, "directory_not_empty");
}

#[test]
fn missing_or_substituted_disks_are_never_initialized_again() {
    let (dir, config) = configured();
    initialize(&config).unwrap();
    let original = dir.path().join("unplugged");
    fs::rename(&config.volumes[0].path, &original).unwrap();
    assert_eq!(status(&config).unwrap().volumes[0].state, "offline");
    assert_eq!(initialize(&config).unwrap().volumes[0].state, "offline");
    assert!(!config.volumes[0].path.exists());
    fs::create_dir(&config.volumes[0].path).unwrap();
    assert_eq!(
        status(&config).unwrap().volumes[0].state,
        "identity_missing"
    );
    let marker = config.volumes[0].path.join(".modelprepper-volume.json");
    let mut identity: serde_json::Value =
        serde_json::from_slice(&fs::read(original.join(".modelprepper-volume.json")).unwrap())
            .unwrap();
    identity["volume_id"] = serde_json::json!("different-disk");
    fs::write(&marker, serde_json::to_vec(&identity).unwrap()).unwrap();
    assert_eq!(
        status(&config).unwrap().volumes[0].state,
        "identity_mismatch"
    );
    assert_eq!(
        initialize(&config).unwrap().volumes[0].state,
        "identity_mismatch"
    );
    fs::write(&marker, b"{partial").unwrap();
    assert_eq!(status(&config).unwrap_err().code, "invalid_volume_identity");
}

#[test]
fn resumes_committed_initialization_and_pending_marker_publication() {
    let (_dir, config) = configured();
    let before = initialize(&config).unwrap();
    edit(&config, "UPDATE vault SET state='initializing'");
    let marker = config.volumes[0].path.join(".modelprepper-volume.json");
    let pending = config.volumes[0].path.join(".modelprepper-volume.pending");
    fs::rename(&marker, &pending).unwrap();
    assert_eq!(status(&config).unwrap().state, "initializing");
    let recovered = initialize(&config).unwrap();
    assert_eq!(recovered.vault_id, before.vault_id);
    assert_eq!(recovered.volumes[0].volume_id, before.volumes[0].volume_id);
    assert_eq!(recovered.volumes[0].state, "mounted");
    assert!(!pending.exists());
    edit(&config, "UPDATE vault SET state='initializing'");
    fs::remove_file(&marker).unwrap();
    assert_eq!(
        initialize(&config).unwrap().volumes[0].volume_id,
        before.volumes[0].volume_id
    );
}

#[test]
fn preflights_all_volumes_before_writing_any_markers_on_recovery() {
    let (dir, mut config) = configured();
    config.volumes.push(Volume {
        label: "backup".into(),
        path: fs::canonicalize(dir.path()).unwrap().join("backup"),
    });
    initialize(&config).unwrap();
    edit(&config, "UPDATE vault SET state='initializing'");
    fs::remove_file(config.volumes[0].path.join(".modelprepper-volume.json")).unwrap();
    let other = config.volumes[1].path.join(".modelprepper-volume.json");
    let before = fs::read(&other).unwrap();
    let mut wrong: serde_json::Value = serde_json::from_slice(&before).unwrap();
    wrong["vault_id"] = serde_json::json!("another-vault");
    fs::write(&other, serde_json::to_vec(&wrong).unwrap()).unwrap();
    assert_eq!(
        initialize(&config).unwrap_err().code,
        "volume_identity_mismatch"
    );
    assert!(
        !config.volumes[0]
            .path
            .join(".modelprepper-volume.json")
            .exists()
    );
    fs::write(&other, &before).unwrap();
    fs::write(config.volumes[1].path.join("user-data"), b"keep").unwrap();
    assert_eq!(initialize(&config).unwrap_err().code, "directory_not_empty");
    assert!(
        !config.volumes[0]
            .path
            .join(".modelprepper-volume.json")
            .exists()
    );
}

#[test]
fn unsupported_or_corrupt_catalogs_and_configuration_drift_fail_closed() {
    let (dir, config) = configured();
    initialize(&config).unwrap();
    let mut changed = config.clone();
    changed.volumes[0].path = fs::canonicalize(dir.path()).unwrap().join("other");
    assert_eq!(
        status(&changed).unwrap_err().code,
        "volume_configuration_changed"
    );
    changed = config.clone();
    changed.volumes.push(Volume {
        label: "extra".into(),
        path: fs::canonicalize(dir.path()).unwrap().join("extra"),
    });
    assert_eq!(
        status(&changed).unwrap_err().code,
        "volume_configuration_changed"
    );
    for sql in [
        "UPDATE vault SET id='bad'",
        "UPDATE vault SET state='unknown'",
        "UPDATE volumes SET id='bad'",
    ] {
        let (_fresh, fresh) = configured();
        initialize(&fresh).unwrap();
        edit(&fresh, sql);
        assert_eq!(status(&fresh).unwrap_err().code, "invalid_catalog");
    }
    edit(&config, "PRAGMA user_version=99");
    assert_eq!(
        initialize(&config).unwrap_err().code,
        "unsupported_catalog_schema"
    );
    let (_fresh, fresh) = configured();
    fs::create_dir(&fresh.catalog_path).unwrap();
    fs::write(fresh.catalog_path.join("writer.lock"), b"").unwrap();
    fs::write(
        fresh.catalog_path.join("catalog.sqlite3"),
        b"not a database",
    )
    .unwrap();
    assert_eq!(status(&fresh).unwrap_err().code, "catalog_failed");
}

#[test]
fn symlinks_cannot_own_a_catalog_volume_or_marker() {
    let (dir, config) = configured();
    let destination = dir.path().join("real");
    fs::create_dir(&destination).unwrap();
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(&destination, &config.volumes[0].path);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(&destination, &config.volumes[0].path);
    // Windows developer mode or symlink privilege is not assumed on every runner.
    if result.is_err() {
        return;
    }
    assert_eq!(initialize(&config).unwrap_err().code, "unsafe_vault_path");
    assert!(!config.catalog_path.exists());
}

#[cfg(windows)]
#[test]
fn windows_junctions_fail_without_requiring_symlink_privilege() {
    let (dir, config) = configured();
    let destination = dir.path().join("real");
    fs::create_dir(&destination).unwrap();
    let result = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:MODELPREPPER_TEST_LINK -Target $env:MODELPREPPER_TEST_TARGET -ErrorAction Stop | Out-Null"])
        .env("MODELPREPPER_TEST_LINK", &config.volumes[0].path)
        .env("MODELPREPPER_TEST_TARGET", &destination)
        .output().unwrap();
    assert!(result.status.success(), "{result:?}");
    assert_eq!(initialize(&config).unwrap_err().code, "unsafe_vault_path");
    assert!(!config.catalog_path.exists());
}

#[cfg(windows)]
#[test]
fn unc_catalogs_are_rejected_before_network_filesystem_access() {
    let (_dir, mut config) = configured();
    config.catalog_path = std::path::PathBuf::from(r"\\server\share\catalog");
    assert_eq!(
        initialize(&config).unwrap_err().code,
        "unsupported_catalog_filesystem"
    );
}
