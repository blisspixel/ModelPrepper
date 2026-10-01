use super::*;
use crate::{
    fixture::{Server, response},
    inventory::{Role, UpstreamHash},
};
use std::io::{Read, Seek, SeekFrom, Write};

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
fn expected(bytes: &[u8]) -> SourceFile {
    SourceFile {
        path: "model.safetensors".into(),
        size_bytes: bytes.len() as u64,
        upstream_hash: Some(UpstreamHash {
            algorithm: HashAlgorithm::Sha256,
            value: digest(bytes),
        }),
        role: Role::Weight,
        required: true,
    }
}
fn run(
    reply: Vec<u8>,
    expected: &SourceFile,
    file: &mut File,
    checkpoint: Option<&Checkpoint>,
    allowance: u64,
) -> Result<Segment> {
    let server = Server::new(vec![reply]);
    let result = stage_with(
        &Http::fixture(&server.origin),
        &server.origin,
        &source(),
        expected,
        file,
        checkpoint,
        allowance,
    );
    let requests = server.finish();
    assert!(requests[0].contains(&format!("/resolve/{}/", "a".repeat(40))));
    assert!(requests[0].to_ascii_lowercase().contains("range: bytes="));
    result
}
fn bytes(file: &mut File) -> Vec<u8> {
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    bytes
}

#[test]
fn crash_tail_recovery_survives_reopen_and_resumes_exact_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("partial");
    let model = expected(b"abcdef");
    let checkpoint = {
        let mut file = File::options()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let committed = run(
            response(206, "Content-Range: bytes 0-2/6\r\n", b"abc"),
            &model,
            &mut file,
            None,
            3,
        )
        .unwrap();
        // Simulate synced payload with no subsequent checkpoint journal commit.
        file.write_all(b"defuncommitted").unwrap();
        file.sync_all().unwrap();
        committed.checkpoint
    };
    let mut file = File::options().read(true).write(true).open(&path).unwrap();
    assert_eq!(
        recover_stage(&source(), &model, &mut file, &checkpoint).unwrap(),
        14
    );
    assert_eq!(bytes(&mut file), b"abc");
    assert_eq!(
        recover_stage(&source(), &model, &mut file, &checkpoint).unwrap(),
        0
    );
    let finished = run(
        response(206, "Content-Range: bytes 3-5/6\r\n", b"def"),
        &model,
        &mut file,
        Some(&checkpoint),
        3,
    )
    .unwrap();
    assert_eq!(finished.verified_sha256, Some(digest(b"abcdef")));
    assert_eq!(bytes(&mut file), b"abcdef");
}

#[test]
fn recovery_never_truncates_when_committed_evidence_is_invalid() {
    let model = expected(b"abcdef");
    let (_, identity) = file_identity(&source(), &model).unwrap();
    let valid = Checkpoint {
        schema_version: 1,
        identity,
        bytes: 3,
        prefix_sha256: digest(b"abc"),
    };
    for case in 0..7 {
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(b"abcTAIL").unwrap();
        let mut checkpoint = valid.clone();
        let mut source = source();
        match case {
            0 => checkpoint.schema_version = 2,
            1 => checkpoint.identity = "wrong".into(),
            2 => checkpoint.bytes = 7,
            3 => {
                file.set_len(2).unwrap();
            }
            4 => checkpoint.prefix_sha256 = digest(b"bad"),
            5 => source.resolved_revision = "b".repeat(40),
            _ => {
                file.seek(SeekFrom::Start(0)).unwrap();
                file.write_all(b"bad").unwrap();
            }
        }
        let before = bytes(&mut file);
        assert_eq!(
            recover_stage(&source, &model, &mut file, &checkpoint)
                .unwrap_err()
                .code,
            "invalid_checkpoint"
        );
        assert_eq!(bytes(&mut file), before);
    }
}

