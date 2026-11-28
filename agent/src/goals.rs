//! Operator-defined goals and receipts produced by the local runtime.
use crate::{
    provider::Call,
    runtime::{Session, Status},
    workspace::{DRAFT_ROOTS, Workspace, hash, parse_note},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
    path::Path,
};

const QUALITY_REPORT: &str = "90_Agent/Reports/Quality-Report.md";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Criteria {
    pub objective: String,
    pub drafts: Vec<String>,
    pub require_validation: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub path: String,
    pub sha256: String,
    pub call_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Validation {
    pub report: Receipt,
    pub exit_code: i64,
    pub drafts: Vec<Receipt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Goal {
    id: String,
    criteria: Criteria,
    created_at: String,
    completed_at: Option<String>,
    drafts: BTreeMap<String, Receipt>,
    validation: Option<Validation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Active,
    Completed,
    Stale,
}

#[derive(Serialize)]
pub struct DraftProgress<'a> {
    pub path: &'a str,
    pub receipt: Option<&'a Receipt>,
    pub current: bool,
}

#[derive(Serialize)]
pub struct Report<'a> {
    pub id: &'a str,
    pub criteria: &'a Criteria,
    pub status: State,
    pub created_at: &'a str,
    pub completed_at: Option<&'a str>,
    pub drafts: Vec<DraftProgress<'a>>,
    pub validation: Option<&'a Validation>,
    pub validation_current: bool,
}

fn editable(session: &Session) -> Result<()> {
    ensure!(
        session.pending.is_empty()
            && session.draft_preview.is_none()
            && !matches!(
                session.status,
                Status::Running | Status::NeedsReconciliation
            ),
        "Finish, decline or reconcile pending work before changing the goal"
    );
    Ok(())
}

