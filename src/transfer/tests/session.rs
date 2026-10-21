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
    assert_eq!(session.spent_bytes().unwrap(), 3);
    drop(session);
    let mut session = StagingSession::open(&path, &source(), &expected()).unwrap();
    assert_eq!(session.recovered_bytes(), 0);
    assert_eq!(
        stage(&mut session, 3, b"def").unwrap().verified_sha256,
        Some(digest(b"abcdef"))
    );
    assert_eq!(session.spent_bytes().unwrap(), 6);
    drop(session);
    let mut session = StagingSession::open(&path, &source(), &expected()).unwrap();
    assert_eq!(session.spent_bytes().unwrap(), 6);
    assert_eq!(
        session.stage(1).unwrap().verified_sha256,
        Some(digest(b"abcdef"))
    );
    assert_eq!(session.spent_bytes().unwrap(), 6);
    assert_eq!(session.stage(0).unwrap_err().code, "invalid_limit");
    assert_eq!(contents(&mut session.file), b"abcdef");
}

#[cfg(unix)]
#[test]
fn closing_a_session_releases_ownership_despite_an_inherited_descriptor() {
    let (_temp, path) = root();
    let session = StagingSession::create(&path, &source(), &expected()).unwrap();
    // A process fork in another thread can retain the open file description
    // briefly before exec closes its CLOEXEC copy.
    let inherited = session._writer.0.try_clone().unwrap();
    drop(session);
    let reopened = StagingSession::open(&path, &source(), &expected()).unwrap();
    assert!(inherited.try_lock().is_err());
    drop(reopened);
    inherited.try_lock().unwrap();
}

#[test]
fn failed_checkpoint_commit_keeps_old_progress_and_next_segment_reconciles_tail() {
    let (_temp, path) = root();
    let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
    stage(&mut session, 0, b"abc").unwrap();
    session.db.execute_batch("PRAGMA query_only=ON;").unwrap();
    // The charge is recorded before any request. A journal that cannot record
    // it must not download or extend the payload.
    assert_eq!(
        session
            .stage_with(&Http::default(), "http://127.0.0.1:9", 3)
            .unwrap_err()
            .code,
        "checkpoint_journal_failed"
    );
    assert_eq!(contents(&mut session.file), b"abc");
    assert_eq!(session.spent_bytes().unwrap(), 3);
    session.db.execute_batch("PRAGMA query_only=OFF;").unwrap();
    session.reserve(3).unwrap();
    assert_eq!(session.spent_bytes().unwrap(), 6);
    session.db.execute_batch("PRAGMA query_only=ON;").unwrap();
    let checkpoint = session.checkpoint().unwrap();
    let server = Server::new(vec![response(
        206,
        "Content-Range: bytes 3-5/6\r\n",
        b"def",
    )]);
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
    assert!(server.finish()[0].contains("bytes=3-"));
    assert_eq!(
        session.commit(&checkpoint_at(6)).unwrap_err().code,
        "checkpoint_journal_failed"
    );
    assert_eq!(contents(&mut session.file), b"abcdef");
    assert_eq!(session.checkpoint().unwrap().bytes, 3);
    assert_eq!(session.spent_bytes().unwrap(), 6);
    session.db.execute_batch("PRAGMA query_only=OFF;").unwrap();
    let final_segment = stage(&mut session, 3, b"def").unwrap();
    assert_eq!(session.recovered_bytes(), 3);
    // The discarded tail was already charged, and the retry is charged again.
    assert_eq!(session.spent_bytes().unwrap(), 9);
    assert_eq!(final_segment.verified_sha256, Some(digest(b"abcdef")));
    assert_eq!(session.checkpoint().unwrap().bytes, 6);
}

