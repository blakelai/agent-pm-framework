use super::{
    editor::Editor,
    markdown::{self, Rendered},
};
use crate::{provider::Usage, runtime::Session, skills::Catalog, workspace::Document};
use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

pub const TABS: &[(&str, &str)] = &[
    ("📁", "All"),
    ("📋", "BRD"),
    ("🎯", "PRD / REQ"),
    ("📚", "Domain"),
    ("🏗️", "Systems"),
    ("❓", "Questions"),
    ("👥", "People / Schedule"),
    ("🗂️", "Backlog"),
    ("⚠️", "Progress / Risk"),
    ("✅", "Verification"),
    ("🤖", "Agent Runs"),
];
pub const HELP: &str = "# PM Agent\n\nF2 switches Browse / Chat. F1 opens help. Ctrl+Q exits when idle.\n\n## Browse\nAlt+1…9 selects a category; Alt+0 lists all categories. Alt+Left/Right cycles categories.\nEnter opens the selected document; Esc returns to the list. F3 collapses the list; F4 maximizes the document. F6 toggles emoji.\nTab switches focus. PgUp/PgDn scroll. Mouse wheel scrolls the pointed pane; drag the right scrollbar. Right-click opens actions. L shows document links. P shows properties. O opens the original in the desktop application. R refreshes immediately. Local knowledge files also refresh automatically every two seconds; drafts are retained and changed inputs require a new preview. E edits supported project fields; N creates a PBI, risk or decision. F5 or Planning opens the local planning view: 1 Schedule, 2 Capacity, 3 Signals; M proposes an iteration move; P/I/A edit allocations, iterations and forecast date; R refreshes; Esc returns. F7 or Delivery opens Changes, UAT, Releases and Benefits: 1-4/Alt+1-4 select tabs, N creates a draft, E edits, O opens the document, R refreshes, Enter focuses details, Esc returns.\n\nF8 or the document context menu opens Impact and Compare: 1 references, 2 baseline diff, 3 source diff; Enter explicitly compares a baseline; O opens a candidate; R revalidates; F4 maximizes; Esc returns. Changed project inputs clear results. Source comparisons use captured snapshots only.

