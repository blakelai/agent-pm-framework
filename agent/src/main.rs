mod chat_cli;

use anyhow::{Result, ensure};
use clap::{Args, Parser, Subcommand};
use pm_agent::{
    change_review,
    config::Config,
    goals::{self, Criteria},
    provider::Event,
    runtime::Session,
    store::Store,
    tools, tui,
    workspace::{Workspace, WorkspaceLock},
};
use std::{io::IsTerminal, path::PathBuf};
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
    Chat(chat_cli::ChatArgs),
    /// Set or inspect evidence-backed goals for a saved Chat session.
    Goal {
        #[command(subcommand)]
        action: GoalAction,
    },
    /// Compare controlled document versions and record version-pinned change reviews.
    ChangeReview {
        #[command(subcommand)]
        action: ReviewAction,
    },
    /// Compare and apply reviewed planning simulations.
    Scenario {
        #[command(subcommand)]
        action: ScenarioAction,
    },
    /// Validate configuration and selected Chat prerequisites without contacting services.
    Doctor {
        #[arg(long, default_value = "workflow")]
        stage: String,
    },
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
enum ScenarioAction {
    /// Compare assignment alternatives and save a pinned local simulation.
    Compare {
        specification: PathBuf,
        #[arg(long)]
        id: String,
    },
    /// Read a saved comparison.
    Show { comparison: String },
    /// Check one saved alternative for application and print the review hash.
    Review {
        comparison: String,
        scenario: String,
    },
    /// Apply the exact reviewed assignments after rechecking all inputs.
    Apply {
        comparison: String,
        scenario: String,
        #[arg(long)]
        expect_review: String,
    },
    /// Recover an interrupted scenario application with the local journal.
    Recover,
}
#[derive(Args)]
struct ReviewSelection {
    /// Stable ID of the controlled document.
    id: String,
    /// Manifest with an accepted approval decision.
    #[arg(long)]
    baseline: String,
    /// Captured candidate manifest. Omit to select the active document.
    #[arg(long)]
    candidate: Option<String>,
    /// Additional evidence record IDs to include in the review.
    #[arg(long)]
    evidence: Vec<String>,
}

impl From<ReviewSelection> for change_review::Request {
    fn from(selection: ReviewSelection) -> Self {
        Self {
            id: selection.id,
            baseline: selection.baseline,
            candidate: selection.candidate,
            evidence: selection.evidence,
        }
    }
}

