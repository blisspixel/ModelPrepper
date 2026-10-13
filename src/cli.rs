use crate::{Config, Error, Inventory, Result, build_plan};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum CommandOutput {
    Json(Value),
    Text(String),
}

pub fn pretty_output(flag: bool, stdout_is_terminal: bool) -> bool {
    flag || stdout_is_terminal
}

const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Model preservation planning and public-source inspection."
)]
pub struct Cli {
    /// Indent JSON. Also used automatically when stdout is a terminal.
    #[arg(long, global = true)]
    pub pretty: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect a public source and persist its exact evidence and destination.
    Propose {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Hugging Face repository id or official hub URL.
        #[arg(long)]
        repo: String,
        /// Branch, tag, or commit to resolve.
        #[arg(long, default_value = "main")]
        revision: String,
    },
    /// Browse saved source reviews offline.
    Proposals {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        #[command(subcommand)]
        command: ProposalCommand,
    },
    /// Record a final review decision. Transfer approval is not available yet.
    Decide {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Exact plan id from proposals list.
        #[arg(long)]
        plan: String,
        /// approve or reject. Approval stays blocked until transfer is qualified.
        #[arg(long, value_enum)]
        decision: DecisionChoice,
    },
    /// Initialize the local catalog and adopt empty storage directories.
    Init {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
    },
    /// Inspect catalog and disk identities without contacting a publisher.
    Status {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
    },
    /// Resolve a public model to a pinned commit and inspect its license evidence.
    Resolve {
        /// Hugging Face repository id or official hub URL.
        #[arg(long)]
        repo: String,
        /// Branch, tag, or commit to resolve.
        #[arg(long, default_value = "main")]
        revision: String,
    },
    /// Hash a local folder against a resolved commit. Reads only; nothing is copied.
    VerifyLocal {
        /// Vault configuration. Required with --plan so no network access is needed.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Saved plan id to verify against, without contacting the publisher.
        #[arg(long, conflicts_with = "repo")]
        plan: Option<String>,
        /// Hugging Face repository id or official hub URL to resolve.
        #[arg(long)]
        repo: Option<String>,
        /// Branch, tag, or commit to resolve.
        #[arg(long, default_value = "main")]
        revision: String,
        /// Local model directory. Symlinks are not followed.
        #[arg(long)]
        path: PathBuf,
    },
    /// Copy or move a verified local folder into the vault.
    Adopt {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Pending plan id whose resolved evidence must match the folder.
        #[arg(long)]
        plan: String,
        /// Local model directory produced outside ModelPrepper.
        #[arg(long)]
        path: PathBuf,
        /// Remove source files only after the destination is re-verified and recorded.
        #[arg(long = "move", conflicts_with = "copy")]
        move_source: bool,
        /// Copy into the vault and leave the source in place. This is the default.
        #[arg(long)]
        copy: bool,
    },
    /// Read-only inventory of local runtime stores.
    Local {
        #[command(subcommand)]
        command: LocalCommand,
    },
    /// Generate or validate configuration without creating a vault.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Preview an untrusted local inventory. Never authorizes model transfers.
    Plan {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Local inventory JSON to preview. It cannot authorize a transfer.
        #[arg(long)]
        inventory: PathBuf,
        /// Observed free bytes, excluding unknown transfer workspace.
        #[arg(long)]
        available_bytes: u64,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProposalCommand {
    List {
        /// Return reviews after this plan id.
        #[arg(long)]
        after: Option<String>,
        /// Page size from 1 to 100.
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
        limit: u32,
    },
    Show {
        /// Exact plan id from proposals list.
        #[arg(long)]
        plan: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum LocalCommand {
    /// List Ollama tags from manifests only. Never calls the daemon or changes blobs.
    List {
        /// Ollama models directory. Defaults to the usual store locations.
        #[arg(long)]
        ollama_dir: Option<PathBuf>,
        /// Vault configuration used only to compare preserved content digests.
        #[arg(long)]
        config: Option<PathBuf>,
        /// text prints the summary; json prints one object.
        #[arg(long, value_enum, default_value = "json")]
        format: OutputFormat,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DecisionChoice {
    Approve,
    Reject,
}

fn located_config(path: &Path) -> Result<Config> {
    crate::vault::locate(&Config::parse(&read_text(path)?)?, path)
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    Init {
        /// Config file to create. Refuses to overwrite an existing file.
        #[arg(long)]
        output: PathBuf,
        /// Catalog directory. Must not overlap the volume.
        #[arg(long)]
        catalog: PathBuf,
        /// Storage directory. Must not overlap the catalog.
        #[arg(long)]
        volume: PathBuf,
    },
    Check {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
    },
}

pub fn execute(cli: Cli) -> Result<CommandOutput> {
    match cli.command {
        Command::Propose {
            config,
            repo,
            revision,
        } => serde_json::to_value(crate::vault::propose(
            &located_config(&config)?,
            &repo,
            &revision,
        )?)
        .map_err(|e| Error::new("output_failed", e.to_string()))
        .map(CommandOutput::Json),
        Command::Proposals { config, command } => {
            let config = located_config(&config)?;
            match command {
                ProposalCommand::List { after, limit } => {
                    serde_json::to_value(crate::vault::proposals(&config, after.as_deref(), limit)?)
                }
                ProposalCommand::Show { plan } => {
                    serde_json::to_value(crate::vault::show_proposal(&config, &plan)?)
                }
            }
            .map_err(|e| Error::new("output_failed", e.to_string()))
            .map(CommandOutput::Json)
        }
        Command::Decide {
            config,
            plan,
            decision,
        } => {
            let decision = match decision {
                DecisionChoice::Approve => crate::vault::Decision::Approved,
                DecisionChoice::Reject => crate::vault::Decision::Rejected,
            };
            serde_json::to_value(crate::vault::decide(
                &located_config(&config)?,
                &plan,
                decision,
            )?)
            .map_err(|e| Error::new("output_failed", e.to_string()))
            .map(CommandOutput::Json)
        }
        Command::Init { config } => {
            let loaded = Config::parse(&read_text(&config)?)?;
            let located = crate::vault::locate(&loaded, &config)?;
            serde_json::to_value(crate::vault::initialize(&located)?)
                .map(CommandOutput::Json)
                .map_err(|e| Error::new("output_failed", e.to_string()))
        }
        Command::Status { config } => {
            let loaded = Config::parse(&read_text(&config)?)?;
            let located = crate::vault::locate(&loaded, &config)?;
            serde_json::to_value(crate::vault::status(&located)?)
                .map(CommandOutput::Json)
                .map_err(|e| Error::new("output_failed", e.to_string()))
        }
        Command::Resolve { repo, revision } => {
            serde_json::to_value(crate::hub::resolve(&repo, &revision)?)
                .map(CommandOutput::Json)
                .map_err(|e| Error::new("output_failed", e.to_string()))
        }
        Command::VerifyLocal {
            config,
            plan,
            repo,
            revision,
            path,
        } => verify_local(config, plan, repo, revision, path),
        Command::Adopt {
            config,
            plan,
            path,
            move_source,
            copy: _,
        } => adopt_local(&config, &plan, &path, move_source),
        Command::Local { command } => match command {
            LocalCommand::List {
                ollama_dir,
                config,
                format,
            } => list_local(ollama_dir, config, format),
        },
        Command::Config {
            command:
                ConfigCommand::Init {
                    output,
                    catalog,
                    volume,
                },
        } => {
            let config = Config::default_for(catalog, volume);
            config.validate()?;
            let text = toml::to_string_pretty(&config)
                .map_err(|e| Error::new("invalid_config", e.to_string()))?;
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&output)
                .map_err(|e| {
                    Error::new("config_write_failed", format!("{}: {e}", output.display()))
                })?;
            file.write_all(text.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|e| Error::new("config_write_failed", e.to_string()))?;
            Ok(CommandOutput::Json(
                json!({"schema_version": 1, "status": "config_created", "path": output, "vault_initialized": false}),
            ))
        }
        Command::Config {
            command: ConfigCommand::Check { config },
        } => {
            let config = Config::parse(&read_text(&config)?)?;
            Ok(CommandOutput::Json(
                json!({"schema_version": 1, "status": "config_valid", "watch_count": config.watches.len(), "volume_count": config.volumes.len()}),
            ))
        }
        Command::Plan {
            config,
            inventory,
            available_bytes,
        } => {
            let config = Config::parse(&read_text(&config)?)?;
            let inventory = Inventory::parse(&read_text(&inventory)?)?;
            serde_json::to_value(build_plan(&config, &inventory, available_bytes)?)
                .map(CommandOutput::Json)
                .map_err(|e| Error::new("output_failed", e.to_string()))
        }
    }
}

fn hash_progress() -> impl FnMut(&str, u64, u64) {
    let mut next = 8 * 1024 * 1024;
    move |path, done, total| {
        if done >= next || done == total {
            let _ = writeln!(std::io::stderr(), "hashing {path}: {done}/{total} bytes");
            next = done.saturating_add(8 * 1024 * 1024);
        }
    }
}

fn verify_local(
    config: Option<PathBuf>,
    plan: Option<String>,
    repo: Option<String>,
    revision: String,
    path: PathBuf,
) -> Result<CommandOutput> {
    let mut progress = hash_progress();
    let inventory = if let Some(plan) = plan {
        let Some(config) = config else {
            return Err(Error::new(
                "invalid_usage",
                "verify-local --plan also needs --config; a saved review needs no network access",
            ));
        };
        crate::vault::show_proposal(&located_config(&config)?, &plan)?
            .review
            .inventory
    } else if let Some(repo) = repo {
        crate::hub::resolve(&repo, &revision)?.inventory
    } else {
        return Err(Error::new(
            "invalid_usage",
            "verify-local needs --repo or both --config and --plan",
        ));
    };
    serde_json::to_value(crate::verify::verify_tree(
        &inventory,
        &path,
        &mut progress,
    )?)
    .map(CommandOutput::Json)
    .map_err(|error| Error::new("output_failed", error.to_string()))
}

fn adopt_local(config: &Path, plan: &str, path: &Path, move_source: bool) -> Result<CommandOutput> {
    let mut progress = hash_progress();
    let outcome = crate::vault::adopt(
        &located_config(config)?,
        plan,
        path,
        move_source,
        &mut progress,
    )?;
    let value = match outcome {
        crate::vault::Outcome::Recorded(adoption) => serde_json::to_value(adoption),
        crate::vault::Outcome::Mismatch(verification) => serde_json::to_value(verification),
    }
    .map_err(|error| Error::new("output_failed", error.to_string()))?;
    Ok(CommandOutput::Json(value))
}

fn list_local(
    ollama_dir: Option<PathBuf>,
    config: Option<PathBuf>,
    format: OutputFormat,
) -> Result<CommandOutput> {
    let preserved = if let Some(config) = config {
        crate::vault::preserved_digests(&located_config(&config)?)?
    } else {
        BTreeSet::new()
    };
    let dirs = match ollama_dir {
        Some(path) => vec![path],
        None => crate::runtime::default_store_dirs(|key| std::env::var(key).ok()),
    };
    let report = crate::runtime::inventory(&dirs, &preserved)?;
    match format {
        OutputFormat::Json => serde_json::to_value(report)
            .map(CommandOutput::Json)
            .map_err(|error| Error::new("output_failed", error.to_string())),
        OutputFormat::Text => Ok(CommandOutput::Text(report.text())),
    }
}

pub fn read_text(path: &Path) -> Result<String> {
    let file = File::open(path)
        .map_err(|e| Error::new("input_read_failed", format!("{}: {e}", path.display())))?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Error::new("input_read_failed", e.to_string()))?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(Error::new(
            "input_too_large",
            "input exceeds the 2 MiB metadata limit",
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::new("invalid_encoding", "input must be UTF-8"))?;
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned())
}
