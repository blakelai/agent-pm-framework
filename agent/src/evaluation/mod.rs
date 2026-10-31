//! Operator-owned, isolated synthetic evaluations of the real Agent runtime.
//! This is not an Agent tool and never grants approval or business acceptance.
mod checks;
mod fixtures;
mod stages;

use crate::{
    config::Config,
    runtime::{Runtime, Session, Status},
    workspace::{Workspace, WorkspaceLock, atomic_write, hash, note},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

pub use fixtures::cases;
const FORMAT: &str = "pm-agent-evaluation-v1";
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub id: String,
    pub title: String,
    pub stage: String,
    pub purpose: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub case: Case,
    pub fixture_version: u32,
    pub application_version: String,
    pub runner_sha256: String,
    pub synthetic: bool,
    pub language: String,
    pub created_at: String,
    pub prompt: String,
    pub inputs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<stages::Task>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Pass,
    Partial,
    Fail,
    NotRun,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub id: String,
    pub outcome: Outcome,
    pub evidence: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub format: String,
    pub manifest_sha256: String,
    pub case_id: String,
    pub synthetic: bool,
    pub state: String,
    pub session_id: Option<String>,
    pub session_status: Option<Status>,
    pub configured_provider: Option<String>,
    pub configured_model: Option<String>,
    pub provider_fingerprint: Option<String>,
    pub max_steps: Option<usize>,
    pub provider_timeout_seconds: Option<u64>,
    pub max_total_tokens: Option<u64>,
    pub max_requests: Option<u64>,
    pub timeout_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits_scope: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub mechanical_outcome: Outcome,
    pub semantic_review: Outcome,
    pub checks: Vec<Check>,
    pub usage: Option<crate::budget::Summary>,
    pub outputs: BTreeMap<String, String>,
    pub evidence_files: BTreeMap<String, String>,
    pub note: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stages: Vec<stages::Observation>,
}
#[derive(Debug, Serialize)]
pub struct Inspection {
    pub directory: PathBuf,
    pub current: bool,
    pub report: Report,
    pub changes: Vec<String>,
}

