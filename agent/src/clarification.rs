//! Explicit question scopes and fail-closed Agent draft gates. No inferred answers.
use crate::{
    inspection,
    records::{self, Records},
    workspace::{self, Document, Workspace},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub const PROFILE: &str = "clarification-v1";
const RESPONSE_FIELDS: &[&str] = &[
    "workflow_status",
    "answer_path",
    "answer_sha256",
    "answer_source",
    "answer_received_at",
    "answer_for_sha256",
    "resolution",
    "resolution_at",
];
#[derive(Clone, Serialize)]
pub struct Blocker {
    pub id: String,
    pub path: String,
    pub reason: String,
}
#[derive(Serialize)]
pub struct Entry {
    pub document: Document,
    pub legacy: bool,
    pub blocked: Vec<String>,
    pub problem: Option<String>,
    pub answer: Option<String>,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub loaded_at: String,
    pub entries: Vec<Entry>,
    pub problem: Option<String>,
}
impl Snapshot {
    pub fn load(w: &Workspace) -> Self {
        match Records::load(w) {
            Err(e) => Self {
                loaded_at: records::timestamp(),
                entries: vec![],
                problem: Some(format!("{e:#}")),
            },
            Ok(records) => {
                let entries = records
                    .active
                    .iter()
                    .filter(|d| d.kind == "question")
                    .map(|d| {
                        let legacy = d.metadata.get("clarification_schema").is_none();
                        let result = if legacy {
                            Ok(BTreeMap::new())
                        } else {
                            affected(&records, d)
                        };
                        let (blocked, mut problem) = match result {
                            Ok(v) => (v.into_keys().collect(), None),
                            Err(e) => (vec![], Some(format!("{e:#}"))),
                        };
                        let answer = if legacy { None } else { answer(w, d).ok() };
                        if !legacy && d.metadata.get("answer_path").is_some() && answer.is_none() {
                            problem.get_or_insert_with(|| "The answer no longer matches the question, scope or source. Request a new response.".into());
                        }
                        Entry {
                            document: d.clone(),
                            legacy,
                            blocked,
                            problem,
                            answer,
                        }
                    })
                    .collect();
                Self {
                    loaded_at: records::timestamp(),
                    entries,
                    problem: None,
                }
            }
        }
    }
}

fn list(meta: &Value, key: &str) -> Result<Vec<String>> {
    meta[key]
        .as_array()
        .with_context(|| format!("{key} must be a list"))?
        .iter()
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .with_context(|| format!("{key} entries must be nonempty strings"))
        })
        .collect()
}
fn safe_target(w: &Workspace, path: &str) -> Result<()> {
    w.path(path)?;
    ensure!(
        path.ends_with(".md")
            && records::MANAGED
                .iter()
                .any(|r| path.starts_with(&format!("{r}/")))
            && !path
                .split('/')
                .any(|p| matches!(p, "Baselines" | "Snapshots" | "Assets")),
        "Affected path must be an exact managed Markdown path"
    );
    Ok(())
}
pub fn scope(records: &Records, meta: &Value) -> Result<()> {
    ensure!(
        meta["clarification_schema"] == PROFILE,
        "Unsupported clarification profile"
    );
    let ids = list(meta, "affected_ids")?;
    let paths = list(meta, "affected_paths")?;
    ensure!(
        ids.len() + paths.len() <= 100,
        "Question scope exceeds 100 explicit targets; split the question"
    );
    ensure!(
        ids.iter().collect::<BTreeSet<_>>().len() == ids.len()
            && paths.iter().collect::<BTreeSet<_>>().len() == paths.len(),
        "Question scope must not contain duplicate targets"
    );
    match meta["blocking_scope"].as_str() {
        Some("project") => ensure!(
            ids.is_empty() && paths.is_empty(),
            "Project scope has no record subset"
        ),
        Some("records") => ensure!(
            !ids.is_empty() || !paths.is_empty(),
            "Record scope needs affected_ids or exact affected_paths"
        ),
        _ => anyhow::bail!("blocking_scope must be project or records"),
    }
    for id in ids {
        records::identifier(&id)?;
        records.by_id(&id).with_context(|| {
            format!("Missing scoped record {id}; repair scope or restore the record")
        })?;
    }
    for path in paths {
        safe_target(&records.workspace, &path)?;
    }
    Ok(())
}
pub fn ask_metadata(w: &Workspace, args: &Value, id: &str, session: &str) -> Result<Value> {
    let meta = json!({"type":"question","id":id,"title":crate::tools::string(args,"question")?,"language":w.language()?,
        "clarification_schema":PROFILE,"workflow_status":"open","blocking_scope":args["blocking_scope"].as_str().unwrap_or("project"),
        "affected_ids":args.get("affected_ids").cloned().unwrap_or(json!([])),"affected_paths":args.get("affected_paths").cloned().unwrap_or(json!([])),
        "owner":null,"due_date":null,"asked_at":records::timestamp(),"session_id":session});
    scope(&Records::load(w)?, &meta)?;
    Ok(meta)
}
fn basis(d: &Document) -> Result<String> {
    let mut meta = d.metadata.clone();
    for key in RESPONSE_FIELDS {
        meta.as_object_mut()
            .context("Question metadata must be a mapping")?
            .remove(*key);
    }
    Ok(workspace::hash(
        records::canonical_json(&json!({"metadata":meta,"body":d.body.trim()} ))?.as_bytes(),
    ))
}
pub fn answer(w: &Workspace, d: &Document) -> Result<String> {
    ensure!(
        d.metadata["answer_for_sha256"] == basis(d)?,
        "Answer does not match the current question/scope; request a new response"
    );
    let path = records::string(&d.metadata, "answer_path")?;
    ensure!(
        path.starts_with("01_Sources/Assets/USER-") && path.ends_with(".txt"),
        "Answer must cite the captured local user input"
    );
    let text = w.read(&path)?;
    ensure!(
        !text.trim().is_empty() && d.metadata["answer_sha256"] == workspace::hash(text.as_bytes()),
        "Answer source changed or is empty"
    );
    ensure!(
        d.metadata["answer_source"]
            == format!(
                "local-user/session/{}",
                records::string(&d.metadata, "session_id")?
            ),
        "Answer session provenance does not match"
    );
    chrono::DateTime::parse_from_rfc3339(&records::string(&d.metadata, "answer_received_at")?)?;
    Ok(text)
}
pub fn record_answer(w: &Workspace, path: &str, session: &str, source: &str) -> Result<()> {
    let mut d = records::read(w, path)?;
    ensure!(
        (d.metadata["session_id"].is_null() || d.metadata["session_id"] == session)
            && d.metadata["workflow_status"] == "open",
        "Question is not open in this session"
    );
    scope(&Records::load(w)?, &d.metadata)?;
    ensure!(
        source.starts_with("01_Sources/Assets/USER-") && source.ends_with(".txt"),
        "Answer must use a captured user input"
    );
    d.metadata["session_id"] = json!(session);
    let text = w.read(source)?;
    ensure!(!text.trim().is_empty(), "Answer cannot be empty");
    let mut meta = d.metadata.clone();
    meta["answer_for_sha256"] = json!(basis(&d)?);
    meta["answer_path"] = json!(source);
    meta["answer_sha256"] = json!(workspace::hash(text.as_bytes()));
    meta["answer_source"] = json!(format!("local-user/session/{session}"));
    meta["answer_received_at"] = json!(records::timestamp());
    meta.as_object_mut().unwrap().remove("resolution");
    meta.as_object_mut().unwrap().remove("resolution_at");
    ensure!(
        workspace::hash(w.read(path)?.as_bytes()) == d.hash,
        "Question changed while recording the response; retry"
    );
    workspace::atomic_write(&w.path(path)?, workspace::note(meta, &d.body)?.as_bytes())
}
pub fn resolve(w: &Workspace, path: &str, session: &str, resolution: &str) -> Result<()> {
    let d = records::read(w, path)?;
    ensure!(
        d.metadata["session_id"] == session && d.metadata["workflow_status"] == "open",
        "Question is not open in this session"
    );
    scope(&Records::load(w)?, &d.metadata)?;
    answer(w, &d)?;
    ensure!(
        !resolution.trim().is_empty(),
        "Explain which missing premise the response supplies"
    );
    let mut meta = d.metadata;
    ensure!(
        workspace::hash(w.read(path)?.as_bytes()) == d.hash,
        "Question changed while resolving; retry"
    );
    meta["workflow_status"] = json!("resolved");
    meta["resolution"] = json!(resolution);
    meta["resolution_at"] = json!(records::timestamp());
    workspace::atomic_write(&w.path(path)?, workspace::note(meta, &d.body)?.as_bytes())
}
fn affected(records: &Records, d: &Document) -> Result<BTreeMap<String, String>> {
    scope(records, &d.metadata)?;
    match d.metadata["workflow_status"].as_str() {
        Some("resolved") => {
            answer(&records.workspace, d)?;
            records::string(&d.metadata, "resolution")?;
            chrono::DateTime::parse_from_rfc3339(&records::string(&d.metadata, "resolution_at")?)?;
            return Ok(BTreeMap::new());
        }
        Some("open") => {}
        _ => anyhow::bail!("Scoped questions must be open or resolved with an actual response"),
    }
    let mut paths = BTreeMap::new();
    if d.metadata["blocking_scope"] == "project" {
        paths.insert("*".into(), "Project-wide information gap".into());
        return Ok(paths);
    }
    let mut roots = list(&d.metadata, "affected_ids")?;
    for path in list(&d.metadata, "affected_paths")? {
        paths.insert(path.clone(), "Explicit affected path".into());
        if let Some(id) = records
            .active
            .iter()
            .find(|r| r.path == path)
            .and_then(|r| r.id.clone())
        {
            roots.push(id);
        }
    }
    for id in roots {
        let root = records.by_id(&id)?;
        paths.insert(root.path.clone(), format!("Explicit affected record {id}"));
        for c in inspection::graph(records, &id)?.0 {
            paths.entry(c.path).or_insert_with(|| {
                format!(
                    "Recorded dependency: {}",
                    c.chain
                        .iter()
                        .map(|e| format!("{} → {} ({})", e.from, e.to, e.basis))
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            });
        }
    }
    Ok(paths)
}

/// Evaluate both current and proposed dependencies; removing a link cannot bypass a gate.
pub fn blockers(
    w: &Workspace,
    path: &str,
    content: Option<&str>,
    citations: &[Value],
) -> Result<Vec<Blocker>> {
    let records = Records::load(w)?;
    let questions: Vec<_> = records
        .active
        .iter()
        .filter(|d| d.kind == "question" && d.metadata.get("clarification_schema").is_some())
        .collect();
    if questions.is_empty() {
        return Ok(vec![]);
    }
    let mut targets = BTreeSet::from([path.to_string()]);
    let mut unresolved = vec![];
    let mut versions = content
        .map(workspace::parse_note)
        .transpose()?
        .into_iter()
        .collect::<Vec<_>>();
    let proposed_id = versions
        .first()
        .and_then(|(m, _)| m["id"].as_str())
        .map(String::from);
    for d in records.active.iter().filter(|d| {
        d.path == path
            || proposed_id
                .as_deref()
                .is_some_and(|id| d.id.as_deref() == Some(id))
    }) {
        versions.push((d.metadata.clone(), d.body.clone()));
    }
    for (meta, body) in versions {
        // The same ID under a new filename still refers to the scoped record.
        if let Some(id) = meta["id"].as_str().and_then(|id| records.by_id(id).ok()) {
            targets.insert(id.path.clone());
        }
        for raw in records::wiki_refs(&format!("{meta}\n{body}")) {
            match records.resolve_path(raw) {
                Ok(p) => {
                    targets.insert(p);
                }
                Err(e) => unresolved.push(e.to_string()),
            }
        }
        for c in meta["contexts"].as_array().into_iter().flatten() {
            let matches: Vec<_> = records
                .active
                .iter()
                .filter(|d| d.kind == "glossary" && d.metadata["context"] == *c)
                .collect();
            if matches.len() == 1 {
                targets.insert(matches[0].path.clone());
            } else {
                unresolved.push("Ambiguous or missing glossary context".into());
            }
        }
        for p in meta["evidence_paths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .chain(
                meta["sources"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s["resource"].as_str()),
            )
        {
            targets.insert(p.into());
        }
    }
    for citation in citations {
        if let Some(p) = citation["path"].as_str() {
            targets.insert(p.into());
        }
    }
    let mut found = vec![];
    for q in questions {
        let reason = match affected(&records, q) {
            Err(e) => Some(format!("Question contract unavailable: {e:#}")),
            Ok(paths) if paths.is_empty() => None,
            Ok(paths) if paths.contains_key("*") => paths.get("*").cloned(),
            Ok(paths) => targets
                .iter()
                .find_map(|p| paths.get(p).map(|r| format!("{p}: {r}")))
                .or_else(|| {
                    (!unresolved.is_empty()).then(|| {
                        format!("Cannot establish independence: {}", unresolved.join("; "))
                    })
                }),
        };
        if let Some(reason) = reason {
            found.push(Blocker {
                id: q.id.clone().unwrap(),
                path: q.path.clone(),
                reason,
            });
        }
    }
    Ok(found)
}
pub fn ensure_draft(w: &Workspace, path: &str, content: &str, citations: &[Value]) -> Result<()> {
    let blocked = blockers(w, path, Some(content), citations)?;
    ensure!(
        blocked.is_empty(),
        "Unresolved clarification blocks this draft: {}",
        blocked
            .iter()
            .map(|b| format!("{}: {}", b.id, b.reason))
            .collect::<Vec<_>>()
            .join("; ")
    );
    Ok(())
}
pub fn check(records: &Records, d: &Document) -> Result<()> {
    affected(records, d).map(|_| ())
}
pub fn has_open(w: &Workspace) -> Result<bool> {
    let r = Records::load(w)?;
    for d in r
        .active
        .iter()
        .filter(|d| d.kind == "question" && d.metadata.get("clarification_schema").is_some())
    {
        if !affected(&r, d)?.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}
