use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use pm_agent::{
    config::Config,
    provider::Event,
    runtime::{Runtime, Session, Status},
    store::Store,
    tools, tui,
    workspace::Workspace,
};
use std::{
    fs::OpenOptions,
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
    /// Start the terminal workspace browser and Agent.
    Tui,
    /// List project records as JSON, without contacting a model.
    Scan,
    /// Show durable Agent sessions.
    Sessions,
    /// Execute a deterministic PM command (Python 3.10+ required).
    Pm {
        #[arg(long, default_value = "python3")]
        python: String,
        command: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Run or resume an Agent task; questions and approvals pause it durably.
    Chat {
        prompt: Option<String>,
        #[arg(long, default_value = "workflow")]
        stage: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, conflicts_with = "deny")]
        approve: bool,
        #[arg(long)]
        deny: bool,
        #[arg(long)]
        reconcile: Option<String>,
    },
    /// Validate configuration and local prerequisites without making model requests.
    Doctor,
}
#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let workspace = Workspace::open(&cli.vault)?;
    let state_dir = workspace.path(".pm-agent")?;
    std::fs::create_dir_all(&state_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(workspace.path(".pm-agent/workspace.lock")?)?;
    lock.try_lock().context(
        "Another PM Agent process is using this workspace; close it before opening another writer",
    )?;
    let config_path = cli.config.unwrap_or_else(|| state_dir.join("config.toml"));
    match cli.command.unwrap_or(Command::Tui) {
        Command::Tui => {
            ensure!(
                std::io::stdin().is_terminal(),
                "TUI requires a terminal; use scan or chat for noninteractive access"
            );
            tui::run(workspace, config_path).await?;
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
        Command::Pm {
            python,
            command,
            args,
        } => {
            let result = tools::run_pm(&workspace, &python, &command, &args).await?;
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
            let output = tokio::process::Command::new(&c.python)
                .args([
                    "-c",
                    "import sys; assert sys.version_info >= (3,10); print(sys.version.split()[0])",
                ])
                .output()
                .await?;
            ensure!(
                output.status.success(),
                "Python 3.10+ is required for built-in PM tools"
            );
            println!("Python: {}", String::from_utf8_lossy(&output.stdout).trim());
            println!(
                "Local configuration passed. Provider/MCP live compatibility has NOT been tested."
            );
        }
        Command::Chat {
            prompt,
            stage,
            session,
            approve,
            deny,
            reconcile,
        } => {
            let config = Config::load(&config_path)?;
            let runtime = Runtime::open(workspace, config).await?;
            let mut s = match session {
                Some(id) => runtime.store.load::<Session>(&id)?,
                None => Session::new(&runtime.config, &stage)?,
            };
            let mut approved = None;
            if approve || deny {
                if approve {
                    approved = s.pending.first().map(|c| c.id.clone());
                }
                runtime.decision(&mut s, approve)?;
            }
            if let Some(result) = reconcile {
                runtime.reconcile(&mut s, &result)?;
            }
            if let Some(prompt) = prompt {
                runtime.input(&mut s, &prompt)?;
            } else if matches!(
                s.status,
                Status::Failed | Status::Cancelled | Status::Completed
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
                Status::Failed | Status::Cancelled | Status::NeedsReconciliation
            ) {
                std::process::exit(2);
            }
        }
    }
    Ok(())
}
