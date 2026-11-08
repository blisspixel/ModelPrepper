use super::*;
use crate::{
    fixture::{Server, response},
    inventory::{HashAlgorithm, Role, Source, SourceFile, UpstreamHash},
    plan::digest,
};
use std::{fs, io::Write, path::Path};

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

fn file(path: &str, bytes: &[u8], role: Role) -> SourceFile {
    SourceFile {
        path: path.into(),
        size_bytes: bytes.len() as u64,
        upstream_hash: Some(UpstreamHash {
            algorithm: HashAlgorithm::Sha256,
            value: digest(bytes),
        }),
        role,
        required: true,
    }
}

fn root() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(temp.path()).unwrap().join("revision");
    (temp, path)
}

fn stage(session: &mut RevisionStage, body: &[u8], total: u64, start: u64) -> RevisionSegment {
    let headers = format!(
        "Content-Range: bytes {start}-{}/{total}\r\n",
        start + body.len() as u64 - 1
    );
    let server = Server::new(vec![response(206, &headers, body)]);
    let segment = session
        .stage_next_with(
            &Http::fixture(&server.origin),
            &server.origin,
            body.len() as u64,
        )
        .unwrap();
    let request = server.finish().remove(0).to_ascii_lowercase();
    assert!(request.contains(&format!("bytes={start}-")));
    assert!(!request.contains("accept-encoding"));
    assert!(request.contains(&segment.path.to_ascii_lowercase()));
    segment
}

fn part(root: &Path, index: usize) -> Vec<u8> {
    fs::read(
        root.join("members")
            .join(format!("{index:04}"))
            .join("payload.part"),
    )
    .unwrap()
}

#[test]
fn a_frozen_set_stages_license_and_notice_without_sealing() {
    let (_temp, path) = root();
    let files = vec![
        file("model.safetensors", b"abcdef", Role::Weight),
        file("LICENSE", b"mit", Role::License),
        file("NOTICE", b"note", Role::Notice),
    ];
    let mut session = RevisionStage::create(&path, &source(), &files).unwrap();
    assert!(!session.is_staged().unwrap());
    assert_eq!(
        RevisionStage::open(&path, &source(), &files)
            .err()
            .unwrap()
            .code,
        "writer_busy"
    );
    let license = stage(&mut session, b"mit", 3, 0);
    assert_eq!(license.path, "LICENSE");
    assert!(license.segment.verified_sha256.is_some());
    assert!(!license.staged);
    let notice = stage(&mut session, b"note", 4, 0);
    assert_eq!(notice.path, "NOTICE");
    assert!(notice.segment.verified_sha256.is_some());
    let first = stage(&mut session, b"abc", 6, 0);
    assert_eq!(first.path, "model.safetensors");
    assert!(first.segment.verified_sha256.is_none());
    assert_eq!(session.spent_bytes().unwrap(), 3 + 4 + 3);
    {
        let mut tail = fs::OpenOptions::new()
            .append(true)
            .open(path.join("members/0002/payload.part"))
            .unwrap();
        tail.write_all(b"ZZ").unwrap();
    }
    drop(session);
    let mut session = RevisionStage::open(&path, &source(), &files).unwrap();
    assert_eq!(session.recovered_bytes().unwrap(), 2);
    assert_eq!(part(&path, 2), b"abc");
    assert_eq!(session.spent_bytes().unwrap(), 10);
    let finished = stage(&mut session, b"def", 6, 3);
    assert!(finished.segment.verified_sha256.is_some());
    assert!(finished.staged);
    assert!(session.is_staged().unwrap());
    assert_eq!(part(&path, 0), b"mit");
    assert_eq!(part(&path, 1), b"note");
    assert_eq!(part(&path, 2), b"abcdef");
    assert!(path.join("members/0002/payload.part").is_file());
    assert!(!path.join("seal").exists());
    assert_eq!(
        session.stage_next(1).err().unwrap().code,
        "revision_already_staged"
    );
    let names: Vec<_> = fs::read_dir(&path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names.len(), 3);
}

