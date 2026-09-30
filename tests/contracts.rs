use modelprepper::config::{Volume, Watch};
use modelprepper::inventory::{EvidenceStatus, HashAlgorithm, Role, SourceFile, UpstreamHash};
use modelprepper::plan::Blocker;
use modelprepper::validation::{
    normalize_repo, unique_paths, validate_file_path, validate_hex, validate_revision,
};
use modelprepper::{Config, Inventory, build_plan};

fn config() -> Config {
    Config::parse(include_str!("../examples/config.toml")).unwrap()
}

#[test]
fn version_one_fixture_has_a_stable_identity() {
    let plan = build_plan(&config(), &inventory(), 500_000_000_000).unwrap();
    assert_eq!(
        plan.plan_id,
        "1cf5f58c1b7819a7f5104edbdf7de51168ca25ca37797e8e48e1f4a9a14d2795"
    );
    assert_eq!(
        plan.policy_digest,
        "3a30ff35811c93095815ea8a4a21fcda725b5c0f9201bf2d33d968d2145a60dc"
    );
}
fn inventory() -> Inventory {
    Inventory::parse(include_str!("fixtures/inventory.json")).unwrap()
}

#[test]
fn portable_paths_reject_cross_platform_escapes_and_collisions() {
    for path in [
        "model.safetensors",
        "nested/config.json",
        "model-00001-of-00002.safetensors",
        "a/my file.txt",
        "COM0.json",
        "complex.tar.md",
    ] {
        validate_file_path(path).unwrap();
    }
    for path in [
        "",
        "/a",
        "a//b",
        "../a",
        "a/./b",
        "a/../b",
        "a\\b",
        "C:/a",
        "a:stream",
        "a.",
        "a ",
        "a\nb",
        "a\u{7f}",
        "a?b",
        "a*b",
        "<a>",
        "a|b",
        "\"a\"",
        "模型.json",
        "CON",
        "con.txt",
        "CON .txt",
        "PRN",
        "AUX",
        "NUL",
        "COM1.txt",
        "COM9",
        "LPT1",
        "LPT9.json",
    ] {
        assert!(validate_file_path(path).is_err(), "{path:?}");
    }
    assert!(validate_file_path(&"a".repeat(241)).is_err());
    unique_paths(["a/config.json", "b/config.json"]).unwrap();
    for paths in [
        vec!["a", "A"],
        vec!["file", "file/sub"],
        vec!["a/b", "a"],
        vec!["good", "../bad"],
    ] {
        assert!(unique_paths(paths).is_err());
    }
}

#[test]
fn repository_and_revision_references_are_bounded_and_official() {
    for repo in [
        "owner/model",
        "https://huggingface.co/owner/model/",
        "owner/model.name",
    ] {
        assert!(normalize_repo(repo).is_ok());
    }
    assert_eq!(
        normalize_repo("https://huggingface.co/owner/model").unwrap(),
        "owner/model"
    );
    for repo in [
        "",
        "model",
        "/model",
        "owner/",
        "a/b/c",
        "https://evil.example/a/b",
        "http://huggingface.co/a/b",
        "a/.b",
        "a/b.",
        "a/b..c",
        "a/b?token=secret",
        "a/b#fragment",
        "a/b%2fc",
        "a/模型",
    ] {
        assert!(normalize_repo(repo).is_err(), "{repo:?}");
    }
    assert!(normalize_repo(&format!("{}/b", "a".repeat(97))).is_err());
    for revision in ["main", "v1.0", "refs/pr/1", "0123456789abcdef"] {
        validate_revision(revision).unwrap();
    }
    for revision in ["", "../main", "/main", "a//b", ".", "a/.", "a\nb", "a?b"] {
        assert!(validate_revision(revision).is_err());
    }
    assert!(validate_revision(&"a".repeat(129)).is_err());
    validate_hex("0123456789abcdef", 16).unwrap();
    assert!(validate_hex("ABCDEF", 6).is_err());
    assert!(validate_hex("01234x", 6).is_err());
    assert!(validate_hex("123", 6).is_err());
}

