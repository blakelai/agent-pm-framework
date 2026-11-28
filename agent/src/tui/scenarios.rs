use super::{markdown, view::Command};
use crate::scenarios::{Comparison, Review};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

pub(super) struct Browser {
    ids: Vec<String>,
    comparison: Option<Comparison>,
    preview: Option<Review>,
    selected: usize,
    row_state: ListState,
    list: Rect,
    scroll: usize,
    max_scroll: usize,
    page: usize,
    pub message: String,
}

impl Browser {
    pub fn new(ids: Vec<String>) -> Self {
        Self {
            ids,
            comparison: None,
            preview: None,
            selected: 0,
            row_state: ListState::default(),
            list: Rect::default(),
            scroll: 0,
            max_scroll: 0,
            page: 1,
            message: String::new(),
        }
    }

    pub fn load(&mut self, comparison: Comparison) {
        self.comparison = Some(comparison);
        self.preview = None;
        self.selected = 0;
        self.scroll = 0;
        self.row_state = ListState::default();
        self.message.clear();
    }

    pub fn review(&mut self, review: Review) {
        self.preview = Some(review);
        self.scroll = 0;
        self.message = "Ctrl+S applies these reviewed sprint assignments".into();
    }

    pub fn invalidate(&mut self) {
        if self.preview.take().is_some() {
            self.message =
                "Planning or validation files changed. Open a fresh review before applying".into();
        }
    }

    pub fn result(&mut self, message: String) {
        self.preview = None;
        self.message = message;
    }

    fn rows(&self) -> Vec<&str> {
        match &self.comparison {
            Some(comparison) => comparison.scenarios.keys().map(String::as_str).collect(),
            None => self.ids.iter().map(String::as_str).collect(),
        }
    }

    fn select(&mut self, index: usize) {
        self.selected = index.min(self.rows().len().saturating_sub(1));
        self.preview = None;
        self.scroll = 0;
        self.message.clear();
    }

    pub fn event(&mut self, event: Event) -> Command {
        match event {
            Event::Key(k) if k.kind == KeyEventKind::Press => match (k.code, k.modifiers) {
                (KeyCode::Char('q'), KeyModifiers::CONTROL) => return Command::Quit,
                (KeyCode::Esc, _) => {
                    if self.preview.take().is_some() {
                        self.scroll = 0;
                    } else if self.comparison.take().is_some() {
                        self.select(0);
                        self.row_state = ListState::default();
                    } else {
                        return Command::CloseScenarios;
                    }
                }
                (KeyCode::Up, _) if self.preview.is_none() => {
                    self.select(self.selected.saturating_sub(1))
                }
                (KeyCode::Down, _) if self.preview.is_none() => {
                    self.select(self.selected.saturating_add(1))
                }
                (KeyCode::PageUp, _) => self.scroll = self.scroll.saturating_sub(self.page),
                (KeyCode::PageDown, _) => {
                    self.scroll = (self.scroll + self.page).min(self.max_scroll)
                }
                (KeyCode::Char('r'), _) if self.comparison.is_none() => {
                    return Command::OpenScenarios;
                }
                (KeyCode::Enter, _) if self.preview.is_none() => {
                    if let Some(id) = self.rows().get(self.selected) {
                        return match &self.comparison {
                            None => Command::LoadScenario((*id).into()),
                            Some(comparison) => Command::ReviewScenario {
                                comparison: comparison.id.clone(),
                                scenario: (*id).into(),
                            },
                        };
                    }
                }
                (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                    if let (Some(comparison), Some(review), Some(branch)) = (
                        &self.comparison,
                        &self.preview,
                        self.rows().get(self.selected),
                    ) {
                        return Command::ApplyScenario {
                            comparison: comparison.id.clone(),
                            scenario: (*branch).into(),
                            review: review.review_sha256.clone(),
                        };
                    }
                }
                _ => {}
            },
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left)
                    if self.preview.is_none()
                        && self.list.contains(Position::new(m.column, m.row)) =>
                {
                    self.select(self.row_state.offset() + (m.row - self.list.y) as usize);
                }
                MouseEventKind::ScrollUp => self.scroll = self.scroll.saturating_sub(3),
                MouseEventKind::ScrollDown => self.scroll = (self.scroll + 3).min(self.max_scroll),
                _ => {}
            },
            _ => {}
        }
        Command::None
    }

    pub fn draw(&mut self, f: &mut ratatui::Frame) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(3),
            ])
            .split(f.area());
        f.render_widget(
            Paragraph::new("Planning scenarios · Simulation")
                .style(Style::default().fg(Color::Cyan)),
            layout[0],
        );
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Min(0)])
            .split(layout[1]);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(if self.comparison.is_some() {
                "Alternatives"
            } else {
                "Saved comparisons"
            });
        self.list = block.inner(columns[0]);
        let items = self
            .rows()
            .into_iter()
            .map(|s| ListItem::new(s.to_owned()))
            .collect::<Vec<_>>();
        self.row_state
            .select((!items.is_empty()).then_some(self.selected));
        f.render_stateful_widget(
            List::new(items)
                .block(block)
                .highlight_style(Style::default().fg(Color::Cyan)),
            columns[0],
            &mut self.row_state,
        );
        let detail = match (&self.preview, &self.comparison, self.rows().get(self.selected)) {
            (Some(preview), _, _) => preview.describe(),
            (None, Some(comparison), Some(selected)) => comparison.describe(selected).unwrap_or_else(|e| format!("Scenario unavailable. {e:#}")),
            _ => "Select a comparison with Enter. Create alternatives with scenario compare SPEC --id ID.".into(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Simulation and application review");
        let area = block.inner(columns[1]);
        f.render_widget(block, columns[1]);
        let clip = detail.floor_char_boundary(detail.len().min(200_000));
        let mut lines = markdown::render(&detail[..clip], area.width as usize).lines;
        if clip < detail.len() {
            lines.extend(
                markdown::render(
                    "Display limited. Use scenario show or scenario review for complete data.",
                    area.width as usize,
                )
                .lines,
            );
        }
        self.page = area.height.max(1) as usize;
        self.max_scroll = lines.len().saturating_sub(self.page);
        self.scroll = self.scroll.min(self.max_scroll);
        f.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(self.scroll)
                    .take(self.page)
                    .collect::<Vec<_>>(),
            ),
            area,
        );
        let controls = if self.preview.is_some() {
            "Ctrl+S Apply · PgUp/PgDn Scroll · Esc Back"
        } else {
            "↑/↓ Select · Enter Open or review · PgUp/PgDn Scroll · Esc Back"
        };
        f.render_widget(
            Paragraph::new(format!("{controls}\n{}", self.message)).wrap(Wrap { trim: true }),
            layout[2],
        );
    }
}
