//! Durable session guards. Reported usage is distinct from context and cost.
use crate::{provider::Usage, runtime::Session};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_requests: Option<u64>,
    pub max_total_tokens: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    pub prior_requests_unknown: bool,
    pub requests: Vec<Attempt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Started,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub started_at: String,
    pub finished_at: Option<String>,
    pub state: State,
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub recorded_attempts: u64,
    pub prior_requests_unknown: bool,
    pub attempts_without_total: u64,
    pub reported_total_sum: Option<u64>,
    pub complete_total_tokens: Option<u64>,
}

impl Ledger {
    pub fn begin(&mut self) {
        self.requests.push(Attempt {
            started_at: crate::records::timestamp(),
            finished_at: None,
            state: State::Started,
            usage: None,
        });
    }
    pub fn finish(&mut self, state: State, usage: Option<Usage>) {
        let attempt = self.requests.last_mut().expect("Begin before finishing");
        attempt.finished_at = Some(crate::records::timestamp());
        attempt.state = state;
        attempt.usage = usage;
    }
}

pub fn summary(ledger: Option<&Ledger>) -> Summary {
    let prior_requests_unknown = ledger.is_none_or(|l| l.prior_requests_unknown);
    let requests = ledger.map(|l| l.requests.as_slice()).unwrap_or_default();
    let attempts_without_total = requests
        .iter()
        .filter(|r| r.usage.as_ref().and_then(|u| u.total_tokens).is_none())
        .count() as u64;
    let reported_total_sum = requests
        .iter()
        .filter_map(|r| r.usage.as_ref().and_then(|u| u.total_tokens))
        .try_fold(0u64, |sum, n| sum.checked_add(n));
    Summary {
        recorded_attempts: requests.len() as u64,
        prior_requests_unknown,
        attempts_without_total,
        reported_total_sum,
        complete_total_tokens: if prior_requests_unknown || attempts_without_total > 0 {
            None
        } else {
            reported_total_sum
        },
    }
}

pub fn blocked(limits: &Limits, ledger: Option<&Ledger>) -> Option<String> {
    let s = summary(ledger);
    if let Some(limit) = limits.max_requests {
        if s.prior_requests_unknown {
            return Some("Earlier session request counts are unavailable; the request limit cannot be checked.".into());
        }
        if s.recorded_attempts >= limit {
            return Some(format!(
                "Session request limit reached: {} / {limit} attempts.",
                s.recorded_attempts
            ));
        }
    }
    if let Some(limit) = limits.max_total_tokens {
        let Some(total) = s.complete_total_tokens else {
            return Some("Complete service-reported totals are unavailable (legacy, missing, interrupted or overflowing usage); the token limit cannot be checked.".into());
        };
        if total >= limit {
            return Some(format!(
                "Session reported-token limit reached: {total} / {limit}."
            ));
        }
    }
    None
}

#[derive(Serialize)]
pub struct Report {
    pub limits: Limits,
    pub session_id: Option<String>,
    pub observations: Option<Summary>,
    pub pause_reason: Option<String>,
}