fn checkpoint_at(bytes: u64) -> Checkpoint {
    Checkpoint {
        schema_version: 1,
        identity: String::new(),
        bytes,
        prefix_sha256: digest(b"abcdef"),
    }
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
        "UPDATE checkpoint SET spent='-1';",
        "UPDATE checkpoint SET spent='spent';",
        "UPDATE checkpoint SET spent='18446744073709551616';",
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
    // Charge the next allowance, then write the payload without committing it.
    session.reserve(3).unwrap();
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
        assert_eq!(session.spent_bytes().unwrap(), 6);
        if phase == "committed" {
            assert_eq!(session.recovered_bytes(), 0);
            assert_eq!(session.checkpoint().unwrap().bytes, 6);
            assert_eq!(
                session.stage(1).unwrap().verified_sha256,
                Some(digest(b"abcdef"))
            );
            assert_eq!(session.spent_bytes().unwrap(), 6);
        } else {
            assert_eq!(session.recovered_bytes(), 3);
            assert_eq!(session.checkpoint().unwrap().bytes, 3);
            assert_eq!(contents(&mut session.file), b"abc");
            assert_eq!(
                stage(&mut session, 3, b"def").unwrap().verified_sha256,
                Some(digest(b"abcdef"))
            );
            assert_eq!(session.spent_bytes().unwrap(), 9);
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
    let (_temp, path) = root();
    let session = StagingSession::create(&path, &source(), &expected()).unwrap();
    drop(session);
    fs::write(
        path.join("checkpoint.sqlite3-journal"),
        vec![0; 1024 * 1024 + 1],
    )
    .unwrap();
    assert_eq!(
        StagingSession::open(&path, &source(), &expected())
            .err()
            .unwrap()
            .code,
        "invalid_checkpoint_journal"
    );
    assert!(fs::read(path.join("payload.part")).unwrap().is_empty());
}

#[test]
fn rejected_segments_keep_the_charge_and_side_files_stay_inside_the_cap() {
    let (_temp, path) = root();
    let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
    assert!(sqlite_side_bytes(&path) <= staging_workspace_bound());
    assert_eq!(staging_workspace_bound(), 2 * 1024 * 1024);
    stage(&mut session, 0, b"abc").unwrap();
    let server = Server::new(vec![response(206, "Content-Range: bytes 3-5/6\r\n", b"de")]);
    assert_eq!(
        session
            .stage_with(&Http::fixture(&server.origin), &server.origin, 3)
            .unwrap_err()
            .code,
        "invalid_range"
    );
    assert!(server.finish()[0].contains("bytes=3-"));
    assert_eq!(session.spent_bytes().unwrap(), 6);
    assert_eq!(session.checkpoint().unwrap().bytes, 3);
    assert_eq!(contents(&mut session.file), b"abc");
    drop(session);
    let session = StagingSession::open(&path, &source(), &expected()).unwrap();
    assert_eq!(session.spent_bytes().unwrap(), 6);
    assert_eq!(session.recovered_bytes(), 0);
    assert!(sqlite_side_bytes(&path) <= staging_workspace_bound());
    drop(session);

    let (_temp, path) = root();
    let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
    session
        .db
        .execute(
            "UPDATE checkpoint SET spent=?1 WHERE id=1",
            [u64::MAX.to_string()],
        )
        .unwrap();
    assert_eq!(
        session
            .stage_with(&Http::default(), "http://127.0.0.1:9", 1)
            .unwrap_err()
            .code,
        "size_overflow"
    );
    assert_eq!(session.spent_bytes().unwrap(), u64::MAX);
    assert!(fs::read(path.join("payload.part")).unwrap().is_empty());
}

fn canonical_path() -> String {
    format!("/owner/model/resolve/{}/model.safetensors", "a".repeat(40))
}

fn wire_bytes(exchanges: &[crate::fixture::Exchange]) -> u64 {
    exchanges.iter().fold(0, |sum, exchange| {
        sum + exchange.read_bytes + exchange.written_bytes
    })
}

fn assert_no_credentials(requests: &[impl AsRef<str>]) {
    for request in requests {
        let lower = request.as_ref().to_ascii_lowercase();
        assert!(!lower.contains("authorization:"));
        assert!(!lower.contains("cookie:"));
    }
}

#[test]
fn expired_payload_location_refreshes_once_without_changing_identity_or_charge() {
    let (_temp, path) = root();
    let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
    let identity = session.checkpoint().unwrap().identity.clone();
    stage(&mut session, 0, b"abc").unwrap();
    assert_eq!(session.spent_bytes().unwrap(), 3);
    let canonical = canonical_path();
    let server = Server::new(vec![
        response(302, "Location: /cdn?sig=old-secret\r\n", b""),
        response(403, "", b"sig=old-secret"),
        response(302, "Location: /cdn?sig=new-secret\r\n", b""),
        response(206, "Content-Range: bytes 3-5/6\r\n", b"def"),
    ]);
    let segment = session
        .stage_with(&Http::fixture(&server.origin), &server.origin, 3)
        .unwrap();
    let exchanges = server.finish_measured();
    let requests: Vec<&str> = exchanges
        .iter()
        .map(|exchange| exchange.request.as_str())
        .collect();
    assert_eq!(requests.len(), 4);
    let wire = wire_bytes(&exchanges);
    assert!(wire > 3, "plaintext wire bytes exceed the charged payload");
    assert!(exchanges.iter().all(|exchange| exchange.read_bytes < 8192));
    assert!(exchanges.last().unwrap().written_bytes > 3);
    for request in &requests {
        assert!(request.to_ascii_lowercase().contains("range: bytes=3-5"));
    }
    assert!(requests[0].contains(&canonical));
    assert!(requests[1].contains("GET /cdn?sig=old-secret "));
    assert!(requests[2].contains(&canonical));
    assert!(requests[3].contains("GET /cdn?sig=new-secret "));
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.contains("sig=old-secret"))
            .count(),
        1
    );
    assert_no_credentials(&requests);
    assert_eq!(segment.checkpoint.identity, identity);
    assert_eq!(segment.verified_sha256, Some(digest(b"abcdef")));
    assert_eq!(session.spent_bytes().unwrap(), 6);
    assert_eq!(session.checkpoint().unwrap().bytes, 6);
    assert_eq!(contents(&mut session.file), b"abcdef");
    assert!(
        !String::from_utf8_lossy(&fs::read(path.join("checkpoint.sqlite3")).unwrap())
            .contains("sig=")
    );
}