fn put(w: &Workspace, path: &str, bytes: &[u8]) -> Result<()> {
    atomic_write(&w.path(path)?, bytes)
}
fn runner_hash() -> Result<String> {
    let path = std::env::current_exe()?;
    ensure!(
        fs::metadata(&path)?.len() <= 512 * 1024 * 1024,
        "Evaluation executable exceeds fingerprint bound"
    );
    Ok(hash(&fs::read(path)?))
}
fn put_note(w: &Workspace, path: &str, meta: Value, body: &str) -> Result<()> {
    put(w, path, note(meta, body)?.as_bytes())
}
fn read(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let w = Workspace {
        root: root.to_owned(),
    };
    let path = w.path(relative)?;
    let meta = fs::symlink_metadata(&path)?;
    ensure!(
        meta.is_file() && meta.len() <= MAX_BYTES,
        "Expected bounded regular evaluation file: {relative}"
    );
    Ok(fs::read(path)?)
}
fn inventory(root: &Path) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    let mut bytes = 0;
    for directory in ["vault", "repository"] {
        for entry in walkdir::WalkDir::new(root.join(directory))
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| e.file_name() != ".pm-agent")
        {
            let entry = entry?;
            if entry.file_type().is_dir() {
                continue;
            }
            let path = entry
                .path()
                .strip_prefix(root)?
                .to_str()
                .context("Non-UTF-8 evaluation path")?
                .to_owned();
            let content = read(root, &path)?;
            bytes += content.len();
            ensure!(
                files.len() < 2000 && bytes <= 32 * 1024 * 1024,
                "Evaluation evidence exceeds bounded inventory"
            );
            files.insert(path, hash(&content));
        }
    }
    Ok(files)
}
fn manifest(root: &Path) -> Result<(Manifest, String)> {
    let bytes = read(root, "manifest.json")?;
    let m: Manifest = serde_json::from_slice(&bytes)?;
    ensure!(
        m.format == FORMAT && m.synthetic && matches!(m.fixture_version, 1..=4),
        "Unsupported evaluation manifest"
    );
    ensure!(
        cases()
            .iter()
            .any(|c| c.id == m.case.id && c.stage == m.case.stage),
        "Unknown evaluation case or stage"
    );
    ensure!(
        matches!(m.language.as_str(), "en" | "zh-TW"),
        "Unsupported evaluation language"
    );
    stages::validate_plan(&m)?;
    Ok((m, hash(&bytes)))
}
fn save(root: &Path, report: &Report) -> Result<()> {
    let w = Workspace {
        root: root.to_owned(),
    };
    let mut body = format!(
        "# Synthetic Agent evaluation\n\nCase: `{}`. State: `{}`. Mechanical outcome: `{:?}`. Semantic review: `{:?}`.\n\n{}\n\n## Mechanical observations\n\n| Check | Outcome | Evidence |\n|---|---|---|\n",
        report.case_id,
        report.state,
        report.mechanical_outcome,
        report.semantic_review,
        report.note
    );
    for check in &report.checks {
        body.push_str(&format!(
            "| {} | {:?} | {} |\n",
            check.id,
            check.outcome,
            check
                .evidence
                .replace('|', "\\|")
                .replace(['\n', '\r'], " ")
        ));
    }
    if !report.stages.is_empty() {
        body.push_str("\n## Stage observations\n\nEach stage uses a fresh session and the actual files left by the previous stage. Request and timeout limits apply separately to each stage. A pause or failed stage contract stops the workflow; later stages remain unperformed.\n\n| Stage | Status | Outcome | Requests | Evidence |\n|---|---|---|---:|---|\n");
        for stage in &report.stages {
            body.push_str(&format!("| {} | {:?} | {:?} | {} | [Session](stages/{}/Session.json), [Actions](stages/{}/Actions.json), [Documents](stages/{}/Documents.json) |\n",stage.id,stage.status,stage.outcome,stage.usage.recorded_attempts,stage.id,stage.id,stage.id));
        }
    }
    body.push_str("\n## Human review remains unperformed\n\nReview source-to-claim fidelity, terminology and context boundaries, missing-information sufficiency, actual prose language, acceptance criteria, and unsupported business assertions. Mechanical checks cannot establish these outcomes. Record actual reviewer, time, scope and evidence separately; do not infer approval.\n\n[Task](Task.md) · [Manifest](manifest.json) · [Structured report](report.json)\n\nUse `pm-agent eval inspect DIRECTORY` to check freshness after external edits. These are synthetic observations, not real-project acceptance. Raw session and tool evidence can contain private instructions or service responses; review before sharing. A prepared or interrupted run has no completed outcome.\n");
    if report.evidence_files.contains_key("Session.json") {
        body.push_str(
            "\n[Private raw session](Session.json) · [Private tool evidence](Actions.json)\n",
        );
    }
    put_note(
        &w,
        "Report.md",
        json!({"type":"Evaluation","title":"Synthetic Agent evaluation","language":"en"}),
        &body,
    )?;
    let mut report = report.clone();
    report
        .evidence_files
        .insert("Report.md".into(), hash(&read(root, "Report.md")?));
    put(&w, "report.json", &serde_json::to_vec_pretty(&report)?)
}

pub async fn prepare(
    source: &Workspace,
    case: &str,
    output_parent: &Path,
    language: &str,
) -> Result<PathBuf> {
    let (source, case, output, language) = (
        source.clone(),
        case.to_owned(),
        output_parent.to_owned(),
        language.to_owned(),
    );
    tokio::task::spawn_blocking(move || prepare_local(&source, &case, &output, &language)).await?
}

