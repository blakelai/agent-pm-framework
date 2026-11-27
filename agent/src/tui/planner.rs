use super::{markdown, view::Command};
use crate::{
    schedule::{self, Snapshot},
    workspace::Document,
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Bucket {
    All,
    Unscheduled,
    Sprint(i64),
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Focus {
    Periods,
    Items,
    Detail,
}
struct Destination {
    path: String,
    hash: String,
    choices: Vec<(String, Option<i64>)>,
    state: ListState,
    area: Rect,
}

pub struct Planner {
    snapshot: Snapshot,
    buckets: Vec<(String, Bucket)>,
    period: usize,
    period_list: ListState,
    selected: usize,
    item_list: ListState,
    tab: usize,
    focus: Focus,
    scroll: usize,
    max_scroll: usize,
    picker: Option<Destination>,
    tabs: Vec<Rect>,
    actions: Vec<(Rect, char)>,
    periods_area: Rect,
    items_area: Rect,
    detail_area: Rect,
    bar: Rect,
    dragging: bool,
    pub message: String,
}
impl Planner {
    pub fn new(snapshot: Snapshot) -> Self {
        let mut s = Self {
            snapshot,
            buckets: vec![],
            period: 0,
            period_list: ListState::default(),
            selected: 0,
            item_list: ListState::default(),
            tab: 0,
            focus: Focus::Items,
            scroll: 0,
            max_scroll: 0,
            picker: None,
            tabs: vec![],
            actions: vec![],
            periods_area: Rect::default(),
            items_area: Rect::default(),
            detail_area: Rect::default(),
            bar: Rect::default(),
            dragging: false,
            message: String::new(),
        };
        s.rebuild();
        s
    }
    fn rebuild(&mut self) {
        self.buckets = vec![
            ("All PBIs".into(), Bucket::All),
            ("Unscheduled".into(), Bucket::Unscheduled),
        ];
        for p in &self.snapshot.periods {
            self.buckets.push((
                format!("Sprint {} · {} – {}", p.sprint, p.start, p.end),
                Bucket::Sprint(p.sprint),
            ));
        }
    }
    pub fn replace(&mut self, snapshot: Snapshot) {
        let bucket = self.buckets[self.period].1;
        let selected = self.selected_item().map(|d| d.path.clone());
        self.snapshot = snapshot;
        self.rebuild();
        self.period = self
            .buckets
            .iter()
            .position(|(_, b)| *b == bucket)
            .unwrap_or(0);
        self.selected = selected
            .as_ref()
            .and_then(|path| self.items().iter().position(|d| &d.path == path))
            .unwrap_or(0);
        if selected != self.selected_item().map(|d| d.path.clone()) {
            self.scroll = 0;
        }
        self.message = if self.picker.take().is_some() {
            "Project files refreshed; reopen the destination picker to review current iterations"
        } else {
            "Refreshed from project files"
        }
        .into();
    }
    fn items(&self) -> Vec<&Document> {
        self.snapshot
            .items
            .iter()
            .filter(|d| match self.buckets[self.period].1 {
                Bucket::All => true,
                Bucket::Unscheduled => d.metadata["sprint"].is_null(),
                Bucket::Sprint(s) => d.metadata["sprint"].as_i64() == Some(s),
            })
            .collect()
    }
    fn selected_item(&self) -> Option<&Document> {
        self.items().get(self.selected).copied()
    }
    fn period(&mut self, n: usize) {
        self.period = n.min(self.buckets.len().saturating_sub(1));
        self.selected = 0;
        self.scroll = 0;
    }
    fn select(&mut self, n: usize) {
        self.selected = n.min(self.items().len().saturating_sub(1));
        self.scroll = 0;
    }
    fn pick(&mut self) {
        let Some(d) = self.selected_item() else {
            return;
        };
        if matches!(
            d.metadata["delivery_status"].as_str(),
            Some("done" | "cancelled")
        ) {
            self.message = "Completed/cancelled work stays historical".into();
            return;
        }
        let mut choices = vec![("Unscheduled — remove iteration assignment".into(), None)];
        choices.extend(self.snapshot.periods.iter().map(|p| {
            (
                format!("Sprint {} · {} – {} · {}", p.sprint, p.start, p.end, p.goal),
                Some(p.sprint),
            )
        }));
        let selected = choices
            .iter()
            .position(|(_, s)| *s == d.metadata["sprint"].as_i64())
            .unwrap_or(0);
        self.picker = Some(Destination {
            path: d.path.clone(),
            hash: d.hash.clone(),
            choices,
            state: ListState::default().with_selected(Some(selected)),
            area: Rect::default(),
        });
    }
    fn choose(&mut self) -> Command {
        let Some(p) = self.picker.take() else {
            return Command::None;
        };
        let sprint = p.choices[p.state.selected().unwrap_or(0)].1;
        Command::Schedule {
            path: p.path,
            hash: p.hash,
            sprint,
        }
    }
    fn action(&mut self, c: char) -> Command {
        match c {
            'r' => return Command::Planning,
            'p' => return Command::Edit(schedule::PEOPLE.into()),
            'i' => return Command::Edit(schedule::ITERATIONS.into()),
            'a' => return Command::Edit(schedule::PLANNING.into()),
            'e' if self.tab == 0 => {
                if let Some(d) = self.selected_item() {
                    return Command::Edit(d.path.clone());
                }
            }
            'o' if self.tab == 0 => {
                if let Some(d) = self.selected_item() {
                    return Command::Read(d.path.clone());
                }
            }
            'm' if self.tab == 0 => self.pick(),
            'x' => return Command::ClosePlanning,
            _ => {}
        }
        Command::None
    }
    pub fn event(&mut self, event: Event) -> Command {
        if let Some(p) = &mut self.picker {
            match event {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Esc => self.picker = None,
                    KeyCode::Up => p
                        .state
                        .select(Some(p.state.selected().unwrap_or(0).saturating_sub(1))),
                    KeyCode::Down => p.state.select(Some(
                        (p.state.selected().unwrap_or(0) + 1).min(p.choices.len() - 1),
                    )),
                    KeyCode::Enter => return self.choose(),
                    _ => {}
                },
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => p
                        .state
                        .select(Some(p.state.selected().unwrap_or(0).saturating_sub(1))),
                    MouseEventKind::ScrollDown => p.state.select(Some(
                        (p.state.selected().unwrap_or(0) + 1).min(p.choices.len() - 1),
                    )),
                    MouseEventKind::Down(MouseButton::Left) => {
                        if p.area.contains(Position::new(m.column, m.row)) {
                            let n = p.state.offset() + m.row.saturating_sub(p.area.y) as usize;
                            if n < p.choices.len() {
                                p.state.select(Some(n));
                                return self.choose();
                            }
                        } else {
                            self.picker = None;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            return Command::None;
        }
        match event {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                if k.modifiers.contains(KeyModifiers::CONTROL) {
                    return if k.code == KeyCode::Char('q') {
                        Command::ClosePlanning
                    } else {
                        Command::None
                    };
                }
                match k.code {
                    KeyCode::Esc | KeyCode::F(5) => return Command::ClosePlanning,
                    KeyCode::Char(c @ '1'..='3') => {
                        self.tab = c as usize - '1' as usize;
                        self.scroll = 0;
                        self.focus = if self.tab == 0 {
                            Focus::Items
                        } else {
                            Focus::Detail
                        };
                    }
                    KeyCode::Left => self.period(self.period.saturating_sub(1)),
                    KeyCode::Right => self.period(self.period + 1),
                    KeyCode::Tab => {
                        self.focus = match self.focus {
                            Focus::Periods if self.tab == 0 => Focus::Items,
                            Focus::Periods | Focus::Items => Focus::Detail,
                            Focus::Detail => Focus::Periods,
                        }
                    }
                    KeyCode::Up | KeyCode::Down => {
                        let down = k.code == KeyCode::Down;
                        match self.focus {
                            Focus::Periods => self.period(if down {
                                self.period + 1
                            } else {
                                self.period.saturating_sub(1)
                            }),
                            Focus::Items => self.select(if down {
                                self.selected + 1
                            } else {
                                self.selected.saturating_sub(1)
                            }),
                            Focus::Detail => {
                                self.scroll = if down {
                                    self.scroll.saturating_add(1)
                                } else {
                                    self.scroll.saturating_sub(1)
                                }
                            }
                        }
                    }
                    KeyCode::PageDown => {
                        self.scroll = self.scroll.saturating_add(self.detail_area.height as usize)
                    }
                    KeyCode::PageUp => {
                        self.scroll = self.scroll.saturating_sub(self.detail_area.height as usize)
                    }
                    KeyCode::Home => self.scroll = 0,
                    KeyCode::End => self.scroll = self.max_scroll,
                    KeyCode::Enter if self.focus == Focus::Items => return self.action('o'),
                    KeyCode::Char(c) => return self.action(c),
                    _ => {}
                }
            }
            Event::Mouse(m) => {
                let p = Position::new(m.column, m.row);
                match m.kind {
                    MouseEventKind::Up(_) => self.dragging = false,
                    MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                        self.bar_position(m.row)
                    }
                    MouseEventKind::Down(MouseButton::Left) if self.bar.contains(p) => {
                        self.dragging = true;
                        self.bar_position(m.row);
                    }
                    MouseEventKind::Down(button) => {
                        if let Some(n) = self.tabs.iter().position(|r| r.contains(p)) {
                            self.tab = n;
                            self.scroll = 0;
                            self.focus = if n == 0 { Focus::Items } else { Focus::Detail };
                        } else if let Some((_, c)) =
                            self.actions.iter().find(|(r, _)| r.contains(p))
                        {
                            return self.action(*c);
                        } else if self.periods_area.contains(p) {
                            self.focus = Focus::Periods;
                            self.period(
                                self.period_list.offset()
                                    + m.row.saturating_sub(self.periods_area.y) as usize,
                            );
                        } else if self.items_area.contains(p) {
                            self.focus = Focus::Items;
                            self.select(
                                self.item_list.offset()
                                    + m.row.saturating_sub(self.items_area.y) as usize,
                            );
                            if button == MouseButton::Right {
                                self.pick();
                            }
                        } else if self.detail_area.contains(p) {
                            self.focus = Focus::Detail;
                        }
                    }
                    MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                        let down = m.kind == MouseEventKind::ScrollDown;
                        if self.periods_area.contains(p) {
                            self.period(if down {
                                self.period + 1
                            } else {
                                self.period.saturating_sub(1)
                            });
                        } else if self.items_area.contains(p) {
                            self.select(if down {
                                self.selected + 1
                            } else {
                                self.selected.saturating_sub(1)
                            });
                        } else if self.detail_area.contains(p) || self.bar.contains(p) {
                            self.scroll = if down {
                                self.scroll.saturating_add(3)
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
    fn bar_position(&mut self, row: u16) {
        let offset = row
            .saturating_sub(self.bar.y)
            .min(self.bar.height.saturating_sub(1)) as usize;
        self.scroll = offset.saturating_mul(self.max_scroll)
            / (self.bar.height.saturating_sub(1) as usize).max(1);
    }
    fn details(&self) -> String {
        if self.tab == 2 {
            let signals = self.snapshot.signals();
            return format!(
                "# Project-wide planning signals\n\n{}\n\nThese are deterministic checks of recorded inputs, not a risk score or delivery probability. Use project validation for readiness and acceptance checks.",
                if signals.is_empty() {
                    "No planning conflicts detected in the loaded inputs. This does not establish project readiness.".into()
                } else {
                    signals
                        .iter()
                        .map(|s| format!("- {s}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            );
        }
        if self.tab == 1 {
            return self.capacity();
        }
        let Some(d) = self.selected_item() else {
            return "# No PBIs in this selection\n\nUse N in the document browser to create a PBI, or select another iteration.".into();
        };
        let item = self
            .snapshot
            .forecast
            .as_ref()
            .and_then(|f| f.items.iter().find(|i| i.path == d.path));
        let amount = |v: Option<f64>| {
            v.map(|v| format!("{v:.2} person-days"))
                .unwrap_or_else(|| "unavailable".into())
        };
        let mut body = format!(
            "# {}\n\n{}\n\n- Status: {}\n- Original: {}\n- Remaining demand: {}\n",
            d.id.as_deref().unwrap_or("Missing ID"),
            d.title,
            value(d, "delivery_status"),
            amount(item.and_then(|i| i.original)),
            amount(item.and_then(|i| i.remaining))
        );
        for key in [
            "sprint",
            "owner",
            "depends_on",
            "ready_after",
            "due_date",
            "baseline_id",
        ] {
            body.push_str(&format!("- {key}: {}\n", value(d, key)));
        }
        body.push_str(&format!("\nSource: {}\n\nM changes the iteration through review; E edits the PBI; Enter opens its document.\n\n## Recorded content\n\n{}",d.path,d.body));
        body
    }
    fn capacity(&self) -> String {
        let bucket = self.buckets[self.period].1;
        let matches = |s: i64| match bucket {
            Bucket::All => true,
            Bucket::Sprint(n) => n == s,
            Bucket::Unscheduled => false,
        };
        let mut text = String::from(
            "# Capacity and demand\n\nValues are remaining person-days after the supplied as_of date. They are not calendar durations. Demand is by role, not assigned to individual people.\n\n",
        );
        if let Some(f) = &self.snapshot.forecast {
            text.push_str("| Sprint / role | Capacity | Demand | Balance | Demand / capacity |\n|---|---|---|---|---|\n");
            let mut count = 0;
            for c in f.caps.iter().filter(|c| matches(c.sprint)) {
                count += 1;
                let bar = if c.capacity > 0. {
                    let n = ((c.effort / c.capacity).min(1.) * 10.).round() as usize;
                    format!(
                        "[{}{}]{}",
                        "#".repeat(n),
                        "·".repeat(10 - n),
                        if c.effort > c.capacity { " OVER" } else { "" }
                    )
                } else {
                    "no capacity".into()
                };
                text.push_str(&format!(
                    "| {} / {} | {:.2} | {:.2} | {:.2} | {} |\n",
                    c.sprint,
                    c.role,
                    c.capacity,
                    c.effort,
                    c.capacity - c.effort,
                    bar
                ));
            }
            if count == 0 {
                text.push_str("\nNo capacity rows for this selection. Unscheduled work has no assigned capacity.\n");
            }
        } else {
            text.push_str("Forecast unavailable. Capacity, demand and balances are not calculated; see Signals for required inputs.\n");
        }
        text.push_str("\n## Supplied allocation rows (all projects)\n\nOnly this project's allocations contribute to the forecast above. Cross-project rows remain visible for checking availability.\n\n| Person | Role | Sprint | Project | FTE | Workdays | Absence | Focus | Reserve |\n|---|---|---|---|---|---|---|---|---|\n");
        for row in &self.snapshot.people {
            if matches!(bucket, Bucket::All) || row[2].parse().is_ok_and(matches) {
                text.push_str(&format!("| {} |\n", row.join(" | ")));
            }
        }
        text.push_str("\nP edits allocation inputs; I edits iterations; A edits the forecast date. Blank input remains unknown.\n");
        text
    }
    pub fn draw(&mut self, f: &mut ratatui::Frame) {
        let area = f.area();
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(area);
        self.tabs = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .split(rows[0])
            .to_vec();
        for (n, label) in ["1 Schedule", "2 Capacity", "3 Signals"]
            .into_iter()
            .enumerate()
        {
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(if n == self.tab {
                    Color::Cyan
                } else {
                    Color::Gray
                })),
                self.tabs[n],
            );
        }
        let basis = self
            .snapshot
            .forecast
            .as_ref()
            .map(|p| format!("as_of {}", p.as_of))
            .unwrap_or_else(|| "Forecast unavailable — inspect Signals".into());
        f.render_widget(Paragraph::new(format!("Planning · {basis} · loaded {}\nSelection: {}\nLeft/Right: iteration · Tab: pane · Up/Down: select · PgUp/PgDn: read · R: refresh",self.snapshot.loaded_at,self.buckets[self.period].0)),rows[1]);
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(if self.tab == 0 {
                vec![
                    Constraint::Percentage(25),
                    Constraint::Percentage(30),
                    Constraint::Percentage(45),
                ]
            } else {
                vec![Constraint::Percentage(25), Constraint::Percentage(75)]
            })
            .split(rows[2]);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Iterations")
            .border_style(Style::default().fg(if self.focus == Focus::Periods {
                Color::Cyan
            } else {
                Color::Gray
            }));
        self.periods_area = block.inner(panes[0]);
        f.render_widget(block, panes[0]);
        self.period_list.select(Some(self.period));
        f.render_stateful_widget(
            List::new(self.buckets.iter().map(|(s, _)| ListItem::new(s.as_str())))
                .highlight_style(Style::default().bg(Color::DarkGray)),
            self.periods_area,
            &mut self.period_list,
        );
        self.items_area = Rect::default();
        let detail = if self.tab == 0 {
            let block = Block::default()
                .borders(Borders::ALL)
                .title("PBIs · right-click: move")
                .border_style(Style::default().fg(if self.focus == Focus::Items {
                    Color::Cyan
                } else {
                    Color::Gray
                }));
            self.items_area = block.inner(panes[1]);
            f.render_widget(block, panes[1]);
            let items = self
                .items()
                .iter()
                .map(|d| {
                    ListItem::new(format!(
                        "{} · {}",
                        d.id.as_deref().unwrap_or("Missing ID"),
                        value(d, "delivery_status")
                    ))
                })
                .collect::<Vec<_>>();
            self.item_list.select(Some(self.selected));
            f.render_stateful_widget(
                List::new(items).highlight_style(Style::default().bg(Color::DarkGray)),
                self.items_area,
                &mut self.item_list,
            );
            panes[2]
        } else {
            panes[1]
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Details")
            .border_style(Style::default().fg(if self.focus == Focus::Detail {
                Color::Cyan
            } else {
                Color::Gray
            }));
        self.detail_area = block.inner(detail);
        f.render_widget(block, detail);
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
                    .collect::<Vec<Line>>(),
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
                .wrap(ratatui::widgets::Wrap { trim: false })
                .style(Style::default().fg(Color::Yellow)),
            rows[3],
        );
        self.actions.clear();
        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 6); 6])
            .split(rows[4]);
        for (n, (c, label)) in [
            ('m', "M Move"),
            ('e', "E Edit"),
            ('p', "P People"),
            ('i', "I Iterations"),
            ('a', "A Date"),
            ('x', "Esc Back"),
        ]
        .into_iter()
        .enumerate()
        {
            self.actions.push((buttons[n], c));
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
                buttons[n],
            );
        }
        if let Some(p) = &mut self.picker {
            let width = area.width.min(95);
            let height = area
                .height
                .min(p.choices.len().saturating_add(2).min(u16::MAX as usize) as u16);
            let rect = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            let block = Block::default()
                .borders(Borders::ALL)
                .title("Choose iteration — review before saving · Esc cancels");
            p.area = block.inner(rect);
            f.render_widget(Clear, rect);
            f.render_widget(block, rect);
            f.render_stateful_widget(
                List::new(p.choices.iter().map(|(s, _)| ListItem::new(s.as_str())))
                    .highlight_style(Style::default().bg(Color::DarkGray)),
                p.area,
                &mut p.state,
            );
        }
    }
}
fn value(d: &Document, key: &str) -> String {
    match &d.metadata[key] {
        serde_json::Value::Null => "not supplied".into(),
        serde_json::Value::String(s) if s.is_empty() => "not supplied".into(),
        serde_json::Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planning::{Capacity, Forecast, Item, Period};
    use crossterm::event::{KeyEvent, MouseEvent};
    use serde_json::json;
    fn snapshot() -> Snapshot {
        Snapshot {
            loaded_at: "synthetic fixture".into(),
            periods: vec![Period {
                sprint: 7,
                start: "2026-11-02".parse().unwrap(),
                end: "2026-11-13".parse().unwrap(),
                goal: "Fixture goal".into(),
            }],
            items: vec![Document {
                path: "05_Backlog/PBI-TEST.md".into(),
                title: "合成測試".into(),
                kind: "pbi".into(),
                id: Some("PBI-TEST".into()),
                hash: "fixture-hash".into(),
                metadata: json!({"sprint":null,"delivery_status":"proposed","owner":null}),
                body: "# Synthetic fixture\n\n".repeat(60),
            }],
            people: vec![
                vec![
                    "Fixture person",
                    "Backend",
                    "7",
                    "PROJECT-TEST",
                    "1",
                    "10",
                    "0",
                    "0.8",
                    "0.1",
                ]
                .into_iter()
                .map(String::from)
                .collect(),
            ],
            forecast: Some(Forecast {
                as_of: "2026-11-02".parse().unwrap(),
                periods: vec![],
                caps: vec![Capacity {
                    sprint: 7,
                    role: "Backend".into(),
                    capacity: 7.2,
                    effort: 0.,
                    items: vec![],
                }],
                items: vec![Item {
                    id: "PBI-TEST".into(),
                    path: "05_Backlog/PBI-TEST.md".into(),
                    delivery_status: "proposed".into(),
                    sprint: None,
                    original: None,
                    remaining: None,
                }],
                messages: vec![],
            }),
            problems: vec![],
        }
    }
    fn key(p: &mut Planner, code: KeyCode) -> Command {
        p.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }
    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }
    fn draw(p: &mut Planner, w: u16, h: u16) {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| p.draw(f)).unwrap();
    }
    #[test]
    fn refresh_preserves_reading_but_closes_a_stale_destination_picker() {
        let mut p = Planner::new(snapshot());
        p.focus = Focus::Detail;
        p.scroll = 12;
        p.pick();
        assert!(p.picker.is_some());
        p.replace(snapshot());
        assert!(p.picker.is_none());
        assert!(p.message.contains("reopen"));
        assert_eq!(p.focus, Focus::Detail);
        assert_eq!(p.scroll, 12);
        let mut next = snapshot();
        next.items.clear();
        p.replace(next);
        assert_eq!(p.scroll, 0);
    }
    #[test]
    fn moving_emits_a_proposal_and_escape_closes_only_the_destination_picker() {
        let mut p = Planner::new(snapshot());
        assert_eq!(key(&mut p, KeyCode::Char('m')), Command::None);
        assert!(p.picker.is_some());
        assert_eq!(key(&mut p, KeyCode::Esc), Command::None);
        assert!(p.picker.is_none());
        key(&mut p, KeyCode::Char('m'));
        key(&mut p, KeyCode::Down);
        assert_eq!(
            key(&mut p, KeyCode::Enter),
            Command::Schedule {
                path: "05_Backlog/PBI-TEST.md".into(),
                hash: "fixture-hash".into(),
                sprint: Some(7)
            }
        );
        assert!(p.snapshot.items[0].metadata["sprint"].is_null());
        assert_eq!(
            key(&mut p, KeyCode::Char('p')),
            Command::Edit(schedule::PEOPLE.into())
        );
        assert_eq!(
            key(&mut p, KeyCode::Char('i')),
            Command::Edit(schedule::ITERATIONS.into())
        );
        assert_eq!(
            key(&mut p, KeyCode::Char('a')),
            Command::Edit(schedule::PLANNING.into())
        );
        assert_eq!(key(&mut p, KeyCode::Esc), Command::ClosePlanning);
    }
    #[test]
    fn mouse_selects_views_targets_and_scrollbar_without_saving() {
        let mut p = Planner::new(snapshot());
        draw(&mut p, 140, 35);
        p.event(mouse(
            MouseEventKind::Down(MouseButton::Right),
            p.items_area.x,
            p.items_area.y,
        ));
        draw(&mut p, 140, 35);
        let area = p.picker.as_ref().unwrap().area;
        assert!(matches!(
            p.event(mouse(
                MouseEventKind::Down(MouseButton::Left),
                area.x,
                area.y + 1
            )),
            Command::Schedule {
                sprint: Some(7),
                ..
            }
        ));
        let tab = p.tabs[1];
        p.event(mouse(MouseEventKind::Down(MouseButton::Left), tab.x, tab.y));
        assert_eq!(p.tab, 1);
        assert!(p.details().contains("7.20"));
        key(&mut p, KeyCode::Char('1'));
        draw(&mut p, 140, 35);
        p.event(mouse(
            MouseEventKind::Down(MouseButton::Left),
            p.bar.x,
            p.bar.bottom() - 1,
        ));
        assert_eq!(p.scroll, p.max_scroll);
        p.event(mouse(
            MouseEventKind::ScrollUp,
            p.detail_area.x,
            p.detail_area.y,
        ));
        assert!(p.scroll < p.max_scroll);
        let period = p.periods_area;
        p.event(mouse(
            MouseEventKind::Down(MouseButton::Left),
            period.x,
            period.y + 2,
        ));
        assert!(p.items().is_empty());
        assert_eq!(key(&mut p, KeyCode::Char('m')), Command::None);
    }
    #[test]
    fn refresh_clears_prior_forecast_and_unknowns_are_not_zero_capacity() {
        let mut p = Planner::new(snapshot());
        assert!(p.details().contains("Remaining demand: unavailable"));
        let mut replacement = snapshot();
        replacement.forecast = None;
        replacement
            .problems
            .push("Forecast unavailable: supply as_of".into());
        p.replace(replacement);
        key(&mut p, KeyCode::Char('2'));
        assert!(p.details().contains("Forecast unavailable"));
        assert!(!p.details().contains("7.20"));
        assert!(p.details().contains("Fixture person"));
        key(&mut p, KeyCode::Char('3'));
        assert!(p.details().contains("supply as_of"));
        assert!(!p.details().contains("No planning conflicts"));
        for (w, h) in [(1, 1), (20, 6), (80, 24)] {
            for tab in ['1', '2', '3'] {
                key(&mut p, KeyCode::Char(tab));
                draw(&mut p, w, h);
            }
        }
    }
    #[test]
    fn recorded_completion_is_visible_but_cannot_be_moved() {
        let mut s = snapshot();
        s.items[0].metadata["delivery_status"] = json!("done");
        let mut p = Planner::new(s);
        key(&mut p, KeyCode::Char('m'));
        assert!(p.picker.is_none());
        assert!(p.message.contains("historical"));
        assert_eq!(
            key(&mut p, KeyCode::Enter),
            Command::Read("05_Backlog/PBI-TEST.md".into())
        );
    }
}
