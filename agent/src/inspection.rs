//! Read-only review candidates and byte-grounded comparisons. No semantic verdicts.
use crate::{
    baselines,
    records::{self, Records, string},
    sources,
    workspace::{Document, Workspace, hash},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Target {
    #[default]
    Impact,
    Baseline(String),
    Source,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: String,
    pub target: Target,
}
#[derive(Clone, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub basis: String,
}
#[derive(Serialize)]
pub struct Candidate {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub chain: Vec<Edge>,
}
#[derive(Serialize)]
pub struct Snapshot {
    pub id: String,
    pub path: String,
    pub title: String,
    pub loaded_at: String,
    pub current_sha256: String,
    pub candidates: Vec<Candidate>,
    pub unresolved: Vec<String>,
    /// Discovery only. Membership and integrity are checked on explicit selection.
    pub baselines: Vec<String>,
    pub comparison: Option<Comparison>,
    pub comparison_error: Option<String>,
    pub boundary: String,
}
#[derive(Serialize)]
pub struct Comparison {
    pub basis: Value,
    pub status: String,
    pub diff: Option<Diff>,
}
#[derive(Serialize)]
pub struct Diff {
    pub old_sha256: String,
    pub new_sha256: String,
    pub old_final_newline: bool,
    pub new_final_newline: bool,
    pub algorithm: String,
    pub lines: Vec<DiffLine>,
}
#[derive(Serialize)]
pub struct DiffLine {
    pub kind: char,
    pub old: Option<usize>,
    pub new: Option<usize>,
    pub text: String,
}

fn graph(records: &Records, id: &str) -> Result<(Vec<Candidate>, Vec<String>)> {
    let bypath: BTreeMap<_, _> = records
        .active
        .iter()
        .map(|d| (d.path.as_str(), d.id.as_deref().unwrap()))
        .collect();
    let mut reverse: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut unresolved = BTreeSet::new();
    for d in records.active.iter().filter(|d| d.kind != "report") {
        let consumer = d.id.as_deref().unwrap();
        let mut refs = BTreeMap::new();
        let text = format!("{}\n{}", serde_json::to_string(&d.metadata)?, d.body);
        for raw in records::wiki_refs(&text) {
            match records.resolve_path(raw) {
                Ok(path) => {
                    if let Some(target) = bypath.get(path.as_str()) {
                        refs.insert(target.to_string(), format!("wikilink {raw}"));
                    }
                }
                Err(e) => {
                    unresolved.insert(format!("{consumer}: {raw}: {e}"));
                }
            }
        }
        if let Some(contexts) = d.metadata.get("contexts") {
            for context in contexts.as_array().context("contexts must be a list")? {
                let context = context.as_str().context("context must be a string")?;
                let targets: Vec<_> = records
                    .active
                    .iter()
                    .filter(|t| t.kind == "glossary" && t.metadata["context"] == context)
                    .collect();
                if targets.len() != 1 {
                    unresolved.insert(format!(
                        "{consumer}: context {context} has {} definitions",
                        targets.len()
                    ));
                } else {
                    refs.insert(
                        targets[0].id.clone().unwrap(),
                        format!("contexts: {context}"),
                    );
                }
            }
        }
        if d.kind == "evidence" {
            for (field, prefix, expected) in [
                ("source_id", "", "source"),
                ("repo_id", "REPO-", "repository"),
            ] {
                if let Some(value) = d.metadata.get(field) {
                    let target = value.as_str().map(|s| format!("{prefix}{s}"));
                    match target.and_then(|t| records.by_id(&t).ok()) {
                        Some(t) if t.kind == expected => {
                            refs.insert(t.id.clone().unwrap(), format!("evidence {field}"));
                        }
                        _ => {
                            unresolved.insert(format!("{consumer}: unresolved {field}"));
                        }
                    }
                }
            }
        }
        for (target, basis) in refs {
            reverse
                .entry(target)
                .or_default()
                .insert(consumer.into(), basis);
        }
    }
    let mut chains: BTreeMap<String, Vec<Edge>> = BTreeMap::from([(id.into(), vec![])]);
    let mut queue = VecDeque::from([id.to_string()]);
    while let Some(from) = queue.pop_front() {
        if let Some(edges) = reverse.get(&from) {
            for (to, basis) in edges {
                if !chains.contains_key(to) {
                    let mut chain = chains[&from].clone();
                    chain.push(Edge {
                        from: from.clone(),
                        to: to.clone(),
                        basis: basis.clone(),
                    });
                    chains.insert(to.clone(), chain);
                    queue.push_back(to.clone());
                }
            }
        }
    }
    chains.remove(id);
    let candidates = chains
        .into_iter()
        .map(|(id, chain)| {
            let d = records.by_id(&id).unwrap();
            Candidate {
                id,
                path: d.path.clone(),
                kind: d.kind.clone(),
                chain,
            }
        })
        .collect();
    Ok((candidates, unresolved.into_iter().collect()))
}

