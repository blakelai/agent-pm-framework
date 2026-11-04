//! Human-operated recovery; explicit paths and separate review/execute steps.
use super::{editor::Editor, markdown, view::Command};
use crate::{
    backup,
    workspace::{Workspace, WorkspaceLock},
};
use anyhow::{Context, Result, ensure};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Operation {
    Create,
    Check,
    Restore,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub operation: Operation,
    pub archive: PathBuf,
    pub parent: PathBuf,
    pub sessions: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    Check(Request),
    Execute {
        request: Request,
        manifest: Option<String>,
    },
}
pub enum Reply {
    Checked(backup::Report),
    Completed(backup::Report),
}

pub fn execute(
    workspace: &Workspace,
    lock: &WorkspaceLock,
    job: Job,
    cancel: &CancellationToken,
) -> Result<Reply> {
    match job {
        Job::Check(request) => {
            let report = backup::check_cancellable(&request.archive, cancel)?;
            ensure!(
                !request.sessions || report.includes_sessions,
                "This backup does not contain sessions"
            );
            Ok(Reply::Checked(report))
        }
        Job::Execute { request, manifest } => Ok(Reply::Completed(match request.operation {
            Operation::Create => {
                backup::create_locked(workspace, &request.archive, request.sessions, lock, cancel)?
            }
            Operation::Restore => backup::restore_cancellable(
                &request.archive,
                &request.parent,
                request.sessions,
                Some(
                    manifest
                        .as_deref()
                        .context("Check the archive before restoring")?,
                ),
                cancel,
            )?,
            Operation::Check => anyhow::bail!("A check never executes a write"),
        })),
    }
}

