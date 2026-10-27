mod delivery;
mod editor;
mod form;
mod inspection;
mod markdown;
mod materials;
mod planner;
mod questions;
mod recovery;
mod refresh;
mod view;
use crate::{
    config::Config,
    provider::Event,
    runtime::{Runtime, STAGES, Session, Status},
    skills::Catalog,
    store::Store,
    tools,
    workspace::{Workspace, WorkspaceLock},
};
use anyhow::{Context, Result, bail};
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        EventStream, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
};
use futures_util::StreamExt;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
pub use view::App;
use view::{Command, HELP};

enum Outcome {
    Session(Box<Session>),
    Pm(String),
    Probe(crate::probe::Observation),
    Error(String),
}
enum Action {
    Message(String),
    Decision(bool),
    Continue,
    Independent,
    Answer(String, String),
    Reconcile(String),
}
struct InputModes;
impl Drop for InputModes {
    fn drop(&mut self) {
        let _ = execute!(
            std::io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableMouseCapture,
            DisableBracketedPaste
        );
        ratatui::restore();
    }
}
pub async fn run(workspace: Workspace, config_path: PathBuf, lock: WorkspaceLock) -> Result<()> {
    lock.verify(&workspace)?;
    let mut app = App::new(workspace.documents()?);
    if let Ok(config) = Config::load(&config_path) {
        app.model = config.active()?.model.clone();
    }
    let mut store = Store::open(&workspace.path(".pm-agent/runtime.sqlite3")?)?;
    store.reindex(&app.docs)?;
    let mut monitor = refresh::Monitor::new(&workspace, &app.docs);
    let mut terminal = ratatui::init();
    let _modes = InputModes;
    execute!(
        std::io::stdout(),
        EnableMouseCapture,
        EnableBracketedPaste,
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        )
    )?;
    let mut events = EventStream::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<Outcome>();
    let mut cancellation = CancellationToken::new();
    let mut repaint = tokio::time::interval(std::time::Duration::from_millis(33));
    let mut dirty = true;
    let mut polling = tokio::time::interval(refresh::INTERVAL);
    polling.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (refresh_tx, mut refresh_rx) = mpsc::unbounded_channel();
    let (inspect_tx, mut inspect_rx) = mpsc::unbounded_channel();
    let mut inspection_running = false;
    let mut inspection_ticket = 0u64;
    let (material_tx, mut material_rx) = mpsc::unbounded_channel();
    let mut material_running = false;
    let mut material_ticket = 0u64;
    let (recovery_tx, mut recovery_rx) = mpsc::unbounded_channel();
    loop {
        tokio::select! {
            _ = polling.tick(), if !app.recovery.as_ref().is_some_and(|p| p.running()) => {
                if let Some(request) = monitor.request(refresh::Views::of(&app), app.refresh_error.is_some()) {
                    let workspace = workspace.clone();
                    let sender = refresh_tx.clone();
                    tokio::spawn(async move {
                        let read = request.clone();
                        let result = tokio::task::spawn_blocking(move || refresh::scan(&workspace, &read))
                            .await.context("Refresh worker failed").and_then(|r| r);
                        let _ = sender.send((request, result));
                    });
                }
            },
            Some((request, result)) = refresh_rx.recv() => {
                dirty |= monitor.finish(&request, result, &mut app, &mut store);
            },
            Some((ticket, result)) = inspect_rx.recv() => {
                inspection_running = false;
                if let Some(inspector) = &mut app.inspector { dirty |= inspector.finish(ticket, result); }
            },
            Some((ticket, result)) = material_rx.recv() => {
                material_running = false;
                if let Some(panel) = &mut app.materials {dirty |= panel.finish(ticket, result);}
            },
            Some(result) = recovery_rx.recv() => {
                if let Some(panel) = &mut app.recovery { panel.finish(result); }
                app.busy = false;
                monitor.invalidate();
                dirty = true;
            },
            _ = repaint.tick() => {
                if dirty { terminal.draw(|f| app.draw(f))?; dirty = false; }
            },
            Some(event)=rx.recv()=>{ dirty = true; match event {
                Event::Text(text)=>app.append(&text),
                Event::Notice(text)=>app.notice(&text),
                Event::Usage(usage)=>app.usage=usage,
            }},
            Some(outcome)=done_rx.recv()=>{
                dirty = true;
                monitor.invalidate();
                app.busy=false;
                match outcome {
                    Outcome::Session(s)=>{
                        app.status=format!("{:?} — session {}",s.status,s.id);
                        app.usage=s.usage.clone();
                        app.session=Some(*s);
                    },
                    Outcome::Pm(text)=>app.notice(&format!("PM operation finished; inspect exit_code and reports\n\n{text}")),
                    Outcome::Probe(observation)=>{
                        app.notice(&observation.markdown());
                        app.status=if observation.report.cancelled() {"Probe cancelled; partial observations saved"} else {"Probe finished; review individual observations"}.into();
                    },
                    Outcome::Error(text)=>app.notice(&format!("Error: {text}")),
                }
                refresh(&workspace,&mut app,&mut store);
            },
            event=events.next()=>{
                dirty = true;
                let Some(event)=event else {break;};
                let command = app.event(event?);
                if command != Command::None { monitor.invalidate(); }
                match command {
                    Command::Recovery => app.recovery=Some(recovery::Recovery::default()),
                    Command::CloseRecovery => app.recovery=None,
                    Command::RecoveryJob(job) => {
                        if let Some(panel)=&mut app.recovery && !app.busy {
                            let cancel=CancellationToken::new();
                            panel.start(&job,cancel.clone());
                            app.busy=true;
                            let workspace=workspace.clone();let lock=lock.clone();let sender=recovery_tx.clone();
                            tokio::spawn(async move {
                                let result=tokio::task::spawn_blocking(move || recovery::execute(&workspace,&lock,job,&cancel))
                                    .await.context("Recovery worker failed").and_then(|r|r);
                                let _=sender.send(result);
                            });
                        }
                    },
                    Command::None=>{},
                    Command::Quit=>break,
                    Command::Cancel=>{cancellation.cancel();app.status="Cancelling — waiting for the operation outcome".into();},
                    Command::Refresh=>refresh(&workspace,&mut app,&mut store),
                    Command::Open(target)=>{
                        let result=if target.starts_with("https://") || target.starts_with("http://") {open_document(PathBuf::from(target))} else {workspace.path(&target).and_then(open_document)};
                        if let Err(e)=result {app.status=format!("Open failed: {e}");}
                    },
                    Command::Skills=>match Catalog::discover(&workspace) {Ok(c)=>app.skills_menu(c),Err(e)=>app.notice(&e.to_string())},
                    Command::Planning=>{
                        let snapshot=crate::schedule::Snapshot::load(&workspace);
                        if let Some(planner)=&mut app.planner {planner.replace(snapshot);} else {app.planner=Some(planner::Planner::new(snapshot));}
                    },
                    Command::ClosePlanning=>app.planner=None,
                    Command::Delivery=>{
                        let snapshot=crate::delivery::Snapshot::load(&workspace);
                        if let Some(delivery)=&mut app.delivery {delivery.replace(snapshot);} else {app.delivery=Some(delivery::Delivery::new(snapshot));}
                    },
                    Command::CloseDelivery=>app.delivery=None,
                    Command::Questions=>{
                        let snapshot=crate::clarification::Snapshot::load(&workspace);
                        if let Some(q)=&mut app.questions {q.replace(snapshot);} else {app.questions=Some(questions::Questions::new(snapshot));}
                    }
                    Command::CloseQuestions=>app.questions=None,
                    Command::Materials=>{
                        let catalog=crate::materials::Catalog::load(&workspace);
                        if let Some(panel)=&mut app.materials {panel.replace(catalog);} else {app.materials=Some(materials::Materials::new(catalog));}
                    },
                    Command::CloseMaterials=>app.materials=None,
                    Command::MaterialReview{id,hash}=>match crate::editing::Edit::material_review(&workspace,&id,&hash) {
                        Ok(edit)=>app.form=Some(form::Form::new(edit)),
                        Err(e)=>if let Some(panel)=&mut app.materials {panel.message=format!("Review unavailable: {e:#}");},
                    },
                    Command::MaterialInspect(id)=>{
                        if let Some(panel)=&mut app.materials {
                            if material_running {panel.message="Previous check is finishing; R retries.".into();}
                            else {
                                material_running=true;material_ticket=material_ticket.wrapping_add(1);
                                let ticket=material_ticket;panel.start(ticket);
                                let workspace=workspace.clone();let sender=material_tx.clone();
                                tokio::spawn(async move {
                                    let result=tokio::task::spawn_blocking(move || crate::materials::inspect(&workspace,&id))
                                        .await.context("Material inspection worker failed").and_then(|r|r);
                                    let _=sender.send((ticket,result));
                                });
                            }
                        }
                    },
                    Command::MaterialOpen{id,note_hash,sha256}=>{
                        let result=crate::materials::open_original(&workspace,&id,&note_hash,&sha256).and_then(open_document);
                        if let Err(e)=result && let Some(panel)=&mut app.materials {
                            panel.invalidate();panel.message=format!("Original not opened: {e}");
                        }
                    },
                    Command::Inspect(request)=>{
                        if app.inspector.is_none() {app.inspector=Some(inspection::Inspector::new(request.clone()));}
                        let inspector=app.inspector.as_mut().unwrap();
                        if inspection_running {
                            inspector.message="Previous inspection is finishing; R retries when it completes".into();
                        } else {
                            inspection_ticket=inspection_ticket.wrapping_add(1);
                            inspector.start(request.clone(),inspection_ticket);
                            inspection_running=true;
                            let workspace=workspace.clone();let sender=inspect_tx.clone();let ticket=inspection_ticket;
                            tokio::spawn(async move {
                                let result=tokio::task::spawn_blocking(move || crate::inspection::load(&workspace,&request))
                                    .await.context("Inspection worker failed").and_then(|r|r);
                                let _=sender.send((ticket,result));
                            });
                        }
                    },
                    Command::CloseInspection=>app.inspector=None,
                    Command::Read(path)=>{refresh(&workspace,&mut app,&mut store);app.read_document(&path);},
                    Command::Schedule{path,hash,sprint}=>match crate::schedule::propose(&workspace,&path,&hash,sprint) {
                        Ok(edit)=>{
                            let reviewed=edit.preview(&workspace);
                            let mut form=form::Form::proposed(edit);
                            match reviewed {Ok(p)=>form.preview=Some(p),Err(e)=>form.message=format!("{e:#}")};
                            app.form=Some(form);
                        },
                        Err(e)=>if let Some(planner)=&mut app.planner {planner.message=format!("{e:#}");},
                    },
                    Command::Edit(path)=>match crate::editing::Edit::load(&workspace,&path) {
                        Ok(edit)=>app.form=Some(form::Form::new(edit)),
                        Err(e)=>{
                            app.status=format!("Edit unavailable: {e}");
                            if let Some(planner)=&mut app.planner {planner.message=app.status.clone();}
                            if let Some(delivery)=&mut app.delivery {delivery.message=app.status.clone();}
                            if let Some(panel)=&mut app.materials {panel.message=app.status.clone();}
                        },
                    },
                    Command::New(kind)=>match crate::editing::Edit::create(&workspace,&kind) {
                        Ok(edit)=>app.form=Some(form::Form::new(edit)),
                        Err(e)=>{
                            app.status=format!("Create unavailable: {e}");
                            if let Some(delivery)=&mut app.delivery {delivery.message=app.status.clone();}
                        },
                    },
                    Command::ReviewEdit=>if let Some(form)=&mut app.form {
                        match form.edit.preview(&workspace) {
                            Ok(preview)=>{form.preview=Some(preview);form.message.clear();},
                            Err(e)=>form.message=format!("{e:#}"),
                        }
                    },
                    Command::SaveEdit=>{
                        let result=app.form.as_ref().and_then(|form|form.preview.as_ref().map(|p|form.edit.save(&workspace,p)));
                        match result {
                            Some(Ok(saved))=>{
                                app.form=None;
                                refresh(&workspace,&mut app,&mut store);
                                app.status=saved.warning.unwrap_or_else(||format!("Saved {} · audit {} · /pm plan refreshes forecasts",saved.path,saved.audit_path.unwrap_or_default()));
                                if !app.visible().iter().any(|d|d.path==saved.path) {app.tab=0;app.filter.clear();}
                                app.selected=app.visible().iter().position(|d|d.path==saved.path).unwrap_or(0);
                                app.scroll=0;
                                if let Some(planner)=&mut app.planner {planner.message=app.status.clone();}
                                if let Some(delivery)=&mut app.delivery {delivery.select_path(&saved.path);delivery.message=app.status.clone();}
                                if let Some(panel)=&mut app.materials {panel.message=format!("{} · R reloads review history",app.status);}
                            },
                            Some(Err(e))=>if let Some(form)=&mut app.form {form.preview=None;form.message=format!("{e:#}");},
                            None=>{},
                        }
                    },
                    Command::CloseEdit=>app.form=None,
                    Command::Submit(input)=>{
                        if input.trim()=="/quit" {break;}
                        app.append(&format!("\n\n## You\n\n{input}\n\n"));
                        let result=dispatch(&workspace,&config_path,&mut app,&store,input.clone(),&tx,&done_tx);
                        match result {
                            Ok(Some(token))=>{cancellation=token;app.busy=true;app.append("## Agent\n\n");},
                            Ok(None)=>{},
                            Err(e)=>{app.input.insert(&input);app.notice(&format!("Error: {e}"));},
                        }
                    },
                }
            },
        }
    }
    Ok(())
}
fn refresh(workspace: &Workspace, app: &mut App, store: &mut Store) {
    refresh::manual(workspace, app, store);
}

