use clap::Parser;
use modelprepper::cli::{
    Cli, ColorMode, Command as AppCommand, CommandOutput, ConfigCommand, View, execute, execute_in,
    pretty_output, read_text,
};
use std::fs;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

fn binary(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_modelprepper"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn native_cli_reports_version_help_and_argument_failures() {
    for args in [
        vec!["--version"],
        vec!["--help"],
        vec!["config", "--help"],
        vec!["plan", "--help"],
        vec!["budget", "--help"],
        vec!["kit", "--help"],
        vec!["readiness", "--help"],
        vec!["resolve", "--help"],
        vec!["init", "--help"],
        vec!["status", "--help"],
        vec!["propose", "--help"],
        vec!["proposals", "--help"],
        vec!["decide", "--help"],
    ] {
        let output = binary(&args);
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
    }
    for args in [
        vec![],
        vec!["pull"],
        vec!["plan"],
        vec!["config", "check"],
        vec![
            "plan",
            "--config",
            "x",
            "--inventory",
            "x",
            "--available-bytes",
            "-1",
        ],
    ] {
        assert!(!binary(&args).status.success());
    }
}

#[test]
fn saved_review_commands_are_offline_bounded_and_report_missing_exact_ids() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        toml::to_string(&modelprepper::Config::default_for(
            "catalog".into(),
            "models".into(),
        ))
        .unwrap(),
    )
    .unwrap();
    assert!(
        binary(&["init", "--config", config.to_str().unwrap()])
            .status
            .success()
    );
    let list = binary(&[
        "proposals",
        "--config",
        config.to_str().unwrap(),
        "list",
        "--limit",
        "1",
    ]);
    assert!(list.status.success(), "{list:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&list.stdout).unwrap()["proposals"],
        serde_json::json!([])
    );
    for args in [
        vec![
            "proposals",
            "--config",
            config.to_str().unwrap(),
            "list",
            "--limit",
            "0",
        ],
        vec![
            "proposals",
            "--config",
            config.to_str().unwrap(),
            "show",
            "--plan",
            "bad",
        ],
        vec![
            "decide",
            "--config",
            config.to_str().unwrap(),
            "--plan",
            "bad",
            "--decision",
            "reject",
        ],
        vec![
            "decide",
            "--config",
            config.to_str().unwrap(),
            "--plan",
            "bad",
            "--decision",
            "approve",
        ],
        vec![
            "propose",
            "--config",
            config.to_str().unwrap(),
            "--repo",
            "bad",
        ],
    ] {
        assert!(!binary(&args).status.success());
    }
}

#[test]
fn creates_valid_config_without_creating_a_vault_or_overwriting_user_files() {
    let dir = TempDir::new().unwrap();
    let output = dir.path().join("config.toml");
    let catalog = dir.path().join("catalog");
    let volume = dir.path().join("models");
    let args = [
        "config",
        "init",
        "--output",
        output.to_str().unwrap(),
        "--catalog",
        catalog.to_str().unwrap(),
        "--volume",
        volume.to_str().unwrap(),
    ];
    let result = binary(&args);
    assert!(result.status.success(), "{:?}", result);
    let body: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(body["vault_initialized"], false);
    assert!(!catalog.exists());
    assert!(!volume.exists());
    let before = fs::read(&output).unwrap();
    let again = binary(&args);
    assert!(!again.status.success());
    assert_eq!(fs::read(&output).unwrap(), before);
    let error: serde_json::Value = serde_json::from_slice(&again.stderr).unwrap();
    assert_eq!(error["error"]["code"], "config_write_failed");
    let check = binary(&["config", "check", "--config", output.to_str().unwrap()]);
    assert!(check.status.success());
    let json: serde_json::Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(json["watch_count"], 0);
}

#[test]
fn plan_output_is_json_and_cannot_authorize_local_inventory() {
    let result = binary(&[
        "plan",
        "--config",
        "examples/config.toml",
        "--inventory",
        "tests/fixtures/inventory.json",
        "--available-bytes",
        "500000000000",
    ]);
    assert!(result.status.success(), "{result:?}");
    let plan: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(plan["payload_bytes"], 5504);
    assert_eq!(plan["transfer_authorized"], false);
    assert_eq!(plan["mode"], "offline_preview");
    assert_eq!(plan["workspace_bytes"], serde_json::Value::Null);
    assert!(result.stderr.is_empty());
}

