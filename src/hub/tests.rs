use super::*;
use crate::fixture::{Server, response};
use serde_json::json;
use sha1::{Digest, Sha1};

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn sibling(path: &str, bytes: &[u8]) -> Value {
    let mut git = Sha1::new();
    git.update(format!("blob {}\0", bytes.len()));
    git.update(bytes);
    json!({"rfilename": path, "size": bytes.len(), "blobId": format!("{:x}", git.finalize())})
}

fn document() -> (Value, Vec<u8>) {
    let config = br#"{"model_type":"fixture"}"#.to_vec();
    let mut weight = sibling("model.safetensors", b"weights");
    weight["lfs"] = json!({"sha256": digest(b"weights"), "size": 7});
    let meta = json!({"id": "owner/model", "sha": COMMIT, "private": false, "gated": false, "cardData": {"license": "mit"}, "siblings": [sibling("LICENSE", licenses::MIT.as_bytes()), sibling("config.json", &config), sibling("tokenizer.json", b"{}"), weight]});
    (meta, config)
}

fn inspect(meta: &Value, bodies: Vec<Vec<u8>>) -> Result<Resolution> {
    inspect_pair(meta, meta, bodies)
}

fn inspect_pair(moving: &Value, pinned: &Value, bodies: Vec<Vec<u8>>) -> Result<Resolution> {
    let mut replies = vec![
        response(200, "", &serde_json::to_vec(moving).unwrap()),
        response(200, "", &serde_json::to_vec(pinned).unwrap()),
    ];
    replies.extend(bodies.into_iter().map(|body| response(200, "", &body)));
    let server = Server::new(replies);
    let result = resolve_with(
        &Http::fixture(&server.origin),
        &server.origin,
        "owner/model",
        "main",
    );
    let requests = server.finish();
    assert!(requests[0].starts_with("GET /api/models/owner/model/revision/main?blobs=true "));
    assert!(requests[1].contains(&format!("/revision/{COMMIT}?blobs=true")));
    for request in &requests[2..] {
        assert!(request.contains(&format!("/resolve/{COMMIT}/")));
    }
    result
}

#[test]
fn public_resolution_pins_and_checks_evidence_without_downloading_weights() {
    let (mut meta, config) = document();
    meta["siblings"].as_array_mut().unwrap().extend([
        sibling("README.md", b"readme"),
        sibling("modeling.py", b"code"),
        sibling("onnx/model.onnx", b"alternative"),
    ]);
    let resolved = inspect(&meta, vec![licenses::MIT.as_bytes().to_vec(), config]).unwrap();
    assert_eq!(resolved.inventory.source.resolved_revision, COMMIT);
    assert_eq!(resolved.inventory.layout, "transformers-safetensors-v1");
    assert_eq!(resolved.inventory.license.status, EvidenceStatus::Matched);
    assert_eq!(
        resolved.evidence[0].sha256,
        digest(licenses::MIT.as_bytes())
    );
    assert!(!resolved.transfer_authorized);
    let config = crate::Config::default_for("catalog".into(), "models".into());
    let preview = crate::build_plan(&config, &resolved.inventory, u64::MAX).unwrap();
    assert_eq!(
        preview.blockers,
        vec![crate::plan::Blocker::LocalInventoryUntrusted]
    );
    assert!(preview.excluded_files.contains(&"onnx/model.onnx".into()));
}

#[test]
fn rejects_changed_pin_missing_identities_and_contradictory_lfs_sizes() {
    let (meta, _) = document();
    let mut changed = meta.clone();
    changed["sha"] = json!("a".repeat(40));
    assert_eq!(
        inspect_pair(&meta, &changed, vec![]).unwrap_err().code,
        "source_identity_changed"
    );
    for (field, value, code) in [
        ("blobId", json!("bad"), "invalid_hash"),
        ("rfilename", json!("../escape"), "unsafe_path"),
    ] {
        let mut changed = meta.clone();
        changed["siblings"][0][field] = value;
        assert_eq!(inspect(&changed, vec![]).unwrap_err().code, code);
    }
    let mut changed = meta.clone();
    changed["siblings"][3]["lfs"]["size"] = json!(8);
    assert_eq!(
        inspect(&changed, vec![]).unwrap_err().code,
        "invalid_source_metadata"
    );
    changed["siblings"][3]["lfs"]["size"] = json!(7);
    changed["siblings"][3]["lfs"]["sha256"] = json!("bad");
    assert_eq!(inspect(&changed, vec![]).unwrap_err().code, "invalid_hash");
    let mut changed = meta.clone();
    changed["siblings"][1]["rfilename"] = json!("license");
    assert_eq!(
        inspect(&changed, vec![]).unwrap_err().code,
        "path_collision"
    );
}