pub fn set(workspace: &Workspace, session: &mut Session, mut criteria: Criteria) -> Result<()> {
    editable(session)?;
    ensure!(
        session.goal.is_none(),
        "This session already has a goal; clear it explicitly first"
    );
    criteria.objective = criteria.objective.trim().to_owned();
    ensure!(
        !criteria.objective.is_empty()
            && criteria.objective.len() <= 2048
            && criteria
                .objective
                .chars()
                .all(|ch| !ch.is_control() || matches!(ch, '\n' | '\t')),
        "Give a text objective of 1 to 2048 bytes"
    );
    ensure!(
        (1..=16).contains(&criteria.drafts.len()),
        "Select 1 to 16 exact draft paths"
    );
    let mut unique = BTreeSet::new();
    for path in &criteria.drafts {
        let canonical = Path::new(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        ensure!(
            path == &canonical
                && !path.contains('\\')
                && path.chars().all(|ch| !ch.is_control())
                && path.ends_with(".md")
                && DRAFT_ROOTS.iter().any(|root| path.starts_with(root)),
            "Goal target must be an exact workspace-relative draft Markdown path: {path}"
        );
        ensure!(
            path.len() <= 512 && unique.insert(path),
            "Goal targets must be distinct paths of at most 512 bytes"
        );
        workspace.path(path)?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    session.history.push(json!({"role":"user","content":format!("[Application control: the operator set task goal {id}. Use its saved objective, draft targets and completion criteria in the current instructions.]" )}));
    session.goal = Some(Goal {
        id,
        criteria,
        created_at: crate::records::timestamp(),
        completed_at: None,
        drafts: BTreeMap::new(),
        validation: None,
    });
    Ok(())
}

pub fn clear(session: &mut Session) -> Result<()> {
    editable(session)?;
    ensure!(session.goal.take().is_some(), "This session has no goal");
    session.history.push(json!({"role":"user","content":"[Application control: the operator cleared the saved task goal.]"}));
    Ok(())
}

fn current(workspace: &Workspace, receipt: &Receipt) -> bool {
    workspace
        .read(&receipt.path)
        .is_ok_and(|text| hash(text.as_bytes()) == receipt.sha256)
}

impl Goal {
    pub fn is_active(&self) -> bool {
        self.completed_at.is_none()
    }

    pub(crate) fn allows_draft(&self, path: &str) -> bool {
        !self.is_active() || self.criteria.drafts.iter().any(|target| target == path)
    }

    fn current_drafts(&self, workspace: &Workspace) -> Option<Vec<Receipt>> {
        self.criteria
            .drafts
            .iter()
            .map(|path| {
                self.drafts
                    .get(path)
                    .filter(|receipt| {
                        workspace.read(path).is_ok_and(|text| {
                            hash(text.as_bytes()) == receipt.sha256
                                && parse_note(&text)
                                    .is_ok_and(|(meta, _)| meta["doc_status"] == "draft")
                        })
                    })
                    .cloned()
            })
            .collect()
    }

    pub(crate) fn validation_inputs(
        &self,
        workspace: &Workspace,
        call: &Call,
        args: &Value,
    ) -> Option<Vec<Receipt>> {
        if self.is_active()
            && self.criteria.require_validation
            && call.name == "pm"
            && args["command"] == "validate"
        {
            self.current_drafts(workspace)
        } else {
            None
        }
    }

    /// Record only a completed native write or a validator run bound to the saved drafts.
    pub(crate) fn observe(
        &mut self,
        workspace: &Workspace,
        call: &Call,
        args: &Value,
        result: &Value,
        validation_inputs: Option<Vec<Receipt>>,
    ) -> bool {
        if !self.is_active() {
            return false;
        }
        if call.name == "write_draft"
            && result["status"] == "draft_saved"
            && let (Some(path), Some(sha256)) = (args["path"].as_str(), result["sha256"].as_str())
            && self.allows_draft(path)
        {
            self.drafts.insert(
                path.into(),
                Receipt {
                    path: path.into(),
                    sha256: sha256.into(),
                    call_id: call.id.clone(),
                },
            );
            return true;
        }
        let Some(drafts) = validation_inputs else {
            return false;
        };
        self.validation = (|| {
            if self.current_drafts(workspace)? != drafts || result["engine"] != "rust" {
                return None;
            }
            let exit_code = result["exit_code"].as_i64()?;
            if (exit_code == 0) != (result["successful"] == true) {
                return None;
            }
            let report = result["reports"]
                .as_array()?
                .iter()
                .find(|r| r["path"] == QUALITY_REPORT)?;
            let report = Receipt {
                path: QUALITY_REPORT.into(),
                sha256: report["sha256"].as_str()?.into(),
                call_id: call.id.clone(),
            };
            current(workspace, &report).then_some(Validation {
                report,
                exit_code,
                drafts,
            })
        })();
        true
    }

    pub(crate) fn evidence_complete(&self, workspace: &Workspace) -> bool {
        self.is_active() && self.report(workspace).fulfilled(&self.criteria)
    }

    pub(crate) fn blocked_questions(&self, workspace: &Workspace) -> Result<BTreeSet<String>> {
        let mut questions = BTreeSet::new();
        for path in &self.criteria.drafts {
            questions.extend(
                crate::clarification::blockers(workspace, path, None, &[])?
                    .into_iter()
                    .map(|blocker| blocker.path),
            );
        }
        Ok(questions)
    }

    pub(crate) fn record_completion(&mut self) {
        self.completed_at = Some(crate::records::timestamp());
    }

    pub fn report(&self, workspace: &Workspace) -> Report<'_> {
        let drafts: Vec<_> = self
            .criteria
            .drafts
            .iter()
            .map(|path| {
                let receipt = self.drafts.get(path);
                DraftProgress {
                    path,
                    receipt,
                    current: receipt.is_some_and(|r| current(workspace, r)),
                }
            })
            .collect();
        let validation_current = self.validation.as_ref().is_some_and(|check| {
            current(workspace, &check.report)
                && drafts
                    .iter()
                    .map(|draft| draft.receipt.filter(|_| draft.current))
                    .eq(check.drafts.iter().map(Some))
        });
        let mut report = Report {
            id: &self.id,
            criteria: &self.criteria,
            status: State::Active,
            created_at: &self.created_at,
            completed_at: self.completed_at.as_deref(),
            drafts,
            validation: self.validation.as_ref(),
            validation_current,
        };
        if self.completed_at.is_some() {
            report.status = if report.fulfilled(&self.criteria) {
                State::Completed
            } else {
                State::Stale
            };
        }
        report
    }
}

impl Report<'_> {
    fn fulfilled(&self, criteria: &Criteria) -> bool {
        self.drafts.iter().all(|draft| draft.current)
            && (!criteria.require_validation
                || self.validation_current
                    && self.validation.is_some_and(|check| check.exit_code == 0))
    }

    pub fn markdown(&self) -> String {
        let objective = self
            .criteria
            .objective
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let mut text = format!("Goal {} · {:?}\n\n{objective}\n\n", self.id, self.status);
        for draft in &self.drafts {
            let _ = write!(text, "- Draft `{}`", draft.path);
            match draft.receipt {
                Some(receipt) => {
                    let _ = writeln!(
                        text,
                        " · {} · SHA256 `{}` · tool `{}`",
                        if draft.current { "current" } else { "stale" },
                        receipt.sha256,
                        receipt.call_id
                    );
                }
                None => text.push_str(" · pending\n"),
            }
        }
        if self.criteria.require_validation {
            match self.validation {
                Some(check) => {
                    let _ = writeln!(
                        text,
                        "- `pm validate` exit {} · {} report `{}` · SHA256 `{}` · tool `{}`",
                        check.exit_code,
                        if self.validation_current {
                            "current"
                        } else {
                            "stale"
                        },
                        check.report.path,
                        check.report.sha256,
                        check.report.call_id
                    );
                }
                None => text.push_str("- `pm validate` · pending\n"),
            }
        }
        if let Some(at) = self.completed_at {
            let _ = writeln!(text, "- Completion recorded at {at}");
        }
        text
    }
}
