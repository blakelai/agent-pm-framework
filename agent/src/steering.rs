//! Deliver one operator correction to the current execution boundary.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

pub const MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub turn_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Correction {
    pub id: String,
    pub turn_id: String,
    pub received_at: String,
    pub(crate) text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) superseded_preview: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Receipt {
    pub correction_id: String,
    pub turn_id: String,
    pub received_at: String,
    pub source: String,
    pub sha256: String,
    pub requests_before_correction: usize,
    pub superseded_calls: Vec<String>,
    pub superseded_preview: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Feedback {
    Applied {
        receipt: Receipt,
    },
    Deferred {
        correction_id: String,
        turn_id: String,
    },
    Rejected {
        reason: String,
    },
}

#[derive(Clone)]
pub struct Turn(Arc<Inner>);

struct Inner {
    id: String,
    slot: Mutex<Slot>,
}

struct Slot {
    state: State,
    signal: CancellationToken,
    claimed: bool,
}

enum State {
    Open,
    Queued(Correction),
    Applying,
    Closed,
}

impl Default for Turn {
    fn default() -> Self {
        Self(Arc::new(Inner {
            id: uuid::Uuid::new_v4().to_string(),
            slot: Mutex::new(Slot {
                state: State::Open,
                signal: CancellationToken::new(),
                claimed: false,
            }),
        }))
    }
}

impl Turn {
    pub fn id(&self) -> &str {
        &self.0.id
    }

    pub fn submit(&self, input: Input) -> Result<()> {
        let mut slot = self.0.slot.lock().unwrap();
        ensure!(
            input.turn_id == self.id() && !matches!(slot.state, State::Closed),
            "Stale turn ID. Send the correction to the active Chat turn"
        );
        ensure!(
            matches!(slot.state, State::Open),
            "A correction is already pending"
        );
        ensure!(
            !input.text.trim().is_empty()
                && input.text.len() <= MAX_TEXT_BYTES
                && !input.text.contains('\0'),
            "Provide a nonempty correction up to 16 KiB"
        );
        slot.state = State::Queued(Correction {
            id: uuid::Uuid::new_v4().to_string(),
            turn_id: input.turn_id,
            received_at: crate::records::timestamp(),
            text: input.text,
            superseded_preview: None,
        });
        slot.signal.cancel();
        Ok(())
    }

    pub(crate) fn signal(&self) -> CancellationToken {
        self.0.slot.lock().unwrap().signal.clone()
    }

    pub(crate) fn claim(&self) -> Result<()> {
        let mut slot = self.0.slot.lock().unwrap();
        ensure!(
            !slot.claimed && !matches!(slot.state, State::Closed),
            "Chat turn already used"
        );
        slot.claimed = true;
        Ok(())
    }

    /// Close atomically with the final queue check so a finished turn cannot accept a correction.
    pub(crate) fn take_or_close(&self) -> Option<Correction> {
        let mut slot = self.0.slot.lock().unwrap();
        match std::mem::replace(&mut slot.state, State::Closed) {
            State::Queued(correction) => {
                slot.state = State::Applying;
                Some(correction)
            }
            _ => None,
        }
    }

    pub(crate) fn resume(&self) {
        let mut slot = self.0.slot.lock().unwrap();
        slot.signal = CancellationToken::new();
        slot.state = State::Open;
    }

    pub fn close(&self) {
        self.0.slot.lock().unwrap().state = State::Closed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrections_bind_to_one_open_turn_and_reset_at_a_boundary() {
        let active = Turn::default();
        let stale = Turn::default();
        let input = |turn: &Turn| Input {
            turn_id: turn.id().into(),
            text: "Revise scope".into(),
        };
        assert!(active.submit(input(&stale)).is_err());
        let signal = active.signal();
        assert!(!signal.is_cancelled());
        active.submit(input(&active)).unwrap();
        assert!(signal.is_cancelled());
        assert!(active.submit(input(&active)).is_err());
        let correction = active.take_or_close().unwrap();
        assert_eq!(correction.turn_id, active.id());
        assert!(active.submit(input(&active)).is_err());
        active.resume();
        assert!(!active.signal().is_cancelled());
        assert!(active.take_or_close().is_none());
        assert!(active.submit(input(&active)).is_err());
    }
}