#[test]
fn invalid_and_missing_metadata_fail_with_machine_readable_errors() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.toml");
    fs::write(&path, "schema_version = 2").unwrap();
    let result = binary(&["config", "check", "--config", path.to_str().unwrap()]);
    assert!(!result.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result.stderr).unwrap()["error"]["code"],
        "invalid_config"
    );
    let missing = binary(&[
        "config",
        "check",
        "--config",
        dir.path().join("missing").to_str().unwrap(),
    ]);
    assert!(!missing.status.success());
    let input = dir.path().join("inventory.json");
    fs::write(&input, "{}").unwrap();
    let invalid = binary(&[
        "plan",
        "--config",
        "examples/config.toml",
        "--inventory",
        input.to_str().unwrap(),
        "--available-bytes",
        "1",
    ]);
    assert!(!invalid.status.success());
}

#[test]
fn metadata_reader_is_bounded_utf8_and_accepts_a_bom() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("input");
    assert_eq!(read_text(&path).unwrap_err().code, "input_read_failed");
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert_eq!(read_text(&path).unwrap_err().code, "invalid_encoding");
    fs::write(&path, "\u{feff}hello").unwrap();
    assert_eq!(read_text(&path).unwrap(), "hello");
    fs::write(&path, vec![b'a'; 2 * 1024 * 1024]).unwrap();
    assert_eq!(read_text(&path).unwrap().len(), 2 * 1024 * 1024);
    fs::write(&path, vec![b'a'; 2 * 1024 * 1024 + 1]).unwrap();
    assert_eq!(read_text(&path).unwrap_err().code, "input_too_large");
}

#[test]
fn library_cli_has_the_same_errors_as_the_binary() {
    let dir = TempDir::new().unwrap();
    let output = dir.path().join("absent/config.toml");
    let cli = Cli::try_parse_from([
        "modelprepper",
        "config",
        "init",
        "--output",
        output.to_str().unwrap(),
        "--catalog",
        "catalog",
        "--volume",
        "models",
    ])
    .unwrap();
    let error = execute(cli).unwrap_err();
    assert_eq!(error.code, "config_write_failed");
    assert!(error.to_string().contains("config_write_failed"));
    let _: &dyn std::error::Error = &error;
    let cli = Cli {
        pretty: false,
        json: false,
        quiet: false,
        color: ColorMode::Auto,
        command: AppCommand::Setup(modelprepper::cli::SetupCommand::Config {
            command: ConfigCommand::Init {
                output,
                catalog: "".into(),
                volume: "models".into(),
            },
        }),
    };
    assert!(execute(cli).is_err());
}

#[test]
fn native_vault_commands_resolve_paths_from_config_and_status_stays_offline() {
    let dir = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        toml::to_string(&modelprepper::Config::default_for(
            "catalog".into(),
            "models".into(),
        ))
        .unwrap(),
    )
    .unwrap();
    for command in ["init", "status", "init"] {
        let output = Command::new(env!("CARGO_BIN_EXE_modelprepper"))
            .current_dir(elsewhere.path())
            .args([command, "--config", config.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(body["state"], "ready");
        assert_eq!(body["volumes"][0]["state"], "mounted");
    }
    assert!(
        dir.path()
            .join("models/.modelprepper-volume.json")
            .is_file()
    );
    assert!(!elsewhere.path().join("models").exists());
    for args in [
        vec!["resolve", "--repo", "bad"],
        vec!["status", "--config", "does-not-exist.toml"],
    ] {
        assert!(!binary(&args).status.success());
    }
}

#[test]
fn flag_help_describes_plan_repo_and_decision() {
    for (args, needle) in [
        (vec!["propose", "--help"], "Hugging Face repository"),
        (vec!["decide", "--help"], "Exact plan id"),
        (vec!["proposals", "show", "--help"], "Exact plan id"),
        (vec!["resolve", "--help"], "Branch, tag, or commit"),
    ] {
        let output = binary(&args);
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(needle), "{text}");
    }
}