#[test]
fn recovery_supports_a_committed_empty_prefix_without_trusting_the_tail() {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"uncommitted").unwrap();
    let model = expected(b"abcdef");
    let (_, identity) = file_identity(&source(), &model).unwrap();
    let checkpoint = Checkpoint {
        schema_version: 1,
        identity,
        bytes: 0,
        prefix_sha256: digest(b""),
    };
    assert_eq!(
        recover_stage(&source(), &model, &mut file, &checkpoint).unwrap(),
        11
    );
    assert!(bytes(&mut file).is_empty());
}

#[test]
fn recovery_reports_read_only_truncation_failure_and_keeps_payload() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("partial");
    std::fs::write(&path, b"abcTAIL").unwrap();
    let mut file = File::open(&path).unwrap();
    let model = expected(b"abcdef");
    let (_, identity) = file_identity(&source(), &model).unwrap();
    let checkpoint = Checkpoint {
        schema_version: 1,
        identity,
        bytes: 3,
        prefix_sha256: digest(b"abc"),
    };
    assert_eq!(
        recover_stage(&source(), &model, &mut file, &checkpoint)
            .unwrap_err()
            .code,
        "staging_io_failed"
    );
    assert_eq!(bytes(&mut file), b"abcTAIL");
}

#[test]
fn resumes_identity_bound_segments_and_verifies_the_whole_file() {
    let mut file = tempfile::tempfile().unwrap();
    let model = expected(b"abcdef");
    let first = run(
        response(206, "Content-Range: bytes 0-2/6\r\n", b"abc"),
        &model,
        &mut file,
        None,
        3,
    )
    .unwrap();
    assert!(first.verified_sha256.is_none());
    assert_eq!(first.received_bytes, 3);
    let checkpoint: Checkpoint =
        serde_json::from_str(&serde_json::to_string(&first.checkpoint).unwrap()).unwrap();
    let second = run(
        response(206, "Content-Range: bytes 3-5/6\r\n", b"def"),
        &model,
        &mut file,
        Some(&checkpoint),
        100,
    )
    .unwrap();
    assert_eq!(bytes(&mut file), b"abcdef");
    assert_eq!(second.verified_sha256, Some(digest(b"abcdef")));
    assert_eq!(
        stage_segment(&source(), &model, &mut file, Some(&second.checkpoint), 1)
            .unwrap()
            .received_bytes,
        0
    );
}

#[test]
fn changed_or_corrupt_partials_fail_before_any_source_request() {
    let mut file = tempfile::tempfile().unwrap();
    let model = expected(b"abcdef");
    let first = run(
        response(206, "Content-Range: bytes 0-2/6\r\n", b"abc"),
        &model,
        &mut file,
        None,
        3,
    )
    .unwrap();
    assert_eq!(
        stage_segment(&source(), &model, &mut file, None, 3)
            .unwrap_err()
            .code,
        "invalid_checkpoint"
    );
    for field in 0..4 {
        let mut checkpoint = first.checkpoint.clone();
        match field {
            0 => checkpoint.schema_version = 2,
            1 => checkpoint.identity = "changed".into(),
            2 => checkpoint.bytes = 2,
            _ => checkpoint.prefix_sha256 = "changed".into(),
        }
        assert_eq!(
            stage_segment(&source(), &model, &mut file, Some(&checkpoint), 3)
                .unwrap_err()
                .code,
            "invalid_checkpoint"
        );
    }
    let mut changed = source();
    changed.resolved_revision = "b".repeat(40);
    assert_eq!(
        stage_segment(&changed, &model, &mut file, Some(&first.checkpoint), 3)
            .unwrap_err()
            .code,
        "invalid_checkpoint"
    );
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"bad").unwrap();
    assert_eq!(
        stage_segment(&source(), &model, &mut file, Some(&first.checkpoint), 3)
            .unwrap_err()
            .code,
        "invalid_checkpoint"
    );
    file.write_all(b"too long").unwrap();
    assert_eq!(
        stage_segment(&source(), &model, &mut file, Some(&first.checkpoint), 3)
            .unwrap_err()
            .code,
        "invalid_checkpoint"
    );
}