## Editing\nEnter/Tab edits fields; lists use one value per line. Use left/right to select enum values. Ctrl+S reviews changes, then Ctrl+S saves the reviewed result. Esc reverts the active field or returns from preview. Ctrl+D discards with confirmation.\n\n## Chat\nType immediately. Enter sends; Alt+Enter, Shift+Enter or Ctrl+J inserts a newline. Multiline paste never submits.\nScroll the conversation to read history. Esc returns to the latest message and input. A separate Esc in the input cancels an active operation. Ctrl+C clears only the draft.\n\n## Commands\n/help, /status, /skills, /skills clear, /search TEXT, /stage prd, /pm plan, /sessions, /resume ID, /continue, /approve, /deny, /reconcile ACTUAL_OUTCOME, /new, /quit.\nUse a leading $skill-name to explicitly enable a project Skill. Skills remain selected in the session until cleared. Context values come only from the service; missing values remain unavailable.";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Browse,
    Chat,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Focus {
    Documents,
    Content,
    Composer,
    Transcript,
}
#[derive(Debug, PartialEq)]
pub enum Command {
    None,
    Submit(String),
    Cancel,
    Quit,
    Refresh,
    Open(String),
    Skills,
    Edit(String),
    New(String),
    ReviewEdit,
    SaveEdit,
    CloseEdit,
    Planning,
    ClosePlanning,
    Delivery,
    CloseDelivery,
    Inspect(crate::inspection::Request),
    CloseInspection,
    Read(String),
    Schedule {
        path: String,
        hash: String,
        sprint: Option<i64>,
    },
}
#[derive(Clone)]
enum Choice {
    Inspect,
    Delivery,
    Planning,
    Edit,
    New(&'static str),
    Category(usize),
    Collapse,
    Maximize,
    Open,
    Properties,
    Links,
    Link(String),
    Reference,
    Latest,
    Skills,
    Skill(String),
    None,
}
struct Menu {
    title: String,
    rows: Vec<(String, Choice)>,
    selected: usize,
    offset: usize,
    area: Rect,
}
#[derive(Default)]
struct Regions {
    delivery: Rect,
    planning: Rect,
    browse: Rect,
    chat: Rect,
    categories: Vec<(Rect, usize)>,
    more: Rect,
    list: Rect,
    content: Rect,
    composer: Rect,
    transcript: Rect,
    status: Rect,
    collapse: Rect,
    maximize: Rect,
    bar: Rect,
}
pub struct App {
    pub(super) inspector: Option<super::inspection::Inspector>,
    pub(super) delivery: Option<super::delivery::Delivery>,
    pub(super) planner: Option<super::planner::Planner>,
    pub(super) form: Option<super::form::Form>,
    pub docs: Vec<Document>,
    pub tab: usize,
    pub selected: usize,
    pub scroll: usize,
    pub filter: String,
    pub input: Editor,
    pub output: String,
    pub status: String,
    pub(super) refresh_status: String,
    pub(super) refresh_error: Option<String>,
    pub session: Option<Session>,
    pub stage: String,
    pub busy: bool,
    pub model: String,
    pub usage: Option<Usage>,
    pub mode: Mode,
    pub focus: Focus,
    collapsed: bool,
    maximized: bool,
    restore_scroll: Option<usize>,
    icons: bool,
    browser_focus: Focus,
    bookmarks: BTreeMap<usize, (usize, usize)>,
    list_state: ListState,
    transcript_scroll: usize,
    follow: bool,
    regions: Regions,
    menu: Option<Menu>,
    dragging: bool,
    document: Rendered,
    transcript: Rendered,
    doc_cache: (String, usize),
    chat_cache: (usize, usize),
    editor_offset: usize,
}
impl App {
    pub fn new(docs: Vec<Document>) -> Self {
        Self {
            inspector: None,
            delivery: None,
            planner: None,
            form: None,
            docs,
            tab: 0,
            selected: 0,
            scroll: 0,
            filter: String::new(),
            input: Editor::default(),
            output: HELP.into(),
            status: "Ready — browsing is local".into(),
            refresh_status: "Automatic file refresh enabled (2s polling)".into(),
            refresh_error: None,
            session: None,
            stage: "workflow".into(),
            busy: false,
            model: "not configured".into(),
            usage: None,
            mode: Mode::Browse,
            focus: Focus::Documents,
            collapsed: false,
            maximized: false,
            restore_scroll: None,
            icons: true,
            browser_focus: Focus::Documents,
            bookmarks: BTreeMap::new(),
            list_state: ListState::default(),
            transcript_scroll: 0,
            follow: true,
            regions: Regions::default(),
            menu: None,
            dragging: false,
            document: Rendered::default(),
            transcript: Rendered::default(),
            doc_cache: (String::new(), 0),
            chat_cache: (0, 0),
            editor_offset: 0,
        }
    }
    pub fn visible(&self) -> Vec<&Document> {
        self.visible_in(self.tab)
    }
    fn visible_in(&self, tab: usize) -> Vec<&Document> {
        self.docs
            .iter()
            .filter(|d| tab == 0 || !d.path.starts_with("80_Templates/"))
            .filter(|d| match tab {
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
    pub fn selected_path(&self) -> Option<String> {
        self.visible().get(self.selected).map(|d| d.path.clone())
    }
    pub(super) fn replace_documents(&mut self, docs: Vec<Document>) {
        let selected = self.selected_path();
        let bookmarks = self
            .bookmarks
            .iter()
            .map(|(tab, (n, scroll))| {
                (
                    *tab,
                    self.visible_in(*tab).get(*n).map(|d| d.path.clone()),
                    *scroll,
                )
            })
            .collect::<Vec<_>>();
        self.docs = docs;
        self.selected = selected
            .as_ref()
            .and_then(|path| self.visible().iter().position(|d| &d.path == path))
            .unwrap_or(0);
        if self.selected_path() != selected {
            self.scroll = 0;
            self.restore_scroll = None;
            self.list_state = ListState::default();
        }
        for (tab, path, scroll) in bookmarks {
            let n = path.and_then(|p| self.visible_in(tab).iter().position(|d| d.path == p));
            self.bookmarks
                .insert(tab, (n.unwrap_or(0), if n.is_some() { scroll } else { 0 }));
        }
        // Properties, links and context actions are snapshots of the old record.
        // Close them rather than keep stale choices attached to a new selection.
        self.menu = None;
        self.doc_cache = (String::new(), 0);
    }
    pub(super) fn read_document(&mut self, path: &str) {
        if !self.docs.iter().any(|d| d.path == path) {
            if let Some(i) = &mut self.inspector {
                i.message = "Document is no longer available; R rechecks the project".into();
            }
            if let Some(p) = &mut self.planner {
                p.message = "Document is no longer available; refresh the planning view".into();
            }
            if let Some(d) = &mut self.delivery {
                d.message = "Document is no longer available; refresh the delivery view".into();
            }
            return;
        }
        self.planner = None;
        self.delivery = None;
        self.inspector = None;
        self.tab = 0;
        self.filter.clear();
        self.selected = self
            .visible()
            .iter()
            .position(|d| d.path == path)
            .unwrap_or(0);
        self.scroll = 0;
        self.focus = Focus::Content;
    }
    pub fn append(&mut self, text: &str) {
        self.output.push_str(text);
        // Keep a bounded display projection; complete history stays in the session database.
        if self.output.len() > 200_000 {
            let mut cut = self.output.len() - 150_000;
            while !self.output.is_char_boundary(cut) {
                cut += 1;
            }
            self.output.drain(..cut);
            self.output.insert_str(
                0,
                "Earlier display text omitted; session history remains saved.\n\n",
            );
        }
        self.chat_cache = (0, 0);
    }
    pub fn notice(&mut self, text: &str) {
        self.status = text.lines().next().unwrap_or("").into();
        self.append(&format!("\n\n### Activity\n\n{text}\n\n"));
    }
    pub fn usage_text(&self) -> String {
        match &self.usage {
            Some(u) => format!(
                "Context input: {} | output: {} | total: {} (service)",
                u.input_tokens
                    .map(|n| n.to_string())
                    .unwrap_or("not provided".into()),
                u.output_tokens
                    .map(|n| n.to_string())
                    .unwrap_or("not provided".into()),
                u.total_tokens
                    .map(|n| n.to_string())
                    .unwrap_or("not provided".into())
            ),
            None => "Context: not provided".into(),
        }
    }
    pub fn status_details(&self) -> String {
        format!(
            "# Session\n\nModel: {}\n\nState: {}\n\nSession: {}\n\n{}\n\nSelected Skills: {}\n\n{}\n\n{}",
            self.model,
            if self.busy { "running" } else { "idle" },
            self.session
                .as_ref()
                .map(|s| s.id.as_str())
                .unwrap_or("not started"),
            self.usage_text(),
            self.session
                .as_ref()
                .map(|s| s.selected_skills.join(", "))
                .filter(|s| !s.is_empty())
                .unwrap_or("stage default".into()),
            self.status,
            self.refresh_error
                .as_deref()
                .unwrap_or(&self.refresh_status)
        )
    }
    fn switch(&mut self, mode: Mode) {
        if self.mode == mode {
            return;
        }
        self.menu = None;
        if self.mode == Mode::Browse {
            self.browser_focus = self.focus;
        }
        self.mode = mode;
        self.focus = if mode == Mode::Chat {
            Focus::Composer
        } else {
            self.browser_focus
        };
    }
    fn category(&mut self, tab: usize) {
        self.bookmarks
            .insert(self.tab, (self.selected, self.scroll));
        self.tab = tab;
        (self.selected, self.scroll) = self.bookmarks.get(&tab).copied().unwrap_or((0, 0));
        self.selected = self.selected.min(self.visible().len().saturating_sub(1));
        self.list_state = ListState::default();
        self.doc_cache = (String::new(), 0);
    }
    fn select(&mut self, n: usize) {
        let n = n.min(self.visible().len().saturating_sub(1));
        if n != self.selected {
            self.selected = n;
            self.scroll = 0;
        }
    }
    fn latest(&mut self) {
        self.follow = true;
        self.focus = Focus::Composer;
    }
    fn popup(&mut self, title: &str, rows: Vec<(String, Choice)>) {
        self.menu = Some(Menu {
            title: title.into(),
            rows,
            selected: 0,
            offset: 0,
            area: Rect::default(),
        });
    }
    pub fn skills_menu(&mut self, catalog: Catalog) {
        let mut rows = catalog
            .skills
            .iter()
            .filter(|(n, _)| !catalog.errors.contains_key(*n))
            .map(|(n, s)| {
                (
                    format!("{} — {}", n, s.description),
                    Choice::Skill(n.clone()),
                )
            })
            .collect::<Vec<_>>();
        rows.extend(
            catalog
                .errors
                .iter()
                .map(|(n, e)| (format!("INVALID {n}: {e}"), Choice::None)),
        );
        if rows.is_empty() {
            rows.push(("No project Skills found".into(), Choice::None));
        }
        self.popup("Skills — Enter inserts an explicit invocation", rows);
    }
    fn action(&mut self, choice: Choice) -> Command {
        match choice {
            Choice::Edit
            | Choice::New(_)
            | Choice::Planning
            | Choice::Delivery
            | Choice::Inspect
                if self.busy =>
            {
                self.status = "Wait for the active operation before editing project records".into()
            }
            Choice::Edit => {
                return self
                    .selected_path()
                    .map(Command::Edit)
                    .unwrap_or(Command::None);
            }
            Choice::New(kind) => return Command::New(kind.into()),
            Choice::Planning => return Command::Planning,
            Choice::Delivery => return Command::Delivery,
            Choice::Inspect => {
                if let Some(id) = self.visible().get(self.selected).and_then(|d| d.id.clone()) {
                    return Command::Inspect(crate::inspection::Request {
                        id,
                        target: crate::inspection::Target::Impact,
                    });
                }
                self.status = "Inspection needs a managed project record with an ID".into();
            }
            Choice::Category(n) => self.category(n),
            Choice::Collapse => {
                self.collapsed = !self.collapsed;
                if self.collapsed {
                    self.focus = Focus::Content;
                }
            }
            Choice::Maximize => {
                if self.maximized {
                    if let Some(scroll) = self.restore_scroll.take() {
                        self.scroll = scroll;
                    }
                } else {
                    self.restore_scroll = Some(self.scroll);
                }
                self.maximized = !self.maximized;
                self.focus = Focus::Content;
            }
            Choice::Open => {
                return self
                    .selected_path()
                    .map(Command::Open)
                    .unwrap_or(Command::None);
            }
            Choice::Properties => {
                let text = self
                    .visible()
                    .get(self.selected)
                    .map(|d| {
                        format!(
                            "{}\nType: {}\nSHA256: {}\n{}",
                            d.path,
                            d.kind,
                            d.hash,
                            serde_json::to_string_pretty(&d.metadata).unwrap_or_default()
                        )
                    })
                    .unwrap_or_default();
                self.popup(
                    "Properties / provenance",
                    text.lines()
                        .map(|l| (l.to_string(), Choice::None))
                        .collect(),
                );
            }
            Choice::Links => {
                let links = if self.mode == Mode::Chat {
                    &self.transcript.links
                } else {
                    &self.document.links
                };
                let mut rows = links
                    .iter()
                    .enumerate()
                    .map(|(n, s)| (format!("[{}] {s}", n + 1), Choice::Link(s.clone())))
                    .collect::<Vec<_>>();
                if rows.is_empty() {
                    rows.push(("No links".into(), Choice::None));
                }
                self.popup("Links — Enter opens the selected target", rows);
            }
            Choice::Link(target) => return self.open_link(&target),
            Choice::Reference => {
                if let Some(path) = self.selected_path() {
                    self.switch(Mode::Chat);
                    self.input
                        .insert(&format!("Please inspect [{path}]({path}). "));
                }
            }
            Choice::Latest => self.latest(),
            Choice::Skills => return Command::Skills,
            Choice::Skill(name) => {
                self.switch(Mode::Chat);
                self.input.cursor = 0;
                self.input.insert(&format!("{}{name} ", '$'));
                self.focus = Focus::Composer;
            }
            Choice::None => {}
        }
        Command::None
    }
    fn open_link(&mut self, target: &str) -> Command {
        if target.starts_with("https://") || target.starts_with("http://") {
            return Command::Open(target.into());
        }
        let wiki = target.starts_with("wiki:");
        let target = target
            .strip_prefix("wiki:")
            .unwrap_or(target)
            .split('#')
            .next()
            .unwrap_or("")
            .replace("%20", " ");
        let current = self.selected_path().unwrap_or_default();
        let base = if wiki || self.mode == Mode::Chat {
            ""
        } else {
            current.rsplit_once('/').map(|p| p.0).unwrap_or("")
        };
        let mut parts = base
            .split('/')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();
        for part in target.split('/') {
            match part {
                "." | "" => {}
                ".." => {
                    if parts.pop().is_none() {
                        self.status = "Link escapes workspace".into();
                        return Command::None;
                    }
                }
                _ => parts.push(part.into()),
            }
        }
        let path = parts.join("/");
        let candidates = self
            .docs
            .iter()
            .filter(|d| {
                d.path == path
                    || d.path == format!("{path}.md")
                    || wiki
                        && d.path
                            .rsplit('/')
                            .next()
                            .is_some_and(|s| s == path || s == format!("{path}.md"))
            })
            .map(|d| d.path.clone())
            .collect::<Vec<_>>();
        if candidates.len() == 1 {
            self.filter.clear();
            self.category(0);
            self.switch(Mode::Browse);
            self.collapsed = false;
            self.maximized = false;
            self.selected = self
                .visible()
                .iter()
                .position(|d| d.path == candidates[0])
                .unwrap_or(0);
            self.scroll = 0;
            self.focus = Focus::Content;
        } else if candidates.is_empty() && !wiki && !path.is_empty() {
            return Command::Open(path);
        } else {
            self.status = "Link target missing or ambiguous; select an exact project path".into();
        }
        Command::None
    }
    fn context_menu(&mut self) {
        if self.mode == Mode::Browse {
            self.popup(
                "Document actions",
                vec![
                    ("Edit project fields".into(), Choice::Edit),
                    ("Read document".into(), Choice::Maximize),
                    ("Open original".into(), Choice::Open),
                    ("Properties / provenance".into(), Choice::Properties),
                    ("F8 Impact / compare".into(), Choice::Inspect),
                    ("Open link".into(), Choice::Links),
                    ("Reference in chat".into(), Choice::Reference),
                    ("Collapse / expand list".into(), Choice::Collapse),
                ],
            );
        } else {
            self.popup(
                "Conversation actions",
                vec![
                    ("Latest message and input".into(), Choice::Latest),
                    ("Open link".into(), Choice::Links),
                    ("Project Skills".into(), Choice::Skills),
                ],
            );
        }
    }
    fn scroll_by(&mut self, down: bool, n: usize) {
        let (pos, max) = if self.mode == Mode::Chat {
            self.focus = Focus::Transcript;
            self.follow = false;
            (
                &mut self.transcript_scroll,
                self.transcript
                    .lines
                    .len()
                    .saturating_sub(self.regions.transcript.height as usize),
            )
        } else {
            (
                &mut self.scroll,
                self.document
                    .lines
                    .len()
                    .saturating_sub(self.regions.content.height as usize),
            )
        };
        *pos = if down {
            pos.saturating_add(n).min(max)
        } else {
            pos.saturating_sub(n)
        };
    }
    fn bar_position(&mut self, y: u16) {
        let area = self.regions.bar;
        let max = if self.mode == Mode::Chat {
            self.transcript
                .lines
                .len()
                .saturating_sub(area.height as usize)
        } else {
            self.document
                .lines
                .len()
                .saturating_sub(area.height as usize)
        };
        let offset = max.saturating_mul(y.saturating_sub(area.y) as usize)
            / area.height.saturating_sub(1).max(1) as usize;
        if self.mode == Mode::Chat {
            self.transcript_scroll = offset.min(max);
            self.follow = false;
            self.focus = Focus::Transcript;
        } else {
            self.scroll = offset.min(max);
            self.focus = Focus::Content;
        }
    }
    pub fn event(&mut self, event: Event) -> Command {
        if let Some(inspector) = &mut self.inspector {
            return inspector.event(event);
        }
        if let Some(form) = &mut self.form {
            return match form.event(event) {
                super::form::Action::None => Command::None,
                super::form::Action::Review => Command::ReviewEdit,
                super::form::Action::Save => Command::SaveEdit,
                super::form::Action::Close => Command::CloseEdit,
            };
        }
        if let Some(planner) = &mut self.planner {
            return planner.event(event);
        }
        if let Some(delivery) = &mut self.delivery {
            return delivery.event(event);
        }
        match event {
            Event::Paste(text)
                if self.mode == Mode::Chat
                    && self.focus == Focus::Composer
                    && self.menu.is_none() =>
            {
                self.input.insert(&text)
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => return self.key(key),
            Event::Mouse(mouse) => {
                let p = Position::new(mouse.column, mouse.row);
                if let Some(menu) = &mut self.menu {
                    match mouse.kind {
                        MouseEventKind::ScrollUp => menu.selected = menu.selected.saturating_sub(1),
                        MouseEventKind::ScrollDown => {
                            menu.selected =
                                (menu.selected + 1).min(menu.rows.len().saturating_sub(1))
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            if menu.area.contains(p) && mouse.row > menu.area.y {
                                let n = menu.offset
                                    + mouse.row.saturating_sub(menu.area.y + 1) as usize;
                                if let Some((_, choice)) = menu.rows.get(n).cloned() {
                                    self.menu = None;
                                    return self.action(choice);
                                }
                            } else {
                                self.menu = None;
                            }
                        }
                        _ => {}
                    }
                    return Command::None;
                }
                match mouse.kind {
                    MouseEventKind::Up(_) => self.dragging = false,
                    MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                        self.bar_position(mouse.row)
                    }
                    MouseEventKind::Down(MouseButton::Left) if self.regions.bar.contains(p) => {
                        self.dragging = true;
                        self.bar_position(mouse.row);
                    }
                    MouseEventKind::Down(MouseButton::Left) if self.regions.browse.contains(p) => {
                        self.switch(Mode::Browse)
                    }
                    MouseEventKind::Down(MouseButton::Left) if self.regions.chat.contains(p) => {
                        self.switch(Mode::Chat)
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.regions.delivery.contains(p) =>
                    {
                        return self.action(Choice::Delivery);
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.regions.planning.contains(p) =>
                    {
                        return self.action(Choice::Planning);
                    }
                    MouseEventKind::Down(MouseButton::Left) if self.regions.status.contains(p) => {
                        let text = self.status_details();
                        self.popup(
                            "Session status",
                            text.lines()
                                .map(|l| (l.to_string(), Choice::None))
                                .collect(),
                        );
                    }
                    MouseEventKind::Down(MouseButton::Left) if self.regions.more.contains(p) => {
                        self.categories()
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.regions.collapse.contains(p) =>
                    {
                        return self.action(Choice::Collapse);
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.regions.maximize.contains(p) =>
                    {
                        return self.action(Choice::Maximize);
                    }
                    MouseEventKind::Down(button) => {
                        if let Some((_, n)) =
                            self.regions.categories.iter().find(|(r, _)| r.contains(p))
                        {
                            self.category(*n);
                        } else if self.regions.list.contains(p) {
                            self.focus = Focus::Documents;
                            if mouse.row > self.regions.list.y {
                                self.select(
                                    self.list_state.offset()
                                        + mouse.row.saturating_sub(self.regions.list.y + 1)
                                            as usize,
                                );
                            }
                        } else if self.regions.content.contains(p) {
                            self.focus = Focus::Content;
                        } else if self.regions.transcript.contains(p) {
                            self.focus = Focus::Transcript;
                        } else if self.regions.composer.contains(p) {
                            self.focus = Focus::Composer;
                            self.input.click(
                                self.regions.composer.width.saturating_sub(2) as usize,
                                self.editor_offset
                                    + mouse.row.saturating_sub(self.regions.composer.y + 1)
                                        as usize,
                                mouse.column.saturating_sub(self.regions.composer.x + 1) as usize,
                            );
                        }
                        if button == MouseButton::Right {
                            self.context_menu();
                        }
                    }
                    MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                        let down = mouse.kind == MouseEventKind::ScrollDown;
                        if self.mode == Mode::Browse && self.regions.list.contains(p) {
                            self.focus = Focus::Documents;
                            self.select(if down {
                                self.selected + 3
                            } else {
                                self.selected.saturating_sub(3)
                            });
                        } else if self.regions.transcript.contains(p)
                            || self.regions.content.contains(p)
                            || self.regions.bar.contains(p)
                        {
                            self.scroll_by(down, 3);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Command::None
    }
    fn categories(&mut self) {
        self.popup(
            "Document categories",
            TABS.iter()
                .enumerate()
                .map(|(n, (icon, name))| {
                    (
                        if self.icons {
                            format!("{icon} {name}")
                        } else {
                            name.to_string()
                        },
                        Choice::Category(n),
                    )
                })
                .collect(),
        );
    }
    fn key(&mut self, key: KeyEvent) -> Command {
        if key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && self.mode == Mode::Chat
        {
            self.input.clear();
            return Command::None;
        }
        // A key is consumed by one layer only. Repeat events never cascade Esc into cancellation.
        if let Some(menu) = &mut self.menu {
            match key.code {
                KeyCode::Esc => self.menu = None,
                KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
                KeyCode::Down => {
                    menu.selected = (menu.selected + 1).min(menu.rows.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    let choice = menu
                        .rows
                        .get(menu.selected)
                        .map(|(_, c)| c.clone())
                        .unwrap_or(Choice::None);
                    self.menu = None;
                    return self.action(choice);
                }
                _ => {}
            }
            return Command::None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q') {
            return if self.busy {
                self.status = "Cancel the active operation before exiting".into();
                Command::None
            } else {
                Command::Quit
            };
        }
        match key.code {
            KeyCode::F(1) => {
                self.popup(
                    "Help",
                    HELP.lines()
                        .map(|l| (l.to_string(), Choice::None))
                        .collect(),
                );
                return Command::None;
            }
            KeyCode::F(2) => {
                self.switch(if self.mode == Mode::Browse {
                    Mode::Chat
                } else {
                    Mode::Browse
                });
                return Command::None;
            }
            _ => {}
        }
        if self.mode == Mode::Browse {
            if key.modifiers.contains(KeyModifiers::ALT) {
                match key.code {
                    KeyCode::Char('0') => self.categories(),
                    KeyCode::Char(c) if ('1'..='9').contains(&c) => {
                        self.category((c as u8 - b'1') as usize)
                    }
                    KeyCode::Left => self.category((self.tab + TABS.len() - 1) % TABS.len()),
                    KeyCode::Right => self.category((self.tab + 1) % TABS.len()),
                    _ => {}
                }
                return Command::None;
            }
            match key.code {
                KeyCode::F(5) => return self.action(Choice::Planning),
                KeyCode::F(7) => return self.action(Choice::Delivery),
                KeyCode::F(8) => return self.action(Choice::Inspect),
                KeyCode::F(3) => return self.action(Choice::Collapse),
                KeyCode::F(4) => return self.action(Choice::Maximize),
                KeyCode::F(6) => self.icons = !self.icons,
                KeyCode::Esc => {
                    if self.maximized
                        && let Some(scroll) = self.restore_scroll.take()
                    {
                        self.scroll = scroll;
                    }
                    self.maximized = false;
                    self.collapsed = false;
                    self.focus = Focus::Documents;
                }
                KeyCode::Enter => self.focus = Focus::Content,
                KeyCode::Tab | KeyCode::BackTab => {
                    self.focus = if self.focus == Focus::Documents {
                        Focus::Content
                    } else {
                        self.collapsed = false;
                        self.maximized = false;
                        Focus::Documents
                    };
                }
                KeyCode::Up | KeyCode::Char('k') if self.focus == Focus::Documents => {
                    self.select(self.selected.saturating_sub(1))
                }
                KeyCode::Down | KeyCode::Char('j') if self.focus == Focus::Documents => {
                    self.select(self.selected + 1)
                }
                KeyCode::Up | KeyCode::Char('k') => self.scroll_by(false, 1),
                KeyCode::Down | KeyCode::Char('j') => self.scroll_by(true, 1),
                KeyCode::PageUp => self.scroll_by(false, self.regions.content.height as usize),
                KeyCode::PageDown => self.scroll_by(true, self.regions.content.height as usize),
                KeyCode::Home => self.scroll = 0,
                KeyCode::End => self.scroll = usize::MAX,
                KeyCode::Char('r') => return Command::Refresh,
                KeyCode::Char('e') => return self.action(Choice::Edit),
                KeyCode::Char('n') if !self.busy => self.popup(
                    "New project record",
                    vec![
                        ("PBI".into(), Choice::New("pbi")),
                        ("Risk".into(), Choice::New("risk")),
                        ("Decision".into(), Choice::New("decision")),
                    ],
                ),
                KeyCode::Char('o') => return self.action(Choice::Open),
                KeyCode::Char('p') => return self.action(Choice::Properties),
                KeyCode::Char('l') => return self.action(Choice::Links),
                KeyCode::Char('q') if !self.busy => return Command::Quit,
                _ => {}
            }
        } else {
            match key.code {
                KeyCode::Esc if self.focus == Focus::Transcript => self.latest(),
                KeyCode::Esc if self.busy => return Command::Cancel,
                KeyCode::Esc => {}
                KeyCode::PageUp => self.scroll_by(false, self.regions.transcript.height as usize),
                KeyCode::PageDown => self.scroll_by(true, self.regions.transcript.height as usize),
                KeyCode::Tab if self.focus == Focus::Transcript => self.focus = Focus::Composer,
                KeyCode::BackTab => self.focus = Focus::Transcript,
                KeyCode::Home if self.focus == Focus::Transcript => {
                    self.follow = false;
                    self.transcript_scroll = 0;
                }
                KeyCode::End if self.focus == Focus::Transcript => self.latest(),
                KeyCode::Up if self.focus == Focus::Transcript => self.scroll_by(false, 1),
                KeyCode::Down if self.focus == Focus::Transcript => self.scroll_by(true, 1),
                KeyCode::Enter
                    if self.focus == Focus::Composer
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) =>
                {
                    if self.busy {
                        self.status = "Running — draft retained; Esc cancels".into();
                    } else if !self.input.text.trim().is_empty() {
                        self.follow = true;
                        return Command::Submit(self.input.take());
                    }
                }
                _ if self.focus == Focus::Composer => self.input.key(key),
                _ => {}
            }
        }
        Command::None
    }
    pub fn draw(&mut self, f: &mut ratatui::Frame) {
        self.draw_content(f);
        if let Some(error) = &self.refresh_error {
            let area = f.area();
            let row = Rect::new(area.x, area.y, area.width, area.height.min(1));
            f.render_widget(
                Paragraph::new(format!(
                    "Files unavailable — {error} — retrying automatically"
                ))
                .style(Style::default().fg(Color::Black).bg(Color::Yellow)),
                row,
            );
        }
    }
    fn draw_content(&mut self, f: &mut ratatui::Frame) {
        if let Some(inspector) = &mut self.inspector {
            inspector.draw(f);
            return;
        }
        if let Some(form) = &mut self.form {
            form.draw(f);
            return;
        }
        if let Some(planner) = &mut self.planner {
            planner.draw(f);
            return;
        }
        if let Some(delivery) = &mut self.delivery {
            delivery.draw(f);
            return;
        }
        let all = f.area();
        self.regions = Regions::default();
        let outer = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(all);
        self.regions.browse = Rect::new(
            outer[0].x,
            outer[0].y,
            outer[0].width.min(18),
            outer[0].height,
        );
        self.regions.chat = Rect::new(
            outer[0].x + outer[0].width.min(18),
            outer[0].y,
            outer[0].width.saturating_sub(18).min(18),
            outer[0].height,
        );
        for (area, label, active) in [
            (self.regions.browse, " Browse ", self.mode == Mode::Browse),
            (self.regions.chat, " Agent Chat ", self.mode == Mode::Chat),
        ] {
            f.render_widget(
                Paragraph::new(label).style(if active {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                }),
                area,
            );
        }
        if self.mode == Mode::Browse {
            self.regions.planning = Rect::new(
                outer[0].x + outer[0].width.min(36),
                outer[0].y,
                outer[0].width.saturating_sub(36).min(20),
                outer[0].height,
            );
            f.render_widget(
                Paragraph::new(" F5 Planning ").style(Style::default().fg(Color::Gray)),
                self.regions.planning,
            );
            self.regions.delivery = Rect::new(
                outer[0].x + outer[0].width.min(56),
                outer[0].y,
                outer[0].width.saturating_sub(56).min(20),
                outer[0].height,
            );
            f.render_widget(
                Paragraph::new(" F7 Delivery ").style(Style::default().fg(Color::Gray)),
                self.regions.delivery,
            );
            self.draw_browser(f, outer[1]);
        } else {
            self.draw_chat(f, outer[1]);
        }
        self.regions.status = outer[2];
        let id = self
            .session
            .as_ref()
            .map(|s| s.id.chars().take(8).collect::<String>())
            .unwrap_or("—".into());
        let state = if self.busy {
            "running".to_string()
        } else {
            self.session
                .as_ref()
                .map(|s| format!("{:?}", s.status))
                .unwrap_or("ready".into())
        };
        let status = if self.mode == Mode::Chat {
            let input = self
                .usage
                .as_ref()
                .and_then(|u| u.input_tokens)
                .map(|n| n.to_string())
                .unwrap_or("not provided".into());
            let rest = format!("{state} | ID:{id} | Context input: {input}");
            let available = (outer[2].width as usize).saturating_sub(rest.width() + 3);
            let mut model = String::new();
            for c in self.model.chars() {
                let next = format!("{model}{c}");
                if next.width() > available {
                    break;
                }
                model = next;
            }
            format!("{model} | {rest}")
        } else {
            format!(
                "Auto refresh: 2s | F1 Help | {} | {}",
                self.status, self.refresh_status
            )
        };
        f.render_widget(
            Paragraph::new(status).style(Style::default().fg(Color::Gray)),
            outer[2],
        );
        if let Some(menu) = &mut self.menu {
            let width = all.width.min(90);
            let height = all
                .height
                .saturating_sub(2)
                .min((menu.rows.len() + 2) as u16);
            let area = Rect::new(
                all.x + (all.width - width) / 2,
                all.y + (all.height - height) / 2,
                width,
                height,
            );
            menu.area = area;
            f.render_widget(Clear, area);
            let mut state = ListState::default().with_selected(Some(menu.selected));
            *state.offset_mut() = menu.offset;
            f.render_stateful_widget(
                List::new(
                    menu.rows
                        .iter()
                        .map(|(label, _)| ListItem::new(label.as_str())),
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(menu.title.as_str()),
                )
                .highlight_style(Style::default().bg(Color::DarkGray)),
                area,
                &mut state,
            );
            menu.offset = state.offset();
        }
    }
    fn draw_browser(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(area);
        let mut x = rows[0].x;
        let limit = rows[0].right().saturating_sub(8);
        let mut start = 0;
        let needed = TABS[..=self.tab]
            .iter()
            .enumerate()
            .map(|(i, (icon, name))| self.category_label(i, icon, name).width() as u16 + 2)
            .sum::<u16>();
        if needed > limit.saturating_sub(x) {
            start = self.tab;
        }
        for (i, (icon, name)) in TABS.iter().enumerate().skip(start) {
            let label = self.category_label(i, icon, name);
            let width = label.width() as u16 + 2;
            if x + width > limit {
                break;
            }
            let rect = Rect::new(x, rows[0].y, width, rows[0].height);
            self.regions.categories.push((rect, i));
            f.render_widget(
                Paragraph::new(label).style(if i == self.tab {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                }),
                rect,
            );
            x += width;
        }
        self.regions.more = Rect::new(
            limit.max(rows[0].x),
            rows[0].y,
            rows[0].width.min(8),
            rows[0].height,
        );
        f.render_widget(Paragraph::new(" More ▾"), self.regions.more);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(if self.maximized {
                    0
                } else if self.collapsed {
                    3
                } else {
                    rows[1]
                        .width
                        .div_ceil(4)
                        .clamp(18, 36)
                        .min(rows[1].width / 2)
                }),
                Constraint::Min(0),
            ])
            .split(rows[1]);
        self.regions.list = columns[0];
        self.regions.collapse = Rect::new(
            columns[0].right().saturating_sub(3),
            columns[0].y,
            columns[0].width.min(3),
            columns[0].height.min(1),
        );
        if !self.maximized {
            if self.collapsed {
                f.render_widget(Paragraph::new(" ▶"), columns[0]);
            } else {
                let items = self
                    .visible()
                    .iter()
                    .map(|d| {
                        ListItem::new(format!(
                            "{}{} {}",
                            if self.icons { icon(d) } else { "" },
                            d.id.as_deref().unwrap_or(""),
                            d.title
                        ))
                    })
                    .collect::<Vec<_>>();
                self.list_state.select(if items.is_empty() {
                    None
                } else {
                    Some(self.selected.min(items.len() - 1))
                });
                let border = if self.focus == Focus::Documents {
                    Color::Cyan
                } else {
                    Color::DarkGray
                };
                f.render_stateful_widget(
                    List::new(items)
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(border))
                                .title("Documents"),
                        )
                        .highlight_style(Style::default().bg(Color::DarkGray)),
                    columns[0],
                    &mut self.list_state,
                );
                f.render_widget(Paragraph::new(" ◀"), self.regions.collapse);
            }
        }
        let title = self
            .visible()
            .get(self.selected)
            .map(|d| d.title.clone())
            .unwrap_or("No documents".into());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if self.focus == Focus::Content {
                Color::Cyan
            } else {
                Color::DarkGray
            }))
            .title(title);
        let inner = block.inner(columns[1]);
        f.render_widget(block, columns[1]);
        self.regions.maximize = Rect::new(
            columns[1].right().saturating_sub(11).max(columns[1].x),
            columns[1].y,
            columns[1].width.min(11),
            columns[1].height.min(1),
        );
        f.render_widget(
            Paragraph::new(if self.maximized {
                "[ Restore ]"
            } else {
                "[ Maximize]"
            }),
            self.regions.maximize,
        );
        self.regions.content = Rect::new(
            inner.x,
            inner.y,
            inner.width.saturating_sub(1),
            inner.height,
        );
        let width = self.regions.content.width as usize;
        if let Some(doc) = self.visible().get(self.selected) {
            let key = (format!("{}:{}", doc.path, doc.hash), width);
            if self.doc_cache != key {
                self.document = markdown::render(&doc.body, width);
                self.doc_cache = key;
            }
        } else {
            self.document = markdown::render("No records in this view.", width);
            self.doc_cache = (String::new(), 0);
        }
        self.scroll = self.scroll.min(
            self.document
                .lines
                .len()
                .saturating_sub(inner.height as usize),
        );
        f.render_widget(
            Paragraph::new(
                self.document
                    .lines
                    .iter()
                    .skip(self.scroll)
                    .take(inner.height as usize)
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            self.regions.content,
        );
        self.draw_scrollbar(f, inner, self.document.lines.len(), self.scroll);
    }
    fn category_label(&self, i: usize, icon: &str, name: &str) -> String {
        format!(
            "{}{}{}",
            if i < 9 {
                format!("{} ", i + 1)
            } else {
                String::new()
            },
            if self.icons {
                format!("{icon} ")
            } else {
                String::new()
            },
            name
        )
    }
    fn draw_scrollbar(&mut self, f: &mut ratatui::Frame, area: Rect, len: usize, position: usize) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.regions.bar = Rect::new(area.right() - 1, area.y, 1, area.height);
        // The widget's content length is the number of possible top-row positions.
        let positions = len.saturating_sub(area.height as usize) + 1;
        let mut state = ScrollbarState::new(positions)
            .position(position)
            .viewport_content_length(area.height as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            self.regions.bar,
            &mut state,
        );
    }
    fn draw_chat(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let width = area.width.saturating_sub(2).max(1) as usize;
        let (input, caret) = self.input.rows(width);
        let height = ((input.len() + 2) as u16)
            .clamp(3, area.height.div_ceil(3).max(3))
            .min(area.height);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(height)])
            .split(area);
        self.regions.transcript = Rect::new(
            rows[0].x,
            rows[0].y,
            rows[0].width.saturating_sub(1),
            rows[0].height,
        );
        self.regions.composer = rows[1];
        let key = (self.output.len(), self.regions.transcript.width as usize);
        if self.chat_cache != key {
            self.transcript = markdown::render(&self.output, key.1);
            self.chat_cache = key;
        }
        let max = self
            .transcript
            .lines
            .len()
            .saturating_sub(rows[0].height as usize);
        self.transcript_scroll = if self.follow {
            max
        } else {
            self.transcript_scroll.min(max)
        };
        f.render_widget(
            Paragraph::new(
                self.transcript
                    .lines
                    .iter()
                    .skip(self.transcript_scroll)
                    .take(rows[0].height as usize)
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            self.regions.transcript,
        );
        self.draw_scrollbar(
            f,
            rows[0],
            self.transcript.lines.len(),
            self.transcript_scroll,
        );
        let bg = Style::default().bg(Color::DarkGray).fg(Color::White);
        f.render_widget(Block::default().style(bg), rows[1]);
        let inner = Rect::new(
            rows[1].x.saturating_add(1),
            rows[1].y.saturating_add(1),
            rows[1].width.saturating_sub(2),
            rows[1].height.saturating_sub(2),
        );
        if caret.0 < self.editor_offset {
            self.editor_offset = caret.0;
        }
        if caret.0 >= self.editor_offset + inner.height as usize {
            self.editor_offset = caret
                .0
                .saturating_sub(inner.height.saturating_sub(1) as usize);
        }
        let lines = input
            .iter()
            .skip(self.editor_offset)
            .take(inner.height as usize)
            .map(|s| Line::raw(s.clone()))
            .collect::<Vec<_>>();
        f.render_widget(Paragraph::new(lines).style(bg), inner);
        if self.input.text.is_empty() {
            f.render_widget(
                Paragraph::new("Message…  /skills  Enter sends · Alt+Enter newline")
                    .style(bg.fg(Color::Gray)),
                inner,
            );
        }
        if self.focus == Focus::Composer
            && self.menu.is_none()
            && inner.width > 0
            && inner.height > 0
        {
            f.set_cursor_position((
                inner.x + (caret.1 as u16).min(inner.width - 1),
                inner.y + caret.0.saturating_sub(self.editor_offset) as u16,
            ));
        }
    }
}
fn icon(doc: &Document) -> &'static str {
    match doc.kind.as_str() {
        "brd" => "📋 ",
        "prd" => "🎯 ",
        "requirement" => "🧩 ",
        "question" => "❓ ",
        "person" => "👤 ",
        "iteration" | "milestone" => "📅 ",
        "pbi" => "🗂️ ",
        "risk" => "⚠️ ",
        "change" => "🔁 ",
        "uat" => "☑️ ",
        "release" => "🚀 ",
        "benefit" => "📈 ",
        "test_case" | "testcase" | "test_run" => "✅ ",
        "agent_run" | "Skill" => "🤖 ",
        "context" | "term" | "glossary" => "📚 ",
        "system" | "system_analysis" => "🏗️ ",
        _ => "📄 ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(app: &mut App, code: KeyCode) -> Command {
        app.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }
    fn draw(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        terminal.backend().buffer().clone()
    }
    fn mouse(kind: MouseEventKind, area: Rect) -> Event {
        Event::Mouse(crossterm::event::MouseEvent {
            kind,
            column: area.x,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        })
    }
    #[test]
    fn refresh_preserves_document_identity_category_bookmarks_and_chat_reading_state() {
        let doc = |id: &str| Document {
            path: format!("04_Requirements/BRD/{id}.md"),
            title: id.into(),
            kind: "brd".into(),
            id: Some(id.into()),
            hash: id.into(),
            metadata: serde_json::json!({}),
            body: "# Fixture\n\nBody\n\n".repeat(60),
        };
        let mut a = App::new(vec![doc("B"), doc("C")]);
        a.category(1);
        a.selected = 1;
        a.scroll = 12;
        a.category(0);
        a.selected = 1;
        a.scroll = 10;
        a.focus = Focus::Content;
        a.maximized = true;
        a.replace_documents(vec![doc("A"), doc("B"), doc("C")]);
        assert_eq!(a.selected, 2);
        assert_eq!(a.scroll, 10);
        assert!(a.maximized);
        assert_eq!(a.focus, Focus::Content);
        a.category(1);
        assert_eq!((a.selected, a.scroll), (2, 12));
        a.switch(Mode::Chat);
        a.input.insert("Draft 中文");
        draw(&mut a, 80, 24);
        a.scroll_by(false, 5);
        let reading = a.transcript_scroll;
        let history = a.output.clone();
        a.replace_documents(vec![doc("A"), doc("B"), doc("C")]);
        draw(&mut a, 80, 24);
        assert_eq!(a.mode, Mode::Chat);
        assert_eq!(a.focus, Focus::Transcript);
        assert!(!a.follow);
        assert_eq!(a.transcript_scroll, reading);
        assert_eq!(a.input.text, "Draft 中文");
        assert_eq!(a.output, history);
        a.replace_documents(vec![doc("A")]);
        assert_eq!(a.selected, 0);
        assert_eq!(a.scroll, 0);
        a.refresh_error = Some("Synthetic malformed input".into());
        let rendered = draw(&mut a, 120, 24)
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(rendered.contains("Files unavailable"));
    }
    #[test]
    fn inspection_entry_requires_a_record_preserves_drafts_and_is_browse_only() {
        let mut a = App::new(vec![]);
        assert_eq!(key(&mut a, KeyCode::F(8)), Command::None);
        a.docs.push(Document {
            path: "04_Requirements/REQ-TEST.md".into(),
            title: "Synthetic".into(),
            kind: "requirement".into(),
            id: Some("REQ-TEST".into()),
            hash: "fixture".into(),
            metadata: serde_json::json!({}),
            body: "# Synthetic".into(),
        });
        a.input.insert("Unsent draft 中文");
        let request = crate::inspection::Request {
            id: "REQ-TEST".into(),
            target: crate::inspection::Target::Impact,
        };
        assert_eq!(
            key(&mut a, KeyCode::F(8)),
            Command::Inspect(request.clone())
        );
        a.busy = true;
        assert_eq!(key(&mut a, KeyCode::F(8)), Command::None);
        a.busy = false;
        a.switch(Mode::Chat);
        assert_eq!(key(&mut a, KeyCode::F(8)), Command::None);
        a.switch(Mode::Browse);
        a.inspector = Some(super::super::inspection::Inspector::new(request));
        a.read_document("04_Requirements/REQ-TEST.md");
        assert!(a.inspector.is_none());
        assert_eq!(a.input.text, "Unsent draft 中文");
    }
    #[test]
    fn delivery_entry_preserves_chat_and_blocks_while_running() {
        let mut a = App::new(vec![]);
        a.input.insert("Unfinished chat draft");
        assert_eq!(key(&mut a, KeyCode::F(7)), Command::Delivery);
        draw(&mut a, 120, 30);
        assert_eq!(
            a.event(mouse(
                MouseEventKind::Down(MouseButton::Left),
                a.regions.delivery
            )),
            Command::Delivery
        );
        a.busy = true;
        assert_eq!(key(&mut a, KeyCode::F(7)), Command::None);
        a.busy = false;
        key(&mut a, KeyCode::F(2));
        assert_eq!(key(&mut a, KeyCode::F(7)), Command::None);
        assert_eq!(a.input.text, "Unfinished chat draft");
    }
    #[test]
    fn planning_entry_is_local_to_browse_and_respects_running_operations() {
        let mut a = App::new(vec![]);
        a.input.insert("Unfinished chat draft");
        assert_eq!(key(&mut a, KeyCode::F(5)), Command::Planning);
        draw(&mut a, 120, 30);
        assert_eq!(
            a.event(mouse(
                MouseEventKind::Down(MouseButton::Left),
                a.regions.planning
            )),
            Command::Planning
        );
        a.busy = true;
        assert_eq!(key(&mut a, KeyCode::F(5)), Command::None);
        assert_eq!(
            a.event(mouse(
                MouseEventKind::Down(MouseButton::Left),
                a.regions.planning
            )),
            Command::None
        );
        a.switch(Mode::Chat);
        assert_eq!(key(&mut a, KeyCode::F(5)), Command::None);
        assert_eq!(a.input.text, "Unfinished chat draft");
    }
    #[test]
    fn escape_consumes_reading_focus_before_cancellation_and_ctrl_c_only_clears() {
        let mut a = App::new(vec![]);
        a.switch(Mode::Chat);
        a.busy = true;
        a.input.insert("retained draft");
        a.append(&"\n\nHistory\n".repeat(60));
        draw(&mut a, 80, 24);
        a.event(mouse(MouseEventKind::ScrollUp, a.regions.transcript));
        assert_eq!(a.focus, Focus::Transcript);
        assert!(!a.follow);
        assert_eq!(key(&mut a, KeyCode::Esc), Command::None);
        assert_eq!(a.focus, Focus::Composer);
        assert!(a.follow);
        assert_eq!(a.input.text, "retained draft");
        let repeat =
            KeyEvent::new_with_kind(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Repeat);
        assert_eq!(a.event(Event::Key(repeat)), Command::None);
        assert_eq!(key(&mut a, KeyCode::Esc), Command::Cancel);
        assert_eq!(
            a.event(Event::Key(KeyEvent::new(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL
            ))),
            Command::None
        );
        assert!(a.input.text.is_empty());
        assert!(a.busy);
    }
    #[test]
    fn input_is_ready_multiline_and_survives_mode_switching() {
        let mut a = App::new(vec![]);
        key(&mut a, KeyCode::F(2));
        key(&mut a, KeyCode::Char('中'));
        assert_eq!(a.event(Event::Paste("\n第二行".into())), Command::None);
        key(&mut a, KeyCode::F(2));
        key(&mut a, KeyCode::F(2));
        assert_eq!(
            key(&mut a, KeyCode::Enter),
            Command::Submit("中\n第二行".into())
        );
        assert_eq!(a.focus, Focus::Composer);
        a.busy = true;
        a.input.insert("next");
        assert_eq!(key(&mut a, KeyCode::Enter), Command::None);
        assert_eq!(a.input.text, "next");
    }
    #[test]
    fn list_focus_collapse_maximize_and_category_mouse() {
        let mut a = App::new(vec![]);
        draw(&mut a, 140, 40);
        key(&mut a, KeyCode::Enter);
        assert_eq!(a.focus, Focus::Content);
        key(&mut a, KeyCode::F(3));
        assert!(a.collapsed);
        key(&mut a, KeyCode::F(4));
        assert!(a.maximized);
        key(&mut a, KeyCode::Esc);
        assert!(!a.maximized && !a.collapsed);
        assert_eq!(a.focus, Focus::Documents);
        a.event(Event::Key(KeyEvent::new(
            KeyCode::Char('3'),
            KeyModifiers::ALT,
        )));
        assert_eq!(a.tab, 2);
        a.event(Event::Key(KeyEvent::new(
            KeyCode::Char('0'),
            KeyModifiers::ALT,
        )));
        assert_eq!(a.menu.as_ref().unwrap().rows.len(), 11);
        key(&mut a, KeyCode::Esc);
        draw(&mut a, 140, 40);
        let first = a.regions.categories[0].0;
        a.event(mouse(MouseEventKind::Down(MouseButton::Left), first));
        assert_eq!(a.tab, 0);
    }
    #[test]
    fn streaming_keeps_reading_position_and_scrollbar_drag_can_reach_bottom() {
        let mut a = App::new(vec![]);
        a.switch(Mode::Chat);
        let buffer = draw(&mut a, 70, 20);
        assert_eq!(
            buffer[(a.regions.bar.x, a.regions.bar.bottom() - 1)].symbol(),
            "█"
        );
        a.scroll_by(false, 5);
        let offset = a.transcript_scroll;
        a.append("\n\nnew reply");
        draw(&mut a, 70, 20);
        assert_eq!(a.transcript_scroll, offset);
        let bar = a.regions.bar;
        a.event(mouse(
            MouseEventKind::Down(MouseButton::Left),
            Rect::new(bar.x, bar.bottom() - 1, 1, 1),
        ));
        assert_eq!(
            a.transcript_scroll,
            a.transcript.lines.len() - a.regions.transcript.height as usize
        );
        assert_eq!(a.focus, Focus::Transcript);
        key(&mut a, KeyCode::Esc);
        draw(&mut a, 70, 20);
        assert!(a.follow);
    }
    #[test]
    fn chat_is_borderless_and_input_has_gray_background() {
        let mut a = App::new(vec![]);
        a.switch(Mode::Chat);
        a.output = "**Rendered**".into();
        let buffer = draw(&mut a, 90, 24);
        assert_eq!(
            buffer[(a.regions.composer.x, a.regions.composer.y)].bg,
            Color::DarkGray
        );
        let text = buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(!text.contains("**"));
        assert!(!text.contains('┌'));
        for (w, h) in [(1, 1), (2, 2), (30, 8), (80, 24), (140, 40)] {
            draw(&mut a, w, h);
        }
    }
    #[test]
    fn missing_usage_is_unknown_and_menu_escape_never_cancels() {
        let mut a = App::new(vec![]);
        a.switch(Mode::Chat);
        a.busy = true;
        assert!(a.usage_text().contains("not provided"));
        assert!(!a.usage_text().contains("0%"));
        a.usage = Some(Usage {
            input_tokens: Some(0),
            ..Usage::default()
        });
        assert!(a.usage_text().contains("input: 0"));
        assert!(a.usage_text().contains("output: not provided"));
        key(&mut a, KeyCode::F(1));
        assert_eq!(key(&mut a, KeyCode::Esc), Command::None);
        assert!(a.menu.is_none());
    }
}
