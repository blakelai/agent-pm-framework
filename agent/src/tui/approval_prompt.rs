//! A decision composer. The normal text/image draft is left untouched.
use super::{markdown, tool_display, transcript};
use crate::{
    approval::{self, Decision},
    provider::Call,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use serde_json::Value;

const OPTIONS: [(Decision, &str); 4] = [
    (Decision::Once, "1. Allow once"),
    (Decision::Conversation, "2. Allow for this conversation"),
    (Decision::Always, "3. Always allow (global settings)"),
    (Decision::Deny, "4. Deny and stop"),
];
#[derive(Debug, PartialEq)]
pub struct Selection {
    pub session_id: String,
    pub call: Call,
    pub decision: Decision,
}
pub(super) struct Prompt {
    pub session_id: String,
    pub call: Call,
    pub area: Rect,
    selected: usize,
    offset: usize,
    choices: Rect,
}
impl Prompt {
    pub fn new(session_id: String, call: Call) -> Self {
        Self {
            session_id,
            call,
            area: Rect::default(),
            selected: 0,
            offset: 0,
            choices: Rect::default(),
        }
    }
    pub fn key(&mut self, key: KeyEvent) -> Option<Decision> {
        if key.code == KeyCode::Esc {
            return Some(Decision::Deny);
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return None;
        }
        match key.code {
            KeyCode::Up | KeyCode::BackTab => self.selected = (self.selected + 3) % 4,
            KeyCode::Down | KeyCode::Tab => self.selected = (self.selected + 1) % 4,
            KeyCode::Char(c @ '1'..='4') => self.selected = c as usize - '1' as usize,
            KeyCode::Enter => return Some(OPTIONS[self.selected].0),
            _ => {}
        }
        None
    }
    pub fn mouse(&mut self, mouse: MouseEvent) -> Option<Decision> {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.selected = self.selected.saturating_sub(1),
            MouseEventKind::ScrollDown => self.selected = (self.selected + 1).min(3),
            MouseEventKind::Down(MouseButton::Left)
                if self.choices.contains((mouse.column, mouse.row).into()) =>
            {
                let index = self.offset + mouse.row.saturating_sub(self.choices.y) as usize;
                if index < 4 {
                    self.selected = index;
                    return Some(OPTIONS[index].0);
                }
            }
            _ => {}
        }
        None
    }
    pub fn height(&self, width: u16) -> u16 {
        self.description(width.saturating_sub(2) as usize).len() as u16 + 7
    }
    fn description(&self, width: usize) -> Vec<Line<'static>> {
        let args: Value = serde_json::from_str(&self.call.arguments).unwrap_or_default();
        let content = if self.call.name == "pm" {
            let args = args["args"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ");
            format!("{} {args}", approval::scope(&self.call))
        } else {
            tool_display::describe(&args)
        };
        let mut description = vec![Line::raw(transcript::ellipsize(
            &tool_display::action(&self.call),
            width,
        ))];
        for (prefix, text, limit) in [
            ("Why", approval::reason(&self.call).to_owned(), 2),
            ("Content", content, 2),
            (
                "Future use",
                format!(
                    "Any future arguments for {}; options 2–3.",
                    approval::scope(&self.call)
                ),
                2,
            ),
        ] {
            let clean = transcript::one_line(&text.replace('\n', " · "), 2000);
            let mut lines = markdown::wrap(vec![Line::raw(format!("{prefix}: {clean}"))], width);
            if lines.len() > limit {
                lines.truncate(limit);
                if let Some(last) = lines.last_mut() {
                    *last = Line::raw(format!(
                        "{}…",
                        transcript::one_line(&last.to_string(), width.saturating_sub(1))
                    ));
                }
            }
            description.extend(lines);
        }
        description
    }
    pub fn draw(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        self.area = area;
        let style = Style::default().bg(Color::DarkGray).fg(Color::White);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Tool approval ")
            .style(style);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let width = inner.width as usize;
        let description = self.description(width);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length((description.len() as u16).min(inner.height.saturating_sub(5))),
                Constraint::Min(0),
                Constraint::Length(inner.height.min(1)),
            ])
            .split(inner);
        frame.render_widget(Paragraph::new(description).style(style), rows[0]);
        let mut state = ListState::default().with_selected(Some(self.selected));
        *state.offset_mut() = self.offset;
        frame.render_stateful_widget(
            List::new(OPTIONS.iter().map(|(_, label)| {
                ListItem::new(transcript::ellipsize(label, width.saturating_sub(2)))
            }))
            .style(style)
            .highlight_style(style.fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .highlight_symbol("› "),
            rows[1],
            &mut state,
        );
        self.offset = state.offset();
        self.choices = rows[1];
        frame.render_widget(
            Paragraph::new(transcript::ellipsize(
                "↑↓ / 1–4 select · Enter confirm · Esc deny",
                width,
            ))
            .style(style),
            rows[2],
        );
    }
}
