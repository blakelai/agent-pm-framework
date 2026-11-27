//! Compare pinned document versions and record operator-supplied review observations.
use crate::{
    baselines, inspection,
    records::{self, Records},
    sources,
    validation::{self, Finding},
    workspace::{Document, MAX_FILE, Workspace, atomic_create, hash, note},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: String,
    pub baseline: String,
    pub candidate: Option<String>,
    pub evidence: Vec<String>,
}

#[derive(Serialize)]
pub struct Review {
    pub basis_sha256: String,
    pub compared_at: String,
    pub semantic_findings_status: &'static str,
    pub basis: Basis,
    pub diff: inspection::Diff,
}

#[derive(Serialize)]
pub struct Basis {
    schema_version: u32,
    document_language: String,
    baseline: Manifest,
    approval: Record,
    before: Version,
    candidate_manifest: Option<Manifest>,
    candidate: Version,
    evidence: Vec<Citation>,
    affected: Vec<Impact>,
    mechanical_findings: Vec<Finding>,
    reference_gaps: Vec<String>,
}

#[derive(Serialize)]
struct Manifest {
    id: String,
    path: String,
    sha256: String,
}

#[derive(Serialize)]
struct Version {
    record: Record,
    bytes_path: String,
    version: i64,
    content_sha256: String,
}

#[derive(Serialize)]
struct Record {
    id: String,
    path: String,
    kind: String,
    sha256: String,
}

impl From<&Document> for Record {
    fn from(document: &Document) -> Self {
        Self {
            id: document.id.clone().unwrap_or_default(),
            path: document.path.clone(),
            kind: document.kind.clone(),
            sha256: document.hash.clone(),
        }
    }
}

#[derive(Serialize)]
struct Impact {
    record: Record,
    chain: Vec<inspection::Edge>,
    version: Option<i64>,
    execution_baseline: Option<String>,
    tested_versions: Option<Value>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Side {
    Baseline,
    Candidate,
    Additional,
}

#[derive(PartialEq, Eq, Serialize)]
struct Citation {
    side: Side,
    reference: String,
    path: Option<String>,
    record_id: Option<String>,
    observed_sha256: Option<String>,
    declared_sha256: Option<String>,
    provenance: Option<Value>,
    issue: Option<String>,
}

fn snapshot(w: &Workspace, id: &str, record_id: &str) -> Result<(Manifest, Document, String)> {
    let (manifest, sha256) = baselines::load(w, id)?;
    let entry = manifest["documents"]
        .get(record_id)
        .with_context(|| format!("{record_id} is not a member of {id}"))?;
    let bytes_path = baselines::snapshot_path(id, entry)?;
    let mut document = records::read(w, &bytes_path)?;
    ensure!(
        document.hash == entry["file_sha256"],
        "Snapshot changed during review: {record_id}"
    );
    document.path = records::string(entry, "path")?;
    Ok((
        Manifest {
            id: id.into(),
            path: format!("00_Governance/Baselines/{id}/manifest.json"),
            sha256,
        },
        document,
        bytes_path,
    ))
}

fn version(document: &Document, bytes_path: String) -> Result<Version> {
    Ok(Version {
        record: Record::from(document),
        bytes_path,
        version: records::positive_version(&document.metadata)?,
        content_sha256: records::content_hash(&document.metadata, &document.body)?,
    })
}

fn checked_text(w: &Workspace, path: &str, expected: &str) -> Result<String> {
    let text = w.read(path)?;
    ensure!(
        hash(text.as_bytes()) == expected,
        "Review input changed: {path}"
    );
    Ok(text)
}

fn citations(
    records: &Records,
    before: &Document,
    candidate: &Document,
    additional: &[String],
) -> Result<Vec<Citation>> {
    let mut citations = Vec::new();
    let by_path: BTreeMap<_, _> = records
        .active
        .iter()
        .map(|d| (d.path.as_str(), d))
        .collect();
    for (side, document) in [(Side::Baseline, before), (Side::Candidate, candidate)] {
        for field in ["evidence", "evidence_paths", "sources"] {
            let Some(value) = document.metadata.get(field).filter(|v| !v.is_null()) else {
                continue;
            };
            let Some(items) = value.as_array() else {
                citations.push(Citation::gap(
                    side,
                    field.into(),
                    format!("{field} must be a list"),
                ));
                continue;
            };
            for item in items {
                let (reference, declared) = if field == "sources" {
                    (item["resource"].as_str(), item["sha256"].as_str())
                } else {
                    (item.as_str(), None)
                };
                let Some(reference) = reference.filter(|r| !r.is_empty()) else {
                    citations.push(Citation::gap(
                        side,
                        field.into(),
                        format!("{field} entry needs a reference"),
                    ));
                    continue;
                };
                if field == "sources" && declared.is_none() {
                    citations.push(Citation::gap(
                        side,
                        reference.into(),
                        "Source entry needs a declared SHA256".into(),
                    ));
                    continue;
                }
                let path = if field == "evidence" {
                    records.resolve_path(reference)
                } else {
                    Ok(reference.to_owned())
                };
                let path = path.and_then(|path| {
                    ensure!(
                        field != "evidence"
                            || by_path
                                .get(path.as_str())
                                .is_some_and(|d| d.kind == "evidence"),
                        "Evidence field must link to an active evidence note: {path}"
                    );
                    Ok(path)
                });
                citations.push(Citation::new(
                    side, reference, declared, path, records, &by_path,
                ));
            }
        }
    }
    for id in additional.iter().collect::<BTreeSet<_>>() {
        records::identifier(id)?;
        let document = records.by_id(id)?;
        ensure!(
            document.kind == "evidence",
            "Additional evidence must be an evidence note: {id}"
        );
        citations.push(Citation::new(
            Side::Additional,
            id,
            None,
            Ok(document.path.clone()),
            records,
            &by_path,
        ));
    }
    Ok(citations)
}

impl Citation {
    fn gap(side: Side, reference: String, issue: String) -> Self {
        Self {
            side,
            reference,
            path: None,
            record_id: None,
            observed_sha256: None,
            declared_sha256: None,
            provenance: None,
            issue: Some(issue),
        }
    }