#[test]
fn invalid_range_headers_never_append_to_existing_data() {
    for (reply, code) in [
        (response(200, "", b"abcdef"), "range_not_honored"),
        (response(206, "", b"def"), "invalid_range"),
        (
            response(206, "Content-Range: bytes 0-2/6\r\n", b"def"),
            "invalid_range",
        ),
        (
            response(206, "Content-Range: bytes 3-5/7\r\n", b"def"),
            "invalid_range",
        ),
        (
            response(206, "Content-Range: bytes 3-5/*\r\n", b"def"),
            "invalid_range",
        ),
        (
            response(206, "Content-Range: bytes 3-5/6\r\n", b"de"),
            "invalid_range",
        ),
        (response(416, "", b""), "range_not_honored"),
    ] {
        let mut file = tempfile::tempfile().unwrap();
        let model = expected(b"abcdef");
        let first = run(
            response(206, "Content-Range: bytes 0-2/6\r\n", b"abc"),
            &model,
            &mut file,
            None,
            3,
        )
        .unwrap();
        assert_eq!(
            run(reply, &model, &mut file, Some(&first.checkpoint), 3)
                .unwrap_err()
                .code,
            code
        );
        assert_eq!(bytes(&mut file), b"abc");
    }
}

#[test]
fn interrupted_overlong_and_wrong_hash_bodies_roll_back() {
    for (reply, code) in [
        (b"HTTP/1.1 206 OK\r\nContent-Range: bytes 0-5/6\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabc".to_vec(), "source_read_failed"),
        (b"HTTP/1.1 206 OK\r\nContent-Range: bytes 0-5/6\r\nConnection: close\r\n\r\nabc".to_vec(), "size_mismatch"),
        (b"HTTP/1.1 206 OK\r\nContent-Range: bytes 0-5/6\r\nConnection: close\r\n\r\nabcdefg".to_vec(), "size_mismatch"),
        (response(200, "", b"abcdeg"), "hash_mismatch"),
    ] {
        let mut file = tempfile::tempfile().unwrap();
        assert_eq!(run(reply, &expected(b"abcdef"), &mut file, None, 6).unwrap_err().code, code);
        assert!(bytes(&mut file).is_empty());
    }
    let mut file = tempfile::tempfile().unwrap();
    assert!(
        run(
            response(200, "", b"abcdef"),
            &expected(b"abcdef"),
            &mut file,
            None,
            6
        )
        .unwrap()
        .verified_sha256
        .is_some()
    );
}

#[test]
fn zero_length_git_files_and_bad_input_do_not_need_a_network_request() {
    let mut file = tempfile::tempfile().unwrap();
    let mut model = expected(b"");
    model.upstream_hash = Some(UpstreamHash {
        algorithm: HashAlgorithm::GitSha1,
        value: "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391".into(),
    });
    assert!(
        stage_segment(&source(), &model, &mut file, None, 1)
            .unwrap()
            .verified_sha256
            .is_some()
    );
    assert_eq!(
        stage_segment(&source(), &model, &mut file, None, 0)
            .unwrap_err()
            .code,
        "invalid_limit"
    );
    for field in 0..4 {
        let mut source = source();
        match field {
            0 => source.private = true,
            1 => source.gated = true,
            2 => source.endpoint = "http://evil".into(),
            _ => source.repo_type = "dataset".into(),
        }
        assert_eq!(
            stage_segment(&source, &model, &mut file, None, 1)
                .unwrap_err()
                .code,
            "unsupported_source"
        );
    }
    model.upstream_hash = None;
    assert_eq!(
        stage_segment(&source(), &model, &mut file, None, 1)
            .unwrap_err()
            .code,
        "missing_hash"
    );
}
