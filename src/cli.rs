use crate::{Config, Error, Inventory, Result};

pub use crate::present::{
    clap_text as clap_error_text, columns as terminal_columns, error_text as app_error_text,
};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::{Value, json};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum CommandOutput {
    Json(Value),
    Text { body: String, ok: bool },
}

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub text: bool,
    /// Set when `--json` is passed. It wins over `local list --format text`.
    pub json: bool,
    pub color: bool,
    pub stderr_terminal: bool,
    pub columns: usize,
}

impl View {
    pub fn piped() -> Self {
        Self {
            text: false,
            json: false,
            color: false,
            stderr_terminal: false,
            columns: 80,
        }
    }
}

pub fn pretty_output(flag: bool, stdout_is_terminal: bool) -> bool {
    flag || stdout_is_terminal
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

const MAX_INPUT_BYTES: u64 = 2 * 1024 * 1024;

const AFTER_HELP: &str = "\
First run:
  modelprepper config init --output c.toml --catalog catalog --volume models
  modelprepper init --config c.toml
  modelprepper resolve --repo <org/model>
  modelprepper propose --config c.toml --repo <org/model>
  modelprepper verify-local --repo <org/model> --path <dir>
  modelprepper adopt --config c.toml --plan <id> --path <dir> --copy";

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Keep verified copies of open-weight models on your own disks.",
    after_help = AFTER_HELP,
    term_width = 80
)]
pub struct Cli {
    /// Indent JSON. Also used automatically when stdout is a terminal and JSON is selected.
    #[arg(long, global = true)]
    pub pretty: bool,
    /// Print JSON even when stdout is a terminal.
    #[arg(long, global = true)]
    pub json: bool,
    /// Suppress progress lines.
    #[arg(long, global = true)]
    pub quiet: bool,
    /// Colorize text verdicts. JSON is never colored.
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: ColorMode,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
#[command(term_width = 80)]
pub enum Command {
    #[command(flatten)]
    #[command(next_help_heading = "Setup")]
    Setup(SetupCommand),
    #[command(flatten)]
    #[command(next_help_heading = "Review a source")]
    Review(ReviewCommand),
    #[command(flatten)]
    #[command(next_help_heading = "Local copies")]
    LocalCopies(LocalCopiesCommand),
    #[command(flatten)]
    #[command(next_help_heading = "Preview")]
    Preview(PreviewCommand),
    #[command(flatten)]
    #[command(next_help_heading = "Watches")]
    Watches(WatchCommand),
}

#[derive(Debug, Subcommand)]
#[command(term_width = 80)]
pub enum SetupCommand {
    /// Create or check a vault configuration file.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Initialize the local catalog and adopt empty storage directories.
    Init {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
    },
    /// Inspect catalog, preserved copies, and disk identities offline.
    Status {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
#[command(term_width = 80)]
pub enum ReviewCommand {
    /// Resolve a public model to a pinned commit and inspect its license evidence.
    Resolve {
        /// Hugging Face repository id or official hub URL.
        #[arg(long)]
        repo: String,
        /// Branch, tag, or commit to resolve.
        #[arg(long, default_value = "main")]
        revision: String,
    },
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
}

#[derive(Debug, Subcommand)]
#[command(term_width = 80)]
pub enum WatchCommand {
    /// Run one bounded pass over configured watches.
    Watch {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Absolute staging directory outside the catalog and volumes.
        #[arg(long)]
        stage: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
#[command(term_width = 80)]
pub enum LocalCopiesCommand {
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
    /// Stage and seal one granted revision. Approval stays blocked.
    Fetch {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Pending plan id that has a current grant.
        #[arg(long)]
        plan: String,
        /// Absolute staging directory outside the catalog and volumes.
        #[arg(long)]
        stage: PathBuf,
    },
    /// Copy one sealed revision onto another volume. No network and no approval.
    Replica {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Plan id of the sealed revision.
        #[arg(long)]
        plan: String,
        /// Destination volume label. It must differ from the sealing volume.
        #[arg(long)]
        volume: String,
    },
    /// Rebuild custody from on-volume bundles. No network and no approvals.
    Rebuild {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
    },
    /// Record a local folder that has no upstream proof.
    Import {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Local directory outside the catalog and volumes.
        #[arg(long)]
        path: PathBuf,
        /// Operator repository label. It is not resolved.
        #[arg(long)]
        repo: String,
        /// 40-hex label. It is not an upstream pin.
        #[arg(long)]
        revision: String,
        /// Destination volume label.
        #[arg(long)]
        volume: String,
    },
    /// Read-only inventory of local runtime stores.
    Local {
        #[command(subcommand)]
        command: LocalCommand,
    },
}

#[derive(Debug, Subcommand)]
#[command(term_width = 80)]
pub enum PreviewCommand {
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
        /// GGUF path to keep. Repeat for each file. Requires profile selected-gguf-v1.
        #[arg(long = "select")]
        select: Vec<String>,
    },
    /// Compare pinned candidate sizes with the storage ceiling. Nothing is reserved.
    Budget {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Budget request JSON. See examples/budget.json. Unknown schemas are reported.
        #[arg(long)]
        request: PathBuf,
    },
    /// Report a data-only kit. Slots stay empty until a person names a pick.
    Kit {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
        /// Kit TOML. Unknown schema versions are reported and not rewritten.
        #[arg(long)]
        kit: PathBuf,
    },
    /// Read a local recipe and report offline readiness.
    Readiness {
        /// Versioned runtime recipe JSON. See examples/recipe.json.
        #[arg(long)]
        recipe: PathBuf,
        /// Directory of retained model files. A missing directory is reported and not created.
        #[arg(long)]
        path: PathBuf,
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
        /// text or json. Omit to follow the terminal: text on a terminal, JSON in a pipe.
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,
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
#[command(term_width = 80)]
pub enum ConfigCommand {
    /// Create a config file. Does not create the vault.
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
    /// Check a config file without contacting a publisher.
    Check {
        /// Path to the vault configuration file.
        #[arg(long)]
        config: PathBuf,
    },
}

pub fn execute(cli: Cli) -> Result<CommandOutput> {
    execute_in(cli, View::piped())
}

pub fn execute_in(cli: Cli, view: View) -> Result<CommandOutput> {
    let quiet = cli.quiet;
    match cli.command {
        Command::Review(ReviewCommand::Propose {
            config,
            repo,
            revision,
        }) => {
            let proposal = crate::vault::propose(&located_config(&config)?, &repo, &revision)?;
            show(&view, &proposal, true, |proposal| {
                crate::present::render_proposal(proposal, view.color, view.columns)
            })
        }
        Command::Review(ReviewCommand::Proposals { config, command }) => {
            let config = located_config(&config)?;
            match command {
                ProposalCommand::List { after, limit } => {
                    let page = crate::vault::proposals(&config, after.as_deref(), limit)?;
                    show(&view, &page, true, |page| {
                        crate::present::render_proposals(page, view.color, view.columns)
                    })
                }
                ProposalCommand::Show { plan } => {
                    let proposal = crate::vault::show_proposal(&config, &plan)?;
                    show(&view, &proposal, true, |proposal| {
                        crate::present::render_proposal(proposal, view.color, view.columns)
                    })
                }
            }
        }
        Command::Review(ReviewCommand::Decide {
            config,
            plan,
            decision,
        }) => {
            let decision = match decision {
                DecisionChoice::Approve => crate::vault::Decision::Approved,
                DecisionChoice::Reject => crate::vault::Decision::Rejected,
            };
            let proposal = crate::vault::decide(&located_config(&config)?, &plan, decision)?;
            show(&view, &proposal, true, |proposal| {
                crate::present::render_proposal(proposal, view.color, view.columns)
            })
        }
        Command::Setup(SetupCommand::Init { config }) => {
            let loaded = Config::parse(&read_text(&config)?)?;
            let located = crate::vault::locate(&loaded, &config)?;
            let status = crate::vault::initialize(&located)?;
            show(&view, &status, true, |status| {
                crate::present::render_status(status, view.color, view.columns)
            })
        }
        Command::Setup(SetupCommand::Status { config }) => {
            let loaded = Config::parse(&read_text(&config)?)?;
            let located = crate::vault::locate(&loaded, &config)?;
            let status = crate::vault::status(&located)?;
            show(&view, &status, true, |status| {
                crate::present::render_status(status, view.color, view.columns)
            })
        }
        Command::Review(ReviewCommand::Resolve { repo, revision }) => {
            let resolved = crate::hub::resolve(&repo, &revision)?;
            show(&view, &resolved, true, |resolved| {
                crate::present::render_resolution(resolved, view.color, view.columns)
            })
        }
        Command::LocalCopies(LocalCopiesCommand::VerifyLocal {
            config,
            plan,
            repo,
            revision,
            path,
        }) => verify_local(&view, quiet, config, plan, repo, revision, path),
        Command::LocalCopies(LocalCopiesCommand::Adopt {
            config,
            plan,
            path,
            move_source,
            copy: _,
        }) => adopt_local(&view, quiet, &config, &plan, &path, move_source),
        Command::LocalCopies(LocalCopiesCommand::Fetch {
            config,
            plan,
            stage,
        }) => {
            let seal = crate::vault::fetch(&located_config(&config)?, &plan, &stage)?;
            show(&view, &seal, true, |seal| {
                crate::present::render_seal(seal, view.color, view.columns)
            })
        }
        Command::LocalCopies(LocalCopiesCommand::Replica {
            config,
            plan,
            volume,
        }) => {
            let replica = crate::vault::replica(&located_config(&config)?, &plan, &volume)?;
            show(&view, &replica, true, |replica| {
                crate::present::render_replica(replica, view.color, view.columns)
            })
        }
        Command::LocalCopies(LocalCopiesCommand::Rebuild { config }) => {
            let rebuilt = crate::vault::rebuild(&located_config(&config)?)?;
            show(&view, &rebuilt, true, |rebuilt| {
                crate::present::render_rebuild(rebuilt, view.color, view.columns)
            })
        }
        Command::LocalCopies(LocalCopiesCommand::Import {
            config,
            path,
            repo,
            revision,
            volume,
        }) => {
            let imported = crate::vault::import_baseline(
                &located_config(&config)?,
                &path,
                &repo,
                &revision,
                &volume,
            )?;
            show(&view, &imported, true, |imported| {
                crate::present::render_import(imported, view.color, view.columns)
            })
        }
        Command::LocalCopies(LocalCopiesCommand::Local { command }) => match command {
            LocalCommand::List {
                ollama_dir,
                config,
                format,
            } => list_local(&view, ollama_dir, config, format),
        },
        Command::Setup(SetupCommand::Config {
            command:
                ConfigCommand::Init {
                    output,
                    catalog,
                    volume,
                },
        }) => {
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
            let body = json!({"schema_version": 1, "status": "config_created", "path": output, "vault_initialized": false});
            if view.text {
                Ok(CommandOutput::Text {
                    body: crate::present::render_config_created(&output, view.color, view.columns),
                    ok: true,
                })
            } else {
                Ok(CommandOutput::Json(body))
            }
        }
        Command::Setup(SetupCommand::Config {
            command: ConfigCommand::Check { config: path },
        }) => {
            let config = Config::parse(&read_text(&path)?)?;
            let body = json!({"schema_version": 1, "status": "config_valid", "watch_count": config.watches.len(), "volume_count": config.volumes.len()});
            if view.text {
                Ok(CommandOutput::Text {
                    body: crate::present::render_config_check(
                        config.watches.len(),
                        config.volumes.len(),
                        view.color,
                        view.columns,
                    ),
                    ok: true,
                })
            } else {
                Ok(CommandOutput::Json(body))
            }
        }
        Command::Preview(PreviewCommand::Plan {
            config,
            inventory,
            available_bytes,
            select,
        }) => {
            let config = Config::parse(&read_text(&config)?)?;
            let inventory = Inventory::parse(&read_text(&inventory)?)?;
            let plan = crate::build_plan_selecting(&config, &inventory, available_bytes, &select)?;
            show(&view, &plan, true, |plan| {
                crate::present::render_plan(plan, view.color, view.columns)
            })
        }
        Command::Preview(PreviewCommand::Budget { config, request }) => {
            let config = Config::parse(&read_text(&config)?)?;
            let report = crate::budget::report_json(&config, &read_text(&request)?)?;
            show(&view, &report, true, |report| {
                crate::present::render_budget(report, view.color, view.columns)
            })
        }
        Command::Preview(PreviewCommand::Kit { config, kit }) => {
            let config = Config::parse(&read_text(&config)?)?;
            let text = read_text(&kit)?;
            let parsed = crate::kit::parse(&text)?;
            let kit_dir = kit.parent().unwrap_or_else(|| Path::new("."));
            let report =
                crate::kit::report(&config, &parsed, &|path| load_kit_evidence(kit_dir, path))?;
            show(&view, &report, true, |report| {
                crate::present::render_kit(report, view.color, view.columns)
            })
        }
        Command::Preview(PreviewCommand::Readiness { recipe, path }) => {
            let recipe = crate::readiness::Recipe::parse(&read_text(&recipe)?)?;
            let report = crate::readiness::preflight(&recipe, &path)?;
            show(&view, &report, true, |report| {
                crate::present::render_readiness(report, view.color, view.columns)
            })
        }
        Command::Watches(WatchCommand::Watch { config, stage }) => {
            let report = crate::vault::run_watches(&located_config(&config)?, &stage)?;
            show(&view, &report, true, |report| {
                crate::present::render_watch(report, view.color, view.columns)
            })
        }
    }
}

fn show<T: Serialize>(
    view: &View,
    value: &T,
    ok: bool,
    render: impl FnOnce(&T) -> String,
) -> Result<CommandOutput> {
    if view.text {
        Ok(CommandOutput::Text {
            body: render(value),
            ok,
        })
    } else {
        serde_json::to_value(value)
            .map(CommandOutput::Json)
            .map_err(|error| Error::new("output_failed", error.to_string()))
    }
}

fn verify_local(
    view: &View,
    quiet: bool,
    config: Option<PathBuf>,
    plan: Option<String>,
    repo: Option<String>,
    revision: String,
    path: PathBuf,
) -> Result<CommandOutput> {
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
    let mut progress = crate::present::ProgressLine::stderr(
        view.stderr_terminal,
        quiet,
        view.columns,
        inventory.files.len(),
    );
    let report = crate::verify::verify_tree(&inventory, &path, &mut |file, done, total| {
        progress.update("verifying", file, done, total);
    })?;
    show(view, &report, report.ok, |report| {
        crate::present::render_verification(report, view.color, view.columns)
    })
}

fn adopt_local(
    view: &View,
    quiet: bool,
    config: &Path,
    plan: &str,
    path: &Path,
    move_source: bool,
) -> Result<CommandOutput> {
    let loaded = located_config(config)?;
    let expected = crate::vault::show_proposal(&loaded, plan)
        .map(|proposal| proposal.review.inventory.files.len())
        .unwrap_or(0);
    let mut progress =
        crate::present::ProgressLine::stderr(view.stderr_terminal, quiet, view.columns, expected);
    let outcome = crate::vault::adopt(
        &loaded,
        plan,
        path,
        move_source,
        &mut |phase, file, done, total| progress.update(phase, file, done, total),
    )?;
    match outcome {
        crate::vault::Outcome::Recorded(adoption) => show(view, &adoption, true, |adoption| {
            crate::present::render_adoption(adoption, view.color, view.columns)
        }),
        crate::vault::Outcome::Mismatch(verification) => {
            show(view, &verification, verification.ok, |verification| {
                crate::present::render_verification(verification, view.color, view.columns)
            })
        }
    }
}

fn list_local(
    view: &View,
    ollama_dir: Option<PathBuf>,
    config: Option<PathBuf>,
    format: Option<OutputFormat>,
) -> Result<CommandOutput> {
    let preserved = if let Some(config) = config {
        Some(crate::vault::preserved_digests(&located_config(&config)?)?)
    } else {
        None
    };
    let dirs = match ollama_dir {
        Some(path) => vec![path],
        None => crate::runtime::default_store_dirs(|key| std::env::var(key).ok()),
    };
    let report = crate::runtime::inventory(&dirs, preserved.as_ref())?;
    let text = !view.json
        && match format {
            Some(OutputFormat::Text) => true,
            Some(OutputFormat::Json) => false,
            None => view.text,
        };
    if text {
        Ok(CommandOutput::Text {
            body: crate::present::render_runtime(&report, view.color, view.columns),
            ok: true,
        })
    } else {
        serde_json::to_value(report)
            .map(CommandOutput::Json)
            .map_err(|error| Error::new("output_failed", error.to_string()))
    }
}

fn load_kit_evidence(kit_dir: &Path, path: &str) -> crate::kit::EvidenceLoad {
    let full = kit_dir.join(path);
    match read_text(&full) {
        Ok(text) => {
            crate::kit::EvidenceLoad::Loaded(Box::new(crate::kit::load_evidence_text(&text)))
        }
        Err(error) if error.code == "input_read_failed" => crate::kit::EvidenceLoad::Missing,
        Err(error) => crate::kit::EvidenceLoad::Unreadable(error.message),
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
