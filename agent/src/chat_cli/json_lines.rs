use anyhow::Result;
use pm_agent::{
    diagnostics,
    execution::{Phase, Status, ToolState},
    goals,
    provider::{Call, Event, Usage},
    runtime::Session,
    workspace::Workspace,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Write},
};

#[derive(Serialize)]
struct Envelope<'a> {
    schema_version: u32,
    session_id: Option<&'a str>,
    turn_id: &'a str,
    sequence: u64,
    #[serde(flatten)]
    event: Line<'a>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum Line<'a> {
    #[serde(rename = "turn.steering")]
    Steering {
        #[serde(flatten)]
        feedback: &'a pm_agent::steering::Feedback,
    },
    #[serde(rename = "turn.started")]
    Started { stage: Option<&'a str> },
    #[serde(rename = "turn.state_changed")]
    StateChanged { status: Status },
    #[serde(rename = "turn.progress")]
    Progress { phase: Phase, message: &'a str },
    #[serde(rename = "assistant.text.delta")]
    Text { text: &'a str },
    #[serde(rename = "usage.updated")]
    Usage { usage: &'a Option<Usage> },
    #[serde(rename = "tool.updated")]
    Tool {
        call_id: &'a str,
        tool: &'a str,
        #[serde(flatten)]
        state: ToolState,
        exit_code: Option<i64>,
        artifacts: &'a [Artifact],
    },
    #[serde(rename = "turn.finished")]
    Finished {
        status: Status,
        run: Option<&'a str>,
        pending: Option<Pending<'a>>,
        artifacts: &'a [Artifact],
        diagnostic: Option<&'a str>,
        goal: Option<goals::Report<'a>>,
    },
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    Draft,
    Question,
    Report,
    Run,
    Baseline,
}

#[derive(Clone, Serialize)]
struct Artifact {
    kind: ArtifactKind,
    path: String,
    sha256: Option<String>,
}

impl Artifact {
    fn available(
        w: &Workspace,
        kind: ArtifactKind,
        path: Option<&str>,
        sha256: Option<&str>,
    ) -> Option<Self> {
        let path = path?;
        w.path(path).ok()?.is_file().then(|| Self {
            kind,
            path: path.into(),
            sha256: sha256
                .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_owned),
        })
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Pending<'a> {
    Approval {
        call: Option<PendingCall<'a>>,
    },
    DraftReview {
        preview_id: &'a str,
        path: &'a str,
        text: &'a str,
    },
    Clarification {
        questions: Vec<&'a str>,
    },
    Reconciliation {
        call: Option<PendingCall<'a>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        deferred_correction_id: Option<&'a str>,
    },
}

#[derive(Serialize)]
struct PendingCall<'a> {
    call_id: &'a str,
    tool: &'a str,
}

fn pending(session: &Session) -> Option<Pending<'_>> {
    let call = || {
        session.pending.first().map(|call| PendingCall {
            call_id: &call.id,
            tool: &call.name,
        })
    };
    match session.status {
        Status::WaitingApproval => Some(Pending::Approval { call: call() }),
        Status::WaitingDraftReview => {
            session
                .draft_preview
                .as_ref()
                .map(|draft| Pending::DraftReview {
                    preview_id: &draft.id,
                    path: &draft.path,
                    text: &draft.text,
                })
        }
        Status::WaitingClarification => Some(Pending::Clarification {
            questions: session
                .questions
                .iter()
                .filter(|q| !q.resolved)
                .map(|q| q.path.as_str())
                .chain(session.blocked_questions.iter().map(String::as_str))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        }),
        Status::NeedsReconciliation => Some(Pending::Reconciliation {
            call: call(),
            deferred_correction_id: session
                .deferred_steering
                .as_ref()
                .map(|correction| correction.id.as_str()),
        }),
        _ => None,
    }
}

pub(super) struct Stream {
    output: io::Stdout,
    session_id: Option<String>,
    turn_id: String,
    sequence: u64,
    artifacts: BTreeMap<String, Artifact>,
}

impl Stream {
    pub(super) fn new(turn_id: &str) -> Self {
        Self {
            output: io::stdout(),
            session_id: None,
            turn_id: turn_id.into(),
            sequence: 0,
            artifacts: BTreeMap::new(),
        }
    }

    fn emit(&mut self, event: Line<'_>) -> Result<()> {
        self.sequence += 1;
        let mut output = self.output.lock();
        serde_json::to_writer(
            &mut output,
            &Envelope {
                schema_version: 1,
                session_id: self.session_id.as_deref(),
                turn_id: &self.turn_id,
                sequence: self.sequence,
                event,
            },
        )?;
        output.write_all(b"\n")?;
        output.flush()?;
        Ok(())
    }

    pub(super) fn start(&mut self, session: &Session) -> Result<()> {
        self.session_id = Some(session.id.clone());
        self.emit(Line::Started {
            stage: Some(&session.stage),
        })
    }

    pub(super) fn event(&mut self, w: &Workspace, event: &Event) -> Result<()> {
        match event {
            Event::Steering(feedback) => self.emit(Line::Steering { feedback }),
            Event::Execution(status) => self.emit(Line::StateChanged { status: *status }),
            Event::Progress { phase, message } => self.emit(Line::Progress {
                phase: *phase,
                message,
            }),
            Event::Text(text) => self.emit(Line::Text { text }),
            Event::Usage(usage) => self.emit(Line::Usage { usage }),
            Event::Tool {
                call,
                state,
                result,
            } => {
                let value = match (call.name.as_str(), result.as_deref()) {
                    ("pm" | "write_draft" | "ask_clarification", Some(result)) => {
                        serde_json::from_str::<Value>(result).unwrap_or_default()
                    }
                    _ => Value::Null,
                };
                let artifacts = tool_artifacts(w, call, &value);
                self.artifacts
                    .extend(artifacts.iter().map(|a| (a.path.clone(), a.clone())));
                self.emit(Line::Tool {
                    call_id: &call.id,
                    tool: &call.name,
                    state: *state,
                    exit_code: (call.name == "pm")
                        .then(|| value["exit_code"].as_i64())
                        .flatten(),
                    artifacts: &artifacts,
                })
            }
            Event::InputAccepted(_) | Event::Reasoning(_) | Event::Notice(_) => Ok(()),
        }
    }

    pub(super) fn finish(&mut self, w: &Workspace, session: &Session) -> Result<()> {
        let path = session.run_path();
        let run = w
            .path(&path)
            .is_ok_and(|p| p.is_file())
            .then_some(path.as_str());
        let pending = pending(session);
        let mut artifacts = std::mem::take(&mut self.artifacts);
        if let Some(Pending::Clarification { questions }) = &pending {
            for path in questions {
                if let Some(question) =
                    Artifact::available(w, ArtifactKind::Question, Some(path), None)
                {
                    artifacts.entry(question.path.clone()).or_insert(question);
                }
            }
        }
        let artifacts: Vec<_> = artifacts
            .into_values()
            .filter(|artifact| w.path(&artifact.path).is_ok_and(|p| p.is_file()))
            .collect();
        self.emit(Line::Finished {
            status: session.status,
            run,
            pending,
            artifacts: &artifacts,
            diagnostic: session.error.as_deref(),
            goal: session.goal.as_ref().map(|goal| goal.report(w)),
        })
    }

    pub(super) fn error(&mut self, error: &anyhow::Error) -> Result<()> {
        if self.sequence == 0 {
            self.emit(Line::Started { stage: None })?;
        }
        let artifacts: Vec<_> = std::mem::take(&mut self.artifacts).into_values().collect();
        self.emit(Line::Finished {
            status: Status::Failed,
            run: None,
            pending: None,
            artifacts: &artifacts,
            diagnostic: Some(&diagnostics::format_error(error)),
            goal: None,
        })
    }
}

fn tool_artifacts(w: &Workspace, call: &Call, result: &Value) -> Vec<Artifact> {
    let mut artifacts = vec![];
    let mut add = |kind, path, hash| artifacts.extend(Artifact::available(w, kind, path, hash));
    match call.name.as_str() {
        "write_draft" if result["status"] == "draft_saved" => {
            let args = serde_json::from_str::<Value>(&call.arguments).unwrap_or_default();
            add(
                ArtifactKind::Draft,
                args["path"].as_str(),
                result["sha256"].as_str(),
            );
        }
        "ask_clarification" | "write_draft"
            if matches!(
                result["status"].as_str(),
                Some("waiting_for_user" | "blocked_by_grounding")
            ) =>
        {
            add(
                ArtifactKind::Question,
                result["question_path"].as_str(),
                None,
            );
        }
        "pm" => {
            for report in result["reports"].as_array().into_iter().flatten() {
                let path = report["path"].as_str();
                let kind = if path.is_some_and(|p| p.starts_with("90_Agent/Runs/")) {
                    ArtifactKind::Run
                } else {
                    ArtifactKind::Report
                };
                add(kind, path, report["sha256"].as_str());
            }
            add(
                ArtifactKind::Baseline,
                result["manifest"].as_str(),
                result["manifest_sha256"].as_str(),
            );
        }
        _ => {}
    }
    artifacts
}