    fn new(
        side: Side,
        reference: &str,
        declared: Option<&str>,
        path: Result<String>,
        records: &Records,
        by_path: &BTreeMap<&str, &Document>,
    ) -> Self {
        let resolved = path.and_then(|path| {
            let digest = hash(records.workspace.read(&path)?.as_bytes());
            Ok((path, digest))
        });
        let (path, digest) = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                let mut citation = Self::gap(side, reference.into(), format!("{error:#}"));
                citation.declared_sha256 = declared.map(String::from);
                return citation;
            }
        };
        let record = by_path.get(path.as_str());
        let provenance = record.filter(|d| d.kind == "evidence").map(|d| {
            let metadata = &d.metadata;
            json!({
                "assertion_type": metadata["assertion_type"],
                "locator": metadata["locator"],
                "source_id": metadata["source_id"],
                "repo_id": metadata["repo_id"],
                "snapshot_id": metadata["snapshot_id"],
                "source_path": metadata["source_path"],
                "source_sha256": metadata["source_sha256"],
                "workflow_status": metadata["workflow_status"],
            })
        });
        let issue = if record.is_some_and(|d| d.hash != digest) {
            Some("Evidence changed during review".into())
        } else if declared.is_some_and(|expected| expected != digest) {
            Some("Declared SHA256 differs from the observed source bytes".into())
        } else {
            None
        };
        Self {
            side,
            reference: reference.into(),
            path: Some(path),
            record_id: record.and_then(|d| d.id.clone()),
            observed_sha256: Some(digest),
            declared_sha256: declared.map(String::from),
            provenance,
            issue,
        }
    }
}