pub fn load(w: &Workspace, request: &Request) -> Result<Snapshot> {
    records::identifier(&request.id)?;
    let records = Records::load(w)?;
    let d = records.by_id(&request.id)?;
    let (candidates, mut unresolved) = graph(&records, &request.id)?;
    let mut baselines = vec![];
    let directory = w.path("00_Governance/Baselines")?;
    if directory.exists() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let Some(id) = entry.file_name().to_str().map(String::from) else {
                continue;
            };
            if records::identifier(&id).is_err() {
                continue;
            }
            match w.path(&format!("00_Governance/Baselines/{id}/manifest.json")) {
                Ok(path) if path.is_file() => baselines.push(id),
                Err(e) => unresolved.push(format!("Baseline {id}: {e}")),
                _ => {}
            }
        }
    }
    baselines.sort();
    let comparison = match &request.target {
        Target::Impact => Ok(None),
        Target::Baseline(id) => baseline(&records, d, id).map(Some),
        Target::Source => source(&records, d).map(Some),
    };
    let (comparison, comparison_error) = match comparison {
        Ok(c) => (c, None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    // Reject mixed reads of active notes; snapshots have their own byte checks.
    let stamp = |r: &Records| {
        r.active
            .iter()
            .map(|d| (&d.path, &d.hash))
            .map(|(p, h)| (p.clone(), h.clone()))
            .collect::<BTreeMap<_, _>>()
    };
    ensure!(
        stamp(&records) == stamp(&Records::load(w)?),
        "Project changed during inspection; refresh"
    );
    Ok(Snapshot {
        id: request.id.clone(), path: d.path.clone(), title: d.title.clone(),
        loaded_at: records::timestamp(), current_sha256: d.hash.clone(), candidates,
        unresolved, baselines, comparison, comparison_error,
        boundary: "Recorded references identify review candidates, not invalid requirements. One shortest reference chain is shown per candidate. Unresolved links and unrecorded relationships limit coverage. Captured baselines and evidence are never rewritten.".into(),
    })
}

fn baseline(records: &Records, d: &Document, id: &str) -> Result<Comparison> {
    let (manifest, sha) = baselines::load(&records.workspace, id)?;
    let entry = manifest["documents"]
        .get(d.id.as_deref().unwrap())
        .context("Document is not in this baseline")?;
    let path = baselines::snapshot_path(id, entry)?;
    let old = records.workspace.read(&path)?;
    let current = records.workspace.read(&d.path)?;
    ensure!(
        hash(old.as_bytes()) == entry["file_sha256"] && hash(current.as_bytes()) == d.hash,
        "Document changed during comparison; refresh"
    );
    let (approved, decision, approval_note) = match baselines::approval(records, id) {
        Ok((approved_manifest, path)) => {
            ensure!(
                approved_manifest == manifest,
                "Baseline changed during approval check; refresh"
            );
            (true, Some(path), None)
        }
        Err(e) => (false, None, Some(e.to_string())),
    };
    compare(
        &old,
        &current,
        json!({"type":"baseline_to_current","baseline_id":id,
        "manifest_sha256":sha,"approval_verified":approved,"approval_decision":decision,
        "approval_note":approval_note,"old_path":path,"new_path":d.path,
        "baseline_version":entry["version"],"current_version":d.metadata["version"],
        "scope":"Exact Markdown bytes, including frontmatter; approval is a recorded decision, not identity attestation"}),
    )
}

fn source(records: &Records, d: &Document) -> Result<Comparison> {
    ensure!(
        d.kind == "evidence",
        "Source comparison requires a repository evidence record"
    );
    let repo_id = string(&d.metadata, "repo_id")
        .context("Material evidence has no repository snapshot comparison")?;
    let old_id = string(&d.metadata, "snapshot_id")?;
    let path = string(&d.metadata, "source_path")?;
    let old = sources::manifest(&records.workspace, &repo_id, &old_id)?;
    let expected = old["files"]
        .get(&path)
        .context("Evidence path is missing from pinned snapshot")?;
    ensure!(
        *expected == d.metadata["source_sha256"],
        "Evidence source hash does not match pinned snapshot"
    );
    ensure!(
        old["revision"] == d.metadata["revision"] && old["dirty"] == d.metadata["dirty"],
        "Evidence revision does not match pinned snapshot"
    );
    let repo = records.by_id(&format!("REPO-{repo_id}"))?;
    ensure!(
        repo.kind == "repository",
        "Evidence repository registration has the wrong type"
    );
    let new_id = string(&repo.metadata, "latest_snapshot")?;
    let new = if old_id == new_id {
        old.clone()
    } else {
        sources::manifest(&records.workspace, &repo_id, &new_id)?
    };
    let basis = json!({"type":"pinned_source_to_latest_snapshot","repo_id":repo_id,
        "source_path":path,"old_snapshot":old_id,"new_snapshot":new_id,
        "old_revision":old["revision"],"new_revision":new["revision"],
        "old_dirty":old["dirty"],"new_dirty":new["dirty"],
        "old_captured_at":old["captured_at"],"new_captured_at":new["captured_at"],
        "evidence_locator":d.metadata["locator"],"assertion_type":d.metadata["assertion_type"],
        "scope":"Full captured file, not only the evidence locator. No live repository read or automatic synchronization"});
    let Some(new_hash) = new["files"].get(&path) else {
        return Ok(Comparison {
            basis,
            status: "not_captured_in_latest_scope; deletion is not established".into(),
            diff: None,
        });
    };
    let read = |snapshot: &str, expected: &Value| -> Result<String> {
        let text = records
            .workspace
            .read(&format!("03_Systems/Snapshots/{snapshot}/files/{path}"))?;
        ensure!(
            hash(text.as_bytes()) == *expected,
            "Snapshot changed during comparison; refresh"
        );
        Ok(text)
    };
    compare(&read(&old_id, expected)?, &read(&new_id, new_hash)?, basis)
}
fn compare(old: &str, new: &str, basis: Value) -> Result<Comparison> {
    Ok(Comparison {
        basis,
        status: if old == new {
            "unchanged"
        } else {
            "changed; review required"
        }
        .into(),
        diff: Some(diff(old, new)?),
    })
}

/// Bounded exact line edit script; coarse replacement is explicit, never a semantic guess.
pub fn diff(old: &str, new: &str) -> Result<Diff> {
    let a: Vec<_> = old.split_inclusive('\n').collect();
    let b: Vec<_> = new.split_inclusive('\n').collect();
    ensure!(
        a.len() + b.len() <= 20_000,
        "Comparison exceeds the 20,000 combined line limit; narrow the source scope"
    );
    let prefix = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (n, m) = (a.len() - prefix - suffix, b.len() - prefix - suffix);
    let detailed = (n + 1).saturating_mul(m + 1) <= 1_000_000;
    let mut lines = vec![];
    let line = |kind, old, new, text: &str| DiffLine {
        kind,
        old,
        new,
        text: text.strip_suffix('\n').unwrap_or(text).to_string(),
    };
    for (i, text) in a[..prefix].iter().enumerate() {
        lines.push(line(' ', Some(i + 1), Some(i + 1), text));
    }
    let (mut i, mut j) = (0, 0);
    let mut table = if detailed {
        vec![0u32; (n + 1) * (m + 1)]
    } else {
        vec![]
    };
    if detailed {
        for x in (0..n).rev() {
            for y in (0..m).rev() {
                table[x * (m + 1) + y] = if a[prefix + x] == b[prefix + y] {
                    1 + table[(x + 1) * (m + 1) + y + 1]
                } else {
                    table[(x + 1) * (m + 1) + y].max(table[x * (m + 1) + y + 1])
                };
            }
        }
    }
    while i < n || j < m {
        if detailed && i < n && j < m && a[prefix + i] == b[prefix + j] {
            lines.push(line(
                ' ',
                Some(prefix + i + 1),
                Some(prefix + j + 1),
                a[prefix + i],
            ));
            i += 1;
            j += 1;
        } else if i < n
            && (j == m || !detailed || table[(i + 1) * (m + 1) + j] >= table[i * (m + 1) + j + 1])
        {
            lines.push(line('-', Some(prefix + i + 1), None, a[prefix + i]));
            i += 1;
        } else {
            lines.push(line('+', None, Some(prefix + j + 1), b[prefix + j]));
            j += 1;
        }
    }
    for k in 0..suffix {
        lines.push(line(
            ' ',
            Some(a.len() - suffix + k + 1),
            Some(b.len() - suffix + k + 1),
            a[a.len() - suffix + k],
        ));
    }
    Ok(Diff {
        old_sha256: hash(old.as_bytes()),
        new_sha256: hash(new.as_bytes()),
        old_final_newline: old.ends_with('\n'),
        new_final_newline: new.ends_with('\n'),
        algorithm: if detailed {
            "line LCS"
        } else {
            "bounded coarse replacement; not a minimal diff"
        }
        .into(),
        lines,
    })
}
