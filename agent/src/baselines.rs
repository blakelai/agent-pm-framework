use crate::{
    records::{self, Records, content_hash, identifier, positive_version, string, truth},
    workspace::{Document, Workspace, atomic_write, hash},
};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Component, Path},
};

pub const DOCUMENT_TYPES: &[&str] = &[
    "brd",
    "prd",
    "requirement",
    "glossary",
    "design",
    "contract",
];
pub fn capture(records: &Records, id: &str, includes: &[String]) -> Result<Value> {
    identifier(id)?;
    ensure!(
        !includes.is_empty() && includes.iter().collect::<BTreeSet<_>>().len() == includes.len(),
        "Baseline needs a nonempty, unique ID list"
    );
    let relative = format!("00_Governance/Baselines/{id}");
    let folder = records.workspace.path(&relative)?;
    ensure!(
        !folder.exists(),
        "Baseline ID already exists; capture a new ID"
    );
    let mut entries = BTreeMap::new();
    let mut payloads = vec![];
    for id in includes {
        identifier(id)?;
        let d = records.by_id(id)?;
        let blocked = crate::clarification::blockers(&records.workspace, &d.path, None, &[])?;
        ensure!(
            blocked.is_empty(),
            "Baseline blocked by unresolved clarification: {}",
            blocked
                .iter()
                .map(|b| b.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        ensure!(
            DOCUMENT_TYPES.contains(&d.kind.as_str()),
            "Baseline supports controlled documents only: {id}"
        );
        let version = positive_version(&d.metadata)?;
        let bytes = fs::read(records.workspace.path(&d.path)?)?;
        ensure!(
            hash(&bytes) == d.hash,
            "Document changed during capture: {id}; refresh before retry"
        );
        entries.insert(id, json!({"path":d.path,"version":version,"content_sha256":content_hash(&d.metadata,&d.body)?,"file_sha256":hash(&bytes),"snapshot":format!("files/{id}.md")}));
        payloads.push((id, bytes));
    }
    let manifest =
        json!({"schema_version":2,"id":id,"created_at":records::timestamp(),"documents":entries});
    let mut bytes = serde_json::to_vec_pretty(&manifest)?;
    bytes.push(b'\n');
    fs::create_dir_all(folder.parent().unwrap())?;
    // Exclusive reservation prevents replacement. The manifest is published last;
    // an interrupted partial directory cannot be mistaken for a complete candidate.
    fs::create_dir(&folder)?;
    fs::create_dir(folder.join("files"))?;
    for (id, bytes) in payloads {
        atomic_write(&folder.join(format!("files/{id}.md")), &bytes)?;
    }
    atomic_write(&folder.join("manifest.json"), &bytes)?;
    Ok(
        json!({"manifest":format!("{relative}/manifest.json"),"manifest_sha256":hash(&bytes),"candidate":true,"approved":false}),
    )
}
pub(crate) fn snapshot_path(id: &str, entry: &Value) -> Result<String> {
    let snapshot = string(entry, "snapshot")?;
    ensure!(
        Path::new(&snapshot)
            .components()
            .all(|c| matches!(c, Component::Normal(_))),
        "Baseline snapshot must stay inside its folder"
    );
    Ok(format!("00_Governance/Baselines/{id}/{snapshot}"))
}
pub fn load(workspace: &Workspace, id: &str) -> Result<(Value, String)> {
    identifier(id)?;
    let text = workspace.read(&format!("00_Governance/Baselines/{id}/manifest.json"))?;
    let manifest: Value = serde_json::from_str(&text)?;
    ensure!(
        manifest["id"] == id && manifest["schema_version"] == 2,
        "Invalid baseline manifest"
    );
    let entries = manifest["documents"]
        .as_object()
        .filter(|v| !v.is_empty())
        .context("Empty or invalid baseline")?;
    for (document_id, entry) in entries {
        identifier(document_id)?;
        let path = snapshot_path(id, entry)?;
        let d = records::read(workspace, &path)?;
        ensure!(
            d.hash == string(entry, "file_sha256")?,
            "Baseline snapshot modified: {document_id}"
        );
        ensure!(
            d.id.as_deref() == Some(document_id)
                && d.metadata["version"] == entry["version"]
                && content_hash(&d.metadata, &d.body)? == string(entry, "content_sha256")?,
            "Baseline content does not match manifest: {document_id}"
        );
    }
    Ok((manifest, hash(text.as_bytes())))
}
pub fn approval(records: &Records, id: &str) -> Result<(Value, String)> {
    let (manifest, sha) = load(&records.workspace, id)?;
    let matching: Vec<_> = records
        .active
        .iter()
        .filter(|d| {
            d.kind == "decision"
                && d.metadata["decision_type"] == "baseline_approval"
                && d.metadata["baseline_id"] == id
        })
        .collect();
    ensure!(
        !matching
            .iter()
            .any(|d| d.metadata["workflow_status"] == "rescinded"
                && !truth(&d.metadata["simulation"])),
        "Approval has been rescinded; capture a new baseline if a new decision is needed"
    );
    for d in matching {
        let m = &d.metadata;
        if !truth(&m["simulation"])
            && m["workflow_status"] == "accepted"
            && m["baseline_sha256"] == sha
            && ["decided_by", "decided_at", "decision_evidence"]
                .iter()
                .all(|k| truth(&m[k]))
        {
            return Ok((manifest, d.path.clone()));
        }
    }
    bail!("No accepted decision for this exact baseline manifest: {id}")
}
pub fn pinned_document(records: &Records, baseline: &str, id: &str) -> Result<Document> {
    let (manifest, _) = approval(records, baseline)?;
    let entry = manifest["documents"]
        .get(id)
        .context("Document is not in the approved baseline")?;
    records::read(&records.workspace, &snapshot_path(baseline, entry)?)
}
pub fn impact(records: &Records, id: &str) -> Result<Value> {
    identifier(id)?;
    let source = records.by_id(id)?;
    let bypath: BTreeMap<_, _> = records
        .active
        .iter()
        .map(|d| (d.path.as_str(), d.id.as_deref().unwrap()))
        .collect();
    let contexts: BTreeMap<_, _> = records
        .active
        .iter()
        .filter(|d| d.kind == "glossary")
        .filter_map(|d| Some((d.metadata["context"].as_str()?, d.id.as_deref()?)))
        .collect();
    let mut reverse: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for d in &records.active {
        if d.kind == "report" {
            continue;
        }
        let text = format!("{}\n{}", serde_json::to_string(&d.metadata)?, d.body);
        let mut refs = BTreeSet::new();
        for raw in records::wiki_refs(&text) {
            if let Ok(target) = records.resolve(raw)
                && let Some(id) = bypath.get(target.path.as_str())
            {
                refs.insert(id.to_string());
            }
        }
        if let Some(contexts_value) = d.metadata.get("contexts") {
            for c in contexts_value
                .as_array()
                .context("contexts must be a list")?
            {
                if let Some(id) = c.as_str().and_then(|c| contexts.get(c)) {
                    refs.insert(id.to_string());
                }
            }
        }
        for reference in refs {
            reverse
                .entry(reference)
                .or_default()
                .insert(d.id.clone().unwrap());
        }
    }
    let mut reached = BTreeSet::new();
    let mut queue = VecDeque::from([id.to_owned()]);
    while let Some(next) = queue.pop_front() {
        if let Some(downstream) = reverse.get(&next) {
            for d in downstream {
                if d != id && reached.insert(d.clone()) {
                    queue.push_back(d.clone());
                }
            }
        }
    }
    let candidates: Vec<_> = records
        .active
        .iter()
        .filter(|d| reached.contains(d.id.as_deref().unwrap()))
        .collect();
    let list = if candidates.is_empty() {
        "No downstream references found.".into()
    } else {
        candidates
            .iter()
            .map(|d| format!("- {} ({})", records::link(d), d.kind))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let body = format!(
        "# Change impact candidates\n\nStarting point: {}\n\nExpanded through document and context references. Matches are review candidates, not proof of invalidity. Historical baselines preserve their pinned versions.\n\n{list}",
        records::link(source)
    );
    let report = records::write_report(
        &records.workspace,
        &format!("90_Agent/Reports/Impact-{id}.md"),
        &format!("REPORT-IMPACT-{id}"),
        &body,
    )?;
    Ok(
        json!({"report":report,"affected":candidates.iter().map(|d| d.id.as_deref().unwrap()).collect::<Vec<_>>()}),
    )
}