pub fn load(w: &Workspace, request: &Request) -> Result<Review> {
    records::identifier(&request.id)?;
    let mut records = Records::load(w)?;
    let project_stamp = stamp(&records);
    let language = w.language()?;
    let (baseline, before, old_bytes) = snapshot(w, &request.baseline, &request.id)?;
    let approval_path = baselines::approved_decision(&records, &baseline.id, &baseline.sha256)?;
    let approval = records
        .active
        .iter()
        .find(|d| d.path == approval_path)
        .context("Approval decision is unavailable")?;
    let approval = Record::from(approval);

    let (candidate_manifest, candidate, new_bytes) = match &request.candidate {
        Some(id) => {
            ensure!(id != &baseline.id, "Select two distinct manifests");
            let (manifest, document, bytes) = snapshot(w, id, &request.id)?;
            (Some(manifest), document, bytes)
        }
        None => {
            let document = records.by_id(&request.id)?.clone();
            (None, document.clone(), document.path)
        }
    };
    ensure!(
        baselines::DOCUMENT_TYPES.contains(&before.kind.as_str()) && before.kind == candidate.kind,
        "Baseline and candidate need the same controlled document type"
    );
    let old = checked_text(w, &old_bytes, &before.hash)?;
    let new = checked_text(w, &new_bytes, &candidate.hash)?;
    let diff = inspection::diff(&old, &new)?;
    let before_version = version(&before, old_bytes)?;
    let candidate_version = version(&candidate, new_bytes)?;

    let (reachable, reference_gaps) = inspection::graph(&records, &request.id)?;
    let mut affected = vec![];
    for entry in reachable.into_iter().filter(|e| {
        matches!(
            e.kind.as_str(),
            "requirement" | "pbi" | "testcase" | "test_run"
        )
    }) {
        let document = records.by_id(&entry.id)?;
        affected.push(Impact {
            record: Record::from(document),
            chain: entry.chain,
            version: document.metadata["version"].as_i64(),
            execution_baseline: document.metadata["baseline_id"].as_str().map(String::from),
            tested_versions: document.metadata.get("requirement_versions").cloned(),
        });
    }
    let evidence = citations(&records, &before, &candidate, &request.evidence)?;
    records
        .active
        .retain(|d| d.id.as_deref() != Some(request.id.as_str()));
    records.active.push(candidate.clone());
    let mut mechanical_findings = validation::candidate_findings(&records, &candidate, &language);
    if before_version.content_sha256 != candidate_version.content_sha256
        && before_version.version >= candidate_version.version
    {
        mechanical_findings.push(Finding {
            level: "ERROR",
            code: "CHANGE_VERSION",
            file: candidate.path.clone(),
            message: format!(
                "Changed content requires a version greater than {}",
                before_version.version
            ),
        });
    }
    for citation in &evidence {
        if let Some(issue) = &citation.issue {
            let file = if citation.side == Side::Baseline {
                &before.path
            } else {
                &candidate.path
            };
            mechanical_findings.push(Finding {
                level: "WARN",
                code: "REVIEW_EVIDENCE",
                file: file.clone(),
                message: format!(
                    "{} · {} · {issue}",
                    side_label(citation.side),
                    citation.reference
                ),
            });
        }
    }

    let current = Records::load(w)?;
    ensure!(
        stamp(&current) == project_stamp
            && w.language()? == language
            && citations(&current, &before, &candidate, &request.evidence)? == evidence,
        "Project or evidence changed during review; repeat the comparison"
    );
    for manifest in std::iter::once(&baseline).chain(candidate_manifest.iter()) {
        ensure!(
            baselines::load(w, &manifest.id)?.1 == manifest.sha256,
            "Manifest changed during review: {}",
            manifest.id
        );
    }
    let basis = Basis {
        schema_version: 1,
        document_language: language,
        baseline,
        approval,
        before: before_version,
        candidate_manifest,
        candidate: candidate_version,
        evidence,
        affected,
        mechanical_findings,
        reference_gaps,
    };
    Ok(Review {
        basis_sha256: hash(&serde_json::to_vec(&basis)?),
        compared_at: records::timestamp(),
        semantic_findings_status: "pending",
        basis,
        diff,
    })
}

fn stamp(records: &Records) -> BTreeMap<String, String> {
    records
        .active
        .iter()
        .map(|d| (d.path.clone(), d.hash.clone()))
        .collect()
}

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Baseline => "Baseline",
        Side::Candidate => "Candidate",
        Side::Additional => "Additional",
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observations {
    reviewed_by: String,
    findings: Vec<Observation>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    side: FindingSide,
    line: usize,
    observation: String,
    evidence: Vec<String>,
    affected: Vec<String>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum FindingSide {
    Baseline,
    Candidate,
}

impl FindingSide {
    fn label(self) -> &'static str {
        match self {
            Self::Baseline => "Baseline",
            Self::Candidate => "Candidate",
        }
    }
}