#[test]
fn overlapping_paths_fail_at_config_generation_and_check() {
    let dir = TempDir::new().unwrap();
    let output = dir.path().join("c3.toml");
    let created = binary(&[
        "config",
        "init",
        "--output",
        output.to_str().unwrap(),
        "--catalog",
        "x",
        "--volume",
        "x",
    ]);
    assert!(!created.status.success(), "{created:?}");
    assert!(!output.exists());
    let error: serde_json::Value = serde_json::from_slice(&created.stderr).unwrap();
    assert_eq!(error["error"]["code"], "overlapping_vault_paths");
    fs::write(
        &output,
        toml::to_string(&modelprepper::Config::default_for("x".into(), "x".into())).unwrap(),
    )
    .unwrap();
    let check = binary(&["config", "check", "--config", output.to_str().unwrap()]);
    assert!(!check.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&check.stderr).unwrap()["error"]["code"],
        "overlapping_vault_paths"
    );
    let dotted = dir.path().join("dotted.toml");
    let created = binary(&[
        "config",
        "init",
        "--output",
        dotted.to_str().unwrap(),
        "--catalog",
        "./models",
        "--volume",
        "models",
    ]);
    assert!(!created.status.success(), "{created:?}");
    assert!(!dotted.exists());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&created.stderr).unwrap()["error"]["code"],
        "unsafe_vault_path"
    );
}

#[test]
fn pretty_flag_indents_json_and_closed_stdout_does_not_panic() {
    assert!(pretty_output(true, false));
    assert!(pretty_output(false, true));
    assert!(!pretty_output(false, false));
    let dir = TempDir::new().unwrap();
    let output = dir.path().join("config.toml");
    assert!(
        binary(&[
            "config",
            "init",
            "--output",
            output.to_str().unwrap(),
            "--catalog",
            "catalog",
            "--volume",
            "models",
        ])
        .status
        .success()
    );
    let compact = binary(&["config", "check", "--config", output.to_str().unwrap()]);
    assert_eq!(
        compact.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    let pretty = binary(&[
        "config",
        "check",
        "--pretty",
        "--config",
        output.to_str().unwrap(),
    ]);
    assert!(pretty.status.success(), "{pretty:?}");
    assert!(pretty.stdout.windows(3).any(|bytes| bytes == b"\n  "));

    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    let mut child = Command::new(env!("CARGO_BIN_EXE_modelprepper"))
        .args(["config", "check", "--config", output.to_str().unwrap()])
        .stdout(writer)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    let status = child.wait().unwrap();
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(status.success(), "{status:?} {stderr}");
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert!(!stderr.contains("Broken pipe"), "{stderr}");
}

#[test]
fn help_is_grouped_and_fits_eighty_columns() {
    let output = binary(&["--help"]);
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8(output.stdout).unwrap();
    for needle in [
        "Keep verified copies of open-weight models on your own disks.",
        "Setup",
        "Review a source",
        "Local copies",
        "Preview",
        "First run:",
    ] {
        assert!(text.contains(needle), "{needle} missing from {text}");
    }
    for line in text.lines() {
        assert!(line.chars().count() <= 80, "wide help line: {line}");
    }
    let config_help = String::from_utf8(binary(&["config", "--help"]).stdout).unwrap();
    assert!(
        config_help.contains("Create a config file"),
        "{config_help}"
    );
    assert!(config_help.contains("Check a config file"), "{config_help}");
    for line in config_help.lines() {
        assert!(line.chars().count() <= 80, "wide config help line: {line}");
    }
    let missing = binary(&["status"]);
    assert_eq!(missing.status.code(), Some(2));
    let error: serde_json::Value = serde_json::from_slice(&missing.stderr).unwrap();
    assert_eq!(error["error"]["code"], "invalid_usage");
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .contains("status")
    );
}

#[test]
fn json_wins_over_local_text_format_and_text_is_available_in_a_pipe() {
    let dir = TempDir::new().unwrap();
    let store = dir.path().join("empty-store");
    fs::create_dir(&store).unwrap();
    let text = binary(&[
        "local",
        "list",
        "--ollama-dir",
        store.to_str().unwrap(),
        "--format",
        "text",
    ]);
    assert!(text.status.success(), "{text:?}");
    let body = String::from_utf8(text.stdout).unwrap();
    assert!(body.contains("next:"), "{body}");
    assert!(!body.trim_start().starts_with('{'), "{body}");
    let forced = binary(&[
        "--json",
        "local",
        "list",
        "--ollama-dir",
        store.to_str().unwrap(),
        "--format",
        "text",
    ]);
    assert!(forced.status.success(), "{forced:?}");
    let value: serde_json::Value = serde_json::from_slice(&forced.stdout).unwrap();
    assert_eq!(value["schema_version"], 2);
    assert!(value["note"].as_str().unwrap().contains("--config"));
}

