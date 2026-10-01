use crate::{Config, Error, Inventory, Result, build_plan};
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Model preservation planning and public-source inspection."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Inspect a public source and persist its exact evidence and destination.
    Propose {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        repo: String,
        #[arg(long, default_value = "main")]
        revision: String,
    },
    /// Browse saved source reviews offline.
    Proposals {
        #[arg(long)]
        config: PathBuf,
        #[command(subcommand)]
        command: ProposalCommand,
    },
    /// Record a final review decision. Transfer approval is not available yet.
    Decide {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        plan: String,
        #[arg(long, value_enum)]
        decision: DecisionChoice,
    },
    /// Initialize the local catalog and adopt empty storage directories.
    Init {
        #[arg(long)]
        config: PathBuf,
    },
    /// Inspect catalog and disk identities without contacting a publisher.
    Status {
        #[arg(long)]
        config: PathBuf,
    },
    /// Resolve a public model to a pinned commit and inspect its license evidence.
    Resolve {
        #[arg(long)]
        repo: String,
        #[arg(long, default_value = "main")]
        revision: String,
    },
    /// Generate or validate configuration without creating a vault.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Preview an untrusted local inventory. Never authorizes model transfers.
    Plan {
        #[arg(long)]
        config: PathBuf,
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
        #[arg(long)]
        after: Option<String>,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
        limit: u32,
    },
    Show {
        #[arg(long)]
        plan: String,
    },
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
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        catalog: PathBuf,
        #[arg(long)]
        volume: PathBuf,
    },
    Check {
        #[arg(long)]
        config: PathBuf,
    },
}

pub fn execute(cli: Cli) -> Result<Value> {
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
        .map_err(|e| Error::new("output_failed", e.to_string())),
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
        }
        Command::Init { config } => {
            let loaded = Config::parse(&read_text(&config)?)?;
            let located = crate::vault::locate(&loaded, &config)?;
            serde_json::to_value(crate::vault::initialize(&located)?)
                .map_err(|e| Error::new("output_failed", e.to_string()))
        }
        Command::Status { config } => {
            let loaded = Config::parse(&read_text(&config)?)?;
            let located = crate::vault::locate(&loaded, &config)?;
            serde_json::to_value(crate::vault::status(&located)?)
                .map_err(|e| Error::new("output_failed", e.to_string()))
        }
        Command::Resolve { repo, revision } => {
            serde_json::to_value(crate::hub::resolve(&repo, &revision)?)
                .map_err(|e| Error::new("output_failed", e.to_string()))
        }
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
            Ok(
                json!({"schema_version": 1, "status": "config_created", "path": output, "vault_initialized": false}),
            )
        }
        Command::Config {
            command: ConfigCommand::Check { config },
        } => {
            let config = Config::parse(&read_text(&config)?)?;
            Ok(
                json!({"schema_version": 1, "status": "config_valid", "watch_count": config.watches.len(), "volume_count": config.volumes.len()}),
            )
        }
        Command::Plan {
            config,
            inventory,
            available_bytes,
        } => {
            let config = Config::parse(&read_text(&config)?)?;
            let inventory = Inventory::parse(&read_text(&inventory)?)?;
            serde_json::to_value(build_plan(&config, &inventory, available_bytes)?)
                .map_err(|e| Error::new("output_failed", e.to_string()))
        }
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