#[test]
fn strict_parsers_reject_unknown_fields_and_bad_types_without_echoing_secret_values() {
    let text = include_str!("../examples/config.toml");
    for invalid in [
        format!("token = \"do-not-print-this\"\n{text}"),
        text.replace(
            "max_stored_bytes = 500_000_000_000",
            "max_stored_bytes = -1",
        ),
        text.replace("include_gguf = false", "include_gguf = false\nextra = 1"),
    ] {
        let error = Config::parse(&invalid).unwrap_err();
        assert_eq!(error.code, "invalid_config");
        assert!(!error.message.contains("do-not-print-this"));
    }
    assert!(Inventory::parse("{}").is_err());
    let json = include_str!("fixtures/inventory.json").replace(
        "\"schema_version\": 1,",
        "\"unexpected\": true, \"schema_version\": 1,",
    );
    assert!(Inventory::parse(&json).is_err());
}

#[test]
fn config_validation_explains_bad_limits_and_unsupported_policies() {
    let cases: Vec<fn(&mut Config)> = vec![
        |c| c.schema_version = 2,
        |c| c.catalog_path = "".into(),
        |c| c.licenses.allow.clear(),
        |c| c.licenses.allow = vec!["other".into()],
        |c| c.licenses.allow = vec!["mit".into(), "mit".into()],
        |c| c.storage.max_stored_bytes = 0,
        |c| c.transfer.max_transfer_bytes_per_period = 0,
        |c| c.transfer.max_parallel_files = 0,
        |c| c.transfer.max_parallel_files = 9,
        |c| c.transfer.period = "local-week".into(),
        |c| c.approval.mode = "auto".into(),
        |c| c.files.profile = "all-files".into(),
        |c| c.files.include_repo_code = true,
        |c| c.files.include_gguf = true,
        |c| c.volumes[0].label.clear(),
        |c| c.volumes[0].label = "unsafe\nlabel".into(),
        |c| c.volumes[0].path = "".into(),
        |c| {
            c.volumes.push(Volume {
                label: "primary".into(),
                path: "other".into(),
            })
        },
        |c| {
            c.volumes.push(Volume {
                label: "second".into(),
                path: "models".into(),
            })
        },
        |c| c.volumes.clear(),
        |c| c.storage.default_volume = "missing".into(),
        |c| c.watches[0].source = "mirror".into(),
        |c| c.watches[0].repo_type = "dataset".into(),
        |c| c.watches[0].update_policy = "replace".into(),
        |c| c.watches[0].repo_id = "../model".into(),
        |c| c.watches[0].revision = "../main".into(),
        |c| c.watches.push(c.watches[0].clone()),
    ];
    for mutate in cases {
        let mut value = config();
        mutate(&mut value);
        assert!(value.validate().is_err());
        assert!(value.canonical_bytes().is_err());
    }
    let defaults = Config::default_for("catalog".into(), "models".into());
    defaults.validate().unwrap();
    Config::parse(&toml::to_string(&defaults).unwrap()).unwrap();
}

#[test]
fn inventory_validates_identity_evidence_hash_algorithms_and_all_paths() {
    let cases: Vec<fn(&mut Inventory)> = vec![
        |i| i.schema_version = 2,
        |i| i.source.endpoint = "https://mirror.example".into(),
        |i| i.source.repo_type = "dataset".into(),
        |i| i.source.repo_id = "../model".into(),
        |i| i.source.requested_revision = "../main".into(),
        |i| i.source.resolved_revision = "main".into(),
        |i| i.files[0].path = "../model".into(),
        |i| i.files[0].upstream_hash.as_mut().unwrap().value = "bad".into(),
        |i| i.files[1].upstream_hash.as_mut().unwrap().value = "bad".into(),
        |i| i.files.push(i.files[0].clone()),
        |i| i.license.paths = vec!["not-found".into()],
        |i| i.license.paths = vec!["config.json".into()],
        |i| i.license.paths = vec!["LICENSE".into(), "license".into()],
    ];
    for mutate in cases {
        let mut value = inventory();
        mutate(&mut value);
        assert!(value.validate().is_err());
        assert!(build_plan(&config(), &value, u64::MAX).is_err());
    }
    let mut notice = inventory();
    notice.files[3].role = Role::Notice;
    notice.validate().unwrap();
}

