mod json_lines;

use anyhow::{Context, Result, ensure};
use clap::Args;
use pm_agent::{
    config::Config,
    provider::Event,
    runtime::{Runtime, Session, Status},
    store::Store,
    workspace::{Workspace, WorkspaceLock},
};
use std::{io::Write, path::Path};
use tokio_util::sync::CancellationToken;

#[derive(Args)]
pub struct ChatArgs {
    prompt: Option<String>,
    #[arg(long, default_value = "workflow")]
    stage: String,
    #[arg(long = "skill")]
    skills: Vec<String>,
    #[arg(long)]
    session: Option<String>,
    #[arg(long, conflicts_with = "deny")]
    approve: bool,
    /// Confirm the exact PRD draft preview shown by Chat.
    #[arg(long, requires = "session", conflicts_with_all = ["prompt", "approve", "deny", "reconcile", "independent", "answer", "skills"])]
    apply_draft: Option<String>,
    #[arg(long)]
    deny: bool,
    #[arg(long)]
    reconcile: Option<String>,
    /// Continue evidence-backed independent work without answering open questions.
    #[arg(long, requires = "session", conflicts_with_all = ["prompt", "answer", "approve", "deny", "reconcile"])]
    independent: bool,
    /// Associate the prompt with this question ID or exact path.
    #[arg(long, requires_all = ["session", "prompt"], conflicts_with_all = ["approve", "deny", "reconcile"])]
    answer: Option<String>,
    /// Stream versioned execution events as JSON Lines on stdout.
    #[arg(long)]
    json: bool,
}

pub async fn run(args: &ChatArgs, vault: &Path, config: Option<&Path>) -> Result<i32> {
    let mut json = args.json.then(json_lines::Stream::new);
    match run_chat(args, vault, config, &mut json).await {
        Ok(code) => Ok(code),
        Err(error) => match &mut json {
            Some(stream) => {
                stream.error(&error)?;
                Ok(1)
            }
            None => Err(error),
        },
    }
}

async fn run_chat(
    args: &ChatArgs,
    vault: &Path,
    config: Option<&Path>,
    json: &mut Option<json_lines::Stream>,
) -> Result<i32> {
    let workspace = Workspace::open(vault)?;
    let default_config = workspace.path(".pm-agent")?.join("config.toml");
    let config = config.unwrap_or(&default_config);
    let _lock = WorkspaceLock::acquire(&workspace)?;

    if args.deny {
        let id = args
            .session
            .as_deref()
            .context("Use --session with --deny")?;
        let store = Store::open(&workspace.path(".pm-agent/runtime.sqlite3")?)?;
        let mut session: Session = store.load(id)?;
        if let Some(stream) = json {
            stream.start(&session)?;
        }
        let call = session
            .pending
            .first()
            .context("No call awaiting approval")?
            .clone();
        let draft = session.status == Status::WaitingDraftReview;
        pm_agent::approval::deny(&workspace, &store, &mut session, &call)?;
        if let Some(stream) = json {
            stream.finish(&workspace, &session)?;
        } else {
            eprintln!(
                "Session: {}\nState: Cancelled\n{}",
                session.id,
                if draft {
                    "Draft preview discarded; Agent response stopped."
                } else {
                    "Tool use denied; Agent response stopped."
                }
            );
        }
        return Ok(0);
    }

    // A saved pause can be inspected with JSON without loading provider or MCP configuration.
    if let Some(stream) = json
        && args.prompt.is_none()
        && !args.approve
        && args.apply_draft.is_none()
        && args.reconcile.is_none()
        && !args.independent
        && args.skills.is_empty()
        && let Some(id) = &args.session
    {
        let store = Store::open(&workspace.path(".pm-agent/runtime.sqlite3")?)?;
        let saved: Session = store.load(id)?;
        if matches!(
            saved.status,
            Status::WaitingClarification
                | Status::WaitingApproval
                | Status::WaitingDraftReview
                | Status::NeedsReconciliation
        ) {
            stream.start(&saved)?;
            stream.finish(&workspace, &saved)?;
            return Ok(exit_code(saved.status));
        }
    }

    let config = Config::load(config)?;
    let runtime = Runtime::open_chat(workspace, config).await?;
    let mut session = match &args.session {
        Some(id) => runtime.store.load::<Session>(id)?,
        None => Session::new(&runtime.config, &args.stage)?,
    };
    if let Some(stream) = json {
        stream.start(&session)?;
    }
    if !args.skills.is_empty() {
        ensure!(
            session.pending.is_empty(),
            "Cannot change Skills while a tool is pending"
        );
        pm_agent::skills::Catalog::discover(&runtime.workspace)?.resolve(&args.skills)?;
        for name in &args.skills {
            if !session.selected_skills.contains(name) {
                session.selected_skills.push(name.clone());
            }
        }
    }
    let approved = if args.approve {
        let approved = session.pending.first().map(|c| c.id.clone());
        runtime.decision(&mut session, true)?;
        approved
    } else {
        None
    };
    if let Some(preview_id) = &args.apply_draft {
        runtime.confirm_draft(&mut session, preview_id)?;
    }
    if let Some(result) = &args.reconcile {
        runtime.reconcile(&mut session, result)?;
    }
    if args.independent {
        runtime.continue_independent(&mut session)?;
    } else if let Some(prompt) = &args.prompt {
        if let Some(id) = &args.answer {
            runtime.answer(&mut session, id, prompt)?;
        } else {
            runtime.input(&mut session, prompt)?;
        }
    } else if matches!(
        session.status,
        Status::Failed | Status::Cancelled | Status::Completed | Status::LimitReached
    ) {
        session.status = Status::Ready;
    }

    if !(args.approve && session.status == Status::WaitingApproval
        || session.status == Status::WaitingDraftReview)
    {
        stream_run(&runtime, &mut session, approved, json).await?;
    } else if json.is_none()
        && let Some(error) = &session.error
    {
        eprintln!("{error}");
    }

    if let Some(stream) = json {
        stream.finish(&runtime.workspace, &session)?;
    } else {
        print_session(&runtime.workspace, &session)?;
    }
    Ok(exit_code(session.status))
}

