//! Local material/evidence reading; inspection runs in a bounded background worker.
use super::{markdown, view::Command};
use crate::materials::{Catalog, Inspection};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};

pub struct Materials {
    pub catalog: Catalog,
    selected: usize,
    tab: usize,
    claim: usize,
    scroll: usize,
    max_scroll: usize,
    detail_focus: bool,
    maximized: bool,
    icons: bool,
    pub report: Option<Inspection>,
    pending: Option<u64>,
    pub message: String,
    list: ListState,
    list_area: Rect,
    detail_area: Rect,
    bar: Rect,
    actions: Vec<(Rect, char)>,
}

impl Materials {
    pub fn new(catalog: Catalog) -> Self {
        Self {
            catalog,
            selected: 0,
            tab: 0,
            claim: 0,
            scroll: 0,
            max_scroll: 0,
            detail_focus: false,
            maximized: false,
            icons: true,
            report: None,
            pending: None,
            message: "Select a material; Enter or R checks its bytes and evidence.".into(),
            list: ListState::default(),
            list_area: Rect::default(),
            detail_area: Rect::default(),
            bar: Rect::default(),
            actions: vec![],
        }
    }
    pub fn replace(&mut self, catalog: Catalog) {
        let id = self
            .catalog
            .entries
            .get(self.selected)
            .map(|e| e.id.clone());
        self.selected = id
            .and_then(|id| catalog.entries.iter().position(|e| e.id == id))
            .unwrap_or(0);
        self.catalog = catalog;
        self.invalidate();
    }
    pub fn invalidate(&mut self) {
        self.report = None;
        self.pending = None;
        self.message = "Metadata refreshed; Enter or R rechecks current files.".into();
    }
    pub fn start(&mut self, ticket: u64) {
        self.report = None;
        self.pending = Some(ticket);
        self.message = "Checking original, extracted text and linked evidence…".into();
    }
    pub fn finish(&mut self, ticket: u64, result: anyhow::Result<Inspection>) -> bool {
        if self.pending != Some(ticket) {
            return false;
        }
        self.pending = None;
        match result {
            Ok(r)
                if self.catalog.entries.get(self.selected).is_some_and(|e| {
                    e.id == r.source.id && e.note_sha256 == r.source.note_sha256
                }) =>
            {
                self.message = format!(
                    "Checked {} · raw files require explicit recheck",
                    r.checked_at
                );
                self.claim = self.claim.min(r.claims.len().saturating_sub(1));
                self.report = Some(r);
            }
            Ok(_) => self.message = "Source selection or metadata changed; R rechecks.".into(),
            Err(e) => self.message = format!("Inspection unavailable: {e}"),
        }
        true
    }
    fn select(&mut self, index: usize) {
        let next = index.min(self.catalog.entries.len().saturating_sub(1));
        if next != self.selected {
            self.selected = next;
            self.scroll = 0;
            self.claim = 0;
            self.invalidate();
        }
    }
    fn action(&mut self, key: char) -> Command {
        match key {
            '1'..='3' => {
                self.tab = (key as u8 - b'1') as usize;
                self.scroll = 0;
            }
            '[' => {
                self.claim = self.claim.saturating_sub(1);
                self.scroll = 0;
            }
            ']' => {
                self.claim = (self.claim + 1).min(
                    self.report
                        .as_ref()
                        .map(|r| r.claims.len())
                        .unwrap_or(0)
                        .saturating_sub(1),
                );
                self.scroll = 0;
            }
            'r' => {
                return self
                    .catalog
                    .entries
                    .get(self.selected)
                    .map(|e| Command::MaterialInspect(e.id.clone()))
                    .unwrap_or(Command::None);
            }
            'o' => {
                if let Some(r) = &self.report
                    && r.asset_matches
                {
                    return Command::MaterialOpen {
                        id: r.source.id.clone(),
                        note_hash: r.source.note_sha256.clone(),
                        sha256: r.asset_sha256.clone().unwrap(),
                    };
                }
            }
            'n' => {
                let path = if self.tab == 2 {
                    self.report
                        .as_ref()
                        .and_then(|r| r.claims.get(self.claim))
                        .map(|c| c.path.clone())
                } else {
                    self.catalog
                        .entries
                        .get(self.selected)
                        .map(|e| e.path.clone())
                };
                return path.map(Command::Read).unwrap_or(Command::None);
            }
            'x' => return Command::CloseMaterials,
            _ => {}
        }
        Command::None
    }
    fn details(&self) -> String {
        if let Some(p) = &self.catalog.problem {
            return format!(
                "# Materials unavailable\n\n{p}\n\nRepair the source records, then refresh."
            );
        }
        if self.catalog.entries.is_empty() {
            return "# No imported materials\n\nUse pm ingest with a supplied file. No source contents or review results are inferred.".into();
        }
        let Some(r) = &self.report else {
            return format!(
                "# Material inspection\n\n{}\n\nNo byte check is available for the current selection.\n\n1 Overview · 2 Text · 3 Evidence\n\nEnter/R checks this material; O opens a matching original; N opens the current source/evidence note. Recorded review states do not certify the claims.",
                self.message
            );
        };
        match self.tab {
            1 => r.extraction(),
            2 => r.evidence(self.claim),
            _ => r.overview(),
        }
    }
    pub fn event(&mut self, event: Event) -> Command {
        match event {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('q') {
                    return Command::Quit;
                }
                match k.code {
                    KeyCode::F(4) => self.maximized = !self.maximized,
                    KeyCode::F(6) => self.icons = !self.icons,
                    KeyCode::Tab | KeyCode::BackTab => self.detail_focus = !self.detail_focus,
                    KeyCode::Enter => {
                        self.detail_focus = true;
                        return self.action('r');
                    }
                    KeyCode::Esc if self.detail_focus => self.detail_focus = false,
                    KeyCode::Esc => return Command::CloseMaterials,
                    KeyCode::Up if !self.detail_focus => {
                        self.select(self.selected.saturating_sub(1))
                    }
                    KeyCode::Down if !self.detail_focus => self.select(self.selected + 1),
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
                    KeyCode::Char(c)
                        if k.modifiers.is_empty() || k.modifiers == KeyModifiers::ALT =>
                    {
                        return self.action(c);
                    }
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
                            self.select(
                                self.list.offset()
                                    + m.row.saturating_sub(self.list_area.y) as usize,
                            );
                            self.detail_focus = false;
                        } else if self.detail_area.contains(p) {
                            self.detail_focus = true;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) if self.list_area.contains(p) => {
                        self.select(
                            self.list.offset() + m.row.saturating_sub(self.list_area.y) as usize,
                        );
                        self.detail_focus = true;
                        return self.action('r');
                    }
                    MouseEventKind::Drag(MouseButton::Left) if self.bar.contains(p) => {
                        self.drag(m.row)
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        let down = m.kind == MouseEventKind::ScrollDown;
                        if self.list_area.contains(p) {
                            self.select(if down {
                                self.selected + 1
                            } else {
                                self.selected.saturating_sub(1)
                            });
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
                Constraint::Length(2),
            ])
            .split(f.area());
        f.render_widget(
            Paragraph::new(format!(
                "Materials · {}\n{}",
                self.catalog.loaded_at, self.message
            )),
            rows[0],
        );
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(if self.maximized { 0 } else { 30 }),
                Constraint::Min(0),
            ])
            .split(rows[1]);
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Materials")
            .border_style(Style::default().fg(if self.detail_focus {
                Color::Gray
            } else {
                Color::Cyan
            }));
        self.list_area = block.inner(panes[0]);
        f.render_widget(block, panes[0]);
        let items = self
            .catalog
            .entries
            .iter()
            .map(|e| {
                ListItem::new(format!(
                    "{}{}",
                    if self.icons {
                        match e
                            .asset
                            .as_deref()
                            .and_then(|p| std::path::Path::new(p).extension())
                            .and_then(|s| s.to_str())
                        {
                            Some(
                                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff",
                            ) => "🖼️ ",
                            Some("pdf") => "📕 ",
                            Some("pptx") => "📊 ",
                            Some("docx") => "📄 ",
                            Some("txt" | "md") => "📝 ",
                            _ => "📎 ",
                        }
                    } else {
                        ""
                    },
                    e.title
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
            .title(["Overview", "Extracted data", "Evidence"][self.tab])
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
        for (row, buttons) in [
            vec![
                (" 1 Overview ", '1'),
                (" 2 Text ", '2'),
                (" 3 Evidence ", '3'),
                (" [ Previous ", '['),
                (" ] Next ", ']'),
            ],
            vec![
                (" R Check ", 'r'),
                (" O Original ", 'o'),
                (" N Note ", 'n'),
                (" X Back ", 'x'),
            ],
        ]
        .into_iter()
        .enumerate()
        {
            if row as u16 >= rows[2].height {
                continue;
            }
            let mut x = rows[2].x;
            for (label, c) in buttons {
                let area = Rect::new(
                    x,
                    rows[2].y + row as u16,
                    (label.len() as u16).min(rows[2].right().saturating_sub(x)),
                    1,
                );
                f.render_widget(
                    Paragraph::new(label).style(Style::default().fg(Color::Cyan)),
                    area,
                );
                self.actions.push((area, c));
                x = area.right();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, MouseEvent};
    fn catalog() -> Catalog {
        Catalog {
            loaded_at: "synthetic".into(),
            problem: None,
            entries: vec![crate::materials::Entry {
                id: "SRC-ONE".into(),
                path: "01_Sources/Notes/SRC-ONE.md".into(),
                title: "Synthetic image".into(),
                note_sha256: "note".into(),
                asset: Some("01_Sources/Assets/one.png".into()),
                source_sha256: Some("asset".into()),
                extraction_status: Some("pending_visual_review".into()),
                review_status: Some("pending".into()),
            }],
        }
    }
    fn report() -> Inspection {
        Inspection {
            checked_at: "synthetic".into(),
            source: catalog().entries.remove(0),
            asset_sha256: Some("asset".into()),
            asset_matches: true,
            extracted: Some(crate::materials::Preview {
                path: "01_Sources/Extracted/SRC-ONE.md".into(),
                sha256: "extracted".into(),
                text: "# Source data\n\n原始文字\n\n".repeat(80),
                truncated: false,
            }),
            claims: vec![crate::materials::Claim {
                id: "EVD-ONE".into(),
                path: "01_Sources/Evidence/EVD-ONE.md".into(),
                title: "Synthetic claim".into(),
                note_sha256: "claim".into(),
                locator: Some("region".into()),
                source_sha256: Some("asset".into()),
                recorded_status: Some("draft".into()),
                assertion_type: None,
                body: "# Supplied claim".into(),
                truncated: false,
                problems: vec![],
            }],
            total_claims: 1,
            problems: vec!["Actual review pending".into()],
        }
    }
    fn key(p: &mut Materials, code: KeyCode) -> Command {
        p.event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }
    fn draw(p: &mut Materials, width: u16, height: u16) {
        let mut t =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        t.draw(|f| p.draw(f)).unwrap();
    }
    #[test]
    fn material_navigation_mouse_tabs_focus_and_bounded_markdown() {
        let mut p = Materials::new(catalog());
        draw(&mut p, 100, 28);
        assert_eq!(key(&mut p, KeyCode::Char('o')), Command::None);
        assert_eq!(
            key(&mut p, KeyCode::Enter),
            Command::MaterialInspect("SRC-ONE".into())
        );
        assert!(p.detail_focus);
        p.start(1);
        assert!(p.finish(1, Ok(report())));
        assert!(matches!(
            key(&mut p, KeyCode::Char('o')),
            Command::MaterialOpen { .. }
        ));
        key(&mut p, KeyCode::Char('2'));
        draw(&mut p, 100, 28);
        assert!(p.details().contains("原始文字"));
        key(&mut p, KeyCode::End);
        assert!(p.scroll > 0);
        let bar = p.bar;
        p.event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: bar.x,
            row: bar.y,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(p.scroll, 0);
        let tab = p.actions.iter().find(|(_, c)| *c == '3').unwrap().0;
        p.event(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: tab.x,
            row: tab.y,
            modifiers: KeyModifiers::NONE,
        }));
        assert_eq!(p.tab, 2);
        assert_eq!(
            key(&mut p, KeyCode::Char('n')),
            Command::Read("01_Sources/Evidence/EVD-ONE.md".into())
        );
        key(&mut p, KeyCode::F(4));
        assert!(p.maximized);
        for (w, h) in [(1, 1), (20, 5), (90, 24)] {
            draw(&mut p, w, h);
        }
        assert_eq!(key(&mut p, KeyCode::Esc), Command::None);
        assert!(!p.detail_focus);
        assert_eq!(key(&mut p, KeyCode::Esc), Command::CloseMaterials);
    }
    #[test]
    fn material_refresh_and_selection_reject_superseded_results_and_clear_open_action() {
        let mut p = Materials::new(catalog());
        p.start(1);
        p.replace(catalog());
        assert!(!p.finish(1, Ok(report())));
        assert!(p.report.is_none());
        p.start(2);
        p.finish(2, Ok(report()));
        let mut c = catalog();
        let mut next = c.entries[0].clone();
        next.id = "SRC-TWO".into();
        c.entries.push(next);
        p.replace(c);
        p.start(3);
        p.select(1);
        assert!(!p.finish(3, Ok(report())));
        assert_eq!(key(&mut p, KeyCode::Char('o')), Command::None);
        p.replace(Catalog {
            loaded_at: "changed".into(),
            entries: vec![],
            problem: Some("Malformed source".into()),
        });
        assert!(p.details().contains("Malformed source"));
        assert_eq!(key(&mut p, KeyCode::Char('r')), Command::None);
        draw(&mut p, 80, 24);
    }
}