#[test]
fn preview_is_never_authorized_even_when_local_input_claims_matched_rights() {
    let mut inv = inventory();
    inv.license.status = EvidenceStatus::Matched;
    let plan = build_plan(&config(), &inv, u64::MAX).unwrap();
    assert_eq!(plan.payload_bytes, 5504);
    assert_eq!(plan.excluded_files, ["modeling.py"]);
    assert_eq!(plan.blockers, [Blocker::LocalInventoryUntrusted]);
    assert!(!plan.transfer_authorized);
    assert_eq!(plan.mode, "offline_preview");
    assert_eq!(plan.workspace_bytes, None);
    assert_eq!(plan.source.repo_id, "example/model");
}

#[test]
fn blockers_explain_missing_evidence_access_layout_roles_and_capacity() {
    let mut inv = inventory();
    inv.complete = false;
    inv.layout = "custom".into();
    inv.source.gated = true;
    inv.source.private = true;
    inv.license.license_id = "unknown".into();
    inv.files[0].upstream_hash = None;
    inv.files[4].required = true;
    inv.files.retain(|f| f.role != Role::Tokenizer);
    let mut cfg = config();
    cfg.storage.max_stored_bytes = 1;
    cfg.transfer.max_transfer_bytes_per_period = 1;
    let plan = build_plan(&cfg, &inv, 0).unwrap();
    for blocker in [
        Blocker::IncompleteInventory,
        Blocker::UnsupportedLayout,
        Blocker::AccessRestricted,
        Blocker::LicenseNeedsReview,
        Blocker::LicenseNotAllowed,
        Blocker::MissingRequiredRole,
        Blocker::RequiredFileExcluded,
        Blocker::MissingUpstreamHash,
        Blocker::RevisionTooLarge,
        Blocker::TransferBudgetExceeded,
        Blocker::InsufficientSpace,
    ] {
        assert!(plan.blockers.contains(&blocker), "{blocker:?}");
    }
    inv.source.gated = false;
    inv.source.private = true;
    inv.license.status = EvidenceStatus::Matched;
    inv.license.paths.clear();
    assert!(
        build_plan(&config(), &inv, u64::MAX)
            .unwrap()
            .blockers
            .contains(&Blocker::LicenseNeedsReview)
    );
    inv.license.status = EvidenceStatus::Conflicting;
    assert!(
        build_plan(&config(), &inv, u64::MAX)
            .unwrap()
            .blockers
            .contains(&Blocker::AccessRestricted)
    );
}

#[test]
fn byte_arithmetic_never_wraps_and_boundaries_are_inclusive() {
    let mut inv = inventory();
    inv.files[0].size_bytes = u64::MAX;
    assert_eq!(
        build_plan(&config(), &inv, u64::MAX).unwrap_err().code,
        "size_overflow"
    );
    inv.files.retain(|f| f.role == Role::Weight);
    inv.license.paths.clear();
    assert_eq!(
        build_plan(&config(), &inv, u64::MAX).unwrap_err().code,
        "size_overflow"
    );
    let inv = inventory();
    let mut cfg = config();
    cfg.storage.max_stored_bytes = 5504;
    cfg.transfer.max_transfer_bytes_per_period = 5504;
    cfg.storage.min_free_bytes = 1;
    let plan = build_plan(&cfg, &inv, 5505).unwrap();
    assert!(!plan.blockers.contains(&Blocker::InsufficientSpace));
    assert!(!plan.blockers.contains(&Blocker::RevisionTooLarge));
    assert!(!plan.blockers.contains(&Blocker::TransferBudgetExceeded));
    assert!(
        build_plan(&cfg, &inv, 5504)
            .unwrap()
            .blockers
            .contains(&Blocker::InsufficientSpace)
    );
}