fn session_transcript(s: &Session) -> String {
    let mut text = String::new();
    for item in &s.history {
        let role = item["role"].as_str().unwrap_or("");
        if !matches!(role, "user" | "assistant") {
            continue;
        }
        let content = if let Some(s) = item["content"].as_str() {
            s.to_string()
        } else {
            item["content"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|i| i["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()
        };
        let content = content
            .split("\n\n[Application source locator:")
            .next()
            .unwrap_or(&content);
        if !content.is_empty() {
            text.push_str(&format!(
                "\n\n## {}\n\n{content}",
                if role == "user" { "You" } else { "Agent" }
            ));
        }
    }
    if text.is_empty() {
        text = s.last_output.clone();
    }
    text
}

fn open_document(path: PathBuf) -> Result<()> {
    let executable = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(executable)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?
        .wait()?;
    Ok(())
}
fn dispatch(
    workspace: &Workspace,
    config_path: &std::path::Path,
    app: &mut App,
    store: &Store,
    input: String,
    tx: &mpsc::UnboundedSender<Event>,
    done: &mpsc::UnboundedSender<Outcome>,
) -> Result<Option<CancellationToken>> {
    if input == "/skills" {
        app.skills_menu(Catalog::discover(workspace)?);
        return Ok(None);
    }
    if input == "/skills clear" {
        if let Some(s) = &mut app.session {
            anyhow::ensure!(
                s.pending.is_empty(),
                "Cannot change Skills while a tool is pending"
            );
            s.selected_skills.clear();
            store.save(&s.id, s)?;
        }
        app.status = "Custom Skills cleared; the stage Skill remains active".into();
        return Ok(None);
    }
    if input == "/status" {
        app.append(&app.status_details());
        return Ok(None);
    }
    if input == "/budget" {
        let config = Config::read(config_path)?;
        app.notice(
            &crate::budget::Report::new(&config.session_limits, app.session.as_ref()).markdown(),
        );
        return Ok(None);
    }
    if input == "/help" {
        app.append(&format!("\n{HELP}\n"));
        return Ok(None);
    }
    if let Some(query) = input.strip_prefix("/search ") {
        app.filter = query.trim().into();
        app.selected = 0;
        return Ok(None);
    }
    if input == "/search" {
        app.filter.clear();
        return Ok(None);
    }
    if let Some(stage) = input.strip_prefix("/stage ") {
        if app.session.is_some() {
            bail!("Use /new before selecting a different stage");
        }
        if !STAGES.iter().any(|(s, _, _)| *s == stage) {
            bail!(
                "Stages: workflow, intake, system, language, brd, prd, backlog, estimate, review"
            );
        }
        app.stage = stage.into();
        app.status = format!("Stage: {stage}");
        return Ok(None);
    }
    if input == "/sessions" {
        for (id, date) in store.sessions()? {
            app.append(&format!("\n{id}  {date}"));
        }
        return Ok(None);
    }
    if input == "/new" {
        app.session = None;
        app.usage = None;
        app.output.clear();
        app.append("# New session\n");
        app.status = "New session; previous sessions remain saved".into();
        return Ok(None);
    }
    if let Some(id) = input.strip_prefix("/resume ") {
        let s: Session = store.load(id.trim())?;
        app.status = format!(
            "Loaded {:?}; /continue or answer its pending question",
            s.status
        );
        app.stage = s.stage.clone();
        app.usage = s.usage.clone();
        app.output.clear();
        app.append(&session_transcript(&s));
        for q in s.questions.iter().filter(|q| !q.resolved) {
            app.append(&format!("\nOpen question: {}\n", q.question));
        }
        if let Some(c) = s.pending.first() {
            app.append(&format!("\nPending tool: {}\n{}\n", c.name, c.arguments));
        }
        app.session = Some(s);
        return Ok(None);
    }
    if input == "/probe" || input.starts_with("/probe ") {
        let args = input
            .strip_prefix("/probe")
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>();
        let (last, profile) = match args.as_slice() {
            [] => (false, None),
            ["last"] => (true, None),
            ["last", name] => (true, Some(*name)),
            [name] => (false, Some(*name)),
            _ => bail!("Use /probe [PROFILE] or /probe last [PROFILE]"),
        };
        let config = Config::read(config_path)?;
        if last {
            app.notice(&crate::probe::last(workspace, &config, profile)?.markdown());
            return Ok(None);
        }
        // Isolated diagnostics cannot enter Runtime, append conversation history,
        // change the session's usage/model or initialize configured MCP servers.
        let profile = profile.map(String::from);
        let workspace = workspace.clone();
        let config_path = config_path.to_owned();
        let tx = tx.clone();
        let done = done.clone();
        let cancel = CancellationToken::new();
        let signal = cancel.clone();
        tokio::spawn(async move {
            let result = async {
                let report = crate::probe::run(&config, profile.as_deref(), &tx, &signal).await?;
                let path = crate::probe::save(&workspace, &report)?;
                Ok::<_, anyhow::Error>(crate::probe::current(&config_path, report, path))
            }
            .await;
            let _ = done.send(match result {
                Ok(observation) => Outcome::Probe(observation),
                Err(e) => Outcome::Error(e.to_string()),
            });
        });
        return Ok(Some(cancel));
    }
    let config = Config::load(config_path);
    if let Some(command) = input.strip_prefix("/pm ") {
        let parts = command
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        let command = parts.first().context("Missing PM command")?.clone();
        // More complex file paths/arguments use the CLI, which preserves shell quoting.
        let args = parts[1..].to_vec();
        let workspace = workspace.clone();
        let done = done.clone();
        let cancel = CancellationToken::new();
        let signal = cancel.clone();
        tokio::spawn(async move {
            let result = tokio::select! {_=signal.cancelled()=>Err(anyhow::anyhow!("PM operation cancelled; inspect any generated files before repeating it")),r=tools::run_pm(&workspace,&command,&args)=>r};
            let _ = done.send(match result {
                Ok(v) => Outcome::Pm(serde_json::to_string_pretty(&v).unwrap_or_default()),
                Err(e) => Outcome::Error(e.to_string()),
            });
        });
        return Ok(Some(cancel));
    }
    let config = config?;
    app.model = config.active()?.model.clone();
    let action = match input.as_str() {
        "/approve" => Action::Decision(true),
        "/deny" => Action::Decision(false),
        "/continue" => Action::Continue,
        "/continue-independent" => Action::Independent,
        _ => {
            if let Some(answer) = input.strip_prefix("/answer ") {
                let (id, text) = answer
                    .split_once(char::is_whitespace)
                    .context("Use /answer QUESTION-ID TEXT")?;
                Action::Answer(id.into(), text.trim().into())
            } else if let Some(result) = input.strip_prefix("/reconcile ") {
                Action::Reconcile(result.into())
            } else if input.starts_with('/') {
                bail!("Unknown command; use /help")
            } else {
                Action::Message(input)
            }
        }
    };
    let mut session = app
        .session
        .clone()
        .map(Ok)
        .unwrap_or_else(|| Session::new(&config, &app.stage))?;
    if let Action::Message(input) = &action {
        let requested = crate::skills::requested(input)?;
        Catalog::discover(workspace)?.resolve(&requested)?;
    }
    app.session = Some(session.clone());
    let workspace = workspace.clone();
    let tx = tx.clone();
    let done = done.clone();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        let result = async {
            let runtime = Runtime::open(workspace, config).await?;
            let mut approved = None;
            match action {
                Action::Message(input) => runtime.input(&mut session, &input)?,
                Action::Independent => runtime.continue_independent(&mut session)?,
                Action::Answer(id, text) => runtime.answer(&mut session, &id, &text)?,
                Action::Decision(yes) => {
                    if yes {
                        approved = session.pending.first().map(|c| c.id.clone());
                    }
                    runtime.decision(&mut session, yes)?;
                }
                Action::Continue => {
                    if matches!(
                        session.status,
                        Status::Failed
                            | Status::Cancelled
                            | Status::Completed
                            | Status::LimitReached
                    ) {
                        session.status = Status::Ready;
                    }
                }
                Action::Reconcile(result) => runtime.reconcile(&mut session, &result)?,
            }
            runtime.run(&mut session, approved, &tx, &signal).await
        }
        .await;
        let _ = done.send(match result {
            Ok(()) => Outcome::Session(Box::new(session)),
            Err(e) => Outcome::Error(e.to_string()),
        });
    });
    Ok(Some(cancel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Document;
    #[test]
    fn budget_query_is_local_and_preserves_chat_input_context_and_session() {
        let dir = tempfile::tempdir().unwrap();
        crate::workspace::atomic_write(&dir.path().join("00_Governance/Project.md"),crate::workspace::note(serde_json::json!({"type":"project","id":"PROJECT-TEST","language":"en","title":"Synthetic"}),"# Synthetic").unwrap().as_bytes()).unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let path = w.path(".pm-agent/config.toml").unwrap();
        crate::workspace::atomic_write(
            &path,
            br#"
provider = "fixture"
[session_limits]
max_requests = 0
[providers.fixture]
protocol = "chat_completions"
base_url = "http://127.0.0.1:9/v1"
model = ""
[mcp.invalid]
transport = "stdio"
command = "this-command-must-not-run"
"#,
        )
        .unwrap();
        let config = Config::read(&path).unwrap();
        let mut app = App::new(vec![]);
        app.session = Some(Session::new(&config, "prd").unwrap());
        app.input.insert("保留中文草稿");
        app.usage = Some(crate::provider::Usage {
            input_tokens: Some(77),
            output_tokens: None,
            total_tokens: None,
        });
        let session = serde_json::to_value(&app.session).unwrap();
        let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (done, mut finished) = mpsc::unbounded_channel();
        assert!(
            dispatch(&w, &path, &mut app, &store, "/budget".into(), &tx, &done)
                .unwrap()
                .is_none()
        );
        assert!(app.output.contains("Request limit: 0"));
        assert!(app.output.contains("limit reached"));
        assert_eq!(app.input.text, "保留中文草稿");
        assert_eq!(app.usage.unwrap().input_tokens, Some(77));
        assert_eq!(serde_json::to_value(&app.session).unwrap(), session);
        assert!(store.sessions().unwrap().is_empty());
        assert!(rx.try_recv().is_err());
        assert!(finished.try_recv().is_err());
    }
    #[tokio::test]
    async fn probe_cancel_and_local_replay_preserve_session_model_and_usage() {
        let dir = tempfile::tempdir().unwrap();
        crate::workspace::atomic_write(&dir.path().join("00_Governance/Project.md"),crate::workspace::note(serde_json::json!({"type":"project","id":"PROJECT-TEST","language":"en","title":"Synthetic"}),"# Synthetic").unwrap().as_bytes()).unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let config = Config {
            provider: "fixture".into(),
            max_steps: 32,
            session_limits: Default::default(),
            providers: std::collections::BTreeMap::from([(
                "fixture".into(),
                crate::config::ProviderConfig {
                    protocol: crate::config::Protocol::ChatCompletions,
                    base_url: "http://127.0.0.1:9/v1".into(),
                    model: "fixture-model".into(),
                    api_key_env: None,
                    tools: true,
                    request_usage: true,
                    timeout_seconds: 1,
                },
            )]),
            mcp: Default::default(),
        };
        let path = w.path(".pm-agent/config.toml").unwrap();
        crate::workspace::atomic_write(&path, toml::to_string(&config).unwrap().as_bytes())
            .unwrap();
        let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
        let mut app = App::new(vec![]);
        app.session = Some(Session::new(&config, "prd").unwrap());
        app.model = "current-session-model".into();
        app.usage = Some(crate::provider::Usage {
            input_tokens: Some(123),
            output_tokens: None,
            total_tokens: None,
        });
        app.input.insert("Unsent 中文");
        let before = serde_json::to_value(&app.session).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (done, mut finished) = mpsc::unbounded_channel();
        let cancel = dispatch(&w, &path, &mut app, &store, "/probe".into(), &tx, &done)
            .unwrap()
            .unwrap();
        cancel.cancel();
        let Outcome::Probe(o) = finished.recv().await.unwrap() else {
            panic!("Expected isolated probe observation");
        };
        assert!(o.report.cancelled());
        assert_eq!(o.configuration_matches, Some(true));
        assert!(std::iter::from_fn(|| rx.try_recv().ok()).all(|e| matches!(e, Event::Notice(_))));
        assert!(
            dispatch(
                &w,
                &path,
                &mut app,
                &store,
                "/probe last".into(),
                &tx,
                &done
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(serde_json::to_value(&app.session).unwrap(), before);
        assert_eq!(app.model, "current-session-model");
        assert_eq!(app.usage.unwrap().input_tokens, Some(123));
        assert_eq!(app.input.text, "Unsent 中文");
        assert!(store.sessions().unwrap().is_empty());
    }
    #[test]
    fn project_views_follow_types_and_exclude_templates() {
        let doc = |path: &str, kind: &str| Document {
            path: path.into(),
            title: path.into(),
            kind: kind.into(),
            id: None,
            hash: String::new(),
            metadata: serde_json::json!({"type": kind}),
            body: String::new(),
        };
        let mut app = App::new(vec![
            doc("04_Requirements/discovery/Business.md", "brd"),
            doc("04_Requirements/discovery/Product.md", "prd"),
            doc("04_Requirements/REQ-LOOKUP.md", "requirement"),
            doc("07_Decisions/QUESTION-1.md", "question"),
            doc("80_Templates/Question.md", "question"),
            doc("80_Templates/Requirement.md", "requirement"),
        ]);
        assert_eq!(app.visible().len(), 6);
        app.tab = 1;
        assert_eq!(app.visible()[0].kind, "brd");
        assert_eq!(app.visible().len(), 1);
        app.tab = 2;
        assert_eq!(app.visible().len(), 2);
        assert!(
            app.visible()
                .iter()
                .any(|d| d.path.ends_with("REQ-LOOKUP.md"))
        );
        app.tab = 5;
        assert_eq!(app.visible().len(), 1);
        assert_eq!(app.visible()[0].path, "07_Decisions/QUESTION-1.md");
    }

    #[test]
    fn renders_empty_and_small_terminals_without_panicking() {
        for (width, height) in [(140, 40), (80, 24), (30, 8)] {
            let backend = ratatui::backend::TestBackend::new(width, height);
            let mut terminal = ratatui::Terminal::new(backend).unwrap();
            let mut app = App::new(vec![]);
            for tab in 0..view::TABS.len() {
                app.tab = tab;
                terminal.draw(|f| app.draw(f)).unwrap();
            }
        }
    }
}
