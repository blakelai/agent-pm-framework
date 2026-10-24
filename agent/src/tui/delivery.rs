use super::{markdown, view::Command};
use crate::delivery::{Entry, KINDS, Snapshot};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};

pub struct Delivery {
    snapshot: Snapshot,
    tab: usize,
    selected: usize,
    scroll: usize,
    max_scroll: usize,
    detail_focus: bool,
    icons: bool,
    list: ListState,
    tabs: Vec<Rect>,
    list_area: Rect,
    detail_area: Rect,
    bar: Rect,
    actions: Vec<(Rect, char)>,
    pub message: String,
}
impl Delivery {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            tab: 0,
            selected: 0,
            scroll: 0,
            max_scroll: 0,
            detail_focus: false,
            icons: true,
            list: ListState::default(),
            tabs: vec![],
            list_area: Rect::default(),
            detail_area: Rect::default(),
            bar: Rect::default(),
            actions: vec![],
            message: String::new(),
        }
    }
    pub fn replace(&mut self, snapshot: Snapshot) {
        let path = self.current().map(|e| e.document.path.clone());
        self.snapshot = snapshot;
        self.selected = path
            .and_then(|p| self.items().iter().position(|e| e.document.path == p))
            .unwrap_or(0);
        self.scroll = 0;
    }
    pub fn select_path(&mut self, path: &str) {
        if let Some(n) = self.items().iter().position(|e| e.document.path == path) {
            self.selected = n;
            self.scroll = 0;
        }
    }
    fn items(&self) -> Vec<&Entry> {
        self.snapshot
            .entries
            .iter()
            .filter(|e| e.document.kind == KINDS[self.tab])
            .collect()
    }
    fn current(&self) -> Option<&Entry> {
        self.items().get(self.selected).copied()
    }
    fn switch(&mut self, tab: usize) {
        self.tab = tab;
        self.selected = 0;
        self.scroll = 0;
        self.list = ListState::default();
        self.detail_focus = false;
    }
    fn select(&mut self, down: bool) {
        self.selected = if down {
            (self.selected + 1).min(self.items().len().saturating_sub(1))
        } else {
            self.selected.saturating_sub(1)
        };
        self.scroll = 0;
    }
    fn action(&mut self, key: char) -> Command {
        match key {
            'n' => Command::New(KINDS[self.tab].into()),
            'r' => Command::Delivery,
            'x' => Command::CloseDelivery,
            'e' => self
                .current()
                .map(|e| Command::Edit(e.document.path.clone()))
                .unwrap_or(Command::None),
            'o' => self
                .current()
                .map(|e| Command::Read(e.document.path.clone()))
                .unwrap_or(Command::None),
            _ => Command::None,
        }
    }
    fn details(&self) -> String {
        if let Some(problem) = &self.snapshot.problem {
            return format!("# Unavailable\n\n{problem}\n\nR refreshes from the current files.");
        }
        let Some(e) = self.current() else {
            return "# No records\n\nN creates a draft in the project language. Unknown values stay blank. No project decisions or outcomes are supplied automatically.".into();
        };
        let d = &e.document;
        let check = e.problem.as_deref().unwrap_or(
            "No structural errors found for the recorded state; semantic review is still required",
        );
        let mut text = format!(
            "# {}\n\n{}\n\n## Current checks\n\n{check}\n\n## Next gate\n\n{}\n\n## Recorded fields\n\n",
            d.title, d.path, e.next_gate
        );
        for key in ["workflow_status", "owner"].into_iter().chain(
            crate::delivery::fields(&d.kind)
                .into_iter()
                .map(|(k, _)| k)
                .filter(|k| *k != "workflow_status"),
        ) {
            let value = match &d.metadata[key] {
                serde_json::Value::Null => "not supplied".into(),
                serde_json::Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            text.push_str(&format!("- **{key}**: {value}\n"));
        }
        text.push_str(&format!("\n## Document\n\n{}", d.body));
        text
    }
    pub fn event(&mut self, event: Event) -> Command {
        match event {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('q') {
                    return Command::Quit;
                }
                match k.code {
                    KeyCode::F(6) => self.icons = !self.icons,
                    KeyCode::Char(c @ '1'..='4')
                        if k.modifiers.is_empty() || k.modifiers == KeyModifiers::ALT =>
                    {
                        self.switch(c as usize - '1' as usize)
                    }
                    KeyCode::Tab | KeyCode::BackTab => self.detail_focus = !self.detail_focus,
                    KeyCode::Left => self.switch((self.tab + 3) % 4),
                    KeyCode::Right => self.switch((self.tab + 1) % 4),
                    KeyCode::Up if !self.detail_focus => self.select(false),
                    KeyCode::Down if !self.detail_focus => self.select(true),
                    KeyCode::Up => self.scroll = self.scroll.saturating_sub(1),
                    KeyCode::Down => self.scroll = (self.scroll + 1).min(self.max_scroll),
                    KeyCode::PageUp => {
                        self.scroll = self.scroll.saturating_sub(self.detail_area.height as usize)
                    }
                    KeyCode::PageDown => {
                        self.scroll =
                            (self.scroll + self.detail_area.height as usize).min(self.max_scroll)
                    }
                    KeyCode::Home => self.scroll = 0,
                    KeyCode::End => self.scroll = self.max_scroll,
                    KeyCode::Enter => self.detail_focus = true,
                    KeyCode::Esc if self.detail_focus => self.detail_focus = false,
                    KeyCode::Esc => return Command::CloseDelivery,
                    KeyCode::Char(c) if k.modifiers.is_empty() => return self.action(c),
                    _ => {}
                }
            }
            Event::Mouse(m) => {
                let p = Position::new(m.column, m.row);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(n) = self.tabs.iter().position(|a| a.contains(p)) {
                            self.switch(n);
                        } else if let Some((_, c)) =
                            self.actions.iter().find(|(a, _)| a.contains(p))
                        {
                            return self.action(*c);
                        } else if self.bar.contains(p) {
                            self.scroll = self.max_scroll
                                * m.row.saturating_sub(self.bar.y) as usize
                                / self.bar.height.saturating_sub(1).max(1) as usize;
                            self.detail_focus = true;
                        } else if self.list_area.contains(p) {
                            self.selected = (self.list.offset()
                                + m.row.saturating_sub(self.list_area.y) as usize)
                                .min(self.items().len().saturating_sub(1));
                            self.scroll = 0;
                            self.detail_focus = false;
                        } else if self.detail_area.contains(p) {
                            self.detail_focus = true;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) if self.list_area.contains(p) => {
                        self.selected = (self.list.offset()
                            + m.row.saturating_sub(self.list_area.y) as usize)
                            .min(self.items().len().saturating_sub(1));
                        return self.action('e');
                    }
                    MouseEventKind::Drag(MouseButton::Left) if self.bar.contains(p) => {
                        self.scroll = self.max_scroll * m.row.saturating_sub(self.bar.y) as usize
                            / self.bar.height.saturating_sub(1).max(1) as usize;
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let down = m.kind == MouseEventKind::ScrollDown;
                        if self.list_area.contains(p) {
                            self.select(down);
                            self.detail_focus = false;
                        } else if self.detail_area.contains(p) || self.bar.contains(p) {
                            self.detail_focus = true;
                            self.scroll = if down {
                                (self.scroll + 3).min(self.max_scroll)
                            } else {
                                self.scroll.saturating_sub(3)
                            };
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Command::None
    }
    pub fn draw(&mut self, f: &mut ratatui::Frame) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(2),
                Constraint::Min(0),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(f.area());
        self.tabs = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 4); 4])
            .split(rows[0])
            .to_vec();
        let labels = if self.icons {
            ["🔁 1 Changes", "☑️ 2 UAT", "🚀 3 Releases", "📈 4 Benefits"]
        } else {
            ["1 Changes", "2 UAT", "3 Releases", "4 Benefits"]
        };
        for (i, label) in labels.into_iter().enumerate() {
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(if i == self.tab {
                    Color::Cyan
                } else {
                    Color::Gray
                })),
                self.tabs[i],
            );
        }
        f.render_widget(Paragraph::new(format!("Delivery · loaded {}\nRecorded outcomes need human evidence; checks do not approve or deploy.", self.snapshot.loaded_at)), rows[1]);
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(rows[2]);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Records · right-click: edit")
            .border_style(Style::default().fg(if self.detail_focus {
                Color::Gray
            } else {
                Color::Cyan
            }));
        self.list_area = block.inner(panes[0]);
        f.render_widget(block, panes[0]);
        let items = self
            .items()
            .iter()
            .map(|e| {
                ListItem::new(format!(
                    "{}{} · {}",
                    if self.icons {
                        ["🔁 ", "☑️ ", "🚀 ", "📈 "][self.tab]
                    } else {
                        ""
                    },
                    e.document.id.as_deref().unwrap_or("Missing ID"),
                    e.document.metadata["workflow_status"]
                        .as_str()
                        .unwrap_or("unknown")
                ))
            })
            .collect::<Vec<_>>();
        self.list.select(Some(self.selected));
        f.render_stateful_widget(
            List::new(items).highlight_style(Style::default().bg(Color::DarkGray)),
            self.list_area,
            &mut self.list,
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Evidence and missing inputs")
            .border_style(Style::default().fg(if self.detail_focus {
                Color::Cyan
            } else {
                Color::Gray
            }));
        self.detail_area = block.inner(panes[1]);
        f.render_widget(block, panes[1]);
        self.bar = Rect::new(
            self.detail_area.right().saturating_sub(1),
            self.detail_area.y,
            self.detail_area.width.min(1),
            self.detail_area.height,
        );
        self.detail_area.width = self.detail_area.width.saturating_sub(1);
        let rendered = markdown::render(&self.details(), self.detail_area.width as usize);
        self.max_scroll = rendered
            .lines
            .len()
            .saturating_sub(self.detail_area.height as usize);
        self.scroll = self.scroll.min(self.max_scroll);
        f.render_widget(
            Paragraph::new(
                rendered
                    .lines
                    .into_iter()
                    .skip(self.scroll)
                    .take(self.detail_area.height as usize)
                    .collect::<Vec<_>>(),
            ),
            self.detail_area,
        );
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            self.bar,
            &mut ScrollbarState::new(self.max_scroll + 1)
                .viewport_content_length(self.detail_area.height as usize)
                .position(self.scroll),
        );
        f.render_widget(
            Paragraph::new(self.message.as_str())
                .style(Style::default().fg(Color::Yellow))
                .wrap(ratatui::widgets::Wrap { trim: false }),
            rows[3],
        );
        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 5); 5])
            .split(rows[4]);
        self.actions.clear();
        for (i, (c, label)) in [
            ('n', "N New"),
            ('e', "E Edit"),
            ('o', "O Document"),
            ('r', "R Refresh"),
            ('x', "Esc Back"),
        ]
        .into_iter()
        .enumerate()
        {
            self.actions.push((buttons[i], c));
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
                buttons[i],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseEvent};
    use serde_json::json;
    fn snapshot() -> Snapshot {
        Snapshot {
            loaded_at: "synthetic".into(),
            problem: None,
            entries: vec![Entry {
                document: crate::workspace::Document {
                    path: "07_Decisions/Changes/CHANGE-TEST.md".into(),
                    title: "Synthetic change".into(),
                    kind: "change".into(),
                    id: Some("CHANGE-TEST".into()),
                    hash: "synthetic".into(),
                    metadata: json!({"workflow_status":"draft","owner":null}),
                    body: "# Synthetic fixture\n\nOnly test data.\n\n".repeat(60),
                },
                problem: None,
                next_gate: "accepted: supply owner".into(),
            }],
        }
    }
    fn key(d: &mut Delivery, code: KeyCode) -> Command {
        d.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }
    fn draw(d: &mut Delivery, w: u16, h: u16) {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| d.draw(f)).unwrap();
    }
    fn mouse(d: &mut Delivery, kind: MouseEventKind, p: Rect) -> Command {
        d.event(Event::Mouse(MouseEvent {
            kind,
            column: p.x,
            row: p.y,
            modifiers: KeyModifiers::NONE,
        }))
    }
    #[test]
    fn delivery_tabs_and_actions_only_emit_reviewable_commands() {
        let mut d = Delivery::new(snapshot());
        assert!(d.details().contains("not supplied"));
        assert_eq!(
            key(&mut d, KeyCode::Char('e')),
            Command::Edit("07_Decisions/Changes/CHANGE-TEST.md".into())
        );
        for (n, kind) in KINDS.iter().enumerate() {
            d.event(Event::Key(KeyEvent::new(
                KeyCode::Char(char::from(b'1' + n as u8)),
                KeyModifiers::ALT,
            )));
            assert_eq!(
                key(&mut d, KeyCode::Char('n')),
                Command::New((*kind).into())
            );
        }
        key(&mut d, KeyCode::Enter);
        assert!(d.detail_focus);
        assert_eq!(key(&mut d, KeyCode::Esc), Command::None);
        assert!(!d.detail_focus);
        assert_eq!(key(&mut d, KeyCode::Esc), Command::CloseDelivery);
        assert_eq!(key(&mut d, KeyCode::Char('r')), Command::Delivery);
    }
    #[test]
    fn delivery_mouse_scroll_focus_and_refresh_never_reuse_stale_records() {
        let mut d = Delivery::new(snapshot());
        draw(&mut d, 100, 25);
        let a = d.detail_area;
        mouse(&mut d, MouseEventKind::ScrollDown, a);
        assert!(d.scroll > 0 && d.detail_focus);
        let a = d.list_area;
        assert_eq!(
            mouse(&mut d, MouseEventKind::Down(MouseButton::Right), a),
            Command::Edit("07_Decisions/Changes/CHANGE-TEST.md".into())
        );
        let a = d.tabs[1];
        mouse(&mut d, MouseEventKind::Down(MouseButton::Left), a);
        assert_eq!(d.tab, 1);
        assert!(d.details().contains("No records"));
        d.replace(Snapshot {
            entries: vec![],
            loaded_at: "new".into(),
            problem: Some("Unreadable current record".into()),
        });
        assert!(d.details().contains("Unreadable"));
        assert!(d.items().is_empty());
        for (w, h) in [(1, 1), (20, 6), (80, 24)] {
            draw(&mut d, w, h);
        }
    }
}