impl Observations {
    pub fn read(path: &Path) -> Result<Self> {
        let data = sources::bytes(path, MAX_FILE)?;
        serde_json::from_slice(&data).context("Invalid semantic findings JSON")
    }
}

impl Review {
    pub fn summary(&self) -> String {
        let b = &self.basis;
        let mut text = String::new();
        writeln!(
            text,
            "Compared {}\n\nReview basis SHA256: {}\n",
            self.compared_at, self.basis_sha256
        )
        .unwrap();
        writeln!(
            text,
            "Baseline {} · {} · SHA256 {}\n",
            b.baseline.id, b.baseline.path, b.baseline.sha256
        )
        .unwrap();
        writeln!(
            text,
            "Approval {} · SHA256 {}\n",
            b.approval.path, b.approval.sha256
        )
        .unwrap();
        if let Some(manifest) = &b.candidate_manifest {
            writeln!(
                text,
                "Candidate manifest {} · {} · SHA256 {}\n",
                manifest.id, manifest.path, manifest.sha256
            )
            .unwrap();
        }
        for (name, version) in [("Baseline", &b.before), ("Candidate", &b.candidate)] {
            writeln!(
                text,
                "- {name} {} v{} · {} · bytes {} · file SHA256 {} · content SHA256 {}",
                version.record.id,
                version.version,
                version.record.path,
                version.bytes_path,
                version.record.sha256,
                version.content_sha256
            )
            .unwrap();
        }
        text.push_str("\n## Mechanical checks\n\n");
        let status = if self.diff.old_sha256 == self.diff.new_sha256 {
            "identical bytes"
        } else {
            "changed bytes"
        };
        writeln!(text, "Baseline approval and snapshot integrity verified. Comparison shows {status}. Candidate checks use the project at comparison time.\n").unwrap();
        if b.mechanical_findings.is_empty() {
            text.push_str("No scoped mechanical findings.\n");
        }
        for f in &b.mechanical_findings {
            writeln!(
                text,
                "- {} {} · {} · {}",
                f.level, f.code, f.file, f.message
            )
            .unwrap();
        }
        text.push_str("\n## Observed evidence\n\n");
        if b.evidence.is_empty() {
            text.push_str("No evidence references recorded in the review scope.\n");
        }
        for c in &b.evidence {
            let location = c.path.as_deref().unwrap_or(&c.reference);
            writeln!(
                text,
                "- {} · {} · SHA256 {}",
                side_label(c.side),
                c.path
                    .as_deref()
                    .map(note_link)
                    .unwrap_or_else(|| location.to_owned()),
                c.observed_sha256.as_deref().unwrap_or("unavailable")
            )
            .unwrap();
            if let Some(sha) = &c.declared_sha256 {
                writeln!(text, "  - Declared SHA256 {sha}").unwrap();
            }
            if let Some(provenance) = &c.provenance {
                writeln!(text, "  - Recorded provenance {provenance}").unwrap();
            }
        }
        text.push_str("\n## Recorded downstream references\n\n");
        if b.affected.is_empty() {
            text.push_str(
                "No linked requirements, PBIs or tests found in the current project graph.\n",
            );
        }
        for impact in &b.affected {
            let record = &impact.record;
            writeln!(
                text,
                "- {} · {} · {} · SHA256 {}",
                record.id,
                record.kind,
                note_link(&record.path),
                record.sha256
            )
            .unwrap();
            if let Some(version) = impact.version {
                writeln!(text, "  - Document version {version}").unwrap();
            }
            if let Some(baseline) = &impact.execution_baseline {
                writeln!(text, "  - Recorded execution baseline {baseline}").unwrap();
            }
            if let Some(versions) = &impact.tested_versions {
                writeln!(text, "  - Recorded test versions {versions}").unwrap();
            }
            for edge in &impact.chain {
                writeln!(text, "  - {} → {} · {}", edge.from, edge.to, edge.basis).unwrap();
            }
        }
        if !b.reference_gaps.is_empty() {
            text.push_str("\n### Unresolved project references\n\n");
            for gap in &b.reference_gaps {
                writeln!(text, "- {gap}").unwrap();
            }
        }
        text
    }
}

