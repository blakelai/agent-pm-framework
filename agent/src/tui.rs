mod delivery;
mod editor;
mod form;
mod markdown;
mod planner;
mod view;
use crate::{
    config::Config,
    provider::Event,
    runtime::{Runtime, STAGES, Session, Status},
    skills::Catalog,
    store::Store,
    tools,
    workspace::Workspace,
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
    Error(String),
}
enum Action {
    Message(String),
    Decision(bool),
    Continue,
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
pub async fn run(workspace: Workspace, config_path: PathBuf) -> Result<()> {
    let mut app = App::new(workspace.documents()?);
    if let Ok(config) = Config::load(&config_path) {
        app.model = config.active()?.model.clone();
    }
    let mut store = Store::open(&workspace.path(".pm-agent/runtime.sqlite3")?)?;
    store.reindex(&app.docs)?;
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
    loop {
        tokio::select! {
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
                app.busy=false;
                match outcome {
                    Outcome::Session(s)=>{
                        app.status=format!("{:?} — session {}",s.status,s.id);
                        app.usage=s.usage.clone();
                        app.session=Some(*s);
                    },
                    Outcome::Pm(text)=>app.notice(&format!("PM operation finished; inspect exit_code and reports\n\n{text}")),
                    Outcome::Error(text)=>app.notice(&format!("Error: {text}")),
                }
                refresh(&workspace,&mut app,&mut store);
            },
            event=events.next()=>{
                dirty = true;
                let Some(event)=event else {break;};
                match app.event(event?) {
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
                                if let Some(planner)=&mut app.planner {planner.replace(crate::schedule::Snapshot::load(&workspace));planner.message=app.status.clone();}
                                if let Some(delivery)=&mut app.delivery {delivery.replace(crate::delivery::Snapshot::load(&workspace));delivery.select_path(&saved.path);delivery.message=app.status.clone();}
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
    let selected = app.selected_path();
    match workspace.documents() {
        Ok(docs) => {
            app.docs = docs;
            if let Some(path) = selected {
                app.selected = app
                    .visible()
                    .iter()
                    .position(|d| d.path == path)
                    .unwrap_or(0);
            }
            if let Err(e) = store.reindex(&app.docs) {
                app.status = format!("Index failed: {e}");
            }
        }
        Err(e) => app.status = format!("Refresh failed: {e}"),
    }
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
        _ => {
            if let Some(result) = input.strip_prefix("/reconcile ") {
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
                Action::Decision(yes) => {
                    if yes {
                        approved = session.pending.first().map(|c| c.id.clone());
                    }
                    runtime.decision(&mut session, yes)?;
                }
                Action::Continue => {
                    if matches!(
                        session.status,
                        Status::Failed | Status::Cancelled | Status::Completed
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
