//! Stage-scoped evaluation orchestration. This is never an interactive limit.
use super::*;
use crate::{records::Records, store::CallRecord};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub stage: String,
    pub prompt: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub id: String,
    pub session_id: String,
    pub status: Status,
    pub outcome: Outcome,
    pub checks: Vec<Check>,
    pub usage: crate::budget::Summary,
}

pub const KINDS: &[&str] = &["glossary", "brd", "prd", "requirement", "pbi"];

pub fn tasks() -> Vec<Task> {
    [
        ("glossary", "language", "Create only the minimal Access bounded-context glossary supported by the interview and captured Wiki/code. Preserve canonical meanings and distinguish implementation observations from business needs."),
        ("brd", "brd", "Read the actual glossary from the previous stage. Create only a BRD for the supplied narrow duration-validation slice. Preserve the reported problem, exclusions, source evidence and unknown benefits or ownership."),
        ("prd", "prd", "Read the actual glossary and BRD. Create only a PRD for the supplied duration-validation slice, linking its actual BRD ID. Describe observable product behavior without inventing additional scope or thresholds."),
        ("requirement", "prd", "Read the actual glossary, BRD and PRD. Create only the atomic duration-validation requirement, linking its actual PRD ID and preserving traceable evidence and testable conditions."),
        ("pbi", "backlog", "Read the actual glossary, PRD and requirement. Create only an unscheduled PBI satisfying the actual requirement ID and sharing its PRD parent. Include testable acceptance criteria. Leave estimates, staffing, dates and owner decisions unknown."),
        ("verification", "review", "Inspect the actual glossary, BRD, PRD, requirement and PBI. Run validate and coverage after any supported corrections and report all remaining gaps. Do not invent missing decisions, test results, approvals or readiness."),
    ].into_iter().map(|(id, stage, task)| Task {
        id:id.into(), stage:stage.into(),
        prompt:format!("Synthetic staged evaluation only. This stage operates on the actual files retained from earlier stages; no earlier result is implied to be approved. Read Settings and the supplied interview at 01_Sources/Assets/interview.txt. Discover actual snapshot paths with list_source_files and existing draft IDs with list_documents. Use the configured document language. Do not contact external tools, change configuration or create later-stage drafts. {task} Run validate and report findings without inventing missing information. Finish this stage when its requested artifacts and checks are saved."),
    }).collect()
}

pub fn validate_plan(m: &Manifest) -> Result<()> {
    if m.case.id == "draft-stages" {
        ensure!(
            m.fixture_version >= 3,
            "Staged evaluations require fixture version 3"
        );
        let expected = tasks();
        ensure!(
            m.stages.len() == expected.len()
                && m.stages.iter().zip(expected).all(|(a, b)| a.id == b.id
                    && a.stage == b.stage
                    && !a.prompt.trim().is_empty()
                    && a.prompt.len() <= 32 * 1024),
            "Invalid staged evaluation plan"
        );
    } else {
        ensure!(m.stages.is_empty(), "This case has no staged plan");
    }
    Ok(())
}