pub struct Recovery {
    operation: Operation,
    archive: Editor,
    parent: Editor,
    sessions: bool,
    focus: usize,
    review: Option<(Request, Option<backup::Report>)>,
    pending: Option<(Request, CancellationToken)>,
    pub message: String,
    body: String,
    scroll: usize,
    max_scroll: usize,
    tabs: Vec<(Rect, Operation)>,
    fields: Vec<(Rect, usize)>,
    detail: Rect,
    bar: Rect,
    offsets: [usize; 2],
}
impl Default for Recovery {
    fn default() -> Self {
        Self {
            operation: Operation::Create,
            archive: Editor::default(),
            parent: Editor::default(),
            sessions: false,
            focus: 0,
            review: None,
            pending: None,
            message: "Enter absolute paths. Ctrl+S reviews; no model or MCP connection.".into(),
            body: Self::scope().into(),
            scroll: 0,
            max_scroll: 0,
            tabs: vec![],
            fields: vec![],
            detail: Rect::default(),
            bar: Rect::default(),
            offsets: [0; 2],
        }
    }
}
impl Drop for Recovery {
    fn drop(&mut self) {
        if let Some((_, cancel)) = &self.pending {
            cancel.cancel();
        }
    }
}
impl Recovery {
    fn scope() -> &'static str {
        "# Recovery scope\n\nManaged project folders, originals, captured OpenWiki/code snapshots, baselines, review history, Skills and guides are preserved.\n\nPrivate provider/MCP configuration, Git history, application builds and external repositories are excluded. Archives are unencrypted and may contain sensitive project data.\n\nSaved sessions are opt-in. Unsent Chat input is not saved. Unknown tool outcomes remain unresolved; restore never replays tools.\n\nPause external editors for a coherent backup. Restores always create a new private directory; the current Vault stays open.\n\nLimits: 20,000 files, 50 MiB per file, 1 GiB total. Historical migrations and automatic encryption are unavailable."
    }
    pub fn running(&self) -> bool {
        self.pending.is_some()
    }
    fn choices(&self) -> Vec<usize> {
        match self.operation {
            Operation::Create => vec![0, 2, 3],
            Operation::Check => vec![0, 3],
            Operation::Restore => vec![0, 1, 2, 3],
        }
    }
    fn request(&self) -> Result<Request> {
        let path = |text: &str| -> Result<PathBuf> {
            ensure!(
                !text.is_empty() && !text.chars().any(char::is_control),
                "Enter a nonempty path without line breaks or control characters"
            );
            let path = PathBuf::from(text);
            ensure!(
                path.is_absolute(),
                "Use an absolute path (starting with /); paths are not shell-expanded"
            );
            Ok(path)
        };
        Ok(Request {
            operation: self.operation,
            archive: path(&self.archive.text)?,
            parent: if self.operation == Operation::Restore {
                path(&self.parent.text)?
            } else {
                PathBuf::new()
            },
            sessions: self.operation != Operation::Check && self.sessions,
        })
    }
    fn switch(&mut self, operation: Operation) {
        if self.operation != operation {
            self.sessions = false;
        }
        self.operation = operation;
        self.review = None;
        self.focus = 0;
        self.scroll = 0;
        self.body = Self::scope().into();
        self.message = "Paths retained. Ctrl+S reviews/checks the selected operation.".into();
    }
    fn review_text(&mut self, request: Request, report: Option<backup::Report>) {
        let verb = if request.operation == Operation::Create {
            "Create backup"
        } else {
            "Restore into a new Vault"
        };
        self.body = format!("# Review: {verb}\n\nArchive: `{}`\n\nRestore parent: `{}`\n\nInclude saved sessions: **{}**\n\n{}{}\n\nCtrl+S or Execute performs this operation. Esc returns to editing.\n\n{}",
            request.archive.display(),
            if request.parent.as_os_str().is_empty() { "not applicable".into() } else { request.parent.display().to_string() },
            request.sessions,
            report.as_ref().map(|r| format!("Verified archive: {} files / {} bytes; format {}.\n\n", r.files,r.bytes,r.format)).unwrap_or_default(),
            report.as_ref().map(|r| format!("Manifest SHA-256: `{}`\n\nThese counts cover the archive; the restore result reports the selected scope.",r.manifest_sha256)).unwrap_or_else(|| "File counts and sizes are reported after creation. Existing output files are never overwritten.".into()),
            Self::scope());
        self.review = Some((request, report));
        self.scroll = 0;
        self.message =
            "Review ready. Execute is a separate action; nothing has been written.".into();
    }
    fn submit(&mut self) -> Command {
        if let Some((request, report)) = &self.review {
            return Command::RecoveryJob(Job::Execute {
                request: request.clone(),
                manifest: report.as_ref().map(|r| r.manifest_sha256.clone()),
            });
        }
        match self.request() {
            Ok(request) if self.operation == Operation::Create => self.review_text(request, None),
            Ok(request) => return Command::RecoveryJob(Job::Check(request)),
            Err(e) => self.message = format!("{e:#}"),
        }
        Command::None
    }
    pub fn start(&mut self, job: &Job, cancel: CancellationToken) {
        let request = match job {
            Job::Check(r) | Job::Execute { request: r, .. } => r.clone(),
        };
        self.pending = Some((request, cancel));
        self.message =
            "Working locally… Esc requests cancellation; wait for the actual result.".into();
    }
    pub fn finish(&mut self, result: Result<Reply>) {
        let Some((request, _)) = self.pending.take() else {
            return;
        };
        self.review = None;
        match result {
            Ok(Reply::Checked(report)) if request.operation == Operation::Restore => {
                self.review_text(request, Some(report))
            }
            Ok(Reply::Checked(report)) => {
                self.body = format!(
                    "# Archive integrity passed\n\nPath: `{}`\n\n{} files / {} bytes\n\nSaved sessions: {}\n\nFormat: {}\n\nManifest SHA-256: `{}`\n\nThis checks bytes against the manifest, not authorship or business correctness.",
                    report.path.display(),
                    report.files,
                    report.bytes,
                    report.includes_sessions,
                    report.format,
                    report.manifest_sha256
                );
                self.message = "Check complete; no project files were written.".into();
            }
            Ok(Reply::Completed(report)) => {
                self.body = format!(
                    "# Operation completed\n\nResult: `{}`\n\n{} files / {} bytes\n\nSaved sessions: {}\n\nManifest SHA-256: `{}`\n\n{}",
                    report.path.display(),
                    report.files,
                    report.bytes,
                    report.includes_sessions,
                    report.manifest_sha256,
                    if request.operation == Operation::Restore {
                        "The current Vault and Chat remain open. Open the result separately; run validation and configure provider/MCP access before resuming saved sessions."
                    } else {
                        "Backup created. Keep this archive in storage appropriate for the project's data."
                    }
                );
                self.message =
                    "Completed. Esc returns to Browse; a new write requires another review.".into();
            }
            Err(e) => {
                self.body = format!(
                    "# Operation did not complete\n\n{e:#}\n\nReview the inputs and retry. No successful result is claimed."
                );
                self.message =
                    "No completed result. Ctrl+S checks/reviews again; Esc closes.".into();
            }
        }
        self.scroll = 0;
    }
    fn back(&mut self) -> Command {
        if let Some((_, token)) = &self.pending {
            token.cancel();
            self.message =
                "Cancellation requested; waiting for cleanup or the completed result.".into();
        } else if self.review.take().is_some() {
            self.body = Self::scope().into();
            self.message = "Review discarded; Ctrl+S prepares a new review.".into();
            self.scroll = 0;
        } else {
            return Command::CloseRecovery;
        }
        Command::None
    }
    fn scroll(&mut self, down: bool, count: usize) {
        self.scroll = if down {
            self.scroll.saturating_add(count).min(self.max_scroll)
        } else {
            self.scroll.saturating_sub(count)
        };
    }
    pub fn event(&mut self, event: Event) -> Command {
        if self.running() {
            if matches!(event, Event::Key(k) if k.kind == KeyEventKind::Press && k.code == KeyCode::Esc)
                || matches!(event, Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Right))
            {
                return self.back();
            }
            return Command::None;
        }
        match event {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                if k.code == KeyCode::Esc {
                    return self.back();
                }
                if k.code == KeyCode::Char('s') && k.modifiers.contains(KeyModifiers::CONTROL) {
                    return self.submit();
                }
                if k.code == KeyCode::PageDown {
                    self.scroll(true, 10);
                    return Command::None;
                }
                if k.code == KeyCode::PageUp {
                    self.scroll(false, 10);
                    return Command::None;
                }
                if k.modifiers.contains(KeyModifiers::ALT) {
                    match k.code {
                        KeyCode::Char('1') => self.switch(Operation::Create),
                        KeyCode::Char('2') => self.switch(Operation::Check),
                        KeyCode::Char('3') => self.switch(Operation::Restore),
                        _ => {}
                    }
                    return Command::None;
                }
                if self.review.is_some() {
                    return Command::None;
                }
                match k.code {
                    KeyCode::Tab | KeyCode::BackTab => {
                        let choices = self.choices();
                        let pos = choices.iter().position(|n| *n == self.focus).unwrap_or(0);
                        self.focus = choices[(pos
                            + if k.code == KeyCode::BackTab {
                                choices.len() - 1
                            } else {
                                1
                            })
                            % choices.len()];
                    }
                    KeyCode::Enter if self.focus == 3 => return self.submit(),
                    KeyCode::Char(' ') | KeyCode::Enter if self.focus == 2 => {
                        self.sessions = !self.sessions
                    }
                    KeyCode::Enter => {}
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        if self.focus == 0 {
                            self.archive.clear();
                        } else if self.focus == 1 {
                            self.parent.clear();
                        }
                    }
                    _ if self.focus == 0 => self.archive.key(k),
                    _ if self.focus == 1 => self.parent.key(k),
                    _ => {}
                }
            }
            Event::Paste(text) if self.review.is_none() && self.focus < 2 => {
                if text.chars().any(char::is_control) {
                    self.message =
                        "Paste one path without control characters or line breaks.".into();
                } else if self.focus == 0 {
                    self.archive.insert(&text);
                } else {
                    self.parent.insert(&text);
                }
            }
            Event::Mouse(m) => {
                let position = Position::new(m.column, m.row);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Right) => return self.back(),
                    MouseEventKind::ScrollDown => self.scroll(true, 3),
                    MouseEventKind::ScrollUp => self.scroll(false, 3),
                    MouseEventKind::Down(MouseButton::Left) => {
                        if let Some((_, operation)) =
                            self.tabs.iter().find(|(r, _)| r.contains(position))
                        {
                            self.switch(*operation);
                        } else if self.bar.contains(position) {
                            self.scroll = self
                                .max_scroll
                                .saturating_mul(m.row.saturating_sub(self.bar.y) as usize)
                                / self.bar.height.saturating_sub(1).max(1) as usize;
                        } else if let Some((rect, field)) = self
                            .fields
                            .iter()
                            .find(|(r, _)| r.contains(position))
                            .copied()
                        {
                            if field == 3 {
                                return self.submit();
                            }
                            if self.review.is_none() {
                                self.focus = field;
                                if field == 2 {
                                    self.sessions = !self.sessions;
                                } else {
                                    let editor = if field == 0 {
                                        &mut self.archive
                                    } else {
                                        &mut self.parent
                                    };
                                    editor.click(
                                        rect.width.saturating_sub(2) as usize,
                                        self.offsets[field]
                                            + m.row.saturating_sub(rect.y + 1) as usize,
                                        m.column.saturating_sub(rect.x + 1) as usize,
                                    );
                                }
                            }
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
                Constraint::Length(3),
                Constraint::Length(if self.operation == Operation::Restore {
                    3
                } else {
                    0
                }),
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(2),
            ])
            .split(f.area());
        self.tabs.clear();
        self.fields.clear();
        let tabs = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .split(rows[0]);
        for ((op, label), rect) in [
            (Operation::Create, "Alt+1 Backup"),
            (Operation::Check, "Alt+2 Check"),
            (Operation::Restore, "Alt+3 Restore"),
        ]
        .into_iter()
        .zip(tabs.iter().copied())
        {
            f.render_widget(
                Paragraph::new(label).style(Style::default().fg(if self.operation == op {
                    Color::Cyan
                } else {
                    Color::Gray
                })),
                rect,
            );
            self.tabs.push((rect, op));
        }
        for (index, rect, label) in [
            (
                0,
                rows[1],
                if self.operation == Operation::Create {
                    "New archive path"
                } else {
                    "Existing archive path"
                },
            ),
            (1, rows[2], "Existing restore parent"),
        ] {
            if rect.height == 0 {
                continue;
            }
            self.fields.push((rect, index));
            let block = Block::default()
                .borders(Borders::ALL)
                .title(label)
                .border_style(Style::default().fg(if self.focus == index {
                    Color::Cyan
                } else {
                    Color::Gray
                }));
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let editor = if index == 0 {
                &self.archive
            } else {
                &self.parent
            };
            let (lines, (y, x)) = editor.rows(inner.width as usize);
            let offset = y.saturating_sub(inner.height.saturating_sub(1) as usize);
            self.offsets[index] = offset;
            f.render_widget(
                Paragraph::new(
                    lines
                        .into_iter()
                        .skip(offset)
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
                inner,
            );
            if !self.running()
                && self.review.is_none()
                && self.focus == index
                && inner.width > 0
                && inner.height > 0
            {
                f.set_cursor_position((
                    inner.x + (x as u16).min(inner.width - 1),
                    inner.y + (y - offset) as u16,
                ));
            }
        }
        let buttons = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(rows[3]);
        if self.operation != Operation::Check {
            self.fields.push((buttons[0], 2));
            f.render_widget(
                Paragraph::new(format!(
                    "[{}] Include saved sessions",
                    if self.sessions { "x" } else { " " }
                ))
                .style(Style::default().fg(if self.focus == 2 {
                    Color::Cyan
                } else {
                    Color::White
                })),
                buttons[0],
            );
        }
        self.fields.push((buttons[1], 3));
        let label = if self.running() {
            "Working… Esc cancels"
        } else if self.review.is_some() {
            "[ Execute · Ctrl+S ]"
        } else if self.operation == Operation::Check {
            "[ Check · Ctrl+S ]"
        } else {
            "[ Review · Ctrl+S ]"
        };
        f.render_widget(
            Paragraph::new(label).style(Style::default().fg(Color::Yellow)),
            buttons[1],
        );
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Recovery — local files only");
        self.detail = block.inner(rows[4]);
        f.render_widget(block, rows[4]);
        self.bar = Rect::new(
            self.detail.right().saturating_sub(1),
            self.detail.y,
            self.detail.width.min(1),
            self.detail.height,
        );
        let text = Rect {
            width: self.detail.width.saturating_sub(1),
            ..self.detail
        };
        let rendered = markdown::render(&self.body, text.width as usize);
        self.max_scroll = rendered.lines.len().saturating_sub(text.height as usize);
        self.scroll = self.scroll.min(self.max_scroll);
        let len = rendered.lines.len();
        f.render_widget(
            Paragraph::new(
                rendered
                    .lines
                    .into_iter()
                    .skip(self.scroll)
                    .take(text.height as usize)
                    .collect::<Vec<_>>(),
            ),
            text,
        );
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            self.bar,
            &mut ScrollbarState::new(len)
                .position(self.scroll)
                .viewport_content_length(text.height as usize),
        );
        f.render_widget(
            Paragraph::new(format!(
                "{}\nTab: fields · Ctrl+S: review/execute · Esc/right-click: back/cancel",
                self.message
            ))
            .style(Style::default().fg(Color::Gray)),
            rows[5],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::fs;
    fn key(panel: &mut Recovery, code: KeyCode, modifiers: KeyModifiers) -> Command {
        panel.event(Event::Key(crossterm::event::KeyEvent::new(code, modifiers)))
    }
    fn submit(panel: &mut Recovery) -> Command {
        key(panel, KeyCode::Char('s'), KeyModifiers::CONTROL)
    }
    fn fixture() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("00_Governance")).unwrap();
        fs::write(
            dir.path().join("00_Governance/Project.md"),
            "Synthetic project 原文",
        )
        .unwrap();
        fs::write(
            dir.path().join("00_Governance/Settings.md"),
            "Synthetic settings",
        )
        .unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        (dir, w)
    }
    fn run(panel: &mut Recovery, w: &Workspace, lock: &WorkspaceLock, job: Job) {
        let cancel = CancellationToken::new();
        panel.start(&job, cancel.clone());
        let result = execute(w, lock, job, &cancel);
        panel.finish(result);
    }
    fn job(command: Command) -> Job {
        let Command::RecoveryJob(job) = command else {
            panic!("Expected recovery job")
        };
        job
    }
    fn draw(panel: &mut Recovery, width: u16, height: u16) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| panel.draw(f)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }
    #[test]
    fn recovery_create_needs_separate_review_and_retains_the_workspace_lock() {
        let (_dir, w) = fixture();
        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("backup.pmbak");
        let lock = WorkspaceLock::acquire(&w).unwrap();
        let mut panel = Recovery::default();
        panel.archive.insert(path.to_str().unwrap());
        assert_eq!(submit(&mut panel), Command::None);
        assert!(!path.exists());
        assert!(panel.review.is_some());
        assert_eq!(
            key(&mut panel, KeyCode::Char('x'), KeyModifiers::NONE),
            Command::None
        );
        assert_eq!(panel.archive.text, path.to_str().unwrap());
        let pending = job(submit(&mut panel));
        run(&mut panel, &w, &lock, pending);
        assert!(path.is_file());
        assert!(panel.body.contains("Operation completed"));
        assert!(panel.review.is_none());
        assert!(WorkspaceLock::acquire(&w).is_err());
        let clone = lock.clone();
        drop(lock);
        assert!(WorkspaceLock::acquire(&w).is_err());
        drop(clone);
        assert!(WorkspaceLock::acquire(&w).is_ok());
    }
    #[test]
    fn recovery_check_is_read_only_and_restore_is_bound_to_its_review() {
        let (_dir, w) = fixture();
        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("backup.pmbak");
        backup::create(&w, &path, false).unwrap();
        let lock = WorkspaceLock::acquire(&w).unwrap();
        let mut panel = Recovery::default();
        panel.switch(Operation::Check);
        panel.archive.insert(path.to_str().unwrap());
        let before = fs::read(&path).unwrap();
        let pending = job(submit(&mut panel));
        run(&mut panel, &w, &lock, pending);
        assert!(panel.body.contains("integrity passed"));
        assert!(panel.review.is_none());
        assert_eq!(fs::read(&path).unwrap(), before);
        panel.switch(Operation::Restore);
        panel.parent.insert(out.path().to_str().unwrap());
        let pending = job(submit(&mut panel));
        run(&mut panel, &w, &lock, pending);
        assert!(panel.review.is_some());
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
        let pending = job(submit(&mut panel));
        run(&mut panel, &w, &lock, pending);
        assert!(panel.body.contains("Operation completed"));
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 2);
        assert_eq!(
            fs::read(w.root.join("00_Governance/Project.md")).unwrap(),
            "Synthetic project 原文".as_bytes()
        );
    }
    #[test]
    fn recovery_changed_archive_fails_and_requires_a_fresh_review() {
        let (_dir, w) = fixture();
        let out = tempfile::tempdir().unwrap();
        let a = out.path().join("one.pmbak");
        let b = out.path().join("two.pmbak");
        backup::create(&w, &a, false).unwrap();
        fs::write(
            w.root.join("00_Governance/Project.md"),
            "Changed synthetic project",
        )
        .unwrap();
        backup::create(&w, &b, false).unwrap();
        let lock = WorkspaceLock::acquire(&w).unwrap();
        let mut panel = Recovery::default();
        panel.switch(Operation::Restore);
        panel.archive.insert(a.to_str().unwrap());
        panel.parent.insert(out.path().to_str().unwrap());
        let pending = job(submit(&mut panel));
        run(&mut panel, &w, &lock, pending);
        fs::copy(&b, &a).unwrap();
        let pending = job(submit(&mut panel));
        run(&mut panel, &w, &lock, pending);
        assert!(panel.body.contains("changed after review"));
        assert!(panel.review.is_none());
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 2);
        assert!(matches!(
            submit(&mut panel),
            Command::RecoveryJob(Job::Check(_))
        ));
    }
    #[test]
    fn recovery_sessions_are_opt_in_and_preserve_uncertain_calls() {
        let (_dir, w) = fixture();
        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("private.pmbak");
        let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
        store
            .save("session", &serde_json::json!({"status":"interrupted"}))
            .unwrap();
        store
            .begin_call("session", "unknown", "external", "{}")
            .unwrap();
        let lock = WorkspaceLock::acquire(&w).unwrap();
        let mut panel = Recovery::default();
        panel.archive.insert(path.to_str().unwrap());
        panel.sessions = true;
        submit(&mut panel);
        let pending = job(submit(&mut panel));
        run(&mut panel, &w, &lock, pending);
        assert!(backup::check(&path).unwrap().includes_sessions);
        panel.switch(Operation::Restore);
        assert!(!panel.sessions);
        panel.sessions = true;
        panel.parent.insert(out.path().to_str().unwrap());
        let pending = job(submit(&mut panel));
        run(&mut panel, &w, &lock, pending);
        let pending = job(submit(&mut panel));
        let Reply::Completed(report) =
            execute(&w, &lock, pending, &CancellationToken::new()).unwrap()
        else {
            panic!()
        };
        let restored = Store::open(&report.path.join(".pm-agent/runtime.sqlite3")).unwrap();
        assert_eq!(
            restored.call_result("session", "unknown").unwrap(),
            Some(("started".into(), None))
        );
    }
    #[test]
    fn recovery_cancel_blocks_mutations_and_drop_cancels_workers() {
        let (_dir, w) = fixture();
        let out = tempfile::tempdir().unwrap();
        let lock = WorkspaceLock::acquire(&w).unwrap();
        let path = out.path().join("cancelled.pmbak");
        let mut panel = Recovery::default();
        panel.archive.insert(path.to_str().unwrap());
        submit(&mut panel);
        let pending = job(submit(&mut panel));
        let token = CancellationToken::new();
        panel.start(&pending, token.clone());
        assert_eq!(submit(&mut panel), Command::None);
        assert_eq!(
            key(&mut panel, KeyCode::Char('3'), KeyModifiers::ALT),
            Command::None
        );
        assert_eq!(panel.operation, Operation::Create);
        assert_eq!(
            key(&mut panel, KeyCode::Esc, KeyModifiers::NONE),
            Command::None
        );
        assert!(token.is_cancelled());
        assert!(panel.running());
        panel.finish(execute(&w, &lock, pending.clone(), &token));
        assert!(!path.exists());
        assert!(!panel.running());
        assert!(panel.review.is_none());
        let token = CancellationToken::new();
        panel.start(&pending, token.clone());
        drop(panel);
        assert!(token.is_cancelled());
    }
    #[test]
    fn recovery_pre_cancelled_restore_and_wrong_lock_never_publish() {
        let (_dir, w) = fixture();
        let (_other, other) = fixture();
        let out = tempfile::tempdir().unwrap();
        let path = out.path().join("source.pmbak");
        let report = backup::create(&w, &path, false).unwrap();
        let token = CancellationToken::new();
        token.cancel();
        assert!(
            backup::restore_cancellable(
                &path,
                out.path(),
                false,
                Some(&report.manifest_sha256),
                &token
            )
            .is_err()
        );
        let lock = WorkspaceLock::acquire(&other).unwrap();
        assert!(
            backup::create_locked(
                &w,
                &out.path().join("wrong.pmbak"),
                false,
                &lock,
                &CancellationToken::new()
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
    }
    #[test]
    fn recovery_mouse_tabs_inputs_preview_and_small_windows() {
        let mut panel = Recovery::default();
        let display = draw(&mut panel, 100, 28);
        assert!(display.contains("Alt+1 Backup"));
        let click = |rect: Rect| {
            Event::Mouse(crossterm::event::MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: rect.x,
                row: rect.y,
                modifiers: KeyModifiers::NONE,
            })
        };
        panel.event(click(panel.tabs[2].0));
        assert_eq!(panel.operation, Operation::Restore);
        draw(&mut panel, 100, 28);
        panel.event(Event::Paste("/tmp/備份 file.pmbak".into()));
        assert!(panel.archive.text.contains("備份 file"));
        let old = panel.archive.text.clone();
        panel.event(Event::Paste("unsafe\npath".into()));
        assert_eq!(panel.archive.text, old);
        panel.event(click(panel.fields.iter().find(|(_, i)| *i == 2).unwrap().0));
        assert!(panel.sessions);
        panel.event(Event::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 10,
            row: 12,
            modifiers: KeyModifiers::NONE,
        }));
        assert!(panel.scroll > 0 || panel.max_scroll == 0);
        for (width, height) in [(1, 1), (8, 3), (40, 12), (100, 28)] {
            draw(&mut panel, width, height);
        }
    }
    #[test]
    fn recovery_invalid_paths_and_changed_options_cannot_reuse_review() {
        let mut panel = Recovery::default();
        panel.archive.insert("relative.pmbak");
        assert_eq!(submit(&mut panel), Command::None);
        assert!(panel.review.is_none());
        panel.archive.clear();
        panel.archive.insert("/tmp/recovery.pmbak");
        submit(&mut panel);
        assert!(panel.review.is_some());
        assert_eq!(
            key(&mut panel, KeyCode::Esc, KeyModifiers::NONE),
            Command::None
        );
        assert!(panel.review.is_none());
        panel.focus = 2;
        key(&mut panel, KeyCode::Char(' '), KeyModifiers::NONE);
        submit(&mut panel);
        assert!(panel.review.as_ref().unwrap().0.sessions);
        key(&mut panel, KeyCode::Char('3'), KeyModifiers::ALT);
        assert!(panel.review.is_none());
        assert_eq!(submit(&mut panel), Command::None);
        assert!(panel.message.contains("nonempty"));
    }
}