#[test]
fn restricted_or_incomplete_api_responses_fail_before_file_fetches() {
    let (meta, _) = document();
    for (field, value, code) in [
        ("private", json!(true), "access_restricted"),
        ("gated", json!("auto"), "access_restricted"),
        ("gated", Value::Null, "access_restricted"),
        ("id", json!("other/model"), "source_identity_changed"),
        ("sha", json!("bad"), "invalid_hash"),
        (
            "siblings",
            json!([{"rfilename":"a"}]),
            "invalid_source_metadata",
        ),
    ] {
        let mut changed = meta.clone();
        changed[field] = value;
        let server = Server::new(vec![response(
            200,
            "",
            &serde_json::to_vec(&changed).unwrap(),
        )]);
        assert_eq!(
            resolve_with(
                &Http::fixture(&server.origin),
                &server.origin,
                "owner/model",
                "main"
            )
            .unwrap_err()
            .code,
            code
        );
        server.finish();
    }
    assert!(resolve("bad", "main").is_err());
    assert!(resolve("owner/model", "../bad").is_err());
    assert_eq!(endpoint("bad", &[]).unwrap_err().code, "invalid_source");
    assert_eq!(
        endpoint("mailto:a", &[]).unwrap_err().code,
        "invalid_source"
    );
}

#[test]
fn missing_changed_or_conflicting_licenses_never_match() {
    let (meta, config) = document();
    let mut missing = meta.clone();
    missing["siblings"].as_array_mut().unwrap().remove(0);
    let result = inspect(&missing, vec![config.clone()]).unwrap();
    assert_eq!(result.inventory.license.status, EvidenceStatus::Unreviewed);
    assert!(result.evidence.is_empty());
    let mut unknown = meta.clone();
    unknown["cardData"] = Value::Null;
    assert_eq!(
        inspect(
            &unknown,
            vec![licenses::MIT.as_bytes().to_vec(), config.clone()]
        )
        .unwrap()
        .inventory
        .license
        .status,
        EvidenceStatus::Conflicting
    );
    let restricted = format!("{}\nNo commercial use.", licenses::MIT).into_bytes();
    let mut changed = meta.clone();
    changed["siblings"][0] = sibling("LICENSE", &restricted);
    assert_eq!(
        inspect(&changed, vec![restricted, config.clone()])
            .unwrap()
            .inventory
            .license
            .status,
        EvidenceStatus::Conflicting
    );
    let mut notice = meta.clone();
    notice["siblings"]
        .as_array_mut()
        .unwrap()
        .push(sibling("NOTICE", b"notice"));
    assert_eq!(
        inspect(
            &notice,
            vec![
                licenses::MIT.as_bytes().to_vec(),
                b"notice".to_vec(),
                config.clone()
            ]
        )
        .unwrap()
        .inventory
        .license
        .status,
        EvidenceStatus::Conflicting
    );
    assert_eq!(
        inspect(&meta, vec![vec![b'a'; licenses::MIT.len()]])
            .unwrap_err()
            .code,
        "hash_mismatch"
    );
    let mut invalid = meta.clone();
    invalid["siblings"][0] = sibling("LICENSE", &[255]);
    assert_eq!(
        inspect(&invalid, vec![vec![255]]).unwrap_err().code,
        "invalid_license_encoding"
    );
    let mut huge = meta.clone();
    huge["siblings"][0]["size"] = json!(METADATA_LIMIT + 1);
    assert_eq!(
        inspect(&huge, vec![]).unwrap_err().code,
        "metadata_too_large"
    );
}

#[test]
fn unsupported_config_tokenizer_and_weight_choices_stay_unsupported() {
    for config in [
        b"null".to_vec(),
        br#"{"auto_map":{},"model_type":"fixture"}"#.to_vec(),
        b"{}".to_vec(),
    ] {
        let (mut meta, _) = document();
        meta["siblings"][1] = sibling("config.json", &config);
        assert_eq!(
            inspect(&meta, vec![licenses::MIT.as_bytes().to_vec(), config])
                .unwrap()
                .inventory
                .layout,
            "unsupported"
        );
    }
    let (meta, config) = document();
    for remove in [1, 2, 3] {
        let mut changed = meta.clone();
        changed["siblings"].as_array_mut().unwrap().remove(remove);
        let mut bodies = vec![licenses::MIT.as_bytes().to_vec()];
        if remove != 1 {
            bodies.push(config.clone());
        }
        assert_eq!(
            inspect(&changed, bodies).unwrap().inventory.layout,
            "unsupported"
        );
    }
    let mut invalid = meta.clone();
    invalid["siblings"][1] = sibling("config.json", b"not json");
    assert_eq!(
        inspect(
            &invalid,
            vec![licenses::MIT.as_bytes().to_vec(), b"not json".to_vec()]
        )
        .unwrap_err()
        .code,
        "invalid_layout"
    );
    let mut huge = meta.clone();
    huge["siblings"][1]["size"] = json!(METADATA_LIMIT + 1);
    assert_eq!(
        inspect(&huge, vec![licenses::MIT.as_bytes().to_vec()])
            .unwrap_err()
            .code,
        "metadata_too_large"
    );
}