pub fn record(
    w: &Workspace,
    request: &Request,
    id: &str,
    expected: &str,
    observations: Option<&Observations>,
) -> Result<Value> {
    records::identifier(id)?;
    let review = load(w, request)?;
    ensure!(
        review.basis_sha256 == expected,
        "Review basis changed; compare again before recording findings"
    );
    let mut body = format!(
        "# Version-pinned change review\n\n{}\n## Semantic findings\n\n",
        review.summary()
    );
    if let Some(observations) = observations {
        append_observations(&mut body, &review, observations)?;
    } else {
        body.push_str("Pending. The operator supplied no semantic findings.\n");
    }
    let findings_status = if observations.is_some() {
        "supplied"
    } else {
        "pending"
    };
    let content = note(
        json!({"type":"report","id":format!("REPORT-CHANGE-REVIEW-{id}"),
        "title":"Version-pinned change review","language":"en","generated_at":records::timestamp(),
        "review_basis_sha256":review.basis_sha256,"semantic_findings_status":findings_status}),
        &body,
    )?;
    ensure!(
        content.len() as u64 <= MAX_FILE,
        "Change review is larger than 2 MiB; narrow the review"
    );
    let path = format!("90_Agent/Reports/Change-Review-{id}.md");
    atomic_create(&w.path(&path)?, content.as_bytes())?;
    Ok(
        json!({"path":path,"sha256":hash(content.as_bytes()),"basis_sha256":review.basis_sha256,
        "semantic_findings_status":findings_status}),
    )
}

fn append_observations(
    body: &mut String,
    review: &Review,
    observations: &Observations,
) -> Result<()> {
    ensure!(
        !observations.reviewed_by.trim().is_empty()
            && !observations.reviewed_by.chars().any(char::is_control)
            && !observations.findings.is_empty(),
        "Provide a reviewer and at least one observation"
    );
    writeln!(
        body,
        "Reviewer supplied by operator · {}\n",
        observations.reviewed_by
    )
    .unwrap();
    for entry in &observations.findings {
        let in_scope = review.diff.lines.iter().any(|line| {
            let index = match entry.side {
                FindingSide::Baseline => line.old,
                FindingSide::Candidate => line.new,
            };
            index == Some(entry.line)
        });
        ensure!(
            in_scope
                && !entry.observation.trim().is_empty()
                && !entry.evidence.is_empty()
                && !entry.affected.is_empty(),
            "Each observation needs a valid compared line, text, evidence and affected IDs"
        );
        writeln!(body, "### {} line {}\n", entry.side.label(), entry.line).unwrap();
        for line in entry.observation.lines() {
            writeln!(body, "> {line}").unwrap();
        }
        for path in &entry.evidence {
            let citation = review
                .basis
                .evidence
                .iter()
                .find(|c| c.path.as_ref() == Some(path) && c.observed_sha256.is_some())
                .with_context(|| {
                    format!("Finding evidence must use an observed review path: {path}")
                })?;
            writeln!(
                body,
                "\n- Evidence · {} · SHA256 {}",
                note_link(path),
                citation.observed_sha256.as_deref().unwrap()
            )
            .unwrap();
        }
        for id in &entry.affected {
            let record = if id == &review.basis.candidate.record.id {
                Some(&review.basis.candidate.record)
            } else {
                review
                    .basis
                    .affected
                    .iter()
                    .map(|impact| &impact.record)
                    .find(|record| &record.id == id)
            }
            .with_context(|| format!("Finding affected ID is outside the review: {id}"))?;
            writeln!(
                body,
                "- Affected · {} · {} · SHA256 {}",
                record.id,
                note_link(&record.path),
                record.sha256
            )
            .unwrap();
        }
        body.push('\n');
    }
    Ok(())
}

fn note_link(path: &str) -> String {
    match path.strip_suffix(".md") {
        Some(stem)
            if !stem.contains(['[', ']', '|', '#']) && !stem.chars().any(char::is_control) =>
        {
            format!("[[{stem}]]")
        }
        _ => path.into(),
    }
}