#[test]
fn offline_text_view_names_a_verdict_for_each_local_command() {
    let dir = TempDir::new().unwrap();
    // macOS runners keep TempDir under /var, which is a symlink. Vault paths reject that.
    let root = fs::canonicalize(dir.path()).unwrap();
    let config = root.join("c.toml");
    let view = View {
        text: true,
        json: false,
        color: true,
        stderr_terminal: true,
        columns: 80,
    };
    let created = execute_in(
        Cli::try_parse_from([
            "modelprepper",
            "config",
            "init",
            "--output",
            config.to_str().unwrap(),
            "--catalog",
            root.join("catalog").to_str().unwrap(),
            "--volume",
            root.join("models").to_str().unwrap(),
        ])
        .unwrap(),
        view,
    )
    .unwrap();
    match created {
        CommandOutput::Text { body, ok } => {
            assert!(ok, "{body}");
            assert!(body.contains("created"), "{body}");
        }
        CommandOutput::Json(value) => panic!("{value}"),
    }
    let commands = [
        vec![
            "modelprepper",
            "config",
            "check",
            "--config",
            config.to_str().unwrap(),
        ],
        vec!["modelprepper", "init", "--config", config.to_str().unwrap()],
        vec![
            "modelprepper",
            "status",
            "--config",
            config.to_str().unwrap(),
        ],
        vec![
            "modelprepper",
            "proposals",
            "--config",
            config.to_str().unwrap(),
            "list",
        ],
        vec![
            "modelprepper",
            "plan",
            "--config",
            "examples/config.toml",
            "--inventory",
            "tests/fixtures/inventory.json",
            "--available-bytes",
            "500000000000",
        ],
        vec![
            "modelprepper",
            "local",
            "list",
            "--ollama-dir",
            root.to_str().unwrap(),
        ],
    ];
    for args in commands {
        let owned: Vec<String> = args.into_iter().map(str::to_owned).collect();
        match execute_in(Cli::try_parse_from(&owned).unwrap(), view).unwrap() {
            CommandOutput::Text { body, ok } => {
                assert!(ok, "{body}");
                assert!(body.contains("next:"), "{body}");
            }
            CommandOutput::Json(value) => panic!("{value}"),
        }
    }
}

