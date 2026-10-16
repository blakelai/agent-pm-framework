use crate::{
    config::Config,
    provider::Event,
    runtime::{Runtime, STAGES, Session, Status},
    store::Store,
    tools,
    workspace::{Document, Workspace},
};
use anyhow::{Context, Result, bail};
use crossterm::event::{Event as TerminalEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap},
};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const TABS: &[&str] = &[
    "All",
    "BRD",
    "PRD / REQ",
    "Domain",
    "Systems",
    "Questions",
    "People / Schedule",
    "Backlog",
    "Progress / Risk",
    "Verification",
    "Agent Runs",
];
const HELP: &str = "i / : compose | Enter send | Esc browse/cancel | Tab view | arrows select | PgUp/PgDn scroll | r refresh | o open in desktop | q quit\nCommands: /help, /search TEXT, /stage prd, /pm plan, /sessions, /resume ID, /continue, /approve, /deny, /reconcile ACTUAL_OUTCOME, /new\nAll records stay in the workspace. Configure --config to enable an Agent. No model is required for browsing or PM calculations.";

pub struct App {
    pub docs: Vec<Document>,
    pub tab: usize,
    pub selected: usize,
    pub scroll: u16,
    pub filter: String,
    pub input: String,
    pub editing: bool,
    pub output: String,
    pub output_scroll: u16,
    pub output_focus: bool,
    pub status: String,
    pub session: Option<Session>,
    pub stage: String,
    pub busy: bool,
}
impl App {
    pub fn new(docs: Vec<Document>) -> Self {
        Self {
            docs,
            tab: 0,
            selected: 0,
            scroll: 0,
            filter: String::new(),
            input: String::new(),
            editing: false,
            output: HELP.into(),
            output_scroll: 0,
            output_focus: false,
            status: "Ready — browsing is local".into(),
            session: None,
            stage: "workflow".into(),
            busy: false,
        }
    }
    pub fn visible(&self) -> Vec<&Document> {
        self.docs
            .iter()
            .filter(|d| self.tab == 0 || !d.path.starts_with("80_Templates/"))
            .filter(|d| match self.tab {
                1 => d.kind == "brd" || d.path.starts_with("04_Requirements/BRD"),
                2 => {
                    matches!(d.kind.as_str(), "prd" | "requirement")
                        || d.path.starts_with("04_Requirements/PRD")
                        || d.path.starts_with("04_Requirements/Items")
                }
                3 => d.path.starts_with("02_Domain") || d.path == "CONTEXT-MAP.md",
                4 => d.path.starts_with("03_Systems"),
                5 => d.kind == "question",
                6 => d.path.starts_with("06_Delivery"),
                7 => d.path.starts_with("05_Backlog"),
                8 => {
                    d.path.starts_with("07_Decisions")
                        || d.path.starts_with("09_Operations")
                        || d.path.contains("Status")
                }
                9 => d.path.starts_with("08_Verification"),
                10 => d.path.starts_with("90_Agent/Runs"),
                _ => true,
            })
            .filter(|d| {
                self.filter.is_empty()
                    || format!("{} {} {}", d.path, d.title, d.body)
                        .to_lowercase()
                        .contains(&self.filter.to_lowercase())
            })
            .collect()
    }
    fn append(&mut self, text: &str) {
        self.output.push_str(text);
        if self.output.len() > 120_000 {
            let mut i = self.output.len() - 100_000;
            while !self.output.is_char_boundary(i) {
                i += 1;
            }
            self.output.drain(..i);
        }
    }
    pub fn draw(&self, f: &mut ratatui::Frame) {
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(4),
                Constraint::Percentage(32),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(f.area());
        f.render_widget(
            Tabs::new(TABS.iter().copied())
                .select(self.tab)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!("PM Agent — {}", TABS[self.tab])),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            areas[0],
        );
        let middle = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(if f.area().width >= 110 {
                vec![
                    Constraint::Percentage(25),
                    Constraint::Percentage(55),
                    Constraint::Percentage(20),
                ]
            } else {
                vec![Constraint::Percentage(35), Constraint::Percentage(65)]
            })
            .split(areas[1]);
        let docs = self.visible();
        let mut state = ListState::default().with_selected(if docs.is_empty() {
            None
        } else {
            Some(self.selected.min(docs.len() - 1))
        });
        let items = docs
            .iter()
            .map(|d| ListItem::new(format!("{} {}", d.id.as_deref().unwrap_or("·"), d.title)))
            .collect::<Vec<_>>();
        f.render_stateful_widget(
            List::new(items)
                .block(Block::default().borders(Borders::ALL).title(format!(
                    "Documents ({}) {}",
                    docs.len(),
                    self.filter
                )))
                .highlight_style(Style::default().bg(Color::DarkGray).fg(Color::White)),
            middle[0],
            &mut state,
        );
        if let Some(doc) = docs.get(self.selected.min(docs.len().saturating_sub(1))) {
            f.render_widget(
                Paragraph::new(doc.body.as_str())
                    .wrap(Wrap { trim: false })
                    .scroll((self.scroll, 0))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(doc.path.as_str()),
                    ),
                middle[1],
            );
            if middle.len() > 2 {
                f.render_widget(
                    Paragraph::new(format!(
                        "Type: {}\nID: {}\nSHA256: {}\n\n{}\n\nStage: {}\nSession: {}",
                        doc.kind,
                        doc.id.as_deref().unwrap_or("—"),
                        doc.hash,
                        serde_json::to_string_pretty(&doc.metadata).unwrap_or_default(),
                        self.stage,
                        self.session
                            .as_ref()
                            .map(|s| s.id.as_str())
                            .unwrap_or("not started")
                    ))
                    .wrap(Wrap { trim: false })
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Properties / provenance"),
                    ),
                    middle[2],
                );
            }
        } else {
            f.render_widget(
                Paragraph::new(
                    "No records in this view. Unknown project details have not been filled in.",
                )
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL)),
                middle[1],
            );
        }
        let paragraph = Paragraph::new(self.output.as_str()).wrap(Wrap { trim: false });
        let lines = paragraph.line_count(areas[2].width.saturating_sub(2));
        let offset = lines
            .saturating_sub(areas[2].height.saturating_sub(2) as usize)
            .saturating_sub(self.output_scroll as usize)
            .min(u16::MAX as usize) as u16;
        f.render_widget(
            paragraph.scroll((offset, 0)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if self.output_focus {
                        Color::Cyan
                    } else {
                        Color::Reset
                    }))
                    .title(if self.busy {
                        "Agent / running — Esc cancels"
                    } else {
                        "Agent / tools / questions — F2 then PgUp/PgDn to scroll"
                    }),
            ),
            areas[2],
        );
        f.render_widget(
            Paragraph::new(self.input.as_str())
                .style(Style::default().fg(if self.editing {
                    Color::Cyan
                } else {
                    Color::Gray
                }))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(if self.editing {
                            "Message or command — Enter sends"
                        } else {
                            "Press i to type — /help for commands"
                        }),
                ),
            areas[3],
        );
        f.render_widget(Paragraph::new(Line::from(self.status.as_str())), areas[4]);
    }
}
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

