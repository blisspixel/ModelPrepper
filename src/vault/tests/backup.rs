use super::*;
use crate::vault::{initialize, status};

#[test]
fn snapshot_includes_committed_wal_pages_and_reopens_without_the_original_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let config = Config::default_for(root.join("catalog"), root.join("primary"));
    let initialized = initialize(&config).unwrap();
    let source = Connection::open(config.catalog_path.join("catalog.sqlite3")).unwrap();
    source.execute_batch("PRAGMA wal_autocheckpoint=0; PRAGMA wal_checkpoint(TRUNCATE); INSERT INTO collections(name) VALUES ('before-backup')").unwrap();
    assert!(
        config
            .catalog_path
            .join("catalog.sqlite3-wal")
            .metadata()
            .unwrap()
            .len()
            > 32
    );
    let output = root.join("snapshot.sqlite3");
    let receipt = backup_catalog(&config, &output).unwrap();
    assert_eq!(receipt.vault_id, initialized.vault_id);
    assert_eq!(receipt.size_bytes, output.metadata().unwrap().len());
    assert_eq!(
        receipt.sha256,
        crate::plan::digest(&fs::read(&output).unwrap())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(output.metadata().unwrap().permissions().mode() & 0o077, 0);
    }
    source
        .execute("INSERT INTO collections(name) VALUES ('later')", [])
        .unwrap();
    drop(source);
    fs::rename(&config.catalog_path, root.join("original-unavailable")).unwrap();
    fs::create_dir(&config.catalog_path).unwrap();
    fs::copy(&output, config.catalog_path.join("catalog.sqlite3")).unwrap();
    fs::File::create_new(config.catalog_path.join("writer.lock")).unwrap();
    assert_eq!(status(&config).unwrap().vault_id, initialized.vault_id);
    let restored = open_catalog(&config, false).unwrap();
    let names: String = restored
        .db
        .query_row("SELECT group_concat(name) FROM collections", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(names, "before-backup");
}

#[test]
fn snapshots_refuse_owned_paths_existing_outputs_sidecars_and_a_busy_writer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let config = Config::default_for(root.join("catalog"), root.join("primary"));
    initialize(&config).unwrap();
    let backup = root.join("snapshot.db");
    let lock = open_catalog(&config, false).unwrap();
    assert_eq!(
        backup_catalog(&config, &backup).unwrap_err().code,
        "writer_busy"
    );
    drop(lock);
    for managed in [&config.catalog_path, &config.volumes[0].path] {
        assert_eq!(
            backup_catalog(&config, &managed.join("copy.db"))
                .unwrap_err()
                .code,
            "overlapping_vault_paths"
        );
    }
    let sidecar = root.join("snapshot.db-wal");
    assert_eq!(
        backup_catalog(&config, &sidecar).unwrap_err().code,
        "invalid_output"
    );
    fs::write(&sidecar, b"prior").unwrap();
    assert_eq!(
        backup_catalog(&config, &backup).unwrap_err().code,
        "output_exists"
    );
    assert!(!backup.exists());
    assert_eq!(fs::read(&sidecar).unwrap(), b"prior");
    fs::remove_file(sidecar).unwrap();
    fs::write(&backup, b"keep").unwrap();
    assert_eq!(
        backup_catalog(&config, &backup).unwrap_err().code,
        "output_exists"
    );
    assert_eq!(fs::read(backup).unwrap(), b"keep");
}
