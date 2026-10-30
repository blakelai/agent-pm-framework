use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use pm_agent::{
    config::Config,
    provider::Event,
    runtime::{Runtime, Session, Status},
    store::Store,
    tools, tui,
    workspace::{Workspace, WorkspaceLock},
};
use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    version,
    about = "Local project management agent for Obsidian-compatible workspaces"
)]
struct Cli {
    #[arg(long, default_value = ".")]
    vault: PathBuf,
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Prepare, explicitly run, or inspect an isolated synthetic Agent evaluation.
    Eval {
        #[command(subcommand)]
        command: EvalCommand,
    },
    /// Inspect or migrate the runtime database without a model or provider configuration.
    Storage {
        #[command(subcommand)]
        command: StorageCommand,
    },
    /// Create, verify, or restore a private local project backup without a model.
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
    /// Start the terminal workspace browser and Agent.
    Tui,
    /// List project records as JSON, without contacting a model.
    Scan,
    /// Show durable Agent sessions.
    Sessions,
    /// Inspect operator limits and saved session usage without a model or MCP connection.
    Budget {
        #[arg(long)]
        session: Option<String>,
    },
    /// Discover project Skills and report invalid dependencies without contacting a model.
    Skills,
    /// Execute a deterministic native PM command without contacting a model.
    #[command(disable_help_flag = true)]
    Pm {
        command: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run or resume an Agent task; questions and approvals pause it durably.
    Chat {
        prompt: Option<String>,
        #[arg(long, default_value = "workflow")]
        stage: String,
        #[arg(long = "skill")]
        skills: Vec<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, conflicts_with = "deny")]
        approve: bool,
        #[arg(long)]
        deny: bool,
        #[arg(long)]
        reconcile: Option<String>,
        /// Continue evidence-backed independent work without answering open questions.
        #[arg(long, requires = "session", conflicts_with_all = ["prompt","answer","approve","deny","reconcile"])]
        independent: bool,
        /// Associate the prompt with this question ID or exact path.
        #[arg(long, requires_all = ["session","prompt"], conflicts_with_all = ["approve","deny","reconcile"])]
        answer: Option<String>,
    },
    /// Validate configuration and local prerequisites without making model requests.
    Doctor,
    /// Explicit synthetic provider check (up to 3 requests; service charges may apply).
    Probe {
        /// Select a configured profile for this check without changing the active profile.
        #[arg(long)]
        profile: Option<String>,
        /// Show the latest local observation without making requests.
        #[arg(long)]
        last: bool,
    },
}
#[derive(Subcommand)]
enum EvalCommand {
    /// List the built-in synthetic cases without reading a Vault or contacting a model.
    List,
    /// Copy framework instructions into a new private evaluation directory; no model request.
    Prepare {
        #[arg(long)]
        case: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_parser = ["en", "zh-TW"])]
        language: Option<String>,
    },
    /// Run a prepared case with the explicit --config provider. MCP is disabled.
    Run {
        directory: PathBuf,
        #[arg(long, default_value_t = 32)]
        max_requests: u64,
        #[arg(long, default_value_t = 600)]
        timeout_seconds: u64,
    },
    /// Read saved observations and check evidence freshness without a model request.
    Inspect { directory: PathBuf },
}
#[derive(Subcommand)]
enum StorageCommand {
    /// Read the supported schema state without initializing or migrating it.
    Inspect,
    /// Adopt the known legacy schema after saving a private safety snapshot.
    Migrate,
}
#[derive(Subcommand)]
enum BackupCommand {
    /// Back up the selected Vault to a new archive outside that Vault.
    Create {
        #[arg(long)]
        output: PathBuf,
        /// Include private conversations and the durable tool ledger.
        #[arg(long)]
        include_sessions: bool,
    },
    /// Verify archive structure and every file hash; no working Vault is needed.
    Check { archive: PathBuf },
    /// Restore to a new private child directory; no working Vault is needed.
    Restore {
        archive: PathBuf,
        #[arg(long)]
        parent: PathBuf,
        /// Restore saved sessions as well as project files; no tools are replayed.
        #[arg(long)]
        include_sessions: bool,
    },
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Command::Eval { command }) = &cli.command {
        use pm_agent::evaluation;
        let observation = match command {
            EvalCommand::List => {
                println!("{}", serde_json::to_string_pretty(&evaluation::cases())?);
                return Ok(());
            }
            EvalCommand::Prepare {
                case,
                output,
                language,
            } => {
                let source = Workspace::open(&cli.vault)?;
                let language = language.clone().unwrap_or(source.language()?);
                let path = evaluation::prepare(&source, case, output, &language).await?;
                println!(
                    "{}",
                    serde_json::json!({"directory":path,"state":"prepared","model_requests":0})
                );
                return Ok(());
            }
            EvalCommand::Inspect { directory } => evaluation::inspect(directory)?,
            EvalCommand::Run {
                directory,
                max_requests,
                timeout_seconds,
            } => {
                let config = Config::load(cli.config.as_deref().ok_or_else(|| {
                    anyhow::anyhow!("eval run requires an explicit --config path")
                })?)?;
                let cancel = CancellationToken::new();
                let signal = cancel.clone();
                tokio::spawn(async move {
                    let _ = tokio::signal::ctrl_c().await;
                    signal.cancel();
                });
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                let printer = tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        if let Event::Notice(text) = event
                            && (text.starts_with("Tool:") || text.starts_with("Active Skills:"))
                        {
                            eprintln!("{text}");
                        }
                    }
                });
                let result = evaluation::run(
                    directory,
                    config,
                    *max_requests,
                    *timeout_seconds,
                    &tx,
                    &cancel,
                )
                .await;
                drop(tx);
                printer.await?;
                result?
            }
        };
        println!("{}", serde_json::to_string_pretty(&observation)?);
        if !observation.current
            || matches!(command, EvalCommand::Run { .. })
                && observation.report.mechanical_outcome != evaluation::Outcome::Pass
        {
            std::process::exit(2);
        }
        return Ok(());
    }
    // Recovery must work even when the original Vault and provider configuration are gone.
    if let Some(Command::Backup { command }) = &cli.command {
        use pm_agent::backup;
        let report = match command {
            BackupCommand::Create {
                output,
                include_sessions,
            } => backup::create(&Workspace::open(&cli.vault)?, output, *include_sessions)?,
            BackupCommand::Check { archive } => backup::check(archive)?,
            BackupCommand::Restore {
                archive,
                parent,
                include_sessions,
            } => backup::restore(archive, parent, *include_sessions)?,
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let workspace = Workspace::open(&cli.vault)?;
    if let Some(Command::Storage { command }) = &cli.command {
        let path = workspace.path(".pm-agent/runtime.sqlite3")?;
        let report = match command {
            StorageCommand::Inspect => pm_agent::storage::inspect(&path)?,
            StorageCommand::Migrate => {
                let _lock = WorkspaceLock::acquire(&workspace)?;
                pm_agent::storage::migrate(&path)?
            }
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let lock = WorkspaceLock::acquire(&workspace)?;
    let state_dir = workspace.path(".pm-agent")?;
    let config_path = cli.config.unwrap_or_else(|| state_dir.join("config.toml"));
    match cli.command.unwrap_or(Command::Tui) {
        Command::Eval { .. } => {
            unreachable!("Evaluations are dispatched before opening the selected Vault")
        }
        Command::Backup { .. } => unreachable!("Backup is dispatched before opening runtime state"),
        Command::Storage { .. } => {
            unreachable!("Storage is dispatched before loading configuration")
        }
        Command::Budget { session } => {
            let config = Config::read(&config_path)?;
            let saved = match session {
                Some(id) => {
                    let path = workspace.path(".pm-agent/runtime.sqlite3")?;
                    ensure!(path.is_file(), "No saved sessions in this workspace");
                    Some(Store::open(&path)?.load::<Session>(&id)?)
                }
                None => None,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&pm_agent::budget::Report::new(
                    &config.interactive_limits(),
                    saved.as_ref()
                ))?
            );
        }
        Command::Probe { profile, last } => {
            let config = Config::read(&config_path)?;
            if last {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&pm_agent::probe::last(
                        &workspace,
                        &config,
                        profile.as_deref()
                    )?)?
                );
            } else {
                let cancel = CancellationToken::new();
                let signal = cancel.clone();
                tokio::spawn(async move {
                    let _ = tokio::signal::ctrl_c().await;
                    signal.cancel();
                });
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                let printer = tokio::spawn(async move {
                    while let Some(Event::Notice(text)) = rx.recv().await {
                        eprintln!("{text}");
                    }
                });
                let report =
                    pm_agent::probe::run(&config, profile.as_deref(), &tx, &cancel).await?;
                drop(tx);
                printer.await?;
                let passed = report.passed();
                let path = pm_agent::probe::save(&workspace, &report)?;
                let observation = pm_agent::probe::current(&config_path, report, path);
                println!("{}", serde_json::to_string_pretty(&observation)?);
                if !passed {
                    std::process::exit(2);
                }
            }
        }
        Command::Tui => {
            ensure!(
                std::io::stdin().is_terminal(),
                "TUI requires a terminal; use scan or chat for noninteractive access"
            );
            tui::run(workspace, config_path, lock).await?;
        }
        Command::Scan => {
            println!("{}", serde_json::to_string_pretty(&workspace.documents()?)?);
        }
        Command::Sessions => {
            let s = Store::open(&state_dir.join("runtime.sqlite3"))?;
            for (id, date) in s.sessions()? {
                println!("{id}\t{date}");
            }
        }
        Command::Skills => {
            let catalog = pm_agent::skills::Catalog::discover(&workspace)?;
            println!("{}", serde_json::to_string_pretty(&catalog)?);
            ensure!(
                catalog.errors.is_empty(),
                "Some Skills are invalid; see the reported errors"
            );
        }
        Command::Pm { command, args } => {
            let result = tools::run_pm(&workspace, &command, &args).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            if result["successful"] != true {
                std::process::exit(result["exit_code"].as_i64().unwrap_or(2) as i32);
            }
        }
        Command::Doctor => {
            let c = Config::load(&config_path)?;
            println!(
                "Workspace: {}\nProject language: {}\nProvider profile: {}\nProtocol: {:?}\nModel: {}",
                workspace.root.display(),
                workspace.language()?,
                c.provider,
                c.active()?.protocol,
                c.active()?.model
            );
            if let Some(key) = &c.active()?.api_key_env {
                ensure!(
                    std::env::var(key).is_ok_and(|v| !v.is_empty()),
                    "Missing environment variable {key}"
                );
                println!("API key environment variable: present (value hidden)");
            }
            let commands = tools::specs().into_iter().find(|t| t.name == "pm").unwrap();
            println!(
                "PM operations: native Rust ({} commands)",
                commands.parameters["properties"]["command"]["enum"]
                    .as_array()
                    .unwrap()
                    .len()
            );
            println!(
                "Local configuration passed. Provider/MCP live compatibility has NOT been tested."
            );
        }
        Command::Chat {
            prompt,
            stage,
            skills,
            session,
            approve,
            deny,
            reconcile,
            independent,
            answer,
        } => {
            let config = Config::load(&config_path)?;
            let runtime = Runtime::open(workspace, config).await?;
            let mut s = match session {
                Some(id) => runtime.store.load::<Session>(&id)?,
                None => Session::new(&runtime.config, &stage)?,
            };
            let mut approved = None;
            if !skills.is_empty() {
                ensure!(
                    s.pending.is_empty(),
                    "Cannot change Skills while a tool is pending"
                );
                pm_agent::skills::Catalog::discover(&runtime.workspace)?.resolve(&skills)?;
                for name in skills {
                    if !s.selected_skills.contains(&name) {
                        s.selected_skills.push(name);
                    }
                }
            }
            if approve || deny {
                if approve {
                    approved = s.pending.first().map(|c| c.id.clone());
                }
                runtime.decision(&mut s, approve)?;
            }
            if let Some(result) = reconcile {
                runtime.reconcile(&mut s, &result)?;
            }
            if independent {
                runtime.continue_independent(&mut s)?;
            } else if let Some(prompt) = prompt {
                if let Some(id) = answer {
                    runtime.answer(&mut s, &id, &prompt)?;
                } else {
                    runtime.input(&mut s, &prompt)?;
                }
            } else if matches!(
                s.status,
                Status::Failed | Status::Cancelled | Status::Completed | Status::LimitReached
            ) {
                s.status = Status::Ready;
            }
            let cancel = CancellationToken::new();
            let signal = cancel.clone();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                signal.cancel();
            });
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let printer = tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    match event {
                        Event::Text(text) => {
                            print!("{text}");
                            let _ = std::io::stdout().flush();
                        }
                        Event::Notice(text) => eprintln!("\n{text}"),
                        Event::Usage(_) => {}
                    }
                }
            });
            runtime.run(&mut s, approved, &tx, &cancel).await?;
            drop(tx);
            printer.await?;
            eprintln!(
                "\nSession: {}\nState: {:?}\nResume: pm-agent --vault <workspace> --config <config> chat --session {} [reply]",
                s.id, s.status, s.id
            );
            if matches!(
                s.status,
                Status::Failed
                    | Status::Cancelled
                    | Status::NeedsReconciliation
                    | Status::LimitReached
            ) {
                std::process::exit(2);
            }
        }
    }
    Ok(())
}