#[test]
fn readiness_text_and_json_agree_and_do_not_create_or_execute() {
    let dir = TempDir::new().unwrap();
    let model = dir.path().join("model");
    fs::create_dir(&model).unwrap();
    for (name, bytes) in [
        ("config.json", b"{}".as_slice()),
        ("tokenizer.json", br#"{"v":1}"#.as_slice()),
        ("model.safetensors", b"weights".as_slice()),
        ("chat_template.jinja", b"{{ content }}".as_slice()),
    ] {
        fs::write(model.join(name), bytes).unwrap();
    }
    let recipe = dir.path().join("recipe.json");
    fs::write(
        &recipe,
        r#"{
  "schema_version": 1,
  "profile": "publisher-safetensors-text-v1",
  "model": {
    "endpoint": "https://huggingface.co",
    "repo_id": "example/model",
    "resolved_revision": "0123456789abcdef0123456789abcdef01234567",
    "files": [
      {"path": "config.json", "role": "config", "capability": "text-generation", "need": "required", "retention": "retained", "size_bytes": 2, "sha256": "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"},
      {"path": "tokenizer.json", "role": "tokenizer", "capability": "text-generation", "need": "required", "retention": "retained", "size_bytes": 7, "sha256": "afbf9d0f3560b0fd7795e81c42a0a79ee6b6fc67e064f77826aee642cad28d91"},
      {"path": "model.safetensors", "role": "weight", "capability": "text-generation", "need": "required", "retention": "retained", "size_bytes": 7, "sha256": "9a129038d9a00aed0cf6a7ea059ca50a813449061ab87848cf1a13eafdf33b2c"},
      {"path": "chat_template.jinja", "role": "template", "capability": "text-generation", "need": "required", "retention": "retained", "size_bytes": 13, "sha256": "52ed09e8448c395a40c52b4d91dc1818070d481a6afd2354b2653956dbf2dfc5"}
    ]
  },
  "capabilities": ["text-generation"],
  "runtime": {"name": "transformers", "version": "4.55.2", "os": "any", "arch": "any", "artifact": {"retention": "named", "locator": "transformers==4.55.2"}},
  "rights": {"status": "matched", "license_id": "apache-2.0"}
}"#,
    )
    .unwrap();
    let before = fs::read_dir(&model).unwrap().count();
    let json = binary(&[
        "readiness",
        "--recipe",
        recipe.to_str().unwrap(),
        "--path",
        model.to_str().unwrap(),
    ]);
    assert!(json.status.success(), "{json:?}");
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(value["mode"], "offline-preflight");
    assert_eq!(value["offline_readiness"], "untested");
    assert_eq!(value["integrity"], "matched");
    assert_eq!(value["runtime_retention"], "named");
    assert_eq!(value["network_contact"], false);
    assert_eq!(value["executed"], false);
    assert_eq!(value["vault_mutated"], false);
    assert_eq!(value["transfer_authorized"], false);
    let text = Command::new(env!("CARGO_BIN_EXE_modelprepper"))
        .env("MODELPREPPER_TEXT", "1")
        .args([
            "readiness",
            "--recipe",
            recipe.to_str().unwrap(),
            "--path",
            model.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(text.status.success(), "{text:?}");
    let body = String::from_utf8(text.stdout).unwrap();
    for needle in ["untested", "matched", "named", "declared_matched", "next:"] {
        assert!(body.contains(needle), "{needle} missing from {body}");
    }
    assert_eq!(fs::read_dir(&model).unwrap().count(), before);
    let absent = dir.path().join("absent");
    let missing = binary(&[
        "readiness",
        "--recipe",
        recipe.to_str().unwrap(),
        "--path",
        absent.to_str().unwrap(),
    ]);
    assert!(missing.status.success(), "{missing:?}");
    let missing: serde_json::Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(missing["availability"], "unavailable");
    assert!(!absent.exists());
}

#[test]
fn selecting_files_requires_the_gguf_profile_and_stays_unauthorized() {
    let rejected = binary(&[
        "plan",
        "--config",
        "examples/config.toml",
        "--inventory",
        "tests/fixtures/inventory.json",
        "--available-bytes",
        "500000000000",
        "--select",
        "model.safetensors",
    ]);
    assert!(!rejected.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&rejected.stderr).unwrap()["error"]["code"],
        "invalid_usage"
    );
    let dir = TempDir::new().unwrap();
    let mut config =
        modelprepper::Config::default_for(dir.path().join("catalog"), dir.path().join("models"));
    config.files.profile = "selected-gguf-v1".into();
    let config_path = dir.path().join("config.toml");
    fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
    let inventory = dir.path().join("inventory.json");
    fs::write(
        &inventory,
        r#"{
  "schema_version": 1,
  "source": {
    "endpoint": "https://huggingface.co",
    "repo_type": "model",
    "repo_id": "example/gguf",
    "requested_revision": "main",
    "resolved_revision": "0123456789abcdef0123456789abcdef01234567",
    "gated": false,
    "private": false
  },
  "complete": true,
  "layout": "gguf-v1",
  "license": {"license_id": "", "status": "unreviewed", "paths": []},
  "files": [
    {"path": "a-Q4_K_M.gguf", "size_bytes": 3, "upstream_hash": {"algorithm": "sha256", "value": "abababababababababababababababababababababababababababababababab"}, "role": "weight", "required": true},
    {"path": "b-Q8_0.gguf", "size_bytes": 4, "upstream_hash": {"algorithm": "sha256", "value": "abababababababababababababababababababababababababababababababab"}, "role": "weight", "required": true}
  ]
}"#,
    )
    .unwrap();
    let selected = binary(&[
        "plan",
        "--config",
        config_path.to_str().unwrap(),
        "--inventory",
        inventory.to_str().unwrap(),
        "--available-bytes",
        "100",
        "--select",
        "a-Q4_K_M.gguf",
    ]);
    assert!(selected.status.success(), "{selected:?}");
    let plan: serde_json::Value = serde_json::from_slice(&selected.stdout).unwrap();
    assert_eq!(plan["payload_bytes"], 3);
    assert_eq!(plan["representation"], "derivative");
    assert_eq!(plan["transfer_authorized"], false);
    assert_eq!(plan["quantization_label"], "Q4_K_M");
}

