//! Ephemeral execution status, separate from the durable conversation transcript.
use super::transcript::ellipsize;
use crate::runtime::Status;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Progress {
    label: String,
    started: Option<Instant>,
    elapsed: Duration,
    active: bool,
    stopping: bool,
    frame: usize,
    last_frame: Option<Instant>,
}
impl Progress {
    pub fn start(&mut self) {
        *self = Self {
            label: "Preparing conversation".into(),
            started: Some(Instant::now()),
            active: true,
            ..Self::default()
        };
    }
    pub fn update(&mut self, label: impl Into<String>) {
        if !self.stopping {
            self.label = label.into();
        }
    }
    pub fn pause(&mut self, label: impl Into<String>) {
        if !self.stopping {
            self.label = label.into();
            self.active = false;
        }
    }
    pub fn cancel(&mut self) {
        self.label = "Stopping · waiting for the operation outcome".into();
        self.stopping = true;
        self.active = true;
    }
    pub fn finish(&mut self, label: impl Into<String>) {
        self.label = label.into();
        if let Some(started) = self.started.take() {
            self.elapsed = started.elapsed();
        }
        self.active = false;
        self.stopping = false;
    }
    pub fn session(&mut self, status: &Status) {
        self.finish(match status {
            Status::Ready => "Ready",
            Status::Running => "Interrupted · inspect the saved session before continuing",
            Status::Completed => "Completed",
            Status::WaitingClarification => "Waiting for your answer · review Questions",
            Status::WaitingApproval => "Waiting for approval · /approve or /deny",
            Status::NeedsReconciliation => "Outcome unconfirmed · inspect before /reconcile",
            Status::Failed => "Failed · expand the error for details",
            Status::Cancelled => "Cancelled",
            Status::LimitReached => "Session limit reached · review /budget",
        });
    }
    pub fn visible(&self) -> bool {
        !self.label.is_empty()
    }
    /// Redraw only this small status row at 10 Hz, even without provider events.
    pub fn tick(&mut self, now: Instant) -> bool {
        if !self.active
            || self
                .last_frame
                .is_some_and(|t| now.duration_since(t) < Duration::from_millis(100))
        {
            return false;
        }
        self.last_frame = Some(now);
        self.frame = (self.frame + 1) % 10;
        true
    }
    pub fn line(&self, width: usize) -> String {
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let marker = if self.active {
            frames[self.frame]
        } else {
            '•'
        };
        let elapsed = self.started.map_or(self.elapsed, |t| t.elapsed()).as_secs();
        let label = if self.label.is_empty() {
            "Preparing conversation"
        } else {
            &self.label
        };
        let suffix = if width >= 45 && self.active && !self.stopping {
            format!(" · {elapsed}s · Esc cancel")
        } else if width >= 30 && self.started.is_some() {
            format!(" · {elapsed}s")
        } else {
            String::new()
        };
        use unicode_width::UnicodeWidthStr;
        ellipsize(
            &format!(
                "{marker} {}{suffix}",
                ellipsize(label, width.saturating_sub(suffix.width() + 2))
            ),
            width,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn animation_advances_without_events_and_stops_for_all_terminal_states() {
        let mut p = Progress::default();
        p.start();
        p.update("Reading 付款.md");
        let now = Instant::now();
        let before = p.line(80);
        assert!(p.tick(now));
        assert_ne!(before, p.line(80));
        assert!(!p.tick(now + Duration::from_millis(20)));
        for status in [
            Status::WaitingClarification,
            Status::WaitingApproval,
            Status::NeedsReconciliation,
            Status::Completed,
            Status::Failed,
            Status::Cancelled,
            Status::LimitReached,
        ] {
            p.session(&status);
            assert!(!p.tick(now + Duration::from_secs(5)));
            assert!(!p.line(80).contains("Esc cancel"));
        }
    }
    #[test]
    fn cancellation_cannot_be_overwritten_by_late_tool_or_text_events() {
        let mut p = Progress::default();
        p.start();
        p.cancel();
        p.update("Writing response");
        p.pause("Late approval event");
        assert!(p.line(80).contains("waiting for the operation outcome"));
        assert!(p.tick(Instant::now()));
        p.session(&Status::NeedsReconciliation);
        assert!(p.line(80).contains("Outcome unconfirmed"));
        p.start();
        p.update("New task");
        assert!(p.line(80).contains("New task"));
        assert!(p.line(80).contains("Esc cancel"));
        use unicode_width::UnicodeWidthStr;
        for width in [0, 1, 2, 10, 35, 60] {
            assert!(p.line(width).width() <= width);
        }
    }
}