fn prepare_local(
    source: &Workspace,
    case: &str,
    output_parent: &Path,
    language: &str,
) -> Result<PathBuf> {
    let case = cases()
        .into_iter()
        .find(|c| c.id == case)
        .context("Unknown evaluation case; use eval list")?;
    ensure!(
        matches!(language, "en" | "zh-TW"),
        "Evaluation languages are en and zh-TW"
    );
    let parent = output_parent
        .canonicalize()
        .context("Evaluation output parent must exist")?;
    ensure!(
        parent.is_dir() && !parent.starts_with(&source.root),
        "Evaluation output must be outside the seed Vault"
    );
    let dir = tempfile::Builder::new()
        .prefix("pm-eval-")
        .tempdir_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    }
    let w = Workspace {
        root: dir.path().join("vault"),
    };
    fs::create_dir(&w.root)?;
    fixtures::seed(source, &w)?;
    let prompt = fixtures::populate(dir.path(), &w, &case.id, language)?;
    let skill = crate::runtime::STAGES
        .iter()
        .find(|(stage, _, _)| *stage == case.stage)
        .context("Unknown evaluation Skill stage")?
        .1;
    crate::skills::Catalog::discover(&w)?.resolve(&[skill.to_owned()])?;
    let stage_plan = if case.id == "draft-stages" {
        stages::tasks()
    } else {
        vec![]
    };
    let m = Manifest {
        format: FORMAT.into(),
        case,
        fixture_version: 4,
        application_version: env!("CARGO_PKG_VERSION").into(),
        runner_sha256: runner_hash()?,
        synthetic: true,
        language: language.into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        prompt,
        inputs: inventory(dir.path())?,
        stages: stage_plan,
    };
    let root = Workspace {
        root: dir.path().to_owned(),
    };
    put_note(
        &root,
        "Task.md",
        json!({"type":"Evaluation task","title":m.case.title,"language":"en"}),
        &format!(
            "# Synthetic evaluation task\n\n{}\n\n## Submitted task (original wording)\n\n{}",
            m.case.purpose,
            if m.stages.is_empty() {
                m.prompt.clone()
            } else {
                m.stages
                    .iter()
                    .map(|s| format!("### {} ({})\n\n{}", s.id, s.stage, s.prompt))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
        ),
    )?;
    let bytes = serde_json::to_vec_pretty(&m)?;
    put(&root, "manifest.json", &bytes)?;
    save(
        dir.path(),
        &Report {
            format: FORMAT.into(),
            manifest_sha256: hash(&bytes),
            case_id: m.case.id,
            synthetic: true,
            state: "prepared".into(),
            session_id: None,
            session_status: None,
            configured_provider: None,
            configured_model: None,
            provider_fingerprint: None,
            max_steps: None,
            provider_timeout_seconds: None,
            max_total_tokens: None,
            max_requests: None,
            timeout_seconds: None,
            limits_scope: None,
            started_at: None,
            finished_at: None,
            mechanical_outcome: Outcome::NotRun,
            semantic_review: Outcome::NotRun,
            checks: vec![],
            usage: None,
            outputs: BTreeMap::new(),
            evidence_files: BTreeMap::from([(
                "Task.md".into(),
                hash(&read(dir.path(), "Task.md")?),
            )]),
            note: "Prepared only. No model request or semantic review has occurred.".into(),
            stages: vec![],
        },
    )?;
    Ok(dir.keep())
}