#[test]
fn budget_and_kit_compare_pinned_sizes_without_rewriting_or_reserving() {
    let dir = TempDir::new().unwrap();
    let config = dir.path().join("config.toml");
    fs::write(
        &config,
        toml::to_string(&modelprepper::Config::default_for(
            dir.path().join("catalog"),
            dir.path().join("models"),
        ))
        .unwrap(),
    )
    .unwrap();
    let inventory: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/inventory.json")).unwrap();
    let request = dir.path().join("budget.json");
    fs::write(
        &request,
        serde_json::json!({
            "schema_version": 1,
            "candidates": [{
                "name": "fixture",
                "representation": "original",
                "inventory": inventory
            }]
        })
        .to_string(),
    )
    .unwrap();
    let before = fs::read(&request).unwrap();
    let budget = binary(&[
        "budget",
        "--config",
        config.to_str().unwrap(),
        "--request",
        request.to_str().unwrap(),
    ]);
    assert!(budget.status.success(), "{budget:?}");
    let body: serde_json::Value = serde_json::from_slice(&budget.stdout).unwrap();
    assert_eq!(body["rows"][0]["required_bytes"], 5504);
    assert_eq!(body["transfer_authorized"], false);
    assert_eq!(body["workspace_bytes"], serde_json::Value::Null);
    assert_eq!(body["rewritten"], false);
    assert_eq!(fs::read(&request).unwrap(), before);
    let unknown = dir.path().join("unknown.json");
    fs::write(&unknown, "{\"schema_version\":2}\n").unwrap();
    let unknown_before = fs::read(&unknown).unwrap();
    let reported = binary(&[
        "budget",
        "--config",
        config.to_str().unwrap(),
        "--request",
        unknown.to_str().unwrap(),
    ]);
    assert!(reported.status.success(), "{reported:?}");
    let reported: serde_json::Value = serde_json::from_slice(&reported.stdout).unwrap();
    assert_eq!(reported["supported"], false);
    assert_eq!(
        reported["blockers"],
        serde_json::json!(["unsupported_schema"])
    );
    assert_eq!(fs::read(&unknown).unwrap(), unknown_before);

    let kit = dir.path().join("kit.toml");
    fs::write(
        &kit,
        "schema_version = 1\nhardware_tier = \"a note\"\n\n[[text]]\nrepo_id = \"example/model\"\nrevision = \"unpinned\"\nrepresentation = \"original\"\n",
    )
    .unwrap();
    let kit_before = fs::read(&kit).unwrap();
    let kit_out = Command::new(env!("CARGO_BIN_EXE_modelprepper"))
        .env("MODELPREPPER_TEXT", "1")
        .args([
            "kit",
            "--config",
            config.to_str().unwrap(),
            "--kit",
            kit.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(kit_out.status.success(), "{kit_out:?}");
    let text = String::from_utf8(kit_out.stdout).unwrap();
    assert!(text.contains("empty"), "{text}");
    assert!(text.contains("recommendation"), "{text}");
    assert!(text.contains("unpinned"), "{text}");
    assert_eq!(fs::read(&kit).unwrap(), kit_before);
    let stale = dir.path().join("stale.toml");
    fs::write(&stale, "schema_version = 2\n").unwrap();
    let stale_before = fs::read(&stale).unwrap();
    let stale_out = binary(&[
        "kit",
        "--config",
        config.to_str().unwrap(),
        "--kit",
        stale.to_str().unwrap(),
    ]);
    assert!(stale_out.status.success(), "{stale_out:?}");
    let stale_body: serde_json::Value = serde_json::from_slice(&stale_out.stdout).unwrap();
    assert_eq!(stale_body["supported"], false);
    assert_eq!(stale_body["network_contact"], false);
    assert_eq!(stale_body["vault_mutated"], false);
    assert_eq!(fs::read(&stale).unwrap(), stale_before);
}
