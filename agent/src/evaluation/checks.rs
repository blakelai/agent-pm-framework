use super::*;
use crate::{records::Records, store::CallRecord, workspace::Document};

fn checked(id: &str, ok: bool, evidence: impl Into<String>) -> Check {
    Check {
        id: id.into(),
        outcome: if ok { Outcome::Pass } else { Outcome::Fail },
        evidence: evidence.into(),
    }
}
fn decoded(call: &CallRecord) -> Option<(Value, Value)> {
    if call.status != "completed" {
        return None;
    }
    let args: Value = serde_json::from_str(&call.arguments).ok()?;
    let result: Value = serde_json::from_str(call.result.as_ref()?).ok()?;
    if result.get("error").is_some() {
        return None;
    }
    Some((args, result))
}
fn read_path(calls: &[CallRecord], predicate: impl Fn(&str) -> bool) -> bool {
    calls.iter().any(|c| {
        c.name == "read_document"
            && decoded(c).is_some_and(|(a, r)| {
                a["path"].as_str().is_some_and(&predicate)
                    && r["content"].is_string()
                    && r["sha256"].is_string()
            })
    })
}
fn inspected_interview(calls: &[CallRecord], inputs: &BTreeMap<String, String>) -> bool {
    let Some(raw_sha) = inputs.get("vault/01_Sources/Assets/interview.txt") else {
        return false;
    };
    let Some(source_id) = raw_sha.get(..16) else {
        return false;
    };
    let extracted = format!("01_Sources/Extracted/SRC-{source_id}.md");
    calls.iter().any(|call| {
        if call.name != "read_document" {
            return false;
        }
        let Some((args, result)) = decoded(call) else {
            return false;
        };
        let (Some(path), Some(content), Some(reported_sha)) = (
            args["path"].as_str(),
            result["content"].as_str(),
            result["sha256"].as_str(),
        ) else {
            return false;
        };
        let Some(pinned_sha) = inputs.get(&format!("vault/{path}")) else {
            return false;
        };
        // Ingestion creates a content-addressed copy. Reading those identical
        // bytes counts; metadata notes and unrelated extractions do not.
        (path == extracted || path.starts_with("01_Sources/Assets/") && pinned_sha == raw_sha)
            && reported_sha == pinned_sha
            && hash(content.as_bytes()) == *pinned_sha
    })
}
fn pm(calls: &[CallRecord], command: &str) -> Option<Value> {
    calls.iter().rev().find_map(|c| {
        let (a, r) = decoded(c)?;
        (c.name == "pm" && a["command"] == command).then_some(r)
    })
}
fn targets(r: &Records, d: &Document, key: &str, kind: &str) -> Vec<Document> {
    r.targets(d.metadata.get(key), &[kind]).unwrap_or_default()
}
fn has_origin(
    path: &str,
    writes: &BTreeMap<String, Vec<Value>>,
    inputs: &BTreeMap<String, String>,
    seen: &mut Vec<String>,
) -> bool {
    if seen.iter().any(|p| p == path) {
        return false;
    }
    if inputs.contains_key(&format!("vault/{path}"))
        && (path.starts_with("01_Sources/") || path.starts_with("03_Systems/Snapshots/"))
    {
        return true;
    }
    seen.push(path.into());
    writes.get(path).is_some_and(|cs| {
        cs.iter().any(|c| {
            c["path"]
                .as_str()
                .is_some_and(|p| has_origin(p, writes, inputs, &mut seen.clone()))
        })
    })
}

