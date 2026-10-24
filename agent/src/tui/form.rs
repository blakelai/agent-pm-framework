use super::editor::Editor;
use crate::editing::{Edit, FieldType, Preview, Target};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

#[derive(Debug, PartialEq)]
pub enum Action {
    None,
    Review,
    Save,
    Close,
}

pub struct Form {
    pub edit: Edit,
    pub preview: Option<Preview>,
    pub message: String,
    selected: usize,
    input: Editor,
    editing: bool,
    dirty: bool,
    discard: bool,
    list: ListState,
    fields_area: Rect,
    content_area: Rect,
    review_button: Rect,
    discard_button: Rect,
    offset: usize,
    review_scroll: usize,
}
impl Form {
    pub fn proposed(edit: Edit) -> Self {
        let mut form = Self::new(edit);
        form.dirty = true;
        form
    }
    pub fn new(edit: Edit) -> Self {
        let mut form = Self {
            edit,
            preview: None,
            message: String::new(),
            selected: 0,
            input: Editor::default(),
            editing: false,
            dirty: false,
            discard: false,
            list: ListState::default(),
            fields_area: Rect::default(),
            content_area: Rect::default(),
            review_button: Rect::default(),
            discard_button: Rect::default(),
            offset: 0,
            review_scroll: 0,
        };
        form.load();
        form
    }
    fn load(&mut self) {
        let entries = self.edit.entries();
        self.selected = self.selected.min(entries.len().saturating_sub(1));
        self.input.clear();
        if let Some(e) = entries.get(self.selected) {
            self.input.insert(&e.value);
        }
        self.offset = 0;
    }
    fn apply(&mut self) -> bool {
        if !self.editing {
            return true;
        }
        let entries = self.edit.entries();
        let Some(entry) = entries.get(self.selected) else {
            return true;
        };
        match self.edit.apply(&entry.target, &self.input.text) {
            Ok(()) => {
                self.dirty |= entry.value != self.input.text;
                self.editing = false;
                self.message.clear();
                true
            }
            Err(e) => {
                self.message = e.to_string();
                false
            }
        }
    }
    fn select(&mut self, n: usize) {
        if self.apply() {
            self.selected = n;
            self.load();
        }
    }
    fn activate(&mut self) {
        let entries = self.edit.entries();
        let Some(e) = entries.get(self.selected) else {
            return;
        };
        match e.target {
            Target::AddRow(_) | Target::RemoveRow(_, _) => match self.edit.apply(&e.target, "") {
                Ok(()) => {
                    self.dirty = true;
                    self.load();
                }
                Err(e) => self.message = e.to_string(),
            },
            _ => self.editing = true,
        }
    }
    fn review(&mut self) -> Action {
        if self.preview.is_some() {
            return Action::Save;
        }
        if self.apply() {
            self.review_scroll = 0;
            Action::Review
        } else {
            Action::None
        }
    }
    fn leave(&mut self) -> Action {
        let pending = self.editing
            && self
                .edit
                .entries()
                .get(self.selected)
                .is_some_and(|e| e.value != self.input.text);
        if self.dirty || pending {
            self.discard = true;
            Action::None
        } else {
            Action::Close
        }
    }
    pub fn event(&mut self, event: Event) -> Action {
        if self.discard {
            if let Event::Key(k) = event
                && k.kind == KeyEventKind::Press
            {
                match k.code {
                    KeyCode::Char('y') => return Action::Close,
                    KeyCode::Esc | KeyCode::Char('n') => self.discard = false,
                    _ => {}
                }
            }
            return Action::None;
        }
        match event {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                if k.modifiers.contains(KeyModifiers::CONTROL) {
                    match k.code {
                        KeyCode::Char('s') => return self.review(),
                        KeyCode::Char('d') | KeyCode::Char('q') => return self.leave(),
                        KeyCode::Char('c') if self.editing => {
                            self.input.clear();
                            return Action::None;
                        }
                        _ => {}
                    }
                }
                if self.preview.is_some() {
                    match k.code {
                        KeyCode::Esc => {
                            self.preview = None;
                            self.message.clear();
                        }
                        KeyCode::Down | KeyCode::PageDown => {
                            self.review_scroll =
                                self.review_scroll
                                    .saturating_add(if k.code == KeyCode::Down {
                                        1
                                    } else {
                                        self.content_area.height as usize
                                    })
                        }
                        KeyCode::Up | KeyCode::PageUp => {
                            self.review_scroll =
                                self.review_scroll.saturating_sub(if k.code == KeyCode::Up {
                                    1
                                } else {
                                    self.content_area.height as usize
                                })
                        }
                        KeyCode::Home => self.review_scroll = 0,
                        _ => {}
                    }
                    return Action::None;
                }
                if k.code == KeyCode::Esc {
                    if self.editing {
                        self.editing = false;
                        self.load();
                        self.message = "Field changes reverted; other edits remain".into();
                    } else {
                        return self.leave();
                    }
                    return Action::None;
                }
                if k.code == KeyCode::Tab || k.code == KeyCode::BackTab {
                    if self.editing {
                        self.apply();
                    } else {
                        self.activate();
                    }
                    return Action::None;
                }
                if self.editing {
                    if let Some(entry) = self.edit.entries().get(self.selected) {
                        if let FieldType::Choice(values) = entry.kind
                            && matches!(k.code, KeyCode::Left | KeyCode::Right)
                        {
                            let current = values.iter().position(|s| *s == self.input.text);
                            let next = match (current, k.code) {
                                (Some(n), KeyCode::Right) => (n + 1) % values.len(),
                                (Some(n), _) => (n + values.len() - 1) % values.len(),
                                (None, KeyCode::Right) => 0,
                                (None, _) => values.len() - 1,
                            };
                            self.input.clear();
                            self.input.insert(values[next]);
                            return Action::None;
                        }
                        if k.code == KeyCode::Enter
                            && !matches!(entry.target, Target::Body)
                            && !matches!(entry.kind, FieldType::List)
                        {
                            self.apply();
                            return Action::None;
                        }
                    }
                    self.input.key(k);
                } else {
                    match k.code {
                        KeyCode::Down => self.select(self.selected + 1),
                        KeyCode::Up => self.select(self.selected.saturating_sub(1)),
                        KeyCode::PageDown => {
                            self.select(self.selected + self.fields_area.height as usize)
                        }
                        KeyCode::PageUp => self.select(
                            self.selected
                                .saturating_sub(self.fields_area.height as usize),
                        ),
                        KeyCode::Enter => self.activate(),
                        _ => {}
                    }
                }
            }
            Event::Paste(s) if self.editing && self.preview.is_none() => self.input.insert(&s),
            Event::Mouse(m) => {
                let p = Position::new(m.column, m.row);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) if self.review_button.contains(p) => {
                        return self.review();
                    }
                    MouseEventKind::Down(MouseButton::Left) if self.discard_button.contains(p) => {
                        return self.leave();
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                        if self.preview.is_some() =>
                    {
                        self.review_scroll = if m.kind == MouseEventKind::ScrollDown {
                            self.review_scroll.saturating_add(3)
                        } else {
                            self.review_scroll.saturating_sub(3)
                        };
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.preview.is_none() && self.fields_area.contains(p) =>
                    {
                        self.select(
                            self.list.offset() + m.row.saturating_sub(self.fields_area.y) as usize,
                        );
                        self.activate();
                    }
                    MouseEventKind::Down(MouseButton::Left)
                        if self.preview.is_none() && self.content_area.contains(p) =>
                    {
                        if self.edit.entries().get(self.selected).is_some_and(|e| {
                            !matches!(e.target, Target::AddRow(_) | Target::RemoveRow(_, _))
                        }) {
                            self.editing = true;
                            self.input.click(
                                self.content_area.width as usize,
                                self.offset + m.row.saturating_sub(self.content_area.y) as usize,
                                m.column.saturating_sub(self.content_area.x) as usize,
                            );
                        }
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                        if self.preview.is_none() && self.fields_area.contains(p) =>
                    {
                        self.select(if m.kind == MouseEventKind::ScrollDown {
                            self.selected + 3
                        } else {
                            self.selected.saturating_sub(3)
                        })
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Action::None
    }
    pub fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2),
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(area);
        f.render_widget(Paragraph::new(format!("{} {}\nEnter/Tab edits a field · Esc reverts current field / returns · blank values stay unknown",if self.preview.is_some(){"Review"}else{"Edit"},self.edit.path)),parts[0]);
        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(parts[3]);
        self.review_button = buttons[0];
        self.discard_button = buttons[1];
        f.render_widget(
            Paragraph::new(if self.preview.is_some() {
                "[Ctrl+S Save reviewed changes]"
            } else {
                "[Ctrl+S Review changes]"
            })
            .style(Style::default().fg(Color::Cyan)),
            buttons[0],
        );
        f.render_widget(
            Paragraph::new("[Ctrl+D Discard / close]").style(Style::default().fg(Color::Yellow)),
            buttons[1],
        );
        let message = if self.discard {
            "Discard unsaved changes? Y confirms · N / Esc keeps editing"
        } else {
            &self.message
        };
        f.render_widget(
            Paragraph::new(message)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .style(Style::default().fg(if self.discard {
                    Color::Yellow
                } else {
                    Color::Red
                })),
            parts[2],
        );
        if let Some(preview) = &self.preview {
            self.content_area = parts[1];
            let mut lines = vec![
                Line::from(format!("Destination: {}", preview.path)),
                Line::from("No data is written until Ctrl+S. Esc returns to the form."),
                Line::default(),
            ];
            for warning in &preview.warnings {
                lines.push(Line::styled(
                    format!("Note: {warning}"),
                    Style::default().fg(Color::Yellow),
                ));
            }
            lines.push(Line::default());
            if !preview.planning_changes.is_empty() {
                lines.push(Line::styled(
                    "Planning impact",
                    Style::default().fg(Color::Cyan),
                ));
                for change in &preview.planning_changes {
                    lines.extend(change.lines().map(|s| Line::from(s.to_owned())));
                }
                lines.push(Line::default());
            }
            for change in &preview.changes {
                for line in change.lines() {
                    lines.push(Line::styled(
                        line.to_string(),
                        Style::default().fg(if line.starts_with("+ ") {
                            Color::Green
                        } else if line.starts_with("- ") {
                            Color::Red
                        } else {
                            Color::White
                        }),
                    ));
                }
                lines.push(Line::default());
            }
            let text = Text::from(lines);
            let paragraph = Paragraph::new(text).wrap(ratatui::widgets::Wrap { trim: false });
            self.review_scroll = self.review_scroll.min(
                paragraph
                    .line_count(parts[1].width)
                    .saturating_sub(parts[1].height as usize),
            );
            f.render_widget(
                paragraph.scroll((self.review_scroll.min(u16::MAX as usize) as u16, 0)),
                parts[1],
            );
            return;
        }
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(parts[1]);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Fields / table rows");
        self.fields_area = block.inner(panes[0]);
        f.render_widget(block, panes[0]);
        let entries = self.edit.entries();
        self.list.select(Some(self.selected));
        let items = entries
            .iter()
            .map(|e| {
                ListItem::new(format!(
                    "{}  {}",
                    e.label,
                    e.value.lines().next().unwrap_or("")
                ))
            })
            .collect::<Vec<_>>();
        f.render_stateful_widget(
            List::new(items).highlight_style(Style::default().bg(Color::DarkGray)),
            self.fields_area,
            &mut self.list,
        );
        let selected = entries.get(self.selected);
        let hint = selected
            .map(|e| format!("{} — {}", e.label, e.kind.hint()))
            .unwrap_or_default();
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(panes[1]);
        f.render_widget(
            Paragraph::new(hint).wrap(ratatui::widgets::Wrap { trim: false }),
            right[0],
        );
        self.content_area = right[1];
        let (rows, (row, col)) = self.input.rows(self.content_area.width as usize);
        let height = self.content_area.height as usize;
        if row < self.offset {
            self.offset = row;
        }
        if row >= self.offset + height {
            self.offset = row.saturating_sub(height.saturating_sub(1));
        }
        let lines = rows
            .into_iter()
            .skip(self.offset)
            .take(height)
            .map(Line::from)
            .collect::<Vec<_>>();
        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(if self.editing {
                Color::DarkGray
            } else {
                Color::Black
            })),
            self.content_area,
        );
        if self.editing && self.content_area.width > 0 && self.content_area.height > 0 {
            f.set_cursor_position((
                self.content_area.x
                    + col.min(self.content_area.width.saturating_sub(1) as usize) as u16,
                self.content_area.y
                    + row
                        .saturating_sub(self.offset)
                        .min(height.saturating_sub(1)) as u16,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        editing::Target,
        workspace::{Workspace, atomic_write, note},
    };
    use crossterm::event::{KeyEvent, MouseEvent};
    use serde_json::json;
    fn workspace() -> (tempfile::TempDir, Workspace) {
        let d = tempfile::tempdir().unwrap();
        atomic_write(
            &d.path().join("00_Governance/Project.md"),
            note(
                json!({"id":"PROJECT-TEST","type":"project","title":"Fixture","language":"en"}),
                "# Fixture",
            )
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
        atomic_write(&d.path().join("00_Governance/Settings.md"),note(json!({"type":"project_settings","title":"Settings","document_language":"en","agent_language":"en","language":"en"}),"# Settings").unwrap().as_bytes()).unwrap();
        let w = Workspace::open(d.path()).unwrap();
        (d, w)
    }
    fn key(form: &mut Form, code: KeyCode) -> Action {
        form.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }
    fn control(form: &mut Form, c: char) -> Action {
        form.event(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::CONTROL,
        )))
    }
    #[test]
    fn fields_discard_and_paste_do_not_write_or_escape_the_form() {
        let (_d, w) = workspace();
        let mut f = Form::new(Edit::create(&w, "risk").unwrap());
        assert!(f.edit.entries()[0].value.is_empty());
        key(&mut f, KeyCode::Enter);
        f.event(Event::Paste("RISK-TEST".into()));
        key(&mut f, KeyCode::Enter);
        assert_eq!(f.edit.entries()[0].value, "RISK-TEST");
        key(&mut f, KeyCode::Enter);
        f.event(Event::Paste("-UNSAVED".into()));
        key(&mut f, KeyCode::Esc);
        assert_eq!(f.input.text, "RISK-TEST");
        assert_eq!(key(&mut f, KeyCode::Esc), Action::None);
        assert!(f.discard);
        assert_eq!(key(&mut f, KeyCode::Char('n')), Action::None);
        assert!(!f.discard);
        assert_eq!(control(&mut f, 's'), Action::Review);
        assert!(!w.path("07_Decisions/RISK-TEST.md").unwrap().exists());
        control(&mut f, 'd');
        assert_eq!(key(&mut f, KeyCode::Char('y')), Action::Close);
    }
    #[test]
    fn preview_needs_a_separate_save_and_mouse_reaches_fields() {
        let (_d, w) = workspace();
        let mut e = Edit::create(&w, "risk").unwrap();
        for (k, v) in [("id", "RISK-TEST"), ("title", "Synthetic risk")] {
            e.apply(&Target::Property(k.into()), v).unwrap();
        }
        e.apply(&Target::Body, "# Synthetic risk\n\nFixture content.")
            .unwrap();
        let mut f = Form::proposed(e);
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| f.draw(frame)).unwrap();
        f.event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: f.fields_area.x,
            row: f.fields_area.y + 1,
            modifiers: KeyModifiers::NONE,
        }));
        assert!(f.editing);
        assert_eq!(f.selected, 1);
        key(&mut f, KeyCode::Esc);
        assert_eq!(control(&mut f, 's'), Action::Review);
        f.preview = Some(f.edit.preview(&w).unwrap());
        terminal.draw(|frame| f.draw(frame)).unwrap();
        assert_eq!(key(&mut f, KeyCode::Enter), Action::None);
        assert_eq!(control(&mut f, 's'), Action::Save);
        assert!(!w.path("07_Decisions/RISK-TEST.md").unwrap().exists());
        key(&mut f, KeyCode::Esc);
        assert!(f.preview.is_none());
        control(&mut f, 'd');
        assert!(f.discard);
        key(&mut f, KeyCode::Char('n'));
        for (width, height) in [(1, 1), (20, 6), (80, 24)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| f.draw(frame)).unwrap();
        }
    }
}
