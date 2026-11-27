//! Read-only question scopes; answering remains an explicit Chat action.
use super::{markdown, view::Command};
use crate::clarification::Snapshot;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};

pub struct Questions {
    snapshot: Snapshot,
    selected: usize,
    scroll: usize,
    max_scroll: usize,
    detail_focus: bool,
    icons: bool,
    list: ListState,
    list_area: Rect,
    detail_area: Rect,
    bar: Rect,
    actions: Vec<(Rect, char)>,
}
impl Questions {
    pub fn new(snapshot: Snapshot) -> Self {
        Self {
            snapshot,
            selected: 0,
            scroll: 0,
            max_scroll: 0,
            detail_focus: false,
            icons: true,
            list: ListState::default(),
            list_area: Rect::default(),
            detail_area: Rect::default(),
            bar: Rect::default(),
            actions: vec![],
        }
    }
    pub fn replace(&mut self, snapshot: Snapshot) {
        let path = self
            .snapshot
            .entries
            .get(self.selected)
            .map(|e| e.document.path.clone());
        let old_path = path.clone();
        self.selected = path
            .and_then(|p| snapshot.entries.iter().position(|e| e.document.path == p))
            .unwrap_or(0);
        self.snapshot = snapshot;
        if old_path
            != self
                .snapshot
                .entries
                .get(self.selected)
                .map(|e| e.document.path.clone())
        {
            self.scroll = 0;
        }
    }
    fn select(&mut self, down: bool) {
        self.selected = if down {
            (self.selected + 1).min(self.snapshot.entries.len().saturating_sub(1))
        } else {
            self.selected.saturating_sub(1)
        };
        self.scroll = 0;
    }
    fn action(&self, key: char) -> Command {
        match key {
            'r' => Command::Questions,
            'x' => Command::CloseQuestions,
            'o' => self
                .snapshot
                .entries
                .get(self.selected)
                .map(|e| Command::Read(e.document.path.clone()))
                .unwrap_or(Command::None),
            _ => Command::None,
        }
    }
    fn details(&self) -> String {
        if let Some(p) = &self.snapshot.problem {
            return format!("# Questions unavailable\n\n{p}\n\nR retries from current files.");
        }
        let Some(e) = self.snapshot.entries.get(self.selected) else {
            return "# No recorded questions\n\nThis does not establish that all requirements are complete.".into();
        };
        let d = &e.document;
        let mut text = format!("# {}\n\n{}\n\n## Scope and state\n\n", d.title, d.path);
        for key in [
            "workflow_status",
            "blocking_scope",
            "affected_ids",
            "affected_paths",
            "owner",
            "due_date",
            "session_id",
        ] {
            text.push_str(&format!(
                "- {key}: {}\n",
                d.metadata
                    .get(key)
                    .map(ToString::to_string)
                    .unwrap_or("not supplied".into())
            ));
        }
        if e.legacy {
            text.push_str("\nLegacy question: scope was not recorded. Its originating session retains the original draft block. No project-wide scope is inferred.\n");
        }
        if let Some(p) = &e.problem {
            text.push_str(&format!("\n## Unavailable checks\n\n{p}\n\nDependent writes remain blocked. Repair the contract or provide a fresh response.\n"));
        }
        text.push_str("\n## Affected documents\n\n");
        for path in &e.blocked {
            text.push_str(&format!("- {path}\n"));
        }
        if e.blocked.is_empty() {
            text.push_str("No affected paths available; check state and errors above.\n");
        }
        text.push_str(&format!(
            "\n## Recorded question\n\n{}\n\n## Actual response\n\n{}\n",
            d.body,
            e.answer
                .as_deref()
                .unwrap_or("No current, verified response source is available.")
        ));
        if let Some(r) = d.metadata["resolution"].as_str() {
            text.push_str(&format!("\n## Recorded resolution\n\n{r}\n"));
        }
        let id = d.id.as_deref().unwrap_or("QUESTION-ID");
        text.push_str(&format!("\n## Next action\n\nReturn to Chat, resume the originating session if needed, then use `/answer {id} TEXT`. Receipt alone does not resolve a question. `/continue-independent` resumes supported independent work without supplying an answer. Semantic sufficiency still needs review.\n"));
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
                    KeyCode::Tab | KeyCode::BackTab => self.detail_focus = !self.detail_focus,
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
                    KeyCode::Esc => return Command::CloseQuestions,
                    KeyCode::Char(c) if k.modifiers.is_empty() => return self.action(c),
                    _ => {}
                }
            }
            Event::Mouse(m) => {
                let p = Position::new(m.column, m.row);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some((_, c)) = self.actions.iter().find(|(a, _)| a.contains(p)) {
                            return self.action(*c);
                        }
                        if self.bar.contains(p) {
                            self.drag(m.row);
                        } else if self.list_area.contains(p) {
                            self.selected = (self.list.offset()
                                + m.row.saturating_sub(self.list_area.y) as usize)
                                .min(self.snapshot.entries.len().saturating_sub(1));
                            self.scroll = 0;
                            self.detail_focus = false;
                        } else if self.detail_area.contains(p) {
                            self.detail_focus = true;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) if self.list_area.contains(p) => {
                        self.selected = (self.list.offset()
                            + m.row.saturating_sub(self.list_area.y) as usize)
                            .min(self.snapshot.entries.len().saturating_sub(1));
                        return self.action('o');
                    }
                    MouseEventKind::Drag(MouseButton::Left) if self.bar.contains(p) => {
                        self.drag(m.row)
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
    fn drag(&mut self, row: u16) {
        self.scroll = self.max_scroll * row.saturating_sub(self.bar.y) as usize
            / self.bar.height.saturating_sub(1).max(1) as usize;
        self.detail_focus = true;
    }
    pub fn draw(&mut self, f: &mut ratatui::Frame) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(f.area());
        f.render_widget(Paragraph::new(format!("Questions · loaded {}\nExplicit scope and recorded dependencies; missing information stays open.",self.snapshot.loaded_at)),rows[0]);
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(rows[1]);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Questions")
            .border_style(Style::default().fg(if self.detail_focus {
                Color::Gray
            } else {
                Color::Cyan
            }));
        self.list_area = block.inner(panes[0]);
        f.render_widget(block, panes[0]);
        let items = self
            .snapshot
            .entries
            .iter()
            .map(|e| {
                ListItem::new(format!(
                    "{}{} · {}",
                    if self.icons { "❓ " } else { "" },
                    e.document.title,
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
            .title("Scope, evidence and response")
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
                .position(self.scroll)
                .viewport_content_length(self.detail_area.height as usize),
        );
        self.actions.clear();
        let mut x = rows[2].x;
        for (label, key) in [
            (" O Open note ", 'o'),
            (" R Refresh ", 'r'),
            (" X Back ", 'x'),
        ] {
            let area = Rect::new(
                x,
                rows[2].y,
                (label.len() as u16).min(rows[2].right().saturating_sub(x)),
                rows[2].height,
            );
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
                area,
            );
            self.actions.push((area, key));
            x = area.right();
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
            entries: vec![crate::clarification::Entry {
                document: crate::workspace::Document {
                    path: "07_Decisions/QUESTION-TEST.md".into(),
                    title: "Synthetic missing rule".into(),
                    kind: "question".into(),
                    id: Some("QUESTION-TEST".into()),
                    hash: "synthetic".into(),
                    metadata: json!({"workflow_status":"open","blocking_scope":"records","affected_ids":["REQ-TEST"],"owner":null}),
                    body: "# Known evidence\n\nMissing input remains unknown.\n\n".repeat(40),
                },
                legacy: false,
                blocked: vec!["04_Requirements/REQ-TEST.md".into()],
                problem: None,
                answer: None,
            }],
        }
    }
    fn key(q: &mut Questions, code: KeyCode) -> Command {
        q.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }
    #[test]
    fn question_navigation_renders_missing_inputs_and_never_resolves_from_browsing() {
        let mut q = Questions::new(snapshot());
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 24)).unwrap();
        t.draw(|f| q.draw(f)).unwrap();
        assert!(q.details().contains("No current, verified response"));
        key(&mut q, KeyCode::Enter);
        key(&mut q, KeyCode::End);
        assert!(q.scroll > 0);
        assert!(q.detail_focus);
        assert_eq!(key(&mut q, KeyCode::Esc), Command::None);
        assert!(!q.detail_focus);
        assert_eq!(key(&mut q, KeyCode::Char('r')), Command::Questions);
        let area = q.list_area;
        assert_eq!(
            q.event(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                column: area.x,
                row: area.y,
                modifiers: KeyModifiers::NONE
            })),
            Command::Read("07_Decisions/QUESTION-TEST.md".into())
        );
        let old = q.scroll;
        q.replace(snapshot());
        assert_eq!(q.scroll, old);
        q.replace(Snapshot {
            loaded_at: "changed".into(),
            entries: vec![],
            problem: Some("Unreadable fixture".into()),
        });
        assert_eq!(q.scroll, 0);
        assert!(q.details().contains("Unreadable fixture"));
        assert_eq!(key(&mut q, KeyCode::Char('o')), Command::None);
        for (width, height) in [(1, 1), (20, 5), (90, 24)] {
            let mut t =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            t.draw(|f| q.draw(f)).unwrap();
        }
        assert_eq!(key(&mut q, KeyCode::Esc), Command::CloseQuestions);
    }
}