pub fn observe(
    root: &Path,
    m: &Manifest,
    task: &Task,
    session: &Session,
    all: &[CallRecord],
    current: &[CallRecord],
) -> Result<Observation> {
    let full = checks::evaluate(root, m, session, all)?;
    let mut selected: Vec<_> = if task.id == "verification" {
        full
    } else {
        let created = format!("created_{}", task.id);
        full.into_iter()
            .filter(|c| {
                c.id == created
                    || matches!(
                        c.id.as_str(),
                        "preserved_inputs"
                            | "source_inspection"
                            | "draft_state_and_language"
                            | "tool_outcomes_known"
                            | "execution_boundary"
                            | "execution_failed"
                            | "completed_turn"
                            | "versioned_draft_citations"
                            | "source_review_before_write"
                            | "validation_findings"
                    )
            })
            .collect()
    };
    let w = Workspace::open(&root.join("vault"))?;
    let r = Records::load(&w)?;
    let last_write = current.iter().rposition(|c| c.name == "write_draft");
    for command in if task.id == "verification" {
        &["validate", "coverage"][..]
    } else {
        &["validate"][..]
    } {
        let observed = current.iter().enumerate().any(|(i, c)| {
            if c.name != "pm"
                || c.status != "completed"
                || last_write.is_some_and(|write| i <= write)
            {
                return false;
            }
            let (Ok(a), Some(Ok(result))) = (
                serde_json::from_str::<Value>(&c.arguments),
                c.result.as_ref().map(|v| serde_json::from_str::<Value>(v)),
            ) else {
                return false;
            };
            a["command"] == *command
                && if *command == "validate" {
                    result["errors"].is_number()
                } else {
                    result["successful"] == true
                }
        });
        selected.push(Check { id:format!("stage_{command}"),outcome:if observed {Outcome::Pass}else if session.status==Status::Completed {Outcome::Fail}else {Outcome::NotRun},evidence:format!("This stage must execute {command} after its final draft write; an earlier stage's check is insufficient.") });
    }
    if let Some(position) = KINDS.iter().position(|k| *k == task.id) {
        let future = r.active.iter().any(|d| {
            KINDS[position + 1..].contains(&d.kind.as_str())
                && !m.inputs.contains_key(&format!("vault/{}", d.path))
        });
        selected.push(Check {
            id: "stage_scope".into(),
            outcome: if future { Outcome::Fail } else { Outcome::Pass },
            evidence: "Later-stage drafts must not be produced before their stage.".into(),
        });
        let wrote = current.iter().any(|call| {
            if call.name != "write_draft" || call.status != "completed" {
                return false;
            }
            let (Ok(a), Some(Ok(result))) = (
                serde_json::from_str::<Value>(&call.arguments),
                call.result
                    .as_ref()
                    .map(|v| serde_json::from_str::<Value>(v)),
            ) else {
                return false;
            };
            result["status"] == "draft_saved"
                && r.active
                    .iter()
                    .any(|d| d.kind == task.id && a["path"] == d.path)
        });
        selected.push(Check {
            id: "stage_write".into(),
            outcome: if wrote {
                Outcome::Pass
            } else if session.status == Status::Completed {
                Outcome::Fail
            } else {
                Outcome::NotRun
            },
            evidence: "The selected stage must actually write its requested document type.".into(),
        });
    }
    Ok(Observation {
        id: task.id.clone(),
        session_id: session.id.clone(),
        status: session.status.clone(),
        outcome: outcome(&selected),
        checks: selected,
        usage: crate::budget::summary(session.request_ledger.as_ref()),
    })
}

pub fn outcome(checks: &[Check]) -> Outcome {
    if checks.iter().any(|c| c.outcome == Outcome::Fail) {
        Outcome::Fail
    } else if checks.iter().any(|c| c.outcome != Outcome::Pass) {
        Outcome::Partial
    } else {
        Outcome::Pass
    }
}

pub fn can_advance(observation: &Observation) -> bool {
    observation.status == Status::Completed
        && observation.checks.iter().all(|c| {
            c.outcome == Outcome::Pass
                || c.id == "validation_findings" && c.outcome == Outcome::Partial
        })
}

pub fn save_evidence(
    root: &Path,
    task: &Task,
    session: &Session,
    calls: &[CallRecord],
    report: &mut Report,
) -> Result<()> {
    let w = Workspace::open(&root.join("vault"))?;
    let records = Records::load(&w)?;
    let documents: BTreeMap<_, _> = records
        .active
        .iter()
        .filter(|d| KINDS.contains(&d.kind.as_str()) && !d.path.starts_with("80_Templates/"))
        .map(|d| Ok((d.path.clone(), w.read(&d.path)?)))
        .collect::<Result<_>>()?;
    let files = Workspace {
        root: root.to_owned(),
    };
    for (name, bytes) in [
        ("Session.json", serde_json::to_vec_pretty(session)?),
        ("Actions.json", serde_json::to_vec_pretty(calls)?),
        ("Documents.json", serde_json::to_vec_pretty(&documents)?),
    ] {
        ensure!(
            bytes.len() as u64 <= MAX_BYTES,
            "Stage evidence exceeds inspection bound"
        );
        let path = format!("stages/{}/{name}", task.id);
        put(&files, &path, &bytes)?;
        report.evidence_files.insert(path, hash(&bytes));
    }
    Ok(())
}