impl Report {
    pub fn new(limits: &Limits, session: Option<&Session>) -> Self {
        Self {
            limits: limits.clone(),
            session_id: session.map(|s| s.id.clone()),
            observations: session.map(|s| summary(s.request_ledger.as_ref())),
            pause_reason: session.and_then(|s| blocked(limits, s.request_ledger.as_ref())),
        }
    }
    pub fn markdown(&self) -> String {
        let value = |n: Option<u64>| n.map(|n| n.to_string()).unwrap_or("not set".into());
        let mut text = format!(
            "# Session limits\n\n- Session: {}\n- Request limit: {}\n- Reported total-token limit: {}\n",
            self.session_id.as_deref().unwrap_or("none"),
            value(self.limits.max_requests),
            value(self.limits.max_total_tokens),
        );
        if let Some(s) = &self.observations {
            text.push_str(&format!(
                "- Recorded request attempts: {}\n- Earlier requests unknown: {}\n- Attempts without a reported total: {}\n- Sum of available service totals: {}\n- Complete session total: {}\n",
                s.recorded_attempts, s.prior_requests_unknown, s.attempts_without_total,
                s.reported_total_sum.map(|n| n.to_string()).unwrap_or("unavailable (overflow)".into()),
                s.complete_total_tokens.map(|n| n.to_string()).unwrap_or("unavailable".into()),
            ));
        }
        text.push_str(&format!(
            "\n{}\n\nLimits apply across explicit resumes of this session. They pause before the next model request or pending tool. A request already sent can exceed the token threshold. Missing totals are never derived from input/output counts. These values are not context capacity, billing or a hard cost cap.\n\nEdit operator configuration and explicitly continue after reviewing the pause. Limits never reset on resume; a new session has separate accounting. Legacy history cannot be reconstructed. Probes and direct PM commands are separate operations.\n",
            if self.session_id.is_none() {
                "No session loaded; usage observations and pause evaluation are unavailable."
            } else {
                self.pause_reason.as_deref().unwrap_or("No configured limit currently requires a pause.")
            },
        ));
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budget_totals_never_infer_missing_values_and_preserve_zero() {
        let mut ledger = Ledger::default();
        assert_eq!(summary(Some(&ledger)).complete_total_tokens, Some(0));
        ledger.begin();
        ledger.finish(
            State::Completed,
            Some(Usage {
                input_tokens: Some(30),
                output_tokens: Some(2),
                total_tokens: None,
            }),
        );
        assert_eq!(summary(Some(&ledger)).complete_total_tokens, None);
        assert!(
            blocked(
                &Limits {
                    max_requests: None,
                    max_total_tokens: Some(100)
                },
                Some(&ledger)
            )
            .is_some()
        );
        ledger.begin();
        ledger.finish(
            State::Completed,
            Some(Usage {
                input_tokens: Some(2),
                output_tokens: Some(5),
                total_tokens: Some(0),
            }),
        );
        let s = summary(Some(&ledger));
        assert_eq!(s.reported_total_sum, Some(0));
        assert_eq!(s.attempts_without_total, 1);
        assert_eq!(s.recorded_attempts, 2);
        assert_eq!(
            ledger.requests[1].usage.as_ref().unwrap().total_tokens,
            Some(0)
        );
    }
    #[test]
    fn budget_legacy_interrupted_and_overflowing_totals_are_unknown() {
        let limits = Limits {
            max_requests: None,
            max_total_tokens: Some(20),
        };
        assert!(blocked(&limits, None).is_some());
        assert!(blocked(&Limits::default(), None).is_none());
        let mut ledger = Ledger::default();
        ledger.begin();
        assert!(blocked(&limits, Some(&ledger)).is_some());
        assert_eq!(summary(Some(&ledger)).recorded_attempts, 1);
        ledger.finish(
            State::Completed,
            Some(Usage {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(u64::MAX),
            }),
        );
        ledger.begin();
        ledger.finish(
            State::Completed,
            Some(Usage {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(1),
            }),
        );
        assert_eq!(summary(Some(&ledger)).reported_total_sum, None);
        assert!(blocked(&limits, Some(&ledger)).is_some());
    }
    #[test]
    fn budget_limits_are_optional_zero_is_explicit_and_typos_are_rejected() {
        let limits: Limits = toml::from_str("").unwrap();
        assert_eq!(limits, Limits::default());
        let zero: Limits = toml::from_str("max_requests = 0").unwrap();
        assert!(blocked(&zero, Some(&Ledger::default())).is_some());
        assert!(toml::from_str::<Limits>("max_requests = -1").is_err());
        assert!(toml::from_str::<Limits>("max_tokens = 10").is_err());
        assert!(
            Report::new(&zero, None)
                .markdown()
                .contains("No session loaded")
        );
    }
}
