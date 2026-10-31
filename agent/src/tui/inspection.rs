use super::{markdown, view::Command};
use crate::inspection::{Request, Snapshot, Target};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};

fn escaped(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

pub struct Inspector {
    pub request: Request,
    pub ticket: Option<u64>,
    snapshot: Option<Snapshot>,
    tab: usize,
    selected: usize,
    scroll: usize,
    max_scroll: usize,
    detail_focus: bool,
    maximized: bool,
    list: ListState,
    tabs: Vec<Rect>,
    list_area: Rect,
    detail_area: Rect,
    bar: Rect,
    actions: Vec<(Rect, char)>,
    cache: Option<(u16, Vec<Line<'static>>)>,
    pub message: String,
}
impl Inspector {
    pub fn new(request: Request) -> Self {
        Self {
            request,
            ticket: None,
            snapshot: None,
            tab: 0,
            selected: 0,
            scroll: 0,
            max_scroll: 0,
            detail_focus: false,
            maximized: false,
            list: ListState::default(),
            tabs: vec![],
            list_area: Rect::default(),
            detail_area: Rect::default(),
            bar: Rect::default(),
            actions: vec![],
            cache: None,
            message: String::new(),
        }
    }
    pub fn start(&mut self, request: Request, ticket: u64) {
        self.request = request;
        self.ticket = Some(ticket);
        self.message =
            "Reading recorded references and verifying comparison bytes… Esc returns to Browse"
                .into();
        self.cache = None;
        self.scroll = 0;
    }
    pub fn finish(&mut self, ticket: u64, result: anyhow::Result<Snapshot>) -> bool {
        if self.ticket != Some(ticket) {
            return false;
        }
        self.ticket = None;
        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.message =
                    "Read-only. R revalidates captured bytes; no live repository sync.".into();
            }
            Err(e) => {
                self.snapshot = None;
                self.message = format!("Inspection unavailable: {e:#}. R retries.");
            }
        }
        self.cache = None;
        true
    }
    pub fn invalidate(&mut self) {
        self.ticket = None;
        self.snapshot = None;
        self.cache = None;
        self.message="Project inputs changed or became unavailable. Results cleared; R rechecks references and snapshot integrity.".into();
    }
    fn reload(&self, target: Target) -> Command {
        if self.ticket.is_some() {
            Command::None
        } else {
            Command::Inspect(Request {
                id: self.request.id.clone(),
                target,
            })
        }
    }
    fn rows(&self) -> Vec<String> {
        let Some(s) = &self.snapshot else {
            return vec![];
        };
        match self.tab {
            0 => s
                .candidates
                .iter()
                .map(|c| format!("{} · {}", c.id, c.kind))
                .collect(),
            1 => s.baselines.clone(),
            _ => vec![s.id.clone()],
        }
    }
    fn switch(&mut self, tab: usize) -> Command {
        self.tab = tab;
        self.selected = 0;
        self.scroll = 0;
        self.cache = None;
        self.list = ListState::default();
        self.detail_focus = false;
        if tab == 2 {
            self.reload(Target::Source)
        } else {
            Command::None
        }
    }
    fn select(&mut self, down: bool) {
        self.selected = if down {
            (self.selected + 1).min(self.rows().len().saturating_sub(1))
        } else {
            self.selected.saturating_sub(1)
        };
        self.scroll = 0;
        self.cache = None;
    }
    fn action(&mut self, key: char) -> Command {
        match key {
            'r' => self.reload(match self.tab {
                0 => Target::Impact,
                1 => self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.baselines.get(self.selected))
                    .map(|id| Target::Baseline(id.clone()))
                    .unwrap_or_else(|| self.request.target.clone()),
                _ => Target::Source,
            }),
            'x' => Command::CloseInspection,
            'o' => self
                .snapshot
                .as_ref()
                .map(|s| {
                    let path = if self.tab == 0 {
                        s.candidates
                            .get(self.selected)
                            .map(|c| &c.path)
                            .unwrap_or(&s.path)
                    } else {
                        &s.path
                    };
                    Command::Read(path.clone())
                })
                .unwrap_or(Command::None),
            'c' => {
                self.detail_focus = true;
                if self.tab == 1 {
                    self.snapshot
                        .as_ref()
                        .and_then(|s| s.baselines.get(self.selected))
                        .map(|id| self.reload(Target::Baseline(id.clone())))
                        .unwrap_or(Command::None)
                } else {
                    Command::None
                }
            }
            'm' => {
                self.maximized = !self.maximized;
                self.detail_focus = true;
                self.cache = None;
                Command::None
            }
            _ => Command::None,
        }
    }
    pub fn event(&mut self, event: Event) -> Command {
        match event {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('q') {
                    return Command::Quit;
                }
                match k.code {
                    KeyCode::Char(c @ '1'..='3')
                        if k.modifiers.is_empty() || k.modifiers == KeyModifiers::ALT =>
                    {
                        return self.switch(c as usize - '1' as usize);
                    }
                    KeyCode::Left if k.modifiers == KeyModifiers::ALT => {
                        return self.switch((self.tab + 2) % 3);
                    }
                    KeyCode::Right if k.modifiers == KeyModifiers::ALT => {
                        return self.switch((self.tab + 1) % 3);
                    }
                    KeyCode::Tab | KeyCode::BackTab => self.detail_focus = !self.detail_focus,
                    KeyCode::Enter => return self.action('c'),
                    KeyCode::Esc if self.detail_focus || self.maximized => {
                        self.detail_focus = false;
                        self.maximized = false;
                        self.cache = None;
                    }
                    KeyCode::Esc => return Command::CloseInspection,
                    KeyCode::F(4) => return self.action('m'),
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
                    KeyCode::Char(c) => return self.action(c),
                    _ => {}
                }
            }
            Event::Mouse(m) => {
                let point = Position::new(m.column, m.row);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some(i) = self.tabs.iter().position(|r| r.contains(point)) {
                            return self.switch(i);
                        }
                        if let Some((_, c)) = self.actions.iter().find(|(r, _)| r.contains(point)) {
                            return self.action(*c);
                        }
                        if self.list_area.contains(point) {
                            self.selected = (self.list.offset()
                                + m.row.saturating_sub(self.list_area.y) as usize)
                                .min(self.rows().len().saturating_sub(1));
                            self.detail_focus = false;
                            self.scroll = 0;
                            self.cache = None;
                        } else if self.bar.contains(point) {
                            self.bar_position(m.row);
                        } else if self.detail_area.contains(point) {
                            self.detail_focus = true;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) if self.list_area.contains(point) => {
                        self.selected = (self.list.offset()
                            + m.row.saturating_sub(self.list_area.y) as usize)
                            .min(self.rows().len().saturating_sub(1));
                        return self.action('c');
                    }
                    MouseEventKind::Drag(MouseButton::Left) if self.bar.contains(point) => {
                        self.bar_position(m.row)
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let down = m.kind == MouseEventKind::ScrollDown;
                        if self.list_area.contains(point) {
                            self.detail_focus = false;
                            self.select(down);
                        } else if self.detail_area.contains(point) || self.bar.contains(point) {
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
    fn bar_position(&mut self, y: u16) {
        self.detail_focus = true;
        self.scroll = (self
            .max_scroll
            .saturating_mul(y.saturating_sub(self.bar.y) as usize)
            / self.bar.height.saturating_sub(1).max(1) as usize)
            .min(self.max_scroll);
    }
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        if self.ticket.is_some() || self.snapshot.is_none() {
            return markdown::render(&self.message, width as usize).lines;
        }
        let s = self.snapshot.as_ref().unwrap();
        if self.tab == 0 {
            let mut text = format!(
                "# Review candidates for {}\n\n{}\n\n{} candidate(s); loaded {}\n\nCurrent SHA256: {}\n\n",
                s.id,
                s.boundary,
                s.candidates.len(),
                s.loaded_at,
                s.current_sha256
            );
            if let Some(c) = s.candidates.get(self.selected) {
                text.push_str(&format!("## {}\n\n{}\n\n", c.id, c.path));
                for edge in &c.chain {
                    text.push_str(&format!("- {} → {} — {}\n", edge.from, edge.to, edge.basis));
                }
            } else {
                text.push_str("No recorded downstream candidates. This is not proof that a change has no impact.\n");
            }
            if !s.unresolved.is_empty() {
                text.push_str("\n## Unresolved references (whole project)\n\n");
                text.push_str(
                    &s.unresolved
                        .iter()
                        .map(|s| format!("- {s}\n"))
                        .collect::<String>(),
                );
            }
            return markdown::render(&text, width as usize).lines;
        }
        if self.tab == 1
            && !matches!(&self.request.target,Target::Baseline(id) if s.baselines.get(self.selected)==Some(id))
        {
            return markdown::render("# Select a captured baseline\n\nEnter, Compare, or right-click explicitly compares the selected candidate. Membership, integrity and recorded approval are checked then. No baseline is selected automatically.\n\nAn empty list means no captured baseline manifests were found.",width as usize).lines;
        }
        if self.tab == 2 && self.request.target != Target::Source {
            return markdown::render(
                "Use R to compare this evidence with its latest registered snapshot.",
                width as usize,
            )
            .lines;
        }
        if let Some(error) = &s.comparison_error {
            return markdown::render(&format!("# Comparison unavailable\n\n{error}\n\nSupply or repair the referenced data; R revalidates."),width as usize).lines;
        }
        let Some(c) = &s.comparison else {
            return markdown::render("Use R to load the selected comparison.", width as usize)
                .lines;
        };
        let mut text = format!("# Comparison: {}\n\n", c.status);
        for (k, v) in c.basis.as_object().unwrap() {
            text.push_str(&format!(
                "- **{k}**: {}\n",
                v.as_str()
                    .map(String::from)
                    .unwrap_or_else(|| v.to_string())
            ));
        }
        if let Some(d) = &c.diff {
            text.push_str(&format!("\nOld SHA256: {}\n\nNew SHA256: {}\n\n{}; final newline old={} new={}\n\nOld / New line numbers; + added, - removed. Control characters are escaped.\n\n",d.old_sha256,d.new_sha256,d.algorithm,d.old_final_newline,d.new_final_newline));
        }
        let mut lines = markdown::render(&text, width as usize).lines;
        if let Some(d) = &c.diff {
            let mut raw = vec![];
            let mut budget = 200_000usize;
            let mut clipped = false;
            for l in &d.lines {
                if budget == 0 {
                    clipped = true;
                    break;
                }
                let color = match l.kind {
                    '+' => Color::Green,
                    '-' => Color::Red,
                    _ => Color::Gray,
                };
                let safe = escaped(&l.text);
                let row = format!(
                    "{} {:>5} {:>5} | {}",
                    l.kind,
                    l.old.map(|n| n.to_string()).unwrap_or_default(),
                    l.new.map(|n| n.to_string()).unwrap_or_default(),
                    safe
                );
                let shown: String = row.chars().take(budget).collect();
                budget = budget.saturating_sub(shown.chars().count());
                clipped |= shown.len() < row.len();
                raw.push(Line::from(Span::styled(shown, Style::default().fg(color))));
            }
            lines.extend(markdown::wrap(raw, width as usize));
            if clipped {
                lines.extend(markdown::render("\nDisplay limited to 200,000 characters. Use pm inspect for the complete bounded comparison; omitted text is not verified by this display.",width as usize).lines);
            }
        }
        lines
    }
    pub fn draw(&mut self, f: &mut ratatui::Frame) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(2),
                Constraint::Length(1),
            ])
            .split(f.area());
        self.tabs = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 3); 3])
            .split(rows[0])
            .to_vec();
        for (i, label) in ["1 Impact", "2 Baseline diff", "3 Source diff"]
            .iter()
            .enumerate()
        {
            f.render_widget(
                Paragraph::new(*label).style(Style::default().fg(if self.tab == i {
                    Color::Cyan
                } else {
                    Color::Gray
                })),
                self.tabs[i],
            );
        }
        f.render_widget(
            Paragraph::new(format!(
                "Inspect {} · recorded evidence only",
                escaped(&self.request.id)
            )),
            rows[1],
        );
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(if self.maximized { 0 } else { 30 }),
                Constraint::Min(0),
            ])
            .split(rows[2]);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Records · Enter / right-click")
            .border_style(Style::default().fg(if self.detail_focus {
                Color::Gray
            } else {
                Color::Cyan
            }));
        self.list_area = block.inner(panes[0]);
        if !self.maximized {
            f.render_widget(block, panes[0]);
            let items = self
                .rows()
                .into_iter()
                .map(|s| ListItem::new(escaped(&s)))
                .collect::<Vec<_>>();
            self.list.select(Some(self.selected));
            f.render_stateful_widget(
                List::new(items).highlight_style(Style::default().bg(Color::DarkGray)),
                self.list_area,
                &mut self.list,
            );
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .title("References / exact text differences")
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
        if self
            .cache
            .as_ref()
            .is_none_or(|(width, _)| *width != self.detail_area.width)
        {
            self.cache = Some((self.detail_area.width, self.lines(self.detail_area.width)));
        }
        let lines = &self.cache.as_ref().unwrap().1;
        self.max_scroll = lines.len().saturating_sub(self.detail_area.height as usize);
        self.scroll = self.scroll.min(self.max_scroll);
        f.render_widget(
            Paragraph::new(
                lines
                    .iter()
                    .skip(self.scroll)
                    .take(self.detail_area.height as usize)
                    .cloned()
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
            Paragraph::new(escaped(&self.message))
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
            ('c', "Enter Compare"),
            ('o', "O Document"),
            ('r', "R Recheck"),
            ('m', "F4 Maximize"),
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
    use crate::inspection::{Candidate, Comparison, Edge, diff};
    use crossterm::event::{KeyEvent, MouseEvent};
    use serde_json::json;
    fn request() -> Request {
        Request {
            id: "REQ-TEST".into(),
            target: Target::Impact,
        }
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            id: "REQ-TEST".into(),
            path: "04_Requirements/REQ-TEST.md".into(),
            title: "Synthetic".into(),
            loaded_at: "fixture".into(),
            current_sha256: "fixture".into(),
            candidates: vec![Candidate {
                id: "PBI-TEST".into(),
                path: "05_Backlog/PBI-TEST.md".into(),
                kind: "pbi".into(),
                chain: vec![Edge {
                    from: "REQ-TEST".into(),
                    to: "PBI-TEST".into(),
                    basis: "wikilink".into(),
                }],
            }],
            unresolved: vec![],
            baselines: vec!["BASE-A".into(), "BASE-B".into()],
            comparison: Some(Comparison {
                basis: json!({"baseline_id":"BASE-A"}),
                status: "changed".into(),
                diff: Some(diff("old\u{1b}[31m\n", &"new 中文\n".repeat(30)).unwrap()),
            }),
            comparison_error: None,
            boundary: "Review candidates only".into(),
        }
    }
    fn ready() -> Inspector {
        let mut i = Inspector::new(request());
        i.start(request(), 1);
        assert!(i.finish(1, Ok(snapshot())));
        i
    }
    fn key(i: &mut Inspector, k: KeyCode) -> Command {
        i.event(Event::Key(KeyEvent::new(k, KeyModifiers::NONE)))
    }
    fn draw(i: &mut Inspector, width: u16, height: u16) {
        let mut t =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        t.draw(|f| i.draw(f)).unwrap();
    }
    fn mouse(i: &mut Inspector, kind: MouseEventKind, p: Position) -> Command {
        i.event(Event::Mouse(MouseEvent {
            kind,
            column: p.x,
            row: p.y,
            modifiers: KeyModifiers::NONE,
        }))
    }
    #[test]
    fn explicit_baseline_selection_focus_navigation_and_open_document() {
        let mut i = ready();
        assert_eq!(
            key(&mut i, KeyCode::Char('o')),
            Command::Read("05_Backlog/PBI-TEST.md".into())
        );
        assert_eq!(key(&mut i, KeyCode::Char('2')), Command::None);
        assert!(
            i.lines(80)
                .iter()
                .any(|l| l.to_string().contains("Select a captured baseline"))
        );
        key(&mut i, KeyCode::Down);
        assert_eq!(
            key(&mut i, KeyCode::Enter),
            Command::Inspect(Request {
                id: "REQ-TEST".into(),
                target: Target::Baseline("BASE-B".into())
            })
        );
        assert!(i.detail_focus);
        assert_eq!(key(&mut i, KeyCode::Esc), Command::None);
        assert!(!i.detail_focus);
        assert_eq!(key(&mut i, KeyCode::Esc), Command::CloseInspection);
        assert_eq!(
            key(&mut i, KeyCode::Char('3')),
            Command::Inspect(Request {
                id: "REQ-TEST".into(),
                target: Target::Source
            })
        );
    }
    #[test]
    fn changed_inputs_reject_old_worker_results_and_clear_old_diffs() {
        let mut i = ready();
        i.start(request(), 2);
        i.invalidate();
        assert!(!i.finish(2, Ok(snapshot())));
        assert!(i.snapshot.is_none());
        assert_eq!(key(&mut i, KeyCode::Char('r')), Command::Inspect(request()));
        i.start(request(), 3);
        assert!(!i.finish(2, Ok(snapshot())));
        assert!(i.finish(3, Err(anyhow::anyhow!("Malformed fixture"))));
        assert!(i.snapshot.is_none());
        assert!(i.message.contains("Malformed fixture"));
        i.start(request(), 4);
        assert!(i.finish(4, Ok(snapshot())));
        assert!(i.snapshot.is_some());
    }
    #[test]
    fn colored_exact_diff_mouse_scroll_maximize_and_small_windows() {
        let mut i = ready();
        i.tab = 1;
        i.request.target = Target::Baseline("BASE-A".into());
        let lines = i.lines(100);
        assert!(
            lines
                .iter()
                .any(|l| l.spans.iter().any(|s| s.style.fg == Some(Color::Red)))
        );
        assert!(lines.iter().any(|l| l.to_string().contains("\\u{1b}[31m")));
        assert!(lines.iter().all(|l| !l.to_string().contains('\u{1b}')));
        draw(&mut i, 100, 24);
        let area = i.detail_area;
        mouse(
            &mut i,
            MouseEventKind::ScrollDown,
            Position::new(area.x, area.y),
        );
        assert!(i.scroll > 0 && i.detail_focus);
        let bar = i.bar;
        mouse(
            &mut i,
            MouseEventKind::Drag(MouseButton::Left),
            Position::new(bar.x, bar.bottom() - 1),
        );
        assert_eq!(i.scroll, i.max_scroll);
        assert_eq!(key(&mut i, KeyCode::F(4)), Command::None);
        assert!(i.maximized);
        for (w, h) in [(1, 1), (20, 6), (80, 24)] {
            draw(&mut i, w, h);
        }
        key(&mut i, KeyCode::Esc);
        assert!(!i.maximized);
        draw(&mut i, 100, 24);
        let area = i.tabs[0];
        mouse(
            &mut i,
            MouseEventKind::Down(MouseButton::Left),
            Position::new(area.x, area.y),
        );
        assert_eq!(i.tab, 0);
    }
}