pub(super) fn evaluate(
    root: &Path,
    m: &Manifest,
    s: &Session,
    calls: &[CallRecord],
) -> Result<Vec<Check>> {
    let w = Workspace::open(&root.join("vault"))?;
    let actual = inventory(root)?;
    let changed: Vec<_> = m
        .inputs
        .iter()
        .filter(|(p, h)| actual.get(*p) != Some(*h))
        .map(|(p, _)| p.clone())
        .collect();
    let records = Records::load(&w)?;
    let new: Vec<_> = records
        .active
        .iter()
        .filter(|d| {
            !m.inputs.contains_key(&format!("vault/{}", d.path))
                && [
                    "glossary",
                    "brd",
                    "prd",
                    "requirement",
                    "pbi",
                    "system_analysis",
                    "decision",
                ]
                .contains(&d.kind.as_str())
        })
        .collect();
    let mut out = vec![
        checked(
            "preserved_inputs",
            changed.is_empty(),
            if changed.is_empty() {
                "All pinned source and seed bytes are unchanged".into()
            } else {
                format!("Changed inputs: {}", changed.join(", "))
            },
        ),
        checked(
            "source_inspection",
            inspected_interview(calls, &m.inputs),
            "Requires a hash-matched read of the pinned interview, its imported copy or its extraction; metadata and instructions alone are insufficient",
        ),
        checked(
            "draft_state_and_language",
            new.iter().all(|d| {
                d.metadata["doc_status"] == "draft"
                    && d.metadata["language"] == m.language
                    && d.metadata
                        .get("delivery_status")
                        .is_none_or(|v| v.is_null() || v == "proposed")
                    && ["verified", "baseline_id", "reviewed_by", "reviewed_at"]
                        .iter()
                        .all(|k| d.metadata.get(*k).is_none_or(Value::is_null))
            }),
            format!(
                "{} new project records inspected; prose meaning and language still require human review",
                new.len()
            ),
        ),
        checked(
            "tool_outcomes_known",
            calls
                .iter()
                .all(|c| matches!(c.status.as_str(), "completed" | "reconciled")),
            "All dispatched calls must have recorded outcomes; a started call is never replayed by this evaluator",
        ),
    ];
    let paused = matches!(
        s.status,
        Status::WaitingApproval
            | Status::LimitReached
            | Status::Cancelled
            | Status::NeedsReconciliation
    ) || (matches!(m.case.id.as_str(), "draft-chain" | "draft-stages")
        && s.status == Status::WaitingClarification);
    if paused {
        out.push(Check {
            id: "execution_boundary".into(),
            outcome: Outcome::Partial,
            evidence: format!(
                "Session stopped at {:?}; remaining observations are not a completed outcome",
                s.status
            ),
        });
    }
    match m.case.id.as_str() {
        "missing-inputs" | "wiki-code-conflict" => {
            out.push(checked("actual_unresolved_question",s.status==Status::WaitingClarification && s.questions.iter().any(|q| !q.resolved && q.answer.is_none() && records.active.iter().any(|d|d.path==q.path && d.kind=="question" && d.metadata["workflow_status"]=="open")),"Requires a persisted open question, no synthetic answer receipt, and an actual clarification pause"));
            out.push(checked("dependent_work_pending",new.iter().all(|d|!["brd","prd","requirement","pbi"].contains(&d.kind.as_str())) && !records.active.iter().any(|d|d.metadata["workflow_status"]=="forecast"),"No dependent requirement/PBI or completed forecast may be created while the supplied rule is missing"));
            if m.case.id == "wiki-code-conflict" {
                out.push(checked("both_pinned_sources_inspected",read_path(calls,|p|p.contains("03_Systems/Snapshots/")&&p.ends_with("/openwiki/Access.md"))&&read_path(calls,|p|p.contains("03_Systems/Snapshots/")&&p.ends_with("/src/access.rs")),"Requires successful reads of both the pinned Wiki and code, not merely the repository catalog"));
            }
        }
        "draft-chain" | "draft-stages" => {
            out.push(checked(
                "completed_turn",
                s.status == Status::Completed,
                format!("Recorded session state: {:?}", s.status),
            ));
            for kind in ["glossary", "brd", "prd", "requirement", "pbi"] {
                out.push(checked(
                    &format!("created_{kind}"),
                    new.iter().any(|d| d.kind == kind),
                    format!("Actual new {kind} file required; a promised filename does not count"),
                ));
            }
            let linked = new.iter().filter(|d| d.kind == "pbi").any(|pbi| {
                targets(&records, pbi, "satisfies", "requirement")
                    .iter()
                    .any(|req| {
                        targets(&records, req, "parents", "prd").iter().any(|prd| {
                            targets(&records, pbi, "parents", "prd")
                                .iter()
                                .any(|p| p.id == prd.id)
                                && !targets(&records, prd, "parents", "brd").is_empty()
                        })
                    })
            });
            out.push(checked("connected_requirement_chain",linked,"PBI satisfies a requirement whose PRD links a BRD; PBI and requirement share the PRD"));
            let mut writes = BTreeMap::new();
            for call in calls.iter().filter(|c| c.name == "write_draft") {
                if let Some((a, r)) = decoded(call)
                    && r["status"] == "draft_saved"
                    && let Some(path) = a["path"].as_str()
                    && let Some(cs) = a["citations"].as_array()
                    && w.read(path)
                        .is_ok_and(|v| r["sha256"] == hash(v.as_bytes()))
                    && cs.iter().all(|c| {
                        c["path"].as_str().is_some_and(|p| {
                            w.read(p).is_ok_and(|v| c["sha256"] == hash(v.as_bytes()))
                        })
                    })
                {
                    writes.insert(path.to_owned(), cs.clone());
                }
            }
            out.push(checked("versioned_draft_citations",!new.is_empty() && new.iter().all(|d|writes.contains_key(&d.path)&&has_origin(&d.path,&writes,&m.inputs,&mut vec![])),"Final draft bytes must match successful write evidence, current source hashes and a citation path to supplied source material"));
            let pbis: Vec<_> = new.iter().filter(|d| d.kind == "pbi").collect();
            out.push(checked("unscheduled_unknown_effort",!pbis.is_empty()&&pbis.iter().all(|d|d.metadata["sprint"].is_null() && crate::planning::table(&d.body,&["Role","O","M","P"]).is_ok_and(|rows|rows.iter().all(|r|r.iter().skip(1).all(|v|v.trim().is_empty())))),"No supplied effort or iteration: PBI sprint and estimate values must remain unknown"));
            out.push(checked(
                "coverage_executed",
                pm(calls, "coverage").is_some_and(|v| v["successful"] == true),
                "Requires an executed native coverage command",
            ));
            let validation = pm(calls, "validate");
            out.push(Check { id:"validation_findings".into(),outcome:match &validation {Some(v) if v["errors"]==0=>Outcome::Pass,Some(v) if v["errors"].is_number()=>Outcome::Partial,_=>Outcome::Fail},evidence:validation.map_or("No validation findings were recorded".into(),|v|format!("Recorded errors: {}; warnings: {}. Missing owners and review inputs remain gaps, not invented values",v["errors"],v["warnings"])) });
        }
        "capacity-forecast" => {
            out.push(checked(
                "completed_turn",
                s.status == Status::Completed,
                format!("Recorded session state: {:?}", s.status),
            ));
            let forecast = pm(calls, "plan");
            let caps = forecast
                .as_ref()
                .and_then(|r| r["forecast"]["caps"].as_array());
            let near = |v: &Value, n: f64| v.as_f64().is_some_and(|x| (x - n).abs() < 1e-9);
            let values = caps.is_some_and(|cs| {
                [("Backend", 3.6, 4.0), ("QA", 1.8, 1.0)]
                    .iter()
                    .all(|(role, capacity, effort)| {
                        cs.iter().any(|c| {
                            c["role"] == *role
                                && c["sprint"] == 1
                                && near(&c["capacity"], *capacity)
                                && near(&c["effort"], *effort)
                        })
                    })
            });
            out.push(checked("native_capacity_and_demand",forecast.as_ref().is_some_and(|v|v["successful"]==true&&v["engine"]=="rust")&&values,"Native result must contain Backend capacity/demand 3.6/4.0 and QA 1.8/1.0 person-days from the fixture inputs"));
            out.push(checked("unknown_estimate_preserved",forecast.as_ref().and_then(|v|v["forecast"]["items"].as_array()).is_some_and(|items|items.iter().any(|i|i["id"]=="PBI-EVAL-UNKNOWN" && i["sprint"].is_null()&&i["original"].is_null()&&i["remaining"].is_null())),"Unscheduled unknown effort must remain null rather than zero or an invented estimate"));
        }
        _ => anyhow::bail!("Unknown evaluation case"),
    }
    if paused || s.status == Status::Failed {
        for check in &mut out {
            if check.outcome == Outcome::Fail
                && !matches!(
                    check.id.as_str(),
                    "preserved_inputs"
                        | "draft_state_and_language"
                        | "tool_outcomes_known"
                        | "dependent_work_pending"
                )
            {
                check.outcome = Outcome::NotRun;
                check.evidence = format!(
                    "Not fully observed because execution stopped at {:?}. {}",
                    s.status, check.evidence
                );
            }
        }
        if s.status == Status::Failed {
            out.push(checked("execution_failed",false,"The configured-provider execution failed. Inspect the private Session.json; no retry occurred."));
        }
    }
    Ok(out)
}