pub async fn run(workspace: Workspace, config_path: PathBuf) -> Result<()> {
    let docs = workspace.documents()?;
    let mut app = App::new(docs);
    let mut store = Store::open(&workspace.path(".pm-agent/runtime.sqlite3")?)?;
    store.reindex(&app.docs)?;
    let mut terminal = ratatui::init();
    let mut events = EventStream::new();
    let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
    let (done_tx, mut done_rx) = mpsc::unbounded_channel::<Outcome>();
    let mut cancellation = CancellationToken::new();
    // ratatui::init installs a panic hook; restore on ordinary errors as well.
    let result:Result<()>=async {
        loop {
            terminal.draw(|f|app.draw(f))?;
            tokio::select! {
                Some(e)=rx.recv()=>match e {Event::Text(text)=>app.append(&text),Event::Notice(text)=>{app.status=text.lines().next().unwrap_or("").into();app.append(&format!("\n{text}\n"));}},
                Some(result)=done_rx.recv()=>{
                    app.busy=false;
                    match result {Outcome::Session(s)=>{app.status=format!("{:?} — session {}",s.status,s.id);app.session=Some(*s);},Outcome::Pm(text)=>{app.status="PM operation finished; inspect exit_code and reports".into();app.append(&format!("\n{text}\n"));},Outcome::Error(e)=>{app.status=format!("Error: {e}");app.append(&format!("\nError: {e}\n"));}}
                    match workspace.documents(){Ok(docs)=>{app.docs=docs;store.reindex(&app.docs)?;},Err(e)=>{app.status=format!("Refresh failed: {e}");}}
                },
                event=events.next()=>{
                    let Some(event)=event else {break;};let event=event?;
                    let TerminalEvent::Key(key)=event else {continue;};if key.kind!=KeyEventKind::Press {continue;}
                    if key.code==KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                        if app.busy {cancellation.cancel();app.status="Cancellation requested; waiting for state to be saved".into();continue;}else{break;}
                    }
                    if key.code==KeyCode::Esc {app.editing=false;if app.busy{cancellation.cancel();}continue;}
                    if app.editing {
                        match key.code {
                            KeyCode::Char(c)=>app.input.push(c),KeyCode::Backspace=>{app.input.pop();},
                            KeyCode::Enter=>{
                                if app.busy{app.status="Wait or press Esc to cancel the current operation".into();continue;}
                                let input=std::mem::take(&mut app.input);app.editing=false;
                                if input.trim().is_empty(){continue;}
                                app.append(&format!("\n> {input}\n"));
                                let launch=dispatch(&workspace,&config_path,&mut app,&store,input,&tx,&done_tx);
                                match launch {Ok(Some(token))=>{cancellation=token;app.busy=true;},Ok(None)=>{},Err(e)=>{app.status=format!("Error: {e}");app.append(&format!("\nError: {e}\n"));}}
                            },_=>{}
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') if !app.busy=>break,
                            KeyCode::Char('i')|KeyCode::Char(':')=>app.editing=true,
                            KeyCode::Tab=>{app.tab=(app.tab+1)%TABS.len();app.selected=0;app.scroll=0;},
                            KeyCode::BackTab=>{app.tab=(app.tab+TABS.len()-1)%TABS.len();app.selected=0;app.scroll=0;},
                            KeyCode::Down|KeyCode::Char('j')=>{app.selected=(app.selected+1).min(app.visible().len().saturating_sub(1));app.scroll=0;},
                            KeyCode::Up|KeyCode::Char('k')=>{app.selected=app.selected.saturating_sub(1);app.scroll=0;},
                            KeyCode::F(2)=>app.output_focus = !app.output_focus,
                            KeyCode::PageDown if app.output_focus=>app.output_scroll=app.output_scroll.saturating_sub(10),
                            KeyCode::PageUp if app.output_focus=>app.output_scroll=app.output_scroll.saturating_add(10),
                            KeyCode::Home if app.output_focus=>app.output_scroll=u16::MAX,
                            KeyCode::End if app.output_focus=>app.output_scroll=0,
                            KeyCode::PageDown=>app.scroll=app.scroll.saturating_add(10),KeyCode::PageUp=>app.scroll=app.scroll.saturating_sub(10),
                            KeyCode::Char('r')=>match workspace.documents(){Ok(d)=>{app.docs=d;store.reindex(&app.docs)?;app.status="Refreshed from disk".into();},Err(e)=>app.status=e.to_string()},
                            KeyCode::Char('o')=>{if let Some(d)=app.visible().get(app.selected){let path=workspace.path(&d.path)?;open_document(path)?;}},
                            _=>{}
                        }
                    }
                }
            }
        }
        Ok(())
    }.await;
    ratatui::restore();
    result
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
        app.append(&format!("\n{}\n", s.last_output));
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
            for tab in 0..TABS.len() {
                app.tab = tab;
                terminal.draw(|f| app.draw(f)).unwrap();
            }
        }
    }
}