#[test]
fn an_ignored_range_does_not_append_inside_the_set() {
    let (_temp, path) = root();
    let files = vec![file("model.safetensors", b"abcdef", Role::Weight)];
    let mut session = RevisionStage::create(&path, &source(), &files).unwrap();
    let server = Server::new(vec![response(200, "", b"abc")]);
    let error = session
        .stage_next_with(&Http::fixture(&server.origin), &server.origin, 3)
        .err()
        .unwrap();
    assert_eq!(error.code, "range_not_honored");
    assert!(part(&path, 0).is_empty());
    server.finish();
}

#[test]
fn the_set_rejects_an_empty_duplicate_or_changed_membership() {
    let (_temp, path) = root();
    assert_eq!(
        RevisionStage::create(&path, &source(), &[])
            .err()
            .unwrap()
            .code,
        "empty_revision"
    );
    let license = file("LICENSE", b"mit", Role::License);
    let duplicate = vec![file("LICENSE", b"mit", Role::License), license.clone()];
    assert_eq!(
        RevisionStage::create(&path, &source(), &duplicate)
            .err()
            .unwrap()
            .code,
        "duplicate_path"
    );
    let mut many = Vec::new();
    for index in 0..1025 {
        many.push(file(&format!("f{index}.txt"), b"x", Role::Other));
    }
    assert_eq!(
        RevisionStage::create(&path, &source(), &many)
            .err()
            .unwrap()
            .code,
        "too_many_files"
    );
    let files = vec![
        file("LICENSE", b"mit", Role::License),
        file("NOTICE", b"note", Role::Notice),
    ];
    let session = RevisionStage::create(&path, &source(), &files).unwrap();
    drop(session);
    fs::write(path.join("note.txt"), b"extra").unwrap();
    assert_eq!(
        RevisionStage::open(&path, &source(), &files)
            .err()
            .unwrap()
            .code,
        "staging_directory_not_owned"
    );
    fs::remove_file(path.join("note.txt")).unwrap();
    assert_eq!(
        RevisionStage::open(&path, &source(), &[files[0].clone()])
            .err()
            .unwrap()
            .code,
        "revision_set_changed"
    );
}

#[test]
fn the_set_rejects_a_bad_path_source_allowance_or_manifest() {
    let (_temp, path) = root();
    let files = vec![file("LICENSE", b"mit", Role::License)];
    assert_eq!(
        RevisionStage::create(Path::new("relative"), &source(), &files)
            .err()
            .unwrap()
            .code,
        "unsafe_staging_path"
    );
    let mut gated = source();
    gated.gated = true;
    assert_eq!(
        RevisionStage::create(&path, &gated, &files)
            .err()
            .unwrap()
            .code,
        "unsupported_source"
    );
    assert!(!path.exists());
    let mut session = RevisionStage::create(&path, &source(), &files).unwrap();
    assert_eq!(session.stage_next(0).err().unwrap().code, "invalid_limit");
    drop(session);

    fs::write(path.join("members.json"), b"{").unwrap();
    assert_eq!(
        RevisionStage::open(&path, &source(), &files)
            .err()
            .unwrap()
            .code,
        "invalid_revision_stage"
    );

    let (_temp, path) = root();
    let session = RevisionStage::create(&path, &source(), &files).unwrap();
    drop(session);
    fs::write(path.join("members.json"), vec![b' '; (1024 * 1024) + 1]).unwrap();
    assert_eq!(
        RevisionStage::open(&path, &source(), &files)
            .err()
            .unwrap()
            .code,
        "invalid_revision_stage"
    );

    let (_temp, path) = root();
    let session = RevisionStage::create(&path, &source(), &files).unwrap();
    drop(session);
    fs::write(path.join("members/note"), b"x").unwrap();
    assert_eq!(
        RevisionStage::open(&path, &source(), &files)
            .err()
            .unwrap()
            .code,
        "invalid_revision_stage"
    );

    let (_temp, path) = root();
    let session = RevisionStage::create(&path, &source(), &files).unwrap();
    drop(session);
    fs::remove_dir_all(path.join("members")).unwrap();
    fs::write(path.join("members"), b"x").unwrap();
    assert_eq!(
        RevisionStage::open(&path, &source(), &files)
            .err()
            .unwrap()
            .code,
        "invalid_revision_stage"
    );
}