#[test]
fn shard_indices_must_close_exactly_over_the_selected_weight_set() {
    for (index, supported, error) in [
        (
            json!({"weight_map":{"a":"model-00001-of-00002.safetensors","b":"model-00002-of-00002.safetensors"}}),
            true,
            false,
        ),
        (
            json!({"weight_map":{"a":"model-00001-of-00002.safetensors"}}),
            false,
            false,
        ),
        (
            json!({"weight_map":{"a":"../missing.safetensors"}}),
            false,
            false,
        ),
        (json!({"weight_map":{"a":3}}), false, false),
        (json!({"weight_map":{}}), false, false),
        (json!({}), false, false),
        (json!("not json"), false, true),
    ] {
        let (mut meta, config) = document();
        let bytes = if error {
            b"not json".to_vec()
        } else {
            serde_json::to_vec(&index).unwrap()
        };
        let siblings = meta["siblings"].as_array_mut().unwrap();
        siblings.remove(3);
        siblings.extend([
            sibling("model-00001-of-00002.safetensors", b"a"),
            sibling("model-00002-of-00002.safetensors", b"b"),
            sibling("model.safetensors.index.json", &bytes),
        ]);
        let result = inspect(
            &meta,
            vec![licenses::MIT.as_bytes().to_vec(), config, bytes],
        );
        if error {
            assert_eq!(result.unwrap_err().code, "invalid_layout");
        } else {
            assert_eq!(
                result.unwrap().inventory.layout == "transformers-safetensors-v1",
                supported
            );
        }
    }
}

#[test]
fn tokenizer_configuration_cannot_silently_require_repository_code() {
    for (bytes, supported, invalid) in [
        (b"{}".as_slice(), true, false),
        (b"null", false, false),
        (
            br#"{"auto_map":{"AutoTokenizer":"tokenizer.Custom"}}"#,
            false,
            false,
        ),
        (br#"{"trust_remote_code":true}"#, false, false),
        (b"bad json", false, true),
    ] {
        let (mut meta, config) = document();
        meta["siblings"]
            .as_array_mut()
            .unwrap()
            .push(sibling("tokenizer_config.json", bytes));
        let result = inspect(
            &meta,
            vec![licenses::MIT.as_bytes().to_vec(), config, bytes.to_vec()],
        );
        if invalid {
            assert_eq!(result.unwrap_err().code, "invalid_layout");
        } else {
            assert_eq!(
                result.unwrap().inventory.layout == "transformers-safetensors-v1",
                supported
            );
        }
    }
    let (mut meta, _) = document();
    let config = br#"{"model_type":"fixture","trust_remote_code":true}"#.to_vec();
    meta["siblings"][1] = sibling("config.json", &config);
    assert_eq!(
        inspect(&meta, vec![licenses::MIT.as_bytes().to_vec(), config])
            .unwrap()
            .inventory
            .layout,
        "unsupported"
    );
}

#[test]
fn role_selection_is_narrow_and_all_license_paths_are_evidence() {
    for (path, role) in [
        ("LICENSE.md", Role::License),
        ("vendor/COPYING.txt", Role::License),
        ("NOTICE.md", Role::Notice),
        ("generation_config.json", Role::Config),
        ("vocab.txt", Role::Tokenizer),
        ("tokenizer.model", Role::Tokenizer),
        ("chat_template.jinja", Role::Template),
        ("chat_template.json", Role::Template),
        ("README.md", Role::Documentation),
        ("adapter_model.safetensors", Role::Other),
        ("onnx/model.safetensors", Role::Other),
        ("pytorch_model.bin", Role::Other),
    ] {
        assert_eq!(classify(path), role);
    }
    let (mut meta, config) = document();
    meta["cardData"]["license"] = json!("apache-2.0");
    meta["siblings"][0] = sibling("LICENSE", licenses::APACHE.as_bytes());
    assert_eq!(
        inspect(&meta, vec![licenses::APACHE.as_bytes().to_vec(), config])
            .unwrap()
            .inventory
            .license
            .status,
        EvidenceStatus::Matched
    );
}

#[test]
fn evidence_requests_and_combined_bytes_are_bounded() {
    let (mut meta, _) = document();
    for index in 0..16 {
        meta["siblings"]
            .as_array_mut()
            .unwrap()
            .push(sibling(&format!("vendor{index}/LICENSE"), b""));
    }
    assert_eq!(
        inspect(&meta, vec![]).unwrap_err().code,
        "too_many_evidence_files"
    );
    let (mut meta, _) = document();
    let large = vec![b'x'; METADATA_LIMIT as usize];
    meta["siblings"][0] = sibling("LICENSE", &large);
    meta["siblings"]
        .as_array_mut()
        .unwrap()
        .push(sibling("vendor/LICENSE", b"a"));
    assert_eq!(
        inspect(&meta, vec![large]).unwrap_err().code,
        "metadata_too_large"
    );
}