#[test]
fn rejected_or_substituted_refresh_keeps_the_charge_and_the_pinned_identity() {
    let canonical = canonical_path();
    for (replies, code) in [
        (
            vec![
                response(302, "Location: /cdn?sig=old-secret\r\n", b""),
                response(403, "", b"sig=old-secret"),
                response(302, "Location: /cdn?sig=new-secret\r\n", b""),
                response(403, "", b"sig=new-secret"),
            ],
            "payload_location_rejected",
        ),
        (
            vec![
                response(302, "Location: /cdn?sig=old-secret\r\n", b""),
                response(401, "", b"sig=old-secret"),
                response(302, "Location: /cdn?sig=new-secret\r\n", b""),
                response(200, "", b"abcdeg"),
            ],
            "hash_mismatch",
        ),
    ] {
        let (_temp, path) = root();
        let mut session = StagingSession::create(&path, &source(), &expected()).unwrap();
        let identity = session.checkpoint().unwrap().identity.clone();
        let server = Server::new(replies);
        let error = session
            .stage_with(&Http::fixture(&server.origin), &server.origin, 6)
            .unwrap_err();
        assert_eq!(error.code, code);
        assert!(!error.message.contains("sig"));
        let exchanges = server.finish_measured();
        let requests: Vec<&str> = exchanges
            .iter()
            .map(|exchange| exchange.request.as_str())
            .collect();
        let wire = wire_bytes(&exchanges);
        assert!(
            wire > 6,
            "plaintext wire bytes exceed the charged allowance"
        );
        assert!(requests[0].contains(&canonical));
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.contains("sig=old-secret"))
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.contains("sig=new-secret"))
                .count(),
            1
        );
        assert_no_credentials(&requests);
        assert_eq!(session.spent_bytes().unwrap(), 6);
        assert_eq!(session.checkpoint().unwrap().identity, identity);
        assert_eq!(session.checkpoint().unwrap().bytes, 0);
        assert!(contents(&mut session.file).is_empty());
        assert!(
            !String::from_utf8_lossy(&fs::read(path.join("checkpoint.sqlite3")).unwrap())
                .contains("sig=")
        );
    }
}

fn sqlite_side_bytes(path: &std::path::Path) -> u64 {
    ["checkpoint.sqlite3", "checkpoint.sqlite3-journal"]
        .into_iter()
        .map(|name| {
            let file = path.join(name);
            if file.exists() {
                fs::metadata(file).unwrap().len()
            } else {
                0
            }
        })
        .sum()
}