async fn stream_run(
    runtime: &Runtime,
    session: &mut Session,
    approved: Option<String>,
    json: &mut Option<json_lines::Stream>,
) -> Result<()> {
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let signal_handler = tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        signal.cancel();
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let operation = runtime.run(session, approved, &tx, &cancel);
    tokio::pin!(operation);
    let result = loop {
        tokio::select! {
            result = &mut operation => break result,
            Some(event) = rx.recv() => {
                if let Err(error) = print_event(&runtime.workspace, json, event) {
                    cancel.cancel();
                    let _ = operation.await;
                    break Err(error);
                }
            }
        }
    };
    signal_handler.abort();
    // Drain events already queued before a runtime error or normal return.
    while let Ok(event) = rx.try_recv() {
        print_event(&runtime.workspace, json, event)?;
    }
    result
}

fn print_event(
    workspace: &Workspace,
    json: &mut Option<json_lines::Stream>,
    event: Event,
) -> Result<()> {
    if let Some(stream) = json {
        return stream.event(workspace, &event);
    }
    match event {
        Event::Text(text) => {
            print!("{text}");
            let _ = std::io::stdout().flush();
        }
        Event::Notice(text) => eprintln!("\n{text}"),
        Event::Tool { call, state, .. } => eprintln!("\nTool: {} — {state}", call.name),
        _ => {}
    }
    Ok(())
}

fn print_session(workspace: &Workspace, session: &Session) -> Result<()> {
    if session.status == Status::WaitingDraftReview {
        let preview = session
            .draft_preview
            .as_ref()
            .context("Missing saved draft preview")?;
        eprintln!(
            "\nDraft preview {}\n{}\nApply: pm-agent --vault <workspace> --config <config> chat --session {} --apply-draft {}\nDiscard: pm-agent --vault <workspace> chat --session {} --deny",
            preview.id,
            pm_agent::diagnostics::readable(&preview.text),
            session.id,
            preview.id,
            session.id
        );
    }
    eprintln!(
        "\nSession: {}\nState: {:?}\nResume: pm-agent --vault <workspace> --config <config> chat --session {} [reply]",
        session.id, session.status, session.id
    );
    if let Some(goal) = &session.goal {
        eprintln!("\n{}", goal.report(workspace).markdown());
    }
    Ok(())
}

fn exit_code(status: Status) -> i32 {
    match status {
        Status::Failed | Status::Cancelled | Status::NeedsReconciliation | Status::LimitReached => {
            2
        }
        _ => 0,
    }
}
