use modelprepper::{
    integrity::verify_reader,
    inventory::{HashAlgorithm, Role, SourceFile, UpstreamHash},
    licenses,
};
use sha1::{Digest, Sha1};
use sha2::Sha256;

fn file(bytes: &[u8], algorithm: HashAlgorithm) -> SourceFile {
    let value = match algorithm {
        HashAlgorithm::Sha256 => format!("{:x}", Sha256::digest(bytes)),
        HashAlgorithm::GitSha1 => {
            let mut hash = Sha1::new();
            hash.update(format!("blob {}\0", bytes.len()));
            hash.update(bytes);
            format!("{:x}", hash.finalize())
        }
    };
    SourceFile {
        path: "LICENSE".into(),
        size_bytes: bytes.len() as u64,
        role: Role::License,
        required: true,
        upstream_hash: Some(UpstreamHash { algorithm, value }),
    }
}

#[test]
fn verifies_git_objects_lfs_and_empty_files_with_a_local_sha256() {
    for bytes in [b"".as_slice(), b"hello\n", &[42; 150_000]] {
        for algorithm in [HashAlgorithm::GitSha1, HashAlgorithm::Sha256] {
            let expected = file(bytes, algorithm);
            assert_eq!(
                verify_reader(bytes, &expected).unwrap(),
                format!("{:x}", Sha256::digest(bytes))
            );
        }
    }
    // Known Git blob vector; raw SHA-1 of the file is different.
    assert_eq!(
        file(b"hello\n", HashAlgorithm::GitSha1)
            .upstream_hash
            .unwrap()
            .value,
        "ce013625030ba8dba906f756967f9e9ca394464a"
    );
}

#[test]
fn fails_on_truncation_growth_corruption_missing_identity_and_read_errors() {
    let expected = file(b"abc", HashAlgorithm::Sha256);
    for (bytes, code) in [
        (b"ab".as_slice(), "size_mismatch"),
        (b"abcd", "size_mismatch"),
        (b"abd", "hash_mismatch"),
    ] {
        assert_eq!(verify_reader(bytes, &expected).unwrap_err().code, code);
    }
    let mut missing = expected.clone();
    missing.upstream_hash = None;
    assert_eq!(
        verify_reader(b"abc".as_slice(), &missing).unwrap_err().code,
        "missing_hash"
    );
    struct Broken;
    impl std::io::Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("fixture"))
        }
    }
    assert_eq!(
        verify_reader(Broken, &expected).unwrap_err().code,
        "read_failed"
    );
}

#[test]
fn license_matching_requires_complete_unchanged_terms() {
    for (text, identity) in [
        (
            licenses::MIT,
            "c963879647034d6c5d7027d8e2b024213589b749d11ba7320032802307bced9c",
        ),
        (
            licenses::APACHE,
            "0ffddef9e48f8a09aed5caf2d44f7ba1c1be2d9b8e0a6f693b1635b2d5566645",
        ),
    ] {
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(modelprepper::plan::digest(normalized.as_bytes()), identity);
    }
    assert!(licenses::matches("mit", licenses::MIT));
    assert!(licenses::matches(
        "mit",
        &licenses::MIT.replace("<year> <copyright holders>", "2026 Example")
    ));
    assert!(licenses::matches(
        "mit",
        &licenses::MIT.replace("MIT License\n", "")
    ));
    assert!(licenses::matches("apache-2.0", licenses::APACHE));
    assert!(licenses::matches(
        "apache-2.0",
        &licenses::APACHE.replace("[yyyy] [name of copyright owner]", "2024 Alibaba Cloud")
    ));
    for replacement in [
        "Copyright 2024 Holder\nNo commercial use.",
        "Copyright 20xx Holder",
        "2024",
        "2024 ",
    ] {
        assert!(!licenses::matches(
            "apache-2.0",
            &licenses::APACHE.replace("Copyright [yyyy] [name of copyright owner]", replacement)
        ));
    }
    assert!(licenses::matches(
        "apache-2.0",
        &licenses::APACHE.replace('\n', "\r\n")
    ));
    for text in [
        "MIT License",
        "Permission is hereby granted",
        &licenses::MIT.replace("Copyright (c)", "Copyright"),
        &format!("{}\nNo commercial use.", licenses::MIT),
        &format!("Restricted to research.\n{}", licenses::MIT),
        &licenses::MIT.replace("MIT License", "Unknown License"),
    ] {
        assert!(!licenses::matches("mit", text), "{text}");
    }
    assert!(!licenses::matches(
        "apache-2.0",
        "Apache License Version 2.0"
    ));
    assert!(!licenses::matches(
        "apache-2.0",
        &format!("{}\nExtra restrictions.", licenses::APACHE)
    ));
    assert!(!licenses::matches("unknown", licenses::MIT));
}
