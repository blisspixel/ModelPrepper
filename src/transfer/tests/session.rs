use super::*;
use crate::{
    fixture::{Server, response},
    inventory::{HashAlgorithm, Role, UpstreamHash},
};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    process::{Child, Command},
    time::{Duration, Instant},
};

fn source() -> Source {
    Source {
        endpoint: "https://huggingface.co".into(),
        repo_type: "model".into(),
        repo_id: "owner/model".into(),
        requested_revision: "main".into(),
        resolved_revision: "a".repeat(40),
        gated: false,
        private: false,
    }
}
fn expected() -> SourceFile {
    SourceFile {
        path: "model.safetensors".into(),
        size_bytes: 6,
        upstream_hash: Some(UpstreamHash {
            algorithm: HashAlgorithm::Sha256,
            value: digest(b"abcdef"),
        }),
        role: Role::Weight,
        required: true,
    }
}
fn root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(temp.path()).unwrap().join("session");
    (temp, path)
}
fn stage(session: &mut StagingSession, start: u64, body: &[u8]) -> Result<Segment> {
    let headers = format!(
        "Content-Range: bytes {start}-{}/6\r\n",
        start + body.len() as u64 - 1
    );
    let server = Server::new(vec![response(206, &headers, body)]);
    let result = session.stage_with(
        &Http::fixture(&server.origin),
        &server.origin,
        body.len() as u64,
    );
    assert!(server.finish()[0].contains(&format!("bytes={start}-")));
    result
}
fn contents(file: &mut File) -> Vec<u8> {
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn journal_survives_reopen_and_completion_requires_full_verification() {
    let (_temp, path) = root();
    let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
    assert_eq!(session.checkpoint().unwrap().bytes, 0);
    assert_eq!(session.recovered_bytes(), 0);
    assert_eq!(
        StagingSession::open(&path, &source(), &expected())
            .err()
            .unwrap()
            .code,
        "writer_busy"
    );
    let first = stage(&mut session, 0, b"abc").unwrap();
    assert!(first.verified_sha256.is_none());
    assert_eq!(session.checkpoint().unwrap().bytes, 3);
    drop(session);
    let mut session = StagingSession::open(&path, &source(), &expected()).unwrap();
    assert_eq!(session.recovered_bytes(), 0);
    assert_eq!(
        stage(&mut session, 3, b"def").unwrap().verified_sha256,
        Some(digest(b"abcdef"))
    );
    drop(session);
    let mut session = StagingSession::open(&path, &source(), &expected()).unwrap();
    assert_eq!(
        session.stage(1).unwrap().verified_sha256,
        Some(digest(b"abcdef"))
    );
    assert_eq!(session.stage(0).unwrap_err().code, "invalid_limit");
    assert_eq!(contents(&mut session.file), b"abcdef");
}

#[test]
fn failed_checkpoint_commit_keeps_old_progress_and_next_segment_reconciles_tail() {
    let (_temp, path) = root();
    let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
    stage(&mut session, 0, b"abc").unwrap();
    session.db.execute_batch("PRAGMA query_only=ON;").unwrap();
    assert_eq!(
        stage(&mut session, 3, b"def").unwrap_err().code,
        "checkpoint_journal_failed"
    );
    assert_eq!(contents(&mut session.file), b"abcdef");
    assert_eq!(session.checkpoint().unwrap().bytes, 3);
    session.db.execute_batch("PRAGMA query_only=OFF;").unwrap();
    let final_segment = stage(&mut session, 3, b"def").unwrap();
    assert_eq!(session.recovered_bytes(), 3);
    assert_eq!(final_segment.verified_sha256, Some(digest(b"abcdef")));
    assert_eq!(session.checkpoint().unwrap().bytes, 6);
}

#[test]
fn corrupt_or_foreign_journals_never_truncate_payload() {
    for sql in [
        "PRAGMA application_id=0;",
        "PRAGMA user_version=2;",
        "DELETE FROM checkpoint;",
        "UPDATE checkpoint SET identity='wrong';",
        "UPDATE checkpoint SET bytes='999';",
        "UPDATE checkpoint SET bytes='-1';",
        "UPDATE checkpoint SET bytes='not-a-number';",
        "UPDATE checkpoint SET bytes='18446744073709551616';",
        "UPDATE checkpoint SET prefix_sha256='wrong';",
        "UPDATE checkpoint SET prefix_sha256=replace(hex(zeroblob(32)), '0', 'a');",
    ] {
        let (_temp, path) = root();
        let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
        stage(&mut session, 0, b"abc").unwrap();
        session.file.write_all(b"TAIL").unwrap();
        session.file.sync_all().unwrap();
        session.db.execute_batch(sql).unwrap();
        drop(session);
        let before = fs::read(path.join("payload.part")).unwrap();
        assert!(
            StagingSession::open(&path, &source(), &expected()).is_err(),
            "{sql}"
        );
        assert_eq!(fs::read(path.join("payload.part")).unwrap(), before);
    }
}

#[test]
fn journal_refuses_changed_identity_short_or_corrupt_committed_files() {
    let (_temp, path) = root();
    let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
    stage(&mut session, 0, b"abc").unwrap();
    drop(session);
    let mut changed = source();
    changed.resolved_revision = "b".repeat(40);
    assert!(StagingSession::open(&path, &changed, &expected()).is_err());
    let mut changed = expected();
    changed.path = "another.safetensors".into();
    assert!(StagingSession::open(&path, &source(), &changed).is_err());
    for bad in [b"ab".as_slice(), b"badTAIL"] {
        fs::write(path.join("payload.part"), bad).unwrap();
        assert_eq!(
            StagingSession::open(&path, &source(), &expected())
                .err()
                .unwrap()
                .code,
            "invalid_checkpoint"
        );
        assert_eq!(fs::read(path.join("payload.part")).unwrap(), bad);
    }
}

#[test]
fn initialization_does_not_adopt_or_replace_existing_data() {
    let (_temp, path) = root();
    assert_eq!(
        StagingSession::create(Path::new("relative"), &source(), &expected())
            .err()
            .unwrap()
            .code,
        "unsafe_staging_path"
    );
    let mut bad = source();
    bad.private = true;
    assert!(StagingSession::create(&path, &bad, &expected()).is_err());
    assert!(!path.exists());
    fs::create_dir(&path).unwrap();
    assert!(StagingSession::create(&path, &source(), &expected()).is_err());
    fs::write(path.join("user-data"), b"keep").unwrap();
    assert_eq!(
        StagingSession::open(&path, &source(), &expected())
            .err()
            .unwrap()
            .code,
        "staging_directory_not_owned"
    );
    assert_eq!(fs::read(path.join("user-data")).unwrap(), b"keep");
    let (_temp, path) = root();
    let session = StagingSession::create(&path, &source(), &expected()).unwrap();
    drop(session);
    fs::remove_file(path.join("checkpoint.sqlite3")).unwrap();
    assert!(StagingSession::open(&path, &source(), &expected()).is_err());
    assert!(!path.join("checkpoint.sqlite3").exists());
}

#[test]
fn empty_files_and_large_declared_sizes_keep_exact_u64_progress() {
    let (_temp, path) = root();
    let mut file = expected();
    file.size_bytes = 0;
    file.upstream_hash.as_mut().unwrap().value = digest(b"");
    let mut session = StagingSession::create(&path, &source(), &file).unwrap();
    assert_eq!(session.stage(1).unwrap().verified_sha256, Some(digest(b"")));
    drop(session);
    assert_eq!(
        StagingSession::open(&path, &source(), &file)
            .unwrap()
            .checkpoint()
            .unwrap()
            .bytes,
        0
    );
    let (_temp, path) = root();
    file.size_bytes = u64::MAX;
    let session = StagingSession::create(&path, &source(), &file).unwrap();
    drop(session);
    assert_eq!(
        StagingSession::open(&path, &source(), &file)
            .unwrap()
            .checkpoint()
            .unwrap()
            .bytes,
        0
    );
}

#[test]
fn journal_termination_worker() {
    let Some(path) = std::env::var_os("MODELPREPPER_JOURNAL_KILL_ROOT") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
    stage(&mut session, 0, b"abc").unwrap();
    let server = Server::new(vec![response(
        206,
        "Content-Range: bytes 3-5/6\r\n",
        b"def",
    )]);
    let checkpoint = session.checkpoint().unwrap();
    // Genuine payload write/sync, but deliberately stop before journal commit.
    stage_with(
        &Http::fixture(&server.origin),
        &server.origin,
        &source(),
        &expected(),
        &mut session.file,
        Some(&checkpoint),
        3,
    )
    .unwrap();
    server.finish();
    let phase = std::env::var("MODELPREPPER_JOURNAL_KILL_PHASE").unwrap();
    if phase == "transaction" {
        session.db.execute_batch("BEGIN IMMEDIATE;").unwrap();
        session
            .db
            .execute(
                "UPDATE checkpoint SET bytes='6', prefix_sha256=?1 WHERE id=1",
                [digest(b"abcdef")],
            )
            .unwrap();
        assert!(path.join("checkpoint.sqlite3-journal").exists());
    } else if phase == "committed" {
        session
            .commit(&Checkpoint {
                schema_version: 1,
                identity: session.identity.clone(),
                bytes: 6,
                prefix_sha256: digest(b"abcdef"),
            })
            .unwrap();
    }
    fs::write(path.join("ready"), b"ready").unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
fn killed_worker_reopens_real_journal_and_discards_uncommitted_segment() {
    struct Worker(Child);
    impl Drop for Worker {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for phase in ["before_commit", "transaction", "committed"] {
        let (_temp, path) = root();
        let mut worker = Worker(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "transfer::session::tests::journal_termination_worker",
                    "--nocapture",
                ])
                .env("MODELPREPPER_JOURNAL_KILL_ROOT", &path)
                .env("MODELPREPPER_JOURNAL_KILL_PHASE", phase)
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        while !path.join("ready").exists() {
            assert!(
                worker.0.try_wait().unwrap().is_none(),
                "journal worker exited early"
            );
            assert!(Instant::now() < deadline, "journal worker timeout");
            std::thread::sleep(Duration::from_millis(5));
        }
        worker.0.kill().unwrap();
        worker.0.wait().unwrap();
        fs::remove_file(path.join("ready")).unwrap();
        let mut session = StagingSession::open(&path, &source(), &expected()).unwrap();
        if phase == "committed" {
            assert_eq!(session.recovered_bytes(), 0);
            assert_eq!(session.checkpoint().unwrap().bytes, 6);
            assert_eq!(
                session.stage(1).unwrap().verified_sha256,
                Some(digest(b"abcdef"))
            );
        } else {
            assert_eq!(session.recovered_bytes(), 3);
            assert_eq!(session.checkpoint().unwrap().bytes, 3);
            assert_eq!(contents(&mut session.file), b"abc");
            assert_eq!(
                stage(&mut session, 3, b"def").unwrap().verified_sha256,
                Some(digest(b"abcdef"))
            );
        }
    }
}

#[test]
fn oversized_journal_and_nonregular_entries_fail_without_payload_changes() {
    let (_temp, path) = root();
    let session = StagingSession::create(&path, &source(), &expected()).unwrap();
    drop(session);
    File::options()
        .write(true)
        .open(path.join("checkpoint.sqlite3"))
        .unwrap()
        .set_len(1024 * 1024 + 1)
        .unwrap();
    assert_eq!(
        StagingSession::open(&path, &source(), &expected())
            .err()
            .unwrap()
            .code,
        "invalid_checkpoint_journal"
    );
    assert!(fs::read(path.join("payload.part")).unwrap().is_empty());
    let (_temp, path) = root();
    let session = StagingSession::create(&path, &source(), &expected()).unwrap();
    drop(session);
    fs::remove_file(path.join("payload.part")).unwrap();
    fs::create_dir(path.join("payload.part")).unwrap();
    assert_eq!(
        StagingSession::open(&path, &source(), &expected())
            .err()
            .unwrap()
            .code,
        "invalid_checkpoint_journal"
    );
}
