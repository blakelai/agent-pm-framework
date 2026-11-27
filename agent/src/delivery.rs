//! Evidence gates for human-recorded delivery outcomes, never deployment execution.
use crate::{
    baselines,
    editing::FieldType,
    planning,
    records::{self, Records},
    traceability,
    workspace::{Document, Workspace},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const PROFILE: &str = "delivery-v1";
pub const KINDS: &[&str] = &["change", "uat", "release", "benefit"];
pub fn folder(kind: &str) -> Option<&'static str> {
    match kind {
        "change" => Some("07_Decisions/Changes/"),
        "uat" => Some("08_Verification/UAT/"),
        "release" => Some("06_Delivery/Releases/"),
        "benefit" => Some("09_Operations/Benefits/"),
        _ => None,
    }
}
pub fn states(kind: &str) -> &'static [&'static str] {
    match kind {
        "change" => &["draft", "in_review", "accepted", "rejected"],
        "uat" => &["draft", "in_review", "blocked", "accepted", "rejected"],
        "release" => &["draft", "in_review", "ready", "released", "cancelled"],
        "benefit" => &["draft", "observing", "measured"],
        _ => &[],
    }
}
pub fn historical(m: &Value) -> bool {
    KINDS.contains(&m["type"].as_str().unwrap_or(""))
        && matches!(
            m["workflow_status"].as_str(),
            Some("accepted" | "rejected" | "released" | "cancelled" | "measured")
        )
}
pub fn fields(kind: &str) -> Vec<(&'static str, FieldType)> {
    use FieldType::*;
    if !KINDS.contains(&kind) {
        return vec![];
    }
    let mut f = vec![
        ("workflow_status", Choice(states(kind))),
        ("previous_records", List),
        ("evidence", List),
    ];
    match kind {
        "change" => f.extend([
            ("affected_records", List),
            ("baseline_id", Text),
            ("impact_evidence", List),
            ("decided_by", Text),
            ("decided_at", EventTimestamp),
            ("decision_evidence", List),
        ]),
        "uat" => f.extend([
            ("baseline_id", Text),
            ("items", List),
            ("requirements", List),
            ("cases", List),
            ("test_runs", List),
            ("reviewed_by", Text),
            ("reviewed_at", EventTimestamp),
            ("decision_evidence", List),
        ]),
        "release" => f.extend([
            ("baseline_id", Text),
            ("items", List),
            ("test_runs", List),
            ("uat_records", List),
            ("cutover_plan", List),
            ("rollback_plan", List),
            ("handover", List),
            ("go_no_go", Choice(&["go", "no_go"])),
            ("decided_by", Text),
            ("decided_at", EventTimestamp),
            ("decision_evidence", List),
            ("released_by", Text),
            ("released_at", EventTimestamp),
            ("release_evidence", List),
        ]),
        "benefit" => f.extend([
            ("releases", List),
            ("metric", Text),
            ("unit", Text),
            ("measurement_definition", Text),
            ("baseline_value", Number),
            ("target_value", Number),
            ("actual_value", Number),
            ("data_sources", List),
            ("observation_start", Date),
            ("observation_end", Date),
            ("measured_by", Text),
            ("measured_at", EventTimestamp),
            ("measurement_evidence", List),
        ]),
        _ => {}
    }
    f
}
fn refs(
    r: &Records,
    d: &Document,
    field: &str,
    kinds: &[&str],
    required: bool,
) -> Result<Vec<Document>> {
    let docs = r
        .targets(d.metadata.get(field), kinds)
        .with_context(|| field.to_owned())?;
    ensure!(!required || !docs.is_empty(), "Supply {field}");
    let mut ids = BTreeSet::new();
    for target in &docs {
        ensure!(
            target.id != d.id && ids.insert(target.id.clone()),
            "{field} cannot contain self or duplicate references"
        );
        ensure!(
            !records::truth(&target.metadata["simulation"]),
            "{field} includes simulated evidence"
        );
    }
    Ok(docs)
}
fn actual(m: &Value, actor: &str, time: &str) -> Result<()> {
    records::string(m, actor)?;
    let t = records::string(m, time)?;
    ensure!(
        chrono::DateTime::parse_from_rfc3339(&t).is_ok(),
        "{time} needs an actual RFC3339 timestamp with timezone"
    );
    Ok(())
}
fn decision(r: &Records, d: &Document, actor: &str, time: &str) -> Result<()> {
    actual(&d.metadata, actor, time)?;
    refs(r, d, "decision_evidence", &[], true)?;
    Ok(())
}
fn ids(docs: &[Document]) -> BTreeSet<String> {
    docs.iter().filter_map(|d| d.id.clone()).collect()
}

