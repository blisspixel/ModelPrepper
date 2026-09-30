use clap::Parser;
use modelprepper::cli::{Cli, Command as AppCommand, ConfigCommand, execute, read_text};
use std::fs;
use std::process::{Command, Output};
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
        vec!["resolve", "--help"],
        vec!["init", "--help"],
        vec!["status", "--help"],
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
        command: AppCommand::Config {
            command: ConfigCommand::Init {
                output,
                catalog: "".into(),
                volume: "models".into(),
            },
        },
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
