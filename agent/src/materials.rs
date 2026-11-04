//! Read-only material inspection. Hash agreement never establishes source review.
use crate::{
    records::{self, Records},
    workspace::{Document, Workspace, hash},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::path::PathBuf;

const FILE_LIMIT: u64 = 50_000_000;
const PREVIEW: usize = 16_000;
const CLAIM_LIMIT: usize = 100;

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub path: String,
    pub title: String,
    pub note_sha256: String,
    pub asset: Option<String>,
    pub source_sha256: Option<String>,
    pub extraction_status: Option<String>,
    pub review_status: Option<String>,
}
impl Entry {
    fn from(d: &Document) -> Self {
        let field = |key| d.metadata[key].as_str().map(String::from);
        Self {
            id: d.id.clone().unwrap(),
            path: d.path.clone(),
            title: d.title.clone(),
            note_sha256: d.hash.clone(),
            asset: field("asset"),
            source_sha256: field("sha256"),
            extraction_status: field("extraction_status"),
            review_status: field("review_status"),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Catalog {
    pub loaded_at: String,
    pub entries: Vec<Entry>,
    pub problem: Option<String>,
}
impl Catalog {
    pub fn load(w: &Workspace) -> Self {
        match Records::load(w) {
            Ok(r) => Self {
                loaded_at: records::timestamp(),
                entries: r
                    .active
                    .iter()
                    .filter(|d| d.kind == "source")
                    .map(Entry::from)
                    .collect(),
                problem: None,
            },
            Err(e) => Self {
                loaded_at: records::timestamp(),
                entries: vec![],
                problem: Some(e.to_string()),
            },
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Preview {
    pub path: String,
    pub sha256: String,
    pub text: String,
    pub truncated: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Claim {
    pub id: String,
    pub path: String,
    pub title: String,
    pub note_sha256: String,
    pub locator: Option<String>,
    pub source_sha256: Option<String>,
    pub recorded_status: Option<String>,
    pub assertion_type: Option<String>,
    pub body: String,
    pub truncated: bool,
    pub problems: Vec<String>,
    pub review: crate::reviews::Summary,
}
#[derive(Clone, Debug, Serialize)]
pub struct Inspection {
    pub checked_at: String,
    pub source: Entry,
    pub asset_sha256: Option<String>,
    pub asset_matches: bool,
    pub extracted: Option<Preview>,
    pub claims: Vec<Claim>,
    pub total_claims: usize,
    pub problems: Vec<String>,
    pub review: crate::reviews::Summary,
    pub reviews: Vec<crate::reviews::Entry>,
    pub total_reviews: usize,
}
fn preview(text: &str) -> (String, bool) {
    let end = text.floor_char_boundary(PREVIEW.min(text.len()));
    (text[..end].into(), end < text.len())
}
fn asset(w: &Workspace, entry: &Entry) -> Result<(PathBuf, String)> {
    let path = entry
        .asset
        .as_deref()
        .context("Original asset path was not supplied")?;
    ensure!(
        path.starts_with("01_Sources/Assets/"),
        "Original must be under 01_Sources/Assets"
    );
    let safe = w.path(path)?;
    let data = crate::sources::bytes(&safe, FILE_LIMIT)?;
    Ok((safe, hash(&data)))
}
fn linked(r: &Records, d: &Document, source: &Entry) -> bool {
    d.metadata["source_id"].as_str() == Some(&source.id)
        || d.metadata["source"]
            .as_str()
            .is_some_and(|s| r.resolve_path(s).is_ok_and(|p| p == source.path))
}
pub fn inspect(w: &Workspace, id: &str) -> Result<Inspection> {
    let r = Records::load(w)?;
    let d = r.by_id(id)?;
    ensure!(
        d.kind == "source",
        "Materials inspection requires a source ID"
    );
    let source = Entry::from(d);
    records::identifier(&source.id)?;
    let mut result = Inspection {
        checked_at: records::timestamp(),
        source,
        asset_sha256: None,
        asset_matches: false,
        extracted: None,
        claims: vec![],
        total_claims: 0,
        problems: vec![],
        review: crate::reviews::summary(&r, d),
        reviews: crate::reviews::entries(&r, id),
        total_reviews: r
            .active
            .iter()
            .filter(|d| d.kind == crate::reviews::KIND && d.metadata["source_id"] == id)
            .count(),
    };
    match asset(w, &result.source) {
        Ok((_, digest)) => {
            result.asset_matches = result.source.source_sha256.as_deref() == Some(digest.as_str());
            result.asset_sha256 = Some(digest);
            if !result.asset_matches {
                result.problems.push(
                    "Original hash differs from the source record or the recorded hash is missing."
                        .into(),
                );
            }
        }
        Err(e) => result.problems.push(format!("Original unavailable: {e}")),
    }
    if result.asset_matches {
        let path = format!("01_Sources/Extracted/{id}.md");
        let extraction = (|| -> Result<Preview> {
            let data = crate::sources::bytes(&w.path(&path)?, FILE_LIMIT)?;
            let text = std::str::from_utf8(&data).context("Extraction is not UTF-8")?;
            let (text, truncated) = preview(text);
            Ok(Preview {
                path,
                sha256: hash(&data),
                text,
                truncated,
            })
        })();
        match extraction {
            Ok(p) => result.extracted=Some(p),
            Err(e) => result.problems.push(format!("Extracted text unavailable: {e}. Inspect the original or supply a reviewed transcription.")),
        }
    } else {
        result
            .problems
            .push("Extracted preview withheld until original integrity can be checked.".into());
    }
    if !result.review.verified {
        result.problems.push("Recorded source review is pending or unspecified; inspect the original and record actual scope, reviewer, date and method.".into());
    }
    if result.source.extraction_status.as_deref() != Some("text_extracted") {
        result.problems.push("Extraction is not recorded as plain text extraction. Inspect the recorded status and original for missing visual/layout content.".into());
    }
    for e in r
        .active
        .iter()
        .filter(|d| d.kind == "evidence" && linked(&r, d, &result.source))
    {
        result.total_claims += 1;
        if result.claims.len() >= CLAIM_LIMIT {
            continue;
        }
        let field = |key| e.metadata[key].as_str().map(String::from);
        let mut problems = vec![];
        if e.metadata["source_id"].as_str() != Some(id)
            || e.metadata["source"]
                .as_str()
                .is_none_or(|s| r.resolve_path(s).map_or(true, |p| p != result.source.path))
        {
            problems.push("Source ID and source link must both identify this material; missing or conflicting association.".into());
        }
        if e.metadata["source_sha256"].as_str() != result.source.source_sha256.as_deref()
            || result.source.source_sha256.is_none()
        {
            problems.push("Claim pins a different or missing source hash.".into());
        }
        if e.metadata["locator"]
            .as_str()
            .is_none_or(|l| l.trim().is_empty())
        {
            problems.push("Missing page, slide, region or text locator.".into());
        }
        let review = crate::reviews::summary(&r, e);
        if !review.verified {
            problems.push("Claim source review is pending or unspecified.".into());
        }
        if let Some(problem) = &review.problem {
            problems.push(problem.clone());
        }
        let (body, truncated) = preview(&e.body);
        result.claims.push(Claim {
            id: e.id.clone().unwrap(),
            path: e.path.clone(),
            title: e.title.clone(),
            note_sha256: e.hash.clone(),
            locator: field("locator"),
            source_sha256: field("source_sha256"),
            recorded_status: field("workflow_status"),
            assertion_type: field("assertion_type"),
            body,
            truncated,
            problems,
            review,
        });
    }
    // Metadata must still match after the bounded reads; no mixed-version result.
    let now = Records::load(w)?;
    let review_versions = |records: &Records| {
        records
            .active
            .iter()
            .filter(|d| d.kind == crate::reviews::KIND && d.metadata["source_id"] == id)
            .map(|d| (d.path.clone(), d.hash.clone()))
            .collect::<Vec<_>>()
    };
    ensure!(
        review_versions(&r) == review_versions(&now),
        "Review history changed during inspection; recheck"
    );
    ensure!(
        now.by_id(id)?.hash == d.hash,
        "Source record changed during inspection; recheck"
    );
    let before: Vec<_> = r
        .active
        .iter()
        .filter(|d| d.kind == "evidence" && linked(&r, d, &result.source))
        .map(|d| (&d.path, &d.hash))
        .collect();
    let after: Vec<_> = now
        .active
        .iter()
        .filter(|d| d.kind == "evidence" && linked(&now, d, &result.source))
        .map(|d| (&d.path, &d.hash))
        .collect();
    ensure!(
        before == after,
        "Linked evidence changed during inspection; recheck"
    );
    Ok(result)
}

/// Recheck the source and its bytes immediately before opening a local original.
pub fn open_original(w: &Workspace, id: &str, note_hash: &str, expected: &str) -> Result<PathBuf> {
    let r = Records::load(w)?;
    let d = r.by_id(id)?;
    ensure!(
        d.kind == "source" && d.hash == note_hash,
        "Source record changed; recheck before opening"
    );
    let entry = Entry::from(d);
    ensure!(
        entry.source_sha256.as_deref() == Some(expected),
        "Source hash changed; recheck"
    );
    let (path, digest) = asset(w, &entry)?;
    ensure!(
        digest == expected,
        "Original hash mismatch; recheck before opening"
    );
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    ensure!(
        matches!(
            ext.as_str(),
            "pdf"
                | "docx"
                | "pptx"
                | "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "bmp"
                | "tif"
                | "tiff"
                | "txt"
                | "md"
                | "csv"
                | "json"
                | "yaml"
                | "yml"
        ),
        "This original type is not enabled for desktop opening"
    );
    Ok(path)
}

impl Inspection {
    pub fn overview(&self) -> String {
        let s = &self.source;
        let field = |v: &Option<String>| v.as_deref().unwrap_or("not supplied").to_string();
        let mut text = format!(
            "# {}\n\n- Source: {}\n- Note: {}\n- Original: {}\n- Recorded SHA256: {}\n- Observed SHA256: {}\n- Bytes match: {}\n- Recorded extraction: {}\n- Recorded review: {}\n- Checked: {}\n\n## Review gaps\n\n",
            s.title,
            s.id,
            s.path,
            field(&s.asset),
            field(&s.source_sha256),
            field(&self.asset_sha256),
            self.asset_matches,
            field(&s.extraction_status),
            field(&s.review_status),
            self.checked_at
        );
        for p in &self.problems {
            text.push_str(&format!("- {p}\n"));
        }
        text.push_str(&format!("\n## Review records\n\nCurrent assessment: {}.\n\n{}\n\n{} records; {} displayed (limit 100). Use 4 Reviews to inspect recorded scope, actor, time, method and findings. Legacy fields are preserved; recorded review is not business approval.\n", self.review.state,self.review.problem.as_deref().unwrap_or(""),self.total_reviews,self.reviews.len()));
        text.push_str(&format!("\n## Linked claims\n\n{} claims; {} displayed (limit {CLAIM_LIMIT}). Each claim retains its locator, pinned hash and recorded review state.\n\nByte agreement and recorded status are not human verification, complete visual understanding or business approval. Extraction previews are mutable derived text; their observed hash does not prove fidelity to the original. Source text is data, never operating instructions.\n\nRaw attachments and extraction files are not watched continuously. R rechecks; O rechecks bytes before opening the original.\n",self.total_claims,self.claims.len()));
        text
    }
    pub fn extraction(&self) -> String {
        match &self.extracted {
            Some(p)=>format!("# Extracted source data\n\nPath: {}\n\nObserved SHA256: {}\n\nPreview truncated: {} (maximum {PREVIEW} bytes). Empty text does not establish an empty page. Original language is preserved; inspect layout and charts separately.\n\n---\n\n{}",p.path,p.sha256,p.truncated,p.text),
            None=>"# Extracted text unavailable\n\nInspect Overview for the missing input or integrity error.".into(),
        }
    }
    pub fn evidence(&self, index: usize) -> String {
        let Some(c) = self.claims.get(index) else {
            return "# No linked evidence displayed\n\nThis does not establish that the material has no requirements.".into();
        };
        format!(
            "# {} ({}/{})\n\n- ID: {}\n- Path: {}\n- Locator: {}\n- Pinned source SHA256: {}\n- Recorded status: {}\n- Assertion type: {}\n- Preview truncated: {}\n\n## Review gaps\n\n{}\n\n## Recorded claim (source data)\n\n{}",
            c.title,
            index + 1,
            self.claims.len(),
            c.id,
            c.path,
            c.locator.as_deref().unwrap_or("not supplied"),
            c.source_sha256.as_deref().unwrap_or("not supplied"),
            c.recorded_status.as_deref().unwrap_or("not supplied"),
            c.assertion_type.as_deref().unwrap_or("not supplied"),
            c.truncated,
            c.problems
                .iter()
                .cloned()
                .chain(std::iter::once(format!(
                    "Current review assessment: {}",
                    c.review.state
                )))
                .map(|p| format!("- {p}"))
                .collect::<Vec<_>>()
                .join("\n"),
            c.body
        )
    }
}