pub fn inspect(directory: &Path) -> Result<Inspection> {
    let root = directory.canonicalize()?;
    let (m, digest) = manifest(&root)?;
    let report: Report = serde_json::from_slice(&read(&root, "report.json")?)?;
    ensure!(
        report.format == FORMAT && report.synthetic && report.case_id == m.case.id,
        "Unsupported evaluation report"
    );
    let actual = inventory(&root)?;
    let expected = if report.state == "prepared" {
        &m.inputs
    } else {
        &report.outputs
    };
    let mut changes = Vec::new();
    if report.manifest_sha256 != digest {
        changes.push("manifest.json changed".into());
    }
    if report.state == "running" {
        changes.push(
            "Run is active or interrupted; inspect the saved session before any further action"
                .into(),
        );
    }
    for (path, value) in expected {
        if actual.get(path) != Some(value) {
            changes.push(format!("Changed or missing: {path}"));
        }
    }
    for path in actual.keys() {
        if !expected.contains_key(path) {
            changes.push(format!("Added: {path}"));
        }
    }
    for (path, value) in &report.evidence_files {
        if read(&root, path).map(|b| hash(&b)).ok().as_ref() != Some(value) {
            changes.push(format!("Changed evidence: {path}"));
        }
    }
    Ok(Inspection {
        directory: root,
        current: changes.is_empty(),
        report,
        changes,
    })
}