#[derive(Subcommand)]
enum ReviewAction {
    /// Read the diff, recorded references and mechanical findings as JSON.
    Compare {
        #[command(flatten)]
        selection: ReviewSelection,
    },
    /// Save a new review report against an explicitly reviewed comparison basis.
    Record {
        #[command(flatten)]
        selection: ReviewSelection,
        #[arg(long)]
        review_id: String,
        /// Exact basis_sha256 returned by compare.
        #[arg(long)]
        expect: String,
        /// File containing operator-supplied semantic findings as JSON.
        #[arg(long)]
        findings: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum GoalAction {
    /// Save a goal and its exact draft scope. Starts a new session when no ID is supplied.
    Set {
        #[arg(long)]
        session: Option<String>,
        #[arg(long, default_value = "workflow", conflicts_with = "session")]
        stage: String,
        #[arg(long)]
        objective: String,
        #[arg(long, required = true)]
        draft: Vec<String>,
        /// Require a successful native validation after saving the specified drafts.
        #[arg(long)]
        validate: bool,
        #[arg(long)]
        json: bool,
    },
    /// Inspect a saved goal and the freshness of its receipts without loading a provider.
    Status {
        #[arg(long)]
        session: String,
        #[arg(long)]
        json: bool,
    },
    /// Clear the goal from a session with no pending tool decision.
    Clear {
        #[arg(long)]
        session: String,
        #[arg(long)]
        json: bool,
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
        /// Optional request cap per test stage. Omit for no request-count cap.
        #[arg(long)]
        max_requests: Option<u64>,
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
    if let Some(Command::Chat(args)) = &cli.command {
        let code = chat_cli::run(args, &cli.vault, cli.config.as_deref()).await?;
        if code != 0 {
            std::process::exit(code);
        }
        return Ok(());
    }
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
    let state_dir = workspace.path(".pm-agent")?;
    let config_path = cli.config.unwrap_or_else(|| state_dir.join("config.toml"));
    if let Some(Command::Doctor { stage }) = &cli.command {
        // Diagnosis must work without creating a session, spawning MCP or acquiring a writer.
        return pm_agent::doctor::run(&workspace, &config_path, stage);
    }
    let lock = WorkspaceLock::acquire(&workspace)?;
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
        Command::Doctor { .. } => unreachable!("Doctor runs before acquiring a writer"),
        Command::Chat(_) => unreachable!("Chat is dispatched with its own output mode"),
        Command::Scenario { action } => {
            use pm_agent::scenarios;
            let outcome = match action {
                ScenarioAction::Compare { specification, id } => {
                    serde_json::to_value(scenarios::compare(
                        &workspace,
                        &lock,
                        &id,
                        scenarios::Spec::read(&specification)?,
                    )?)?
                }
                ScenarioAction::Show { comparison } => {
                    serde_json::to_value(scenarios::load(&workspace, &comparison)?)?
                }
                ScenarioAction::Review {
                    comparison,
                    scenario,
                } => serde_json::to_value(scenarios::review(&workspace, &comparison, &scenario)?)?,
                ScenarioAction::Apply {
                    comparison,
                    scenario,
                    expect_review,
                } => scenarios::apply(&workspace, &lock, &comparison, &scenario, &expect_review)?,
                ScenarioAction::Recover => scenarios::recover(&workspace, &lock)?,
            };
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        }
        Command::ChangeReview { action } => match action {
            ReviewAction::Compare { selection } => {
                let review = change_review::load(&workspace, &selection.into())?;
                println!("{}", serde_json::to_string_pretty(&review)?);
            }
            ReviewAction::Record {
                selection,
                review_id,
                expect,
                findings,
            } => {
                let findings = findings
                    .as_deref()
                    .map(change_review::Observations::read)
                    .transpose()?;
                let receipt = change_review::record(
                    &workspace,
                    &selection.into(),
                    &review_id,
                    &expect,
                    findings.as_ref(),
                )?;
                println!("{}", serde_json::to_string_pretty(&receipt)?);
            }
        },
        Command::Goal { action } => {
            let store = Store::open(&workspace.path(".pm-agent/runtime.sqlite3")?)?;
            let (session, json) = match action {
                GoalAction::Set {
                    session,
                    stage,
                    objective,
                    draft,
                    validate,
                    json,
                } => {
                    let mut session = match session {
                        Some(id) => store.load::<Session>(&id)?,
                        None => Session::new(&Config::read(&config_path)?, &stage)?,
                    };
                    goals::set(
                        &workspace,
                        &mut session,
                        Criteria {
                            objective,
                            drafts: draft,
                            require_validation: validate,
                        },
                    )?;
                    store.save(&session.id, &session)?;
                    (session, json)
                }
                GoalAction::Status { session, json } => (store.load::<Session>(&session)?, json),
                GoalAction::Clear { session, json } => {
                    let mut session: Session = store.load(&session)?;
                    goals::clear(&mut session)?;
                    store.save(&session.id, &session)?;
                    (session, json)
                }
            };
            let goal = session.goal.as_ref().map(|goal| goal.report(&workspace));
            if json {
                println!(
                    "{}",
                    serde_json::json!({"session_id": session.id,"session_status":session.status,"goal":goal})
                );
            } else {
                println!("Session {} · {:?}", session.id, session.status);
                if let Some(goal) = goal {
                    print!("\n{}", goal.markdown());
                } else {
                    println!("No saved task goal.");
                }
            }
        }
    }
    Ok(())
}
