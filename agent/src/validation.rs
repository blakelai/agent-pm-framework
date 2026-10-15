//! Deterministic readiness and provenance checks; findings never establish approval.
use crate::{
    baselines, governance,
    locale::Catalog,
    profile,
    records::{self, Records, truth},
    traceability,
    workspace::{Document, Workspace},
};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub struct Finding {
    pub level: &'static str,
    pub code: &'static str,
    pub file: String,
    pub message: String,
}
#[derive(Default)]
struct Findings(Vec<Finding>);
impl Findings {
    fn add(
        &mut self,
        level: &'static str,
        code: &'static str,
        file: &str,
        message: impl Into<String>,
    ) {
        self.0.push(Finding {
            level,
            code,
            file: file.into(),
            message: message.into(),
        });
    }
    fn error(&mut self, code: &'static str, file: &str, message: impl Into<String>) {
        self.add("ERROR", code, file, message);
    }
    fn warn(&mut self, code: &'static str, file: &str, message: impl Into<String>) {
        self.add("WARN", code, file, message);
    }
    fn check<T>(&mut self, code: &'static str, file: &str, result: Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.error(code, file, format!("{error:#}"));
                None
            }
        }
    }
}
fn managed(path: &str) -> bool {
    records::MANAGED
        .iter()
        .any(|p| path.starts_with(&format!("{p}/")))
        && !path.split('/').any(|p| p == "Baselines")
}
fn scan(workspace: &Workspace, paths: &[String], findings: &mut Findings) -> Records {
    let mut active = vec![];
    let mut ids = BTreeSet::new();
    for folder in records::MANAGED {
        for path in paths
            .iter()
            .filter(|p| p.starts_with(&format!("{folder}/")) && managed(p))
        {
            if let Err(error) = workspace.path(path) {
                findings.error("PROPERTIES", path, error.to_string());
                continue;
            }
            if !path.ends_with(".md") {
                continue;
            }
            // Plain notes lack an ID; the separate OKF profile reports missing metadata.
            let Some(text) = findings.check("PROPERTIES", path, workspace.read(path)) else {
                continue;
            };
            if !text
                .trim_start_matches('\u{feff}')
                .replace("\r\n", "\n")
                .starts_with("---\n")
            {
                continue;
            }
            let Some(d) = findings.check("PROPERTIES", path, records::read(workspace, path)) else {
                continue;
            };
            if !truth(&d.metadata["id"]) {
                continue;
            }
            let Some(id) = &d.id else {
                findings.error("PROPERTY_TYPE", path, "id must be a string");
                continue;
            };
            if !ids.insert(id.clone()) {
                findings.error("DUPLICATE_ID", path, id.clone());
            }
            if !truth(&d.metadata["type"]) {
                findings.error("TYPE", path, "Missing type");
            }
            for field in [
                "parents",
                "evidence",
                "depends_on",
                "contexts",
                "satisfies",
                "origins",
                "requirements",
                "cases",
                "requirement_versions",
                "items",
                "test_runs",
                "ac_coverage",
            ] {
                if let Some(value) = d.metadata.get(field) {
                    if !value.is_array() {
                        findings.error("PROPERTY_TYPE", path, format!("{field} must be a list"));
                    } else if value.as_array().unwrap().iter().any(|v| !v.is_string()) {
                        findings.error(
                            "PROPERTY_TYPE",
                            path,
                            format!("{field} entries must be strings"),
                        );
                    }
                }
            }
            active.push(d);
        }
    }
    Records::for_validation(workspace, active, paths.to_vec())
}
struct Glossary<'a> {
    document: &'a Document,
    terms: Vec<(String, Vec<String>)>,
}
fn glossaries(records: &Records) -> BTreeMap<String, Glossary<'_>> {
    let heading = Regex::new(r"\*\*([^*]+)\*\*:\s*\n").unwrap();
    let avoid = Regex::new(r"(?m)^_Avoid_:\s*(.+)$").unwrap();
    let separator = Regex::new("[,，、]").unwrap();
    records
        .active
        .iter()
        .filter(|d| d.kind == "glossary")
        .filter_map(|d| {
            let context = d.metadata["context"].as_str()?;
            let mut terms = vec![];
            for h in heading.captures_iter(&d.body) {
                let rest = &d.body[h.get(0).unwrap().end()..];
                let block = rest.split("\n**").next().unwrap();
                let avoided = avoid
                    .captures(block)
                    .map(|c| separator.split(&c[1]).map(str::to_owned).collect())
                    .unwrap_or_default();
                terms.push((h[1].into(), avoided));
            }
            Some((context.into(), Glossary { document: d, terms }))
        })
        .collect()
}
fn strip_fences(body: &str) -> String {
    Regex::new(r"(?s)```.*?```")
        .unwrap()
        .replace_all(body, "")
        .into_owned()
}
fn check_links(records: &Records, d: &Document, findings: &mut Findings) {
    let text = format!("{}\n{}", d.metadata, strip_fences(&d.body));
    for reference in records::wiki_refs(&text) {
        findings.check(
            "LINK",
            &d.path,
            (|| -> Result<()> {
                let path = records.resolve_path(reference)?;
                if let Some((_, anchor)) = reference.split_once("#^") {
                    let anchor = anchor.split('|').next().unwrap();
                    ensure!(
                        records
                            .workspace
                            .read(&path)?
                            .contains(&format!("^{anchor}")),
                        "Block reference not found"
                    );
                }
                Ok(())
            })(),
        );
    }
}
fn agent_document(path: &str) -> bool {
    path.starts_with("90_Agent/")
        || path.starts_with("80_Templates/")
        || path.starts_with(".agents/")
        || path.starts_with("agent/")
        || [
            "AGENTS.md",
            "README.md",
            "HOME.md",
            "References.md",
            "index.md",
            "log.md",
            "00_Governance/Settings.md",
            "00_Governance/Schema.md",
            "00_Governance/Workflow.md",
            "00_Governance/Project-Setup.md",
            "00_Governance/Language-Settings.md",
            "03_Systems/OpenWiki-Integration.md",
            "00_Governance/Baselines-and-Changes.md",
            "00_Governance/Discovery-and-Prioritization.md",
            "03_Systems/Integration-Review.md",
            "06_Delivery/Planning.md",
            "08_Verification/Acceptance-and-Release.md",
            "09_Operations/Benefits-and-Costs.md",
            "tools/README.md",
        ]
        .contains(&path)
}
fn context_names(m: &Value) -> Vec<&str> {
    m["contexts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}
fn terminology(d: &Document, contexts: &BTreeMap<String, Glossary<'_>>, findings: &mut Findings) {
    let names = context_names(&d.metadata);
    let multi = names.iter().copied().collect::<BTreeSet<_>>().len() > 1;
    if multi {
        findings.warn("MULTI_CONTEXT_REVIEW", &d.path, "Review terminology by passage in this multi-context document; global replacements are not inferred");
    }
    let authored = Regex::new(r"`[^`]+`")
        .unwrap()
        .replace_all(&strip_fences(&d.body), "")
        .lines()
        .filter(|line| !line.starts_with('>'))
        .collect::<Vec<_>>()
        .join("\n");
    for name in names {
        let Some(glossary) = contexts.get(name) else {
            findings.error("CONTEXT", &d.path, format!("Unknown context: {name}"));
            continue;
        };
        if d.metadata["doc_status"] == "approved"
            && glossary.document.metadata["doc_status"] != "approved"
        {
            findings.error(
                "GLOSSARY_UNCONFIRMED",
                &d.path,
                format!("Glossary is not approved: {name}"),
            );
        }
        if multi {
            continue;
        }
        for (canonical, avoids) in &glossary.terms {
            for term in avoids
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty() && *s != "無")
            {
                let pattern = if term.is_ascii() {
                    format!(r"(?i)(?:^|[^\w]){}(?:$|[^\w])", regex::escape(term))
                } else {
                    format!("(?i){}", regex::escape(term))
                };
                if Regex::new(&pattern).unwrap().is_match(&authored) {
                    findings.warn("TERMINOLOGY", &d.path, format!("{name}: review term \"{term}\"; suggested canonical term \"{canonical}\""));
                }
            }
        }
    }
}
fn require_targets(
    records: &Records,
    refs: Option<&Value>,
    expected: &[&str],
    missing: &str,
) -> Result<Vec<Document>> {
    let targets = records.targets(refs, expected)?;
    ensure!(!targets.is_empty(), "{missing}");
    Ok(targets)
}
fn pbi(
    records: &Records,
    d: &Document,
    parents: &[Document],
    contexts: &BTreeMap<String, Glossary<'_>>,
    findings: &mut Findings,
) {
    let m = &d.metadata;
    let work = m["work_type"].as_str().unwrap_or("");
    let delivery = m["delivery_status"].as_str().unwrap_or("");
    let requirements = findings
        .check(
            "REQUIREMENT_LINK",
            &d.path,
            records.targets(m.get("satisfies"), &["requirement"]),
        )
        .unwrap_or_default();
    if ["feature", "migration"].contains(&work) && requirements.is_empty() {
        findings.error(
            "REQUIREMENT_LINK",
            &d.path,
            "Feature/migration work needs atomic requirements",
        );
    }
    let allowed: &[&str] = match work {
        "bug" => &["issue", "requirement"],
        "enabler" => &[
            "system_analysis",
            "design",
            "decision",
            "requirement",
            "question",
        ],
        "spike" => &["question"],
        _ => &[],
    };
    if !allowed.is_empty() {
        findings.check(
            "WORK_ORIGIN",
            &d.path,
            require_targets(records, m.get("origins"), allowed, "Missing work origin"),
        );
    }
    if work == "spike"
        && (!truth(&m["research_question"]) || !m["timebox_days"].as_f64().is_some_and(|v| v > 0.))
    {
        findings.error(
            "SPIKE_CONTRACT",
            &d.path,
            "Spike needs a research question and positive timebox_days",
        );
    }
    let acs: BTreeSet<_> = Regex::new(r"AC-\d+")
        .unwrap()
        .find_iter(&d.body)
        .map(|m| m.as_str().to_owned())
        .collect();
    if acs.is_empty() {
        findings.error(
            "AC_REQUIRED",
            &d.path,
            "Provide numbered outcome/acceptance criteria",
        );
    }
    if ["ready", "in_progress", "blocked", "done"].contains(&delivery) {
        if !["ready_by", "ready_at", "readiness_evidence"]
            .iter()
            .all(|k| truth(&m[k]))
        {
            findings.error(
                "READINESS_RECORD",
                &d.path,
                "Provide an actual team readiness record",
            );
        }
        if ["feature", "migration"].contains(&work) {
            let checked = (|| -> Result<Vec<String>> {
                let baseline = m["baseline_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .context("Executing feature/migration work must pin baseline_id")?;
                let (manifest, _) = baselines::approval(records, baseline)?;
                let mut required: BTreeSet<&str> = parents
                    .iter()
                    .chain(&requirements)
                    .filter_map(|d| d.id.as_deref())
                    .collect();
                required.extend(
                    context_names(m)
                        .iter()
                        .filter_map(|c| contexts.get(*c))
                        .filter_map(|g| g.document.id.as_deref()),
                );
                ensure!(
                    required
                        .iter()
                        .all(|id| manifest["documents"].get(*id).is_some()),
                    "Baseline must include PRD, requirements and applicable glossaries"
                );
                let mut warnings = vec![];
                for req in &requirements {
                    if manifest["documents"][req.id.as_deref().unwrap()]["content_sha256"]
                        != records::content_hash(&req.metadata, &req.body)?
                    {
                        warnings.push(format!("Current requirement differs from the execution baseline; PBI remains pinned to {baseline}"));
                    }
                }
                Ok(warnings)
            })();
            if let Some(messages) = findings.check("EXECUTION_BASELINE", &d.path, checked) {
                for m in messages {
                    findings.warn("NEWER_DRAFT", &d.path, m);
                }
            }
        }
    }
    if delivery != "done" {
        return;
    }
    for field in ["acceptance_evidence", "dod_evidence"] {
        findings.check(
            "DONE_EVIDENCE",
            &d.path,
            require_targets(records, m.get(field), &[], &format!("Missing {field}")),
        );
    }
    if !["feature", "migration", "bug"].contains(&work) {
        return;
    }
    let mut mapped = BTreeSet::new();
    let mut case_ids = BTreeSet::new();
    for value in m["ac_coverage"].as_array().into_iter().flatten() {
        let Some((ac, id)) = value.as_str().and_then(|s| s.split_once('=')) else {
            findings.error("AC_COVERAGE", &d.path, "Use AC mapping format AC-01=TC-001");
            continue;
        };
        case_ids.insert(id);
        if !acs.contains(ac) {
            findings.error("AC_COVERAGE", &d.path, "Use AC mapping format AC-01=TC-001");
            continue;
        }
        let checked = (|| -> Result<()> {
            ensure!(
                records.by_id(id)?.kind == "testcase",
                "AC must map to a test case"
            );
            Ok(())
        })();
        if findings.check("AC_COVERAGE", &d.path, checked).is_some() {
            mapped.insert(ac.to_owned());
        }
    }
    if mapped != acs {
        findings.error(
            "AC_COVERAGE",
            &d.path,
            "Not all acceptance criteria are covered",
        );
    }
    if ["feature", "migration"].contains(&work) && !requirements.is_empty() {
        findings.check(
            "DONE_TEST_RESULTS",
            &d.path,
            (|| -> Result<()> {
                let baseline = m["baseline_id"]
                    .as_str()
                    .context("Executing feature/migration work must pin baseline_id")?;
                let pinned = requirements
                    .iter()
                    .map(|d| {
                        baselines::pinned_document(records, baseline, d.id.as_deref().unwrap())
                    })
                    .collect::<Result<Vec<_>>>()?;
                let cases = case_ids
                    .iter()
                    .map(|id| records.by_id(id).cloned())
                    .collect::<Result<Vec<_>>>()?;
                let runs = records.targets(m.get("acceptance_evidence"), &["test_run"])?;
                let latest = traceability::latest_results_from(records, &cases, &pinned, &runs)?;
                ensure!(
                    latest.keys().map(String::as_str).collect::<BTreeSet<_>>() == case_ids
                        && latest.values().all(|d| d.metadata["result"] == "passed"),
                    "AC lacks an actual passing run for its execution baseline"
                );
                Ok(())
            })(),
        );
    }
}
fn file_hash(path: &Path) -> Result<String> {
    ensure!(
        fs::metadata(path)?.is_file(),
        "Expected a regular source file"
    );
    let mut input = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn repository_root(workspace: &Workspace, repo: &Document) -> Result<PathBuf> {
    let value = records::string(&repo.metadata, "repo_path")?;
    let path = if let Some(rest) = value.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME").context("Cannot resolve repository home directory")?)
            .join(rest)
    } else {
        PathBuf::from(value)
    };
    let path = if path.is_absolute() {
        path
    } else {
        workspace.path(path.to_str().context("Repository path must be UTF-8")?)?
    };
    let path = path
        .canonicalize()
        .context("Repository path does not exist")?;
    ensure!(path.is_dir(), "Repository path is not a directory");
    Ok(path)
}
fn source(records: &Records, d: &Document, findings: &mut Findings) {
    let w = &records.workspace;
    let m = &d.metadata;
    if d.kind == "source" {
        let checked = (|| -> Result<String> {
            let asset = records::string(m, "asset")?;
            ensure!(
                asset.starts_with("01_Sources/Assets/"),
                "Archived source must be in 01_Sources/Assets"
            );
            file_hash(&w.path(&asset)?)
        })();
        if let Some(hash) = findings.check("SOURCE_MISSING", &d.path, checked)
            && m["sha256"] != hash
        {
            findings.error("SOURCE_CHANGED", &d.path, "Archived source hash mismatch");
        }
        if m["review_status"] != "verified" {
            findings.warn(
                "SOURCE_REVIEW",
                &d.path,
                format!(
                    "Source review pending: {}",
                    m["extraction_status"].as_str().unwrap_or("None")
                ),
            );
        }
    }
    if d.kind != "evidence" {
        return;
    }
    let verified = m["workflow_status"] == "verified";
    if !truth(&m["locator"]) || !truth(&m["assertion_type"]) {
        findings.error(
            "EVIDENCE_METADATA",
            &d.path,
            "Missing locator or assertion_type",
        );
    }
    if !verified {
        findings.warn(
            "EVIDENCE_REVIEW",
            &d.path,
            "Evidence needs source review; verified does not mean business approval",
        );
    }
    if truth(&m["repo_id"]) {
        let checked = (|| -> Result<()> {
            let repo_id = records::string(m, "repo_id")?;
            let repo = records.by_id(&format!("REPO-{repo_id}"))?;
            let root = repository_root(w, repo)?;
            let relative = records::string(m, "source_path")?;
            let current = Workspace { root }.path(&relative)?;
            if file_hash(&current)? != m["source_sha256"] {
                findings.add(
                    if verified { "ERROR" } else { "WARN" },
                    "STALE_EVIDENCE",
                    &d.path,
                    "Referenced repository file changed; review evidence again",
                );
            }
            let snapshot = records::string(m, "snapshot_id")?;
            let manifest: Value = serde_json::from_str(&profile::read_text(
                w,
                &format!("03_Systems/Snapshots/{snapshot}/manifest.json"),
            )?)?;
            ensure!(
                manifest["files"].is_object(),
                "Snapshot manifest needs a files mapping"
            );
            if manifest["files"][&relative] != m["source_sha256"] {
                findings.add(
                    "ERROR",
                    "SNAPSHOT_MISMATCH",
                    &d.path,
                    "Evidence and snapshot versions differ",
                );
            }
            if truth(&manifest["openwiki_active_run"]) {
                findings.add(
                    "WARN",
                    "OPENWIKI_INCOMPLETE",
                    &d.path,
                    "OpenWiki had an unfinished run when captured",
                );
            }
            Ok(())
        })();
        findings.check("EVIDENCE_SOURCE", &d.path, checked);
    } else if truth(&m["source_id"]) {
        let checked = records::string(m, "source_id").and_then(|id| records.by_id(&id));
        if let Some(source) = findings.check("EVIDENCE_SOURCE", &d.path, checked) {
            if source.metadata["sha256"] != m["source_sha256"] {
                findings.error(
                    "STALE_EVIDENCE",
                    &d.path,
                    "Source version differs from evidence",
                );
            }
            if verified && source.metadata["review_status"] != "verified" {
                findings.error(
                    "VISUAL_REVIEW_REQUIRED",
                    &d.path,
                    "Original material review is incomplete",
                );
            }
        }
    } else {
        findings.error(
            "EVIDENCE_SOURCE",
            &d.path,
            "Evidence must identify material or repository provenance",
        );
    }
}
fn inspect_record(
    records: &Records,
    d: &Document,
    language: Option<&str>,
    contexts: &BTreeMap<String, Glossary<'_>>,
    findings: &mut Findings,
) {
    let m = &d.metadata;
    let kind = d.kind.as_str();
    if let Some(language) = language
        && [
            "brd",
            "prd",
            "requirement",
            "glossary",
            "pbi",
            "system_analysis",
            "testcase",
            "decision",
            "contract",
            "design",
        ]
        .contains(&kind)
    {
        let expected = if agent_document(&d.path) {
            "en"
        } else {
            language
        };
        if m["language"] != expected {
            findings.warn("DOCUMENT_LANGUAGE", &d.path, format!("Declared language is missing or differs from {expected}; review or migrate the authorized scope without changing historical baselines"));
        }
    }
    if kind == "project" && m["workflow_status"] == "unconfigured" {
        findings.warn("PROJECT_SETUP", &d.path, "Complete Project setup; no scope, resource commitments or delivery baseline have been recorded");
    }
    if baselines::DOCUMENT_TYPES.contains(&kind) {
        if ![
            Some("draft"),
            Some("in_review"),
            Some("approved"),
            Some("retired"),
        ]
        .contains(&m["doc_status"].as_str())
            || records::positive_version(m).is_err()
        {
            findings.error(
                "DOCUMENT_STATE",
                &d.path,
                "Controlled documents need doc_status and a positive integer version",
            );
        }
        if m.get("workflow_status").is_some() {
            findings.error(
                "LEGACY_STATUS",
                &d.path,
                "Use doc_status for controlled-document state",
            );
        }
    }
    if kind == "pbi" {
        if ![
            Some("proposed"),
            Some("ready"),
            Some("in_progress"),
            Some("blocked"),
            Some("done"),
            Some("cancelled"),
        ]
        .contains(&m["delivery_status"].as_str())
        {
            findings.error("DELIVERY_STATE", &d.path, "Invalid delivery_status");
        }
        if m.get("workflow_status").is_some() {
            findings.error(
                "LEGACY_STATUS",
                &d.path,
                "Use delivery_status for PBI execution state",
            );
        }
        if ![
            Some("feature"),
            Some("bug"),
            Some("enabler"),
            Some("spike"),
            Some("migration"),
        ]
        .contains(&m["work_type"].as_str())
        {
            findings.error("WORK_TYPE", &d.path, "Specify work_type");
        }
    }
    if ["brd", "prd", "requirement", "pbi"].contains(&kind) {
        if !truth(&m["owner"]) || !truth(&m["contexts"]) {
            findings.error("OWNERSHIP", &d.path, "Specify owner and contexts");
        }
        let evidence = m["evidence"].as_array().cloned().unwrap_or_default();
        if kind != "pbi" && evidence.is_empty() {
            findings.error("EVIDENCE_REQUIRED", &d.path, "Missing evidence links");
        }
        let expected: &[&str] = match kind {
            "prd" => &["brd"],
            "requirement" => &["prd"],
            "pbi" if m["work_type"] == "feature" || m["work_type"] == "migration" => &["prd"],
            _ => &[],
        };
        let parents = json!(m["parents"].as_array().cloned().unwrap_or_default());
        if !expected.is_empty() && parents.as_array().unwrap().is_empty() {
            findings.error(
                "TRACEABILITY",
                &d.path,
                format!("Missing upstream {}", expected[0]),
            );
        }
        let parents = findings
            .check(
                "PARENT_TYPE",
                &d.path,
                records.targets(Some(&parents), expected),
            )
            .unwrap_or_default();
        for reference in evidence.iter().filter_map(Value::as_str) {
            if let Ok(e) = records.resolve(reference) {
                if e.kind != "evidence" {
                    findings.error("EVIDENCE_TYPE", &d.path, "Evidence field must link to EVD");
                } else if m["doc_status"] == "approved"
                    && e.metadata["workflow_status"] != "verified"
                {
                    findings.error("UNVERIFIED_EVIDENCE", &d.path, e.id.unwrap_or_default());
                }
            }
        }
        terminology(d, contexts, findings);
        if kind == "pbi" {
            pbi(records, d, &parents, contexts, findings);
        }
        if m["doc_status"] == "draft" || m["delivery_status"] == "proposed" {
            findings.warn(
                "DRAFT",
                &d.path,
                "Draft/proposed work is not a team delivery commitment",
            );
        }
    }
    if kind == "testcase" {
        findings.check(
            "TEST_TRACE",
            &d.path,
            require_targets(
                records,
                m.get("requirements"),
                &["requirement"],
                "Test case must link to requirements",
            ),
        );
    }
    if kind == "test_run" {
        if ![Some("passed"), Some("failed"), Some("blocked")].contains(&m["result"].as_str()) {
            findings.error("TEST_RESULT", &d.path, "Invalid test result");
        }
        findings.check(
            "TEST_TRACE",
            &d.path,
            require_targets(
                records,
                m.get("cases"),
                &["testcase"],
                "Test run must specify cases",
            ),
        );
        if ![
            "executed_at",
            "executed_by",
            "environment",
            "build",
            "evidence",
            "requirement_versions",
        ]
        .iter()
        .all(|k| truth(&m[k]))
        {
            findings.error("TEST_RECORD", &d.path, "Missing execution time, actor, environment, build, evidence or requirement versions");
        }
    }
    if ["risk", "issue", "action", "question"].contains(&kind)
        && ![Some("closed"), Some("resolved")].contains(&m["workflow_status"].as_str())
        && (!truth(&m["owner"]) || !truth(&m["due_date"]))
    {
        findings.warn(
            "FOLLOWUP_OWNER",
            &d.path,
            "Open follow-up needs an owner and due date",
        );
    }
    source(records, d, findings);
}
fn baseline_checks(records: &Records, paths: &[String], findings: &mut Findings) {
    for d in &records.active {
        if !baselines::DOCUMENT_TYPES.contains(&d.kind.as_str())
            || d.metadata["doc_status"] != "approved"
        {
            continue;
        }
        findings.check(
            "BASELINE_INVALID",
            &d.path,
            (|| -> Result<()> {
                let baseline = d.metadata["baseline_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .context("Approved document needs baseline_id")?;
                let (manifest, _) = baselines::approval(records, baseline)?;
                let entry = &manifest["documents"][d.id.as_deref().unwrap()];
                ensure!(
                    !entry.is_null()
                        && entry["version"] == d.metadata["version"]
                        && entry["content_sha256"] == records::content_hash(&d.metadata, &d.body)?,
                    "Current text differs from its approved version; create a new draft/version"
                );
                Ok(())
            })(),
        );
    }
    for path in paths
        .iter()
        .filter(|p| p.starts_with("00_Governance/Baselines/"))
    {
        let parts: Vec<_> = path.split('/').collect();
        if parts.len() == 4 && parts[3] == "manifest.json" {
            findings.check(
                "BASELINE_INTEGRITY",
                path,
                baselines::load(&records.workspace, parts[2]),
            );
        } else if let Err(error) = records.workspace.path(path) {
            findings.error("BASELINE_INTEGRITY", path, error.to_string());
        }
    }
}
pub fn inspect(workspace: &Workspace) -> Result<Vec<Finding>> {
    let mut findings = Findings::default();
    let language = findings.check(
        "LANGUAGE_SETTINGS",
        "00_Governance/Settings.md",
        governance::settings(workspace, None),
    );
    if language.is_some()
        && let Err(error) = Catalog::load(workspace)
    {
        findings.warn(
            "LANGUAGE_CATALOG",
            "00_Governance/Settings.md",
            format!("{error:#}"),
        );
    }
    let paths = profile::inventory(workspace)?;
    let records = scan(workspace, &paths, &mut findings);
    for d in &records.active {
        check_links(&records, d, &mut findings);
    }
    let contexts = glossaries(&records);
    let language = language
        .as_ref()
        .and_then(|m| m["document_language"].as_str());
    for d in &records.active {
        inspect_record(&records, d, language, &contexts, &mut findings);
    }
    let profile = profile::inspect_paths(workspace, &paths)?;
    for message in profile.errors {
        findings.error("OKF_PROFILE", "90_Agent/OKF-Profile.md", message);
    }
    for message in profile.warnings {
        findings.warn("OKF_GUIDANCE", "90_Agent/OKF-Profile.md", message);
    }
    baseline_checks(&records, &paths, &mut findings);
    Ok(findings.0)
}
pub fn run(workspace: &Workspace, strict: bool) -> Result<Value> {
    let findings = inspect(workspace)?;
    let errors = findings.iter().filter(|f| f.level == "ERROR").count();
    let warnings = findings.iter().filter(|f| f.level == "WARN").count();
    let mut body = format!(
        "# Document quality report\n\nErrors: {errors}; warnings: {warnings} .\n\nChecks cover structure, traceability, source hashes and terminology hints. Semantics, completeness, estimates and business decisions still need review.\n\n"
    );
    for f in &findings {
        body.push_str(&format!(
            "- **{} {}** [[{}]]：{}\n",
            f.level,
            f.code,
            f.file.strip_suffix(".md").unwrap_or(&f.file),
            f.message
        ));
    }
    if findings.is_empty() {
        body.push_str("No detectable issues.\n");
    }
    let report = records::write_report(
        workspace,
        "90_Agent/Reports/Quality-Report.md",
        "REPORT-QUALITY",
        &body,
    )?;
    Ok(
        json!({"report":report,"findings":findings,"errors":errors,"warnings":warnings,"strict":strict,"exit_code":if errors > 0 || strict && warnings > 0 {1} else {0}}),
    )
}