pub async fn run(
    directory: &Path,
    mut config: Config,
    max_requests: Option<u64>,
    timeout_seconds: u64,
    events: &tokio::sync::mpsc::UnboundedSender<crate::provider::Event>,
    cancel: &CancellationToken,
) -> Result<Inspection> {
    ensure!(
        max_requests.is_none_or(|limit| (1..=100).contains(&limit))
            && (1..=3600).contains(&timeout_seconds),
        "Omit the request cap or use 1..=100 requests; use 1..=3600 seconds per stage"
    );
    let root = directory.canonicalize()?;
    let w = Workspace::open(&root.join("vault"))?;
    let _lock = WorkspaceLock::acquire(&w)?;
    let observed = inspect(&root)?;
    ensure!(
        observed.current && observed.report.state == "prepared",
        "Only an unchanged prepared evaluation can run; create a new case instead of replaying it"
    );
    let (m, _) = manifest(&root)?;
    ensure!(
        m.runner_sha256 == runner_hash()?,
        "Evaluation executable changed since preparation; prepare a new case with this binary"
    );
    ensure!(
        config.active()?.tools && !config.active()?.model.trim().is_empty(),
        "Evaluation requires an explicit model with tool support enabled"
    );
    crate::config::validate_endpoint(&config.active()?.base_url)?;
    ensure!(
        config.active()?.timeout_seconds > 0,
        "Invalid runtime limits"
    );
    // No configured external tool process is started, and no call is auto-approved.
    config.mcp.clear();
    let mut session = Session::new(&config, &m.case.stage)?;
    let mut report = observed.report;
    report.state = "running".into();
    report.started_at = Some(chrono::Utc::now().to_rfc3339());
    report.session_id = Some(session.id.clone());
    report.configured_provider = Some(config.provider.clone());
    report.configured_model = Some(config.active()?.model.clone());
    report.provider_fingerprint = Some(crate::runtime::fingerprint(&config)?);
    report.max_steps = None;
    report.provider_timeout_seconds = Some(config.active()?.timeout_seconds);
    report.max_total_tokens = config.session_limits.max_total_tokens;
    report.max_requests = max_requests;
    report.timeout_seconds = Some(timeout_seconds);
    report.limits_scope = Some("stage".into());
    report.note="Configured-provider execution. Request and timeout bounds apply per evaluation stage, not to interactive use. Stages consume actual preceding files in fresh sessions. MCP is disabled; no question is answered and no pending action is approved automatically. This is not provider-wide certification.".into();
    save(&root, &report)?;
    let mut calls = vec![];
    let mut ledger = crate::budget::Ledger::default();
    let result: Result<()> = async {
        let runtime = Runtime::open_for_evaluation(w.clone(), config.clone(), max_requests).await?;
        let plan = if m.stages.is_empty() {
            vec![stages::Task { id:m.case.id.clone(),stage:m.case.stage.clone(),prompt:m.prompt.clone() }]
        } else { m.stages.clone() };
        for (index, task) in plan.iter().enumerate() {
            if index > 0 {
                session = Session::new(&config, &task.stage)?;
            }
            report.session_id = Some(session.id.clone());
            // Persist the current session ID before dispatch so interruption in
            // a later stage points recovery at that stage, not its predecessor.
            report.outputs = inventory(&root)?;
            save(&root, &report)?;
            let _ = events.send(crate::provider::Event::Notice(format!("Evaluation stage {}/{}: {}",index+1,plan.len(),task.id)));
            runtime.input(&mut session, &task.prompt)?;
            let signal = cancel.child_token();
            let execution_result = {
                let execution = runtime.run(&mut session, None, events, &signal);
                tokio::pin!(execution);
                tokio::select! {
                    result = &mut execution => result,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_seconds)) => { signal.cancel(); execution.await }
                }
            };
            if let Some(saved) = &session.request_ledger {
                ledger.prior_requests_unknown |= saved.prior_requests_unknown;
                ledger.requests.extend(saved.requests.iter().cloned());
            } else { ledger.prior_requests_unknown = true; }
            let current = runtime.store.calls(&session.id)?;
            calls.extend(current.iter().cloned());
            execution_result?;
            if !m.stages.is_empty() {
                let observation = stages::observe(&root,&m,task,&session,&calls,&current)?;
                let advance = stages::can_advance(&observation);
                stages::save_evidence(&root,task,&session,&current,&mut report)?;
                report.stages.push(observation);
                report.outputs = inventory(&root)?;
                report.usage = Some(crate::budget::summary(Some(&ledger)));
                save(&root,&report)?;
                if !advance || cancel.is_cancelled() { break; }
            }
        }
        Ok(())
    }.await;
    let error = result.err().map(|e| e.to_string());
    report.state = "finished".into();
    report.finished_at = Some(chrono::Utc::now().to_rfc3339());
    report.session_status = Some(session.status.clone());
    report.usage = Some(crate::budget::summary(Some(&ledger)));
    let files = Workspace { root: root.clone() };
    for (path, bytes) in [
        ("Session.json", serde_json::to_vec_pretty(&session)?),
        ("Actions.json", serde_json::to_vec_pretty(&calls)?),
    ] {
        put(&files, path, &bytes)?;
        report.evidence_files.insert(path.into(), hash(&bytes));
    }
    report.checks=checks::evaluate(&root,&m,&session,&calls).unwrap_or_else(|_|vec![Check { id:"evidence_inspection".into(),outcome:Outcome::Fail,evidence:"Evidence could not be inspected consistently; no successful observation is inferred.".into() }]);
    if !m.stages.is_empty() {
        let completed =
            report.stages.len() == m.stages.len() && report.stages.iter().all(stages::can_advance);
        report.checks.push(Check {id:"stage_sequence".into(),outcome:if completed {Outcome::Pass}else if session.status==Status::Completed && !cancel.is_cancelled() {Outcome::Fail}else{Outcome::NotRun},evidence:format!("{} of {} stages observed; every stage requires its actual output contract before advancing. Each stage has a separate session and evaluation-only limits.",report.stages.len(),m.stages.len())});
    }
    if error.is_some() {
        let bytes = serde_json::to_vec_pretty(&json!({"execution_error":error}))?;
        put(&files, "Execution-error.json", &bytes)?;
        report
            .evidence_files
            .insert("Execution-error.json".into(), hash(&bytes));
        report.checks.push(Check { id:"execution_error".into(),outcome:Outcome::Fail,evidence:"Execution could not finish; inspect the private saved session. No automatic retry occurred.".into() });
    }
    report.mechanical_outcome = if report.checks.iter().any(|c| c.outcome == Outcome::Fail) {
        Outcome::Fail
    } else if report.checks.iter().any(|c| c.outcome != Outcome::Pass) {
        Outcome::Partial
    } else {
        Outcome::Pass
    };
    report.outputs = inventory(&root)?;
    save(&root, &report)?;
    inspect(&root)
}
