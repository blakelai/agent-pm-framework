mod approval_prompt;
mod completion;
mod delivery;
mod editor;
mod form;
mod inspection;
mod markdown;
mod materials;
mod planner;
mod progress;
mod questions;
mod recovery;
mod refresh;
mod tool_display;
mod transcript;
mod view;
use crate::{
    config::Config,
    provider::{Call, Event},
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
    Pm { call: Call, result: String },
    Probe(crate::probe::Observation),
    Error(String),
    RejectedInput { text: String, error: String },
}
enum Action {
    Message(String),
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
    if let Ok(catalog) = Catalog::discover(&workspace) {
        app.update_skills(&catalog);
    }
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
    let mut exit_when_idle = false;
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
    let (clipboard_tx, mut clipboard_rx) = mpsc::unbounded_channel();
    let mut clipboard_running = false;
    loop {
        tokio::select! {
            Some((generation, result)) = clipboard_rx.recv() => {
                clipboard_running = false;
                app.finish_clipboard(generation, result);
                dirty = true;
            },
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
                if app.mode == view::Mode::Chat { dirty |= app.progress.tick(std::time::Instant::now()); }
                if dirty { terminal.draw(|f| app.draw(f))?; dirty = false; }
            },
            Some(event)=rx.recv()=>{ dirty = true; app.provider_event(event); },
            Some(outcome)=done_rx.recv()=>{
                while let Ok(event) = rx.try_recv() { app.provider_event(event); }
                dirty = true;
                monitor.invalidate();
                app.busy=false;
                app.progress.finish("Operation finished");
                let failed = matches!(outcome, Outcome::Error(_) | Outcome::RejectedInput { .. });
                match outcome {
                    Outcome::Session(s)=>{
                        finish_chat_outcome(&workspace,&store,&mut app,*s,cancellation.is_cancelled());
                    },
                    Outcome::Pm {call, result}=>{
                        let value = serde_json::from_str(&result).unwrap_or_default();
                        let label = if tool_display::failed(&value) {"Project operation reported errors · expand for details"} else {"Project operation finished · review the result"};
                        app.status = label.into();
                        app.progress.finish(label);
                        app.messages.tool(&call, "Result recorded", Some(&result));
                    },
                    Outcome::Probe(observation)=>{
                        app.notice(&observation.markdown());
                        app.status=if observation.report.cancelled() {"Probe cancelled; partial observations saved"} else {"Probe finished; review individual observations"}.into();
                        app.progress.finish(app.status.clone());
                    },
                    Outcome::Error(text)=>{
                        app.progress.finish("Failed · expand the error for details");
                        if let Some(id)=app.session.as_ref().map(|s|s.id.clone())
                            && let Ok(saved)=store.load::<Session>(&id) {finish_chat_outcome(&workspace,&store,&mut app,saved,cancellation.is_cancelled());}
                        app.notice(&format!("Error: {text}"));
                    },
                    Outcome::RejectedInput{text,error}=>{
                        app.progress.finish("Input was not accepted · review the error");
                        if app.input.text.is_empty() { app.input.insert(&text); }
                        app.notice(&format!("Input was not accepted; attachments retained: {error}\n\n{text}"));
                    },
                }
                refresh(&workspace,&mut app,&mut store);
                if exit_when_idle && !failed { break; }
                exit_when_idle = false;
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
                    Command::Quit=>{
                        if app.busy { cancellation.cancel(); app.progress.cancel(); exit_when_idle = true; app.status="Stopping before exit; waiting for the recorded outcome".into(); }
                        else { break; }
                    },
                    Command::Cancel=>{cancellation.cancel();app.progress.cancel();app.status="Cancelling — waiting for the operation outcome".into();},
                    Command::Refresh=>refresh(&workspace,&mut app,&mut store),
                    Command::Open(target)=>{
                        let result=if target.starts_with("https://") || target.starts_with("http://") {open_document(PathBuf::from(target))} else {workspace.path(&target).and_then(open_document)};
                        if let Err(e)=result {app.notice(&format!("Error: Open document\n{}", crate::diagnostics::format_error(&e)));}
                    },
                    Command::PasteClipboard=>{
                        if clipboard_running { app.status="Previous clipboard read is finishing; try again shortly".into(); }
                        else {
                            clipboard_running = true;
                            let generation = app.start_clipboard(); let sender = clipboard_tx.clone();
                            tokio::spawn(async move {
                                let result = tokio::task::spawn_blocking(crate::chat_images::clipboard).await
                                    .context("Clipboard worker failed").and_then(|r|r);
                                let _ = sender.send((generation,result));
                            });
                        }
                    },
                    Command::RefreshSkills=>match Catalog::discover(&workspace) { Ok(c)=>app.update_skills(&c), Err(e)=>{app.clear_skills();app.notice(&format!("Error: Load Skills\n{}", crate::diagnostics::format_error(&e)));} },
                    Command::Skills=>match Catalog::discover(&workspace) {Ok(c)=>app.skills_menu(c),Err(e)=>app.notice(&format!("Error: Load Skills\n{}", crate::diagnostics::format_error(&e)))},
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
                    Command::Approve(selection)=>{
                        match decide_tool(&workspace,&config_path,&mut app,&store,&selection,&tx,&done_tx) {
                            Ok(Some(token))=>{cancellation=token;app.start_operation();},
                            Ok(None)=>{},
                            Err(error)=>{
                                if let Ok(saved)=store.load::<Session>(&selection.session_id) {app.finish_session(saved);}
                                app.notice(&format!("Error: {}",crate::diagnostics::format_error(&error)));
                            },
                        }
                    },
                    Command::Submit(input)=>{
                        app.user_message(&input);
                        let result=dispatch(&workspace,&config_path,&mut app,&store,input.clone(),&tx,&done_tx);
                        match result {
                            Ok(Some(token))=>{cancellation=token;app.start_operation();app.messages.boundary();},
                            Ok(None)=>{},
                            Err(e)=>{app.input.insert(&input);app.notice(&format!("Error: {}", crate::diagnostics::format_error(&e)));},
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
fn finish_chat_outcome(
    workspace: &Workspace,
    store: &Store,
    app: &mut App,
    mut session: Session,
    stop_requested: bool,
) {
    // Esc may arrive after the worker saved WaitingApproval but before the UI receives it.
    let error = if stop_requested && session.status == Status::WaitingApproval {
        session
            .pending
            .first()
            .cloned()
            .and_then(|call| crate::approval::deny(workspace, store, &mut session, &call).err())
    } else {
        None
    };
    app.finish_session(session);
    if let Some(error) = error {
        app.notice(&format!(
            "Error: {}",
            crate::diagnostics::format_error(&error)
        ));
    }
}
fn decide_tool(
    workspace: &Workspace,
    config_path: &std::path::Path,
    app: &mut App,
    store: &Store,
    selection: &approval_prompt::Selection,
    tx: &mpsc::UnboundedSender<Event>,
    done: &mpsc::UnboundedSender<Outcome>,
) -> Result<Option<CancellationToken>> {
    anyhow::ensure!(!app.busy, "Wait for the current operation to finish");
    anyhow::ensure!(
        app.session
            .as_ref()
            .is_some_and(|s| s.id == selection.session_id),
        "Approval belongs to a different conversation"
    );
    let mut session: Session = store.load(&selection.session_id)?;
    anyhow::ensure!(
        session.status == Status::WaitingApproval
            && session.pending.first() == Some(&selection.call),
        "Pending tool changed; review the current approval request"
    );
    if selection.decision == crate::approval::Decision::Deny {
        let result = crate::approval::deny(workspace, store, &mut session, &selection.call);
        app.finish_session(session);
        result?;
        app.notice("Tool use denied; Agent response stopped.");
        return Ok(None);
    }
    let config = Config::load(config_path)?;
    let workspace = workspace.clone();
    let expected = selection.call.clone();
    let decision = selection.decision;
    let tx = tx.clone();
    let done = done.clone();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        let result = async {
            let runtime = Runtime::open(workspace, config).await?;
            anyhow::ensure!(
                !signal.is_cancelled(),
                "Cancelled before recording the tool approval"
            );
            let approved = runtime.decide_approval(&mut session, &expected, decision)?;
            if session.status == Status::WaitingApproval {
                if let Some(error) = &session.error {
                    let _ = tx.send(Event::Notice(error.clone()));
                }
                return Ok(());
            }
            runtime.run(&mut session, approved, &tx, &signal).await
        }
        .await;
        let _ = done.send(match result {
            Ok(()) => Outcome::Session(Box::new(session)),
            Err(error) => Outcome::Error(crate::diagnostics::format_error(&error)),
        });
    });
    Ok(Some(cancel))
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
    let input = if input.starts_with('/') {
        input.trim_end().to_owned()
    } else {
        input
    };
    if matches!(input.as_str(), "/approve" | "/deny") {
        let session = app
            .session
            .as_ref()
            .context("No session awaiting approval")?;
        let id = session.id.clone();
        let call = session
            .pending
            .first()
            .context("No call awaiting approval")?
            .clone();
        let decision = if input == "/approve" {
            crate::approval::Decision::Once
        } else {
            crate::approval::Decision::Deny
        };
        let selection = approval_prompt::Selection {
            session_id: id,
            call,
            decision,
        };
        return decide_tool(workspace, config_path, app, store, &selection, tx, done);
    }
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
    if input == "/language" || input.starts_with("/language ") {
        if let Some(tag) = input.strip_prefix("/language ") {
            anyhow::ensure!(
                app.session.is_none(),
                "Use /new before selecting a response language"
            );
            let tag = tag.trim();
            let language = if tag == "auto" {
                None
            } else {
                crate::config::validate_response_language(tag)?;
                Some(tag.to_string())
            };
            app.response_language = Some(language);
        }
        let language = match &app.session {
            Some(session) => session.response_language.clone(),
            None => match &app.response_language {
                Some(language) => language.clone(),
                None => Config::read(config_path)?.response_language,
            },
        };
        app.append(&format!("Response language: {}. For new sessions use /language TAG or /language auto after /new. This TUI selection lasts until exit; set response_language in config.toml for a persistent default.", language.as_deref().unwrap_or("auto (user preference)")));
        return Ok(None);
    }
    if input == "/status" {
        app.append(&app.status_details());
        return Ok(None);
    }
    if input == "/budget" {
        let config = Config::read(config_path)?;
        app.append(
            &crate::budget::Report::new(&config.interactive_limits(), app.session.as_ref())
                .markdown(),
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
        app.reset_chat();
        app.append("New session. Describe the project work or choose a Skill with `$`.");
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
        app.resume_chat(&s);
        for q in s.questions.iter().filter(|q| !q.resolved) {
            app.notice(&format!("Open question: {}\n{}", q.path, q.question));
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
                Err(e) => Outcome::Error(crate::diagnostics::format_error(&e)),
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
        let tx = tx.clone();
        tokio::spawn(async move {
            let call = Call {
                id: format!("local-pm-{}", uuid::Uuid::new_v4()),
                name: "pm".into(),
                arguments: serde_json::json!({"command": command, "args": args}).to_string(),
            };
            let _ = tx.send(Event::Tool {
                call: call.clone(),
                state: "Running".into(),
                result: None,
            });
            let operation = tools::run_pm(&workspace, &command, &args);
            tokio::pin!(operation);
            let result = tokio::select! {
                result = &mut operation => result,
                _ = signal.cancelled() => {
                    let _ = tx.send(Event::Notice("Stop requested; waiting for the native PM worker to finish. No rollback is implied.".into()));
                    operation.await
                }
            };
            let _ = done.send(match result {
                Ok(v) => Outcome::Pm {
                    call,
                    result: v.to_string(),
                },
                Err(e) => {
                    let error = crate::diagnostics::format_error(&e);
                    let _ = tx.send(Event::Tool {
                        call,
                        state: "Failed".into(),
                        result: Some(serde_json::json!({"error": error}).to_string()),
                    });
                    Outcome::Error(error)
                }
            });
        });
        return Ok(Some(cancel));
    }
    let mut config = config?;
    if let Some(language) = &app.response_language {
        config.response_language = language.clone();
    }
    app.model = config.active()?.model.clone();
    let submitted_text = input.clone();
    let action = match input.as_str() {
        "/continue" => Action::Continue,
        "/continue-independent" => Action::Independent,
        _ => {
            if let Some(answer) = input.strip_prefix("/answer ") {
                let (id, text) = answer
                    .split_once(char::is_whitespace)
                    .unwrap_or((answer, ""));
                anyhow::ensure!(
                    !id.is_empty() && (!text.trim().is_empty() || !app.attachments.is_empty()),
                    "Use /answer QUESTION-ID TEXT or attach an image"
                );
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
    let images = if matches!(action, Action::Message(_) | Action::Answer(_, _)) {
        app.attachments.clone()
    } else {
        vec![]
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
        let mut input_accepted = false;
        let result = async {
            let runtime = Runtime::open(workspace, config).await?;
            match action {
                Action::Message(input) => {
                    runtime.input_with_images(&mut session, &input, &images)?
                }
                Action::Independent => runtime.continue_independent(&mut session)?,
                Action::Answer(id, text) => {
                    runtime.answer_with_images(&mut session, &id, &text, &images)?
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
            input_accepted = true;
            if !images.is_empty() {
                let _ = tx.send(Event::InputAccepted(
                    images.iter().map(|image| image.id.clone()).collect(),
                ));
            }
            runtime.run(&mut session, None, &tx, &signal).await
        }
        .await;
        let _ = done.send(match result {
            Ok(()) => Outcome::Session(Box::new(session)),
            Err(e) if !input_accepted && !images.is_empty() => Outcome::RejectedInput {
                text: submitted_text,
                error: crate::diagnostics::format_error(&e),
            },
            Err(e) => Outcome::Error(crate::diagnostics::format_error(&e)),
        });
    });
    Ok(Some(cancel))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Document;
    #[test]
    fn approval_snapshot_rejects_changed_calls_and_denial_needs_no_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace {
            root: dir.path().to_owned(),
        };
        let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
        let mut session:Session=serde_json::from_value(serde_json::json!({"id":"fixture","status":"waiting_approval","provider_fingerprint":"fixture","history":[],"pending":[{"id":"tool","name":"mcp_missing_tool","arguments":"{}"}],"questions":[],"last_output":"","error":null,"stage":"workflow"})).unwrap();
        let mut app = App::new(vec![]);
        app.input.insert("Preserved draft");
        let selection = approval_prompt::Selection {
            session_id: session.id.clone(),
            call: session.pending[0].clone(),
            decision: crate::approval::Decision::Deny,
        };
        session.pending[0].arguments = "{\"changed\":true}".into();
        store.save(&session.id, &session).unwrap();
        app.finish_session(session.clone());
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (done, mut outcomes) = mpsc::unbounded_channel();
        assert!(
            decide_tool(
                &w,
                &dir.path().join("missing.toml"),
                &mut app,
                &store,
                &selection,
                &tx,
                &done
            )
            .unwrap_err()
            .to_string()
            .contains("Pending tool changed")
        );
        let current = approval_prompt::Selection {
            call: session.pending[0].clone(),
            ..selection
        };
        assert!(
            decide_tool(
                &w,
                &dir.path().join("missing.toml"),
                &mut app,
                &store,
                &current,
                &tx,
                &done
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(app.session.as_ref().unwrap().status, Status::Cancelled);
        assert!(app.approval.is_none());
        assert_eq!(app.input.text, "Preserved draft");
        assert!(rx.try_recv().is_err() && outcomes.try_recv().is_err());
        session.id = "cancellation-race".into();
        store.save(&session.id, &session).unwrap();
        finish_chat_outcome(&w, &store, &mut app, session.clone(), true);
        assert_eq!(app.session.as_ref().unwrap().status, Status::Cancelled);
        let saved: Session = store.load(&session.id).unwrap();
        assert!(saved.pending.is_empty());
        assert!(app.approval.is_none());
    }
    #[tokio::test]
    async fn worker_startup_failure_retains_mcp_program_and_underlying_cause_without_arguments() {
        let dir = tempfile::tempdir().unwrap();
        crate::workspace::atomic_write(&dir.path().join("00_Governance/Project.md"),crate::workspace::note(serde_json::json!({"type":"project","id":"PROJECT-TEST","language":"en","title":"Synthetic"}),"# Synthetic").unwrap().as_bytes()).unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let path = w.path(".pm-agent/config.toml").unwrap();
        let program = dir.path().join("missing-mcp-fixture");
        let config = format!(
            r#"
provider = "fixture"
[providers.fixture]
protocol = "chat_completions"
base_url = "http://127.0.0.1:9/v1"
model = "fixture"
[mcp.fixture]
transport = "stdio"
command = {}
args = ["SYNTHETIC_SECRET_DO_NOT_DISPLAY"]
"#,
            serde_json::to_string(program.to_str().unwrap()).unwrap()
        );
        crate::workspace::atomic_write(&path, config.as_bytes()).unwrap();
        let mut app = App::new(vec![]);
        let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
        let (tx, _) = mpsc::unbounded_channel();
        let (done, mut finished) = mpsc::unbounded_channel();
        dispatch(
            &w,
            &path,
            &mut app,
            &store,
            "Synthetic startup fixture".into(),
            &tx,
            &done,
        )
        .unwrap()
        .unwrap();
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), finished.recv())
            .await
            .unwrap()
            .unwrap();
        let Outcome::Error(error) = outcome else {
            panic!("Expected startup error")
        };
        assert!(
            error.contains(program.to_str().unwrap()),
            "Program is missing from diagnostic: {error}"
        );
        assert!(
            error.contains("os error 2"),
            "Underlying cause is missing: {error}"
        );
        assert!(!error.contains("SYNTHETIC_SECRET_DO_NOT_DISPLAY"));
    }
    #[tokio::test]
    async fn rejected_image_input_and_local_commands_preserve_pending_attachments() {
        let dir = tempfile::tempdir().unwrap();
        crate::workspace::atomic_write(&dir.path().join("00_Governance/Project.md"),crate::workspace::note(serde_json::json!({"type":"project","id":"PROJECT-TEST","language":"en","title":"Synthetic"}),"# Synthetic").unwrap().as_bytes()).unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let path = w.path(".pm-agent/config.toml").unwrap();
        crate::workspace::atomic_write(
            &path,
            br#"
provider = "fixture"
[providers.fixture]
protocol = "chat_completions"
base_url = "http://127.0.0.1:9/v1"
model = "fixture"
"#,
        )
        .unwrap();
        let config = Config::load(&path).unwrap();
        let mut app = App::new(vec![]);
        let mut session = Session::new(&config, "workflow").unwrap();
        session.status = Status::WaitingApproval;
        app.session = Some(session);
        app.attachments
            .push(crate::chat_images::Image::from_rgba(1, 1, &[255; 4]).unwrap());
        let id = app.attachments[0].id.clone();
        let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (done, mut finished) = mpsc::unbounded_channel();
        assert!(
            dispatch(&w, &path, &mut app, &store, "/status".into(), &tx, &done)
                .unwrap()
                .is_none()
        );
        assert_eq!(app.attachments[0].id, id);
        dispatch(
            &w,
            &path,
            &mut app,
            &store,
            "Image draft".into(),
            &tx,
            &done,
        )
        .unwrap()
        .unwrap();
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), finished.recv())
            .await
            .unwrap()
            .unwrap();
        let Outcome::RejectedInput { text, error } = outcome else {
            panic!("Expected rejected input")
        };
        assert_eq!(text, "Image draft");
        assert!(error.contains("pending tool"));
        assert_eq!(app.attachments[0].id, id);
        assert!(
            !std::iter::from_fn(|| rx.try_recv().ok())
                .any(|e| matches!(e, Event::InputAccepted(_)))
        );
        assert!(!w.root.join("01_Sources/Assets").exists());
    }
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
            dispatch(&w, &path, &mut app, &store, "/budget ".into(), &tx, &done)
                .unwrap()
                .is_none()
        );
        assert!(app.output.contains("Request limit: not set"));
        assert!(!app.output.contains("limit reached"));
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
            response_language: None,
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
    #[test]
    fn language_selection_is_local_and_requires_a_new_session() {
        let dir = tempfile::tempdir().unwrap();
        crate::workspace::atomic_write(&dir.path().join("00_Governance/Project.md"), crate::workspace::note(serde_json::json!({"type":"project","id":"PROJECT-TEST","language":"en","title":"Synthetic"}), "# Synthetic").unwrap().as_bytes()).unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let path = w.path(".pm-agent/config.toml").unwrap();
        crate::workspace::atomic_write(
            &path,
            br#"provider = "fixture"
response_language = "ja-JP"
[providers.fixture]
protocol = "chat_completions"
base_url = "http://127.0.0.1:9/v1"
model = "fixture"
"#,
        )
        .unwrap();
        let config = Config::load(&path).unwrap();
        let mut app = App::new(vec![]);
        let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
        let (tx, mut events) = mpsc::unbounded_channel();
        let (done, mut outcomes) = mpsc::unbounded_channel();
        dispatch(
            &w,
            &path,
            &mut app,
            &store,
            "/language zh-TW".into(),
            &tx,
            &done,
        )
        .unwrap();
        assert_eq!(app.response_language, Some(Some("zh-TW".into())));
        assert!(
            dispatch(
                &w,
                &path,
                &mut app,
                &store,
                "/language en bad".into(),
                &tx,
                &done
            )
            .is_err()
        );
        assert_eq!(app.response_language, Some(Some("zh-TW".into())));
        app.session = Some(Session::new(&config, "workflow").unwrap());
        assert!(
            dispatch(
                &w,
                &path,
                &mut app,
                &store,
                "/language en".into(),
                &tx,
                &done
            )
            .is_err()
        );
        assert_eq!(
            app.session.as_ref().unwrap().response_language.as_deref(),
            Some("ja-JP")
        );
        dispatch(&w, &path, &mut app, &store, "/new".into(), &tx, &done).unwrap();
        assert_eq!(app.response_language, Some(Some("zh-TW".into())));
        dispatch(
            &w,
            &path,
            &mut app,
            &store,
            "/language auto".into(),
            &tx,
            &done,
        )
        .unwrap();
        assert_eq!(app.response_language, Some(None));
        assert_eq!(
            Config::read(&path).unwrap().response_language.as_deref(),
            Some("ja-JP")
        );
        assert!(events.try_recv().is_err());
        assert!(outcomes.try_recv().is_err());
    }
}
