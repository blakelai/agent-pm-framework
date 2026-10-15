use crate::{
    records::{self, Records, fingerprint, positive_version, truth},
    workspace::Document,
};
use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn latest_results(
    records: &Records,
    cases: &[Document],
    requirements: &[Document],
) -> Result<BTreeMap<String, Document>> {
    latest_results_from(records, cases, requirements, &records.active)
}
pub fn latest_results_from(
    records: &Records,
    cases: &[Document],
    requirements: &[Document],
    runs: &[Document],
) -> Result<BTreeMap<String, Document>> {
    let mut tokens = BTreeMap::new();
    for req in requirements {
        positive_version(&req.metadata)?;
        tokens.insert(req.id.as_deref().unwrap(), fingerprint(req)?);
    }
    let mut needed = BTreeMap::new();
    for case in cases {
        let covered = records.targets(case.metadata.get("requirements"), &["requirement"])?;
        needed.insert(
            case.id.as_deref().unwrap(),
            covered
                .iter()
                .filter_map(|d| tokens.get(d.id.as_deref().unwrap()).cloned())
                .collect::<BTreeSet<_>>(),
        );
    }
    let mut latest: BTreeMap<String, ((DateTime<FixedOffset>, bool), Document)> = BTreeMap::new();
    for d in runs {
        let m = &d.metadata;
        if d.kind != "test_run"
            || truth(&m["simulation"])
            || ![
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
            continue;
        }
        let Some(when) = m["executed_at"].as_str().and_then(|s| {
            DateTime::parse_from_rfc3339(s)
                .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f%:z"))
                .ok()
        }) else {
            continue;
        };
        if !matches!(records.targets(m.get("evidence"), &[]), Ok(v) if !v.is_empty()) {
            continue;
        }
        let versions: BTreeSet<_> = m["requirement_versions"]
            .as_array()
            .context("requirement_versions must be a list")?
            .iter()
            .map(|v| {
                v.as_str()
                    .context("Requirement version token must be a string")
            })
            .collect::<Result<_>>()?;
        for case in records.targets(m.get("cases"), &["testcase"])? {
            let id = case.id.as_deref().unwrap();
            let Some(required) = needed.get(id).filter(|s| !s.is_empty()) else {
                continue;
            };
            if !required
                .iter()
                .all(|token| versions.contains(token.as_str()))
            {
                continue;
            }
            let rank = (when, m["result"] != "passed");
            if latest.get(id).is_none_or(|(previous, _)| rank >= *previous) {
                latest.insert(id.into(), (rank, d.clone()));
            }
        }
    }
    Ok(latest.into_iter().map(|(id, (_, d))| (id, d)).collect())
}
pub struct Coverage {
    pub pbis: Vec<Document>,
    pub cases: Vec<Document>,
    pub runs: Vec<Document>,
    pub releases: Vec<Document>,
    pub missing: BTreeSet<String>,
}
pub fn evidence_for(records: &Records, req: &Document) -> Result<Coverage> {
    let id = req.id.as_deref().context("Requirement needs an id")?;
    let (mut pbis, mut cases) = (vec![], vec![]);
    for d in &records.active {
        let field = match d.kind.as_str() {
            "pbi" => "satisfies",
            "testcase" => "requirements",
            _ => continue,
        };
        if records
            .targets(d.metadata.get(field), &["requirement"])?
            .iter()
            .any(|target| target.id.as_deref() == Some(id))
        {
            if d.kind == "pbi" {
                pbis.push(d.clone());
            } else {
                cases.push(d.clone());
            }
        }
    }
    let latest = latest_results(records, &cases, std::slice::from_ref(req))?;
    let missing = cases
        .iter()
        .filter(|case| {
            latest
                .get(case.id.as_deref().unwrap())
                .is_none_or(|r| r.metadata["result"] != "passed")
        })
        .map(|d| d.id.clone().unwrap())
        .collect::<BTreeSet<_>>();
    let mut run_ids = BTreeSet::new();
    let runs: Vec<_> = latest
        .values()
        .filter(|d| d.metadata["result"] == "passed" && run_ids.insert(d.id.clone().unwrap()))
        .cloned()
        .collect();
    let mut releases = vec![];
    for d in &records.active {
        let m = &d.metadata;
        if d.kind != "release" || m["workflow_status"] != "released" || truth(&m["simulation"]) {
            continue;
        }
        let release_items = records.targets(m.get("items"), &["pbi"])?;
        let release_runs = records.targets(m.get("test_runs"), &["test_run"])?;
        if !pbis.is_empty()
            && pbis.iter().all(|p| {
                p.metadata["delivery_status"] == "done"
                    && release_items.iter().any(|r| r.id == p.id)
            })
            && !runs.is_empty()
            && missing.is_empty()
            && runs
                .iter()
                .all(|r| release_runs.iter().any(|d| d.id == r.id))
        {
            releases.push(d.clone());
        }
    }
    Ok(Coverage {
        pbis,
        cases,
        runs,
        releases,
        missing,
    })
}
pub fn report(records: &Records) -> Result<Value> {
    let mut body = "# Atomic requirement coverage\n\nCoverage refers to current requirement versions; historical releases pin their baselines. A passing record must match version and hash and provide execution evidence. Evidence sufficiency still needs review. Simulations do not qualify.\n\n| Requirement | PBI | Test cases | Matching passed runs | Release | Gaps |\n|---|---|---|---|---|---|\n".to_owned();
    let mut rows = vec![];
    for req in records.active.iter().filter(|d| d.kind == "requirement") {
        let coverage = evidence_for(records, req)?;
        let mut gaps = vec![];
        if coverage.pbis.is_empty() {
            gaps.push("No PBI assigned");
        }
        if coverage
            .pbis
            .iter()
            .any(|d| d.metadata["delivery_status"] != "done")
        {
            gaps.push("Work incomplete");
        }
        if coverage.cases.is_empty() {
            gaps.push("Missing test cases");
        }
        if coverage.runs.is_empty() || !coverage.missing.is_empty() {
            gaps.push("Missing current-version test results");
        }
        if coverage.releases.is_empty() {
            gaps.push("Not released");
        }
        let names = |docs: &[Document]| {
            if docs.is_empty() {
                "—".into()
            } else {
                docs.iter()
                    .map(records::link)
                    .collect::<Vec<_>>()
                    .join("、")
            }
        };
        body.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            records::link(req),
            names(&coverage.pbis),
            names(&coverage.cases),
            names(&coverage.runs),
            names(&coverage.releases),
            if gaps.is_empty() {
                "Coverage complete".into()
            } else {
                gaps.join("；")
            }
        ));
        let ids = |docs: &[Document]| {
            docs.iter()
                .map(|d| d.id.clone().unwrap())
                .collect::<Vec<_>>()
        };
        rows.push(json!({"requirement":req.id,"pbis":ids(&coverage.pbis),"cases":ids(&coverage.cases),"passed_runs":ids(&coverage.runs),"releases":ids(&coverage.releases),"missing_cases":coverage.missing,"gaps":gaps}));
    }
    if rows.is_empty() {
        body.push_str("\nNo atomic requirements.\n");
    }
    Ok(
        json!({"report":records::write_report(&records.workspace,"90_Agent/Reports/Traceability-Report.md","REPORT-TRACE",&body)?,"coverage":rows}),
    )
}