/// Check all selected UAT cases against the approved requirement versions.
/// A selected older pass cannot hide a newer failure for the same version.
fn uat_scope(r: &Records, d: &Document) -> Result<()> {
    let baseline = records::string(&d.metadata, "baseline_id")?;
    baselines::approval(r, &baseline)?;
    let requirements = refs(r, d, "requirements", &["requirement"], true)?;
    let mut pinned = vec![];
    for req in &requirements {
        pinned.push(baselines::pinned_document(
            r,
            &baseline,
            req.id.as_deref().unwrap(),
        )?);
    }
    let items = refs(r, d, "items", &["pbi"], true)?;
    let requirement_ids = ids(&requirements);
    let mut item_requirements = BTreeSet::new();
    for item in &items {
        ensure!(
            item.metadata["delivery_status"] == "done",
            "UAT acceptance needs completed item {}",
            item.title
        );
        crate::validation::completed_item(r, item)?;
        if matches!(
            item.metadata["work_type"].as_str(),
            Some("feature" | "migration")
        ) {
            ensure!(
                item.metadata["baseline_id"] == baseline,
                "Item execution baseline differs from UAT"
            );
        }
        item_requirements.extend(ids(
            &r.targets(item.metadata.get("satisfies"), &["requirement"])?
        ));
    }
    ensure!(
        item_requirements == requirement_ids,
        "UAT requirements must match the selected items' satisfies scope"
    );
    let cases = refs(r, d, "cases", &["testcase"], true)?;
    let mut covered = BTreeSet::new();
    for case in &cases {
        let scope = ids(&r.targets(case.metadata.get("requirements"), &["requirement"])?);
        ensure!(
            !scope.is_empty() && scope.is_subset(&requirement_ids),
            "UAT case scope is outside selected requirements"
        );
        covered.extend(scope);
    }
    ensure!(
        covered == requirement_ids,
        "Each UAT requirement needs a selected case"
    );
    let runs = refs(r, d, "test_runs", &["test_run"], true)?;
    let latest = traceability::latest_results(r, &cases, &pinned)?;
    let run_ids = ids(&runs);
    for case in &cases {
        let run = latest
            .get(case.id.as_deref().unwrap())
            .context("Missing actual version-matched test result")?;
        ensure!(
            run.metadata["result"] == "passed" && run_ids.contains(run.id.as_deref().unwrap()),
            "Latest version-matched result must pass and appear in test_runs: {}",
            case.title
        );
    }
    for run in &runs {
        ensure!(
            run.metadata["result"] == "passed",
            "UAT cannot select failed or blocked runs"
        );
        actual(&run.metadata, "executed_by", "executed_at")?;
        refs(r, run, "evidence", &[], true)?;
        ensure!(
            chrono::DateTime::parse_from_rfc3339(run.metadata["executed_at"].as_str().unwrap())?
                <= chrono::DateTime::parse_from_rfc3339(
                    d.metadata["reviewed_at"]
                        .as_str()
                        .context("Supply reviewed_at")?
                )?,
            "UAT review precedes selected test execution"
        );
    }
    Ok(())
}
/// Legacy Change/Release records retain their old contract. They are visibly
/// unreviewed in the delivery view and cannot be edited into new outcomes there.
pub fn check(r: &Records, d: &Document) -> Result<()> {
    if !KINDS.contains(&d.kind.as_str()) {
        return Ok(());
    }
    let m = &d.metadata;
    if m.get("workflow_schema").is_none() && matches!(d.kind.as_str(), "change" | "release") {
        return Ok(());
    }
    ensure!(
        m["workflow_schema"] == PROFILE,
        "Use workflow_schema: {PROFILE}"
    );
    let state = records::string(m, "workflow_status")?;
    ensure!(
        states(&d.kind).contains(&state.as_str()),
        "Invalid {} workflow_status",
        d.kind
    );
    refs(r, d, "previous_records", &[&d.kind], false)?;
    refs(r, d, "evidence", &[], false)?;
    for (field, ty) in fields(&d.kind) {
        if m.get(field).is_none_or(|v| v.is_null()) {
            continue;
        }
        match ty {
            FieldType::List if field != "previous_records" && field != "evidence" => {
                let kinds: &[&str] = match field {
                    "items" => &["pbi"],
                    "requirements" => &["requirement"],
                    "cases" => &["testcase"],
                    "test_runs" => &["test_run"],
                    "uat_records" => &["uat"],
                    "releases" => &["release"],
                    _ => &[],
                };
                refs(r, d, field, kinds, false)?;
            }
            FieldType::Number => {
                ensure!(
                    m[field].as_f64().is_some_and(f64::is_finite),
                    "{field} must be a finite number or null"
                );
            }
            FieldType::Date => {
                planning::date(m[field].as_str().context("Expected date string")?)?;
            }
            FieldType::EventTimestamp => {
                ensure!(
                    chrono::DateTime::parse_from_rfc3339(
                        m[field].as_str().context("Expected timestamp string")?
                    )
                    .is_ok(),
                    "{field} needs RFC3339 timezone"
                );
            }
            FieldType::Choice(options) => {
                ensure!(
                    m[field].as_str().is_some_and(|s| options.contains(&s)),
                    "Invalid {field}"
                );
            }
            FieldType::Text => {
                records::string(m, field)?;
            }
            _ => {}
        }
    }
    if state == "draft" || state == "blocked" {
        return Ok(());
    }
    records::string(m, "owner")?;
    if state == "in_review" {
        return Ok(());
    }
    ensure!(
        !records::truth(&m["simulation"]),
        "Simulations cannot establish delivery outcomes"
    );
    match d.kind.as_str() {
        "change" => {
            decision(r, d, "decided_by", "decided_at")?;
            refs(r, d, "affected_records", &[], true)?;
            refs(r, d, "impact_evidence", &[], true)?;
            if let Some(b) = m["baseline_id"].as_str() {
                baselines::approval(r, b)?;
            }
        }
        "uat" => {
            decision(r, d, "reviewed_by", "reviewed_at")?;
            if state == "accepted" {
                uat_scope(r, d)?;
            }
        }
        "release" => {
            decision(r, d, "decided_by", "decided_at")?;
            if state == "cancelled" {
                ensure!(m["go_no_go"] == "no_go", "Cancellation needs no_go");
                return Ok(());
            }
            ensure!(
                m["go_no_go"] == "go",
                "Release readiness needs an actual go decision"
            );
            let baseline = records::string(m, "baseline_id")?;
            baselines::approval(r, &baseline)?;
            let items = refs(r, d, "items", &["pbi"], true)?;
            ensure!(
                items
                    .iter()
                    .all(|i| i.metadata["delivery_status"] == "done"),
                "Release items must be done"
            );
            let uats = refs(r, d, "uat_records", &["uat"], true)?;
            let runs = refs(r, d, "test_runs", &["test_run"], true)?;
            let mut covered_items = BTreeSet::new();
            let mut required_runs = BTreeSet::new();
            for uat in &uats {
                ensure!(
                    uat.metadata["workflow_schema"] == PROFILE
                        && uat.metadata["workflow_status"] == "accepted"
                        && uat.metadata["baseline_id"] == baseline,
                    "Release requires accepted UAT on the same baseline"
                );
                check(r, uat)?;
                ensure!(
                    chrono::DateTime::parse_from_rfc3339(
                        uat.metadata["reviewed_at"].as_str().unwrap()
                    )? <= chrono::DateTime::parse_from_rfc3339(m["decided_at"].as_str().unwrap())?,
                    "Go decision precedes UAT acceptance"
                );
                covered_items.extend(ids(&r.targets(uat.metadata.get("items"), &["pbi"])?));
                required_runs.extend(ids(
                    &r.targets(uat.metadata.get("test_runs"), &["test_run"])?
                ));
            }
            ensure!(
                covered_items == ids(&items),
                "Release and UAT item scopes must match"
            );
            ensure!(
                required_runs == ids(&runs),
                "Release test_runs must match accepted UAT runs"
            );
            for field in ["cutover_plan", "rollback_plan", "handover"] {
                refs(r, d, field, &[], true)?;
            }
            if state == "released" {
                actual(m, "released_by", "released_at")?;
                let release =
                    chrono::DateTime::parse_from_rfc3339(m["released_at"].as_str().unwrap())?;
                ensure!(
                    release
                        >= chrono::DateTime::parse_from_rfc3339(m["decided_at"].as_str().unwrap())?,
                    "Release time precedes go decision"
                );
                refs(r, d, "release_evidence", &[], true)?;
            }
        }
        "benefit" => {
            for key in ["metric", "unit", "measurement_definition"] {
                records::string(m, key)?;
            }
            for key in ["baseline_value", "target_value"] {
                ensure!(
                    m[key].as_f64().is_some_and(f64::is_finite),
                    "Supply {key}; unknown is not zero"
                );
            }
            refs(r, d, "data_sources", &[], true)?;
            let start = planning::date(&records::string(m, "observation_start")?)?;
            let end = planning::date(&records::string(m, "observation_end")?)?;
            ensure!(start <= end, "Observation window is inverted");
            if state == "measured" {
                ensure!(
                    m["actual_value"].as_f64().is_some_and(f64::is_finite),
                    "Supply actual_value; unknown is not zero"
                );
                actual(m, "measured_by", "measured_at")?;
                let at = chrono::DateTime::parse_from_rfc3339(m["measured_at"].as_str().unwrap())?;
                ensure!(
                    at.date_naive() >= end,
                    "Measurement precedes the observation window end"
                );
                refs(r, d, "measurement_evidence", &[], true)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub struct Entry {
    pub document: Document,
    pub problem: Option<String>,
    pub next_gate: String,
}
pub struct Snapshot {
    pub entries: Vec<Entry>,
    pub loaded_at: String,
    pub problem: Option<String>,
}
impl Snapshot {
    pub fn load(w: &Workspace) -> Self {
        let mut s = Self {
            entries: vec![],
            loaded_at: records::timestamp(),
            problem: None,
        };
        match Records::load(w) {
            Err(e) => s.problem = Some(format!("Delivery records unavailable: {e:#}")),
            Ok(r) => {
                for d in &r.active {
                    if !KINDS.contains(&d.kind.as_str()) {
                        continue;
                    }
                    let legacy = d.metadata["workflow_schema"] != PROFILE;
                    let problem = if legacy {
                        Some("Legacy/unsupported schema: no delivery-v1 outcome check; create a linked new record".into())
                    } else {
                        check(&r, d).err().map(|e| format!("{e:#}"))
                    };
                    let next_gate = if legacy || historical(&d.metadata) {
                        "Historical or legacy record; open linked evidence and use a new record for a correction".into()
                    } else {
                        let mut candidate = d.clone();
                        let target = match d.kind.as_str() {
                            "change" | "uat" => "accepted",
                            "release" if d.metadata["workflow_status"] == "ready" => "released",
                            "release" => "ready",
                            _ => "measured",
                        };
                        candidate.metadata["workflow_status"] = json!(target);
                        match check(&r, &candidate) {
                            Ok(()) => format!(
                                "{target}: required fields and links present; this is not an approval or an automatic transition"
                            ),
                            Err(e) => format!("{target}: {e:#}"),
                        }
                    };
                    s.entries.push(Entry {
                        document: d.clone(),
                        problem,
                        next_gate,
                    });
                }
            }
        }
        s
    }
    pub fn value(&self) -> Value {
        json!({"loaded_at":self.loaded_at,"problem":self.problem,"trust":"recorded data; structural checks do not attest decisions or outcomes","records":self.entries.iter().map(|e| json!({"path":e.document.path,"id":e.document.id,"type":e.document.kind,"workflow_status":e.document.metadata["workflow_status"],"problem":e.problem,"next_gate":e.next_gate})).collect::<Vec<_>>()})
    }
}