#[test]
fn canonical_identity_ignores_set_order_and_space_observation_but_binds_content_and_policy() {
    let mut cfg = config();
    cfg.volumes.push(Volume {
        label: "backup".into(),
        path: "backup".into(),
    });
    cfg.watches.push(Watch {
        repo_id: "example/second".into(),
        ..cfg.watches[0].clone()
    });
    let mut inv = inventory();
    inv.files.push(SourceFile {
        path: "NOTICE".into(),
        size_bytes: 0,
        role: Role::Notice,
        required: true,
        upstream_hash: Some(UpstreamHash {
            algorithm: HashAlgorithm::GitSha1,
            value: "a".repeat(40),
        }),
    });
    inv.license.paths.push("NOTICE".into());
    let original = build_plan(&cfg, &inv, u64::MAX).unwrap();
    cfg.licenses.allow.reverse();
    cfg.volumes.reverse();
    cfg.watches.reverse();
    inv.files.reverse();
    inv.license.paths.reverse();
    inv.source.repo_id = "https://huggingface.co/example/model".into();
    cfg.watches[0].repo_id = format!("https://huggingface.co/{}", cfg.watches[0].repo_id);
    let equivalent = build_plan(&cfg, &inv, 0).unwrap();
    assert_eq!(original.plan_id, equivalent.plan_id);
    assert_eq!(original.policy_digest, equivalent.policy_digest);
    for mutate in [
        (|i: &mut Inventory| i.source.resolved_revision = "b".repeat(40)) as fn(&mut Inventory),
        |i| i.files[0].size_bytes += 1,
        |i| i.license.status = EvidenceStatus::Conflicting,
        |i| i.files[0].upstream_hash.as_mut().unwrap().value = "c".repeat(40),
        |i| i.files[0].required = false,
    ] {
        let mut changed = inv.clone();
        mutate(&mut changed);
        assert_ne!(
            original.plan_id,
            build_plan(&cfg, &changed, u64::MAX).unwrap().plan_id
        );
    }
    cfg.volumes[0].path = "relocated".into();
    assert_ne!(
        original.plan_id,
        build_plan(&cfg, &inv, u64::MAX).unwrap().plan_id
    );
}

#[test]
fn selection_is_role_aware_and_never_admits_repository_code_or_alternate_weight_formats() {
    let mut inv = inventory();
    for (path, role) in [
        ("weights.bin", Role::Weight),
        ("weights.gguf", Role::Weight),
        ("config.py", Role::Config),
        ("unrelated.exe", Role::Documentation),
        ("unknown.json", Role::Other),
        ("README.md", Role::Documentation),
        ("chat.jinja", Role::Template),
        ("vocab.txt", Role::Tokenizer),
        ("sp.model", Role::Tokenizer),
        ("NOTICE.txt", Role::Notice),
        ("licenses/rights.md", Role::License),
        ("random.json", Role::Weight),
        ("invalid.md", Role::Config),
        ("tokenizer.bin", Role::Tokenizer),
        ("template.txt", Role::Template),
        ("rights.json", Role::License),
        ("LICENSE", Role::License),
    ] {
        if path == "LICENSE" {
            continue;
        }
        inv.files.push(SourceFile {
            path: path.into(),
            size_bytes: 1,
            upstream_hash: None,
            role,
            required: true,
        });
    }
    let plan = build_plan(&config(), &inv, u64::MAX).unwrap();
    for path in [
        "README.md",
        "chat.jinja",
        "vocab.txt",
        "sp.model",
        "NOTICE.txt",
        "licenses/rights.md",
    ] {
        assert!(plan.selected_files.iter().any(|file| file.path == path));
    }
    for path in [
        "weights.bin",
        "weights.gguf",
        "config.py",
        "unrelated.exe",
        "unknown.json",
        "random.json",
        "invalid.md",
        "tokenizer.bin",
        "template.txt",
        "rights.json",
    ] {
        assert!(plan.excluded_files.iter().any(|file| file == path));
    }
    assert!(plan.blockers.contains(&Blocker::RequiredFileExcluded));
    assert!(plan.blockers.contains(&Blocker::MissingUpstreamHash));
}
