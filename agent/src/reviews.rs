//! Version-bound human review records. Recorded claims are not authenticated events.
use crate::{
    editing::FieldType,
    records::{self, Records},
    workspace::{self, Document, Workspace},
};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const KIND: &str = "material_review";
pub const PROFILE: &str = "material-review-v1";
pub const FOLDER: &str = "01_Sources/Reviews/";

pub fn fields() -> Vec<(&'static str, FieldType)> {
    use FieldType::*;
    vec![
        ("workflow_status", Choice(&["draft", "recorded"])),
        ("coverage", Choice(&["partial", "full"])),
        ("locator", Text),
        ("review_scope", Text),
        ("reviewed_by", Text),
        ("reviewed_at", EventTimestamp),
        ("review_method", Text),
        (
            "review_basis",
            Choice(&["original", "original_and_extraction"]),
        ),
        ("outcome", Choice(&["verified", "follow_up", "unavailable"])),
        ("findings", Text),
        ("follow_up", Text),
        ("previous_records", List),
    ]
}
fn target<'a>(r: &'a Records, id: &str) -> Result<(&'a Document, &'a Document)> {
    let d = r.by_id(id)?;
    let s = match d.kind.as_str() {
        "source" => d,
        "evidence" => {
            ensure!(
                !records::truth(&d.metadata["repo_id"]),
                "Repository evidence uses its separate review procedure"
            );
            let s = r.by_id(&records::string(&d.metadata, "source_id")?)?;
            ensure!(
                s.kind == "source",
                "Evidence must reference a material source"
            );
            ensure!(
                r.resolve_path(&records::string(&d.metadata, "source")?)? == s.path,
                "Evidence source link and ID disagree"
            );
            ensure!(
                d.metadata["source_sha256"] == s.metadata["sha256"],
                "Evidence pins a different material version"
            );
            records::string(&d.metadata, "locator")?;
            records::string(&d.metadata, "assertion_type")?;
            s
        }
        _ => anyhow::bail!("Review requires a material source or its evidence claim"),
    };
    Ok((d, s))
}
fn original(w: &Workspace, s: &Document) -> Result<String> {
    let path = records::string(&s.metadata, "asset")?;
    ensure!(
        path.starts_with("01_Sources/Assets/"),
        "Original must be under 01_Sources/Assets"
    );
    let digest = workspace::hash(&crate::sources::bytes(&w.path(&path)?, 50_000_000)?);
    ensure!(
        s.metadata["sha256"] == digest,
        "Original hash mismatch; restore or import a new material version"
    );
    Ok(digest)
}
fn extraction(w: &Workspace, source_id: &str) -> Result<String> {
    let data = crate::sources::bytes(
        &w.path(&format!("01_Sources/Extracted/{source_id}.md"))?,
        50_000_000,
    )?;
    std::str::from_utf8(&data).context("Extraction is not UTF-8")?;
    Ok(workspace::hash(&data))
}
pub fn prepare(w: &Workspace, id: &str, expected: &str) -> Result<Value> {
    let r = Records::load(w)?;
    let (d, s) = target(&r, id)?;
    ensure!(
        d.hash == expected,
        "Review target changed; inspect it again"
    );
    let sha = original(w, s)?;
    let source_id = s.id.as_deref().context("Source ID missing")?;
    Ok(
        json!({"id":null,"type":KIND,"title":null,"language":w.language()?,
        "workflow_schema":PROFILE,"workflow_status":"draft",
        "target_id":id,"target":records::link(d),"target_sha256":d.hash,
        "source_id":source_id,"source":records::link(s),"source_sha256":sha,
        "extraction_sha256":extraction(w,source_id).ok(),
        "coverage":null,"locator":if d.kind=="evidence"{d.metadata["locator"].clone()}else{Value::Null},
        "review_scope":null,"reviewed_by":null,"reviewed_at":null,"review_method":null,
        "review_basis":null,"outcome":null,"findings":null,"follow_up":null,"previous_records":[] }),
    )
}
fn digest(m: &Value, key: &str) -> Result<()> {
    let value = records::string(m, key)?;
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{key} must be a lowercase SHA256"
    );
    Ok(())
}
fn present(m: &Value, key: &str) -> Result<String> {
    let value = records::string(m, key)?;
    ensure!(!value.trim().is_empty(), "Supply {key}");
    Ok(value)
}
fn predecessors<'a>(r: &'a Records, d: &Document) -> Result<Vec<&'a Document>> {
    let mut seen = BTreeSet::new();
    let mut previous = vec![];
    for v in d.metadata["previous_records"]
        .as_array()
        .context("previous_records must be a list")?
    {
        let p = r.resolve(v.as_str().context("Previous review must be a link")?)?;
        let p = r.by_id(p.id.as_deref().context("Previous review needs an ID")?)?;
        ensure!(
            p.kind == KIND
                && p.metadata["workflow_schema"] == PROFILE
                && p.metadata["workflow_status"] == "recorded"
                && p.id != d.id
                && p.metadata["target_id"] == d.metadata["target_id"]
                && p.metadata["source_id"] == d.metadata["source_id"]
                && seen.insert(p.path.clone()),
            "Previous review must be a distinct recorded review of the same target and source"
        );
        previous.push(p);
    }
    Ok(previous)
}
pub fn check(r: &Records, d: &Document) -> Result<()> {
    if d.kind != KIND {
        return Ok(());
    }
    let m = &d.metadata;
    ensure!(
        m["workflow_schema"] == PROFILE,
        "Unknown material review schema"
    );
    ensure!(
        d.path.starts_with(FOLDER),
        "Material reviews belong in {FOLDER}"
    );
    for key in [
        "id",
        "title",
        "language",
        "target_id",
        "source_id",
        "target",
        "source",
    ] {
        present(m, key)?;
    }
    for key in ["target_sha256", "source_sha256"] {
        digest(m, key)?;
    }
    if !m["extraction_sha256"].is_null() {
        digest(m, "extraction_sha256")?;
    }
    ensure!(
        matches!(m["workflow_status"].as_str(), Some("draft" | "recorded")),
        "Review status must be draft or recorded"
    );
    for (key, values) in [
        ("coverage", &["partial", "full"][..]),
        ("outcome", &["verified", "follow_up", "unavailable"][..]),
        ("review_basis", &["original", "original_and_extraction"][..]),
    ] {
        if !m[key].is_null() {
            ensure!(
                m[key].as_str().is_some_and(|s| values.contains(&s)),
                "Invalid {key}"
            );
        }
    }
    let previous = predecessors(r, d)?;
    if m["workflow_status"] == "draft" {
        return Ok(());
    }
    ensure!(
        !records::truth(&m["simulation"]),
        "Simulated reviews cannot establish a recorded outcome"
    );
    for key in [
        "coverage",
        "locator",
        "review_scope",
        "reviewed_by",
        "reviewed_at",
        "review_method",
        "review_basis",
        "outcome",
        "findings",
    ] {
        present(m, key)?;
    }
    let at = chrono::DateTime::parse_from_rfc3339(&present(m, "reviewed_at")?)?;
    ensure!(
        at <= chrono::Utc::now(),
        "Review time cannot be in the future"
    );
    if m["coverage"] == "partial" || m["outcome"] != "verified" {
        present(m, "follow_up")?;
    }
    if m["review_basis"] == "original_and_extraction" {
        digest(m, "extraction_sha256")?;
    }
    for p in previous {
        let old = chrono::DateTime::parse_from_rfc3339(&present(&p.metadata, "reviewed_at")?)?;
        ensure!(
            at > old,
            "Correction review time must follow each previous review"
        );
    }
    Ok(())
}
/// Freshness is separate from historical structure. No old result is rewritten.
pub fn current(r: &Records, d: &Document) -> Result<()> {
    let m = &d.metadata;
    let (t, s) = target(r, &present(m, "target_id")?)?;
    ensure!(
        m["source_id"] == s.id.as_deref().unwrap_or("")
            && r.resolve_path(&present(m, "source")?)? == s.path
            && r.resolve_path(&present(m, "target")?)? == t.path,
        "Review target/source association changed"
    );
    ensure!(
        t.hash == present(m, "target_sha256")?,
        "Reviewed target content changed; create a new review"
    );
    ensure!(
        original(&r.workspace, s)? == present(m, "source_sha256")?,
        "Reviewed material version changed"
    );
    if t.kind == "evidence" && !m["locator"].is_null() {
        ensure!(
            m["locator"] == t.metadata["locator"],
            "Claim locator changed; review its exact recorded locator"
        );
    }
    if m["review_basis"] == "original_and_extraction" {
        ensure!(
            extraction(&r.workspace, &present(m, "source_id")?)?
                == present(m, "extraction_sha256")?,
            "Reviewed extraction changed or was not captured; create a new review"
        );
    }
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub state: String,
    pub verified: bool,
    pub records: Vec<String>,
    pub problem: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub id: String,
    pub path: String,
    pub target_id: String,
    pub status: String,
    pub content: String,
}
pub fn entries(r: &Records, source_id: &str) -> Vec<Entry> {
    r.active.iter().filter(|d|d.kind==KIND && d.metadata["source_id"]==source_id).take(100).map(|d|{
        let m=&d.metadata;
        let mut content=format!("# {}\n\n",d.title);
        for key in ["id","workflow_status","target_id","target_sha256","source_id","source_sha256","extraction_sha256","coverage","locator","review_scope","reviewed_by","reviewed_at","review_method","review_basis","outcome","findings","follow_up","previous_records"] {
            content.push_str(&format!("- {key}: {}\n",m[key]));
        }
        if let Err(e)=check(r,d).and_then(|_|current(r,d)){content.push_str(&format!("\n## Review gap\n\n{e}\n"));}
        let end=d.body.floor_char_boundary(16000.min(d.body.len()));
        content.push_str(&format!("\n## Recorded observations\n\n{}\n\nBody truncated: {}. Recorded facts do not authenticate the reviewer or approve business requirements.\n",&d.body[..end],end<d.body.len()));
        let end=content.floor_char_boundary(16_000.min(content.len()));
        let truncated=end<content.len();
        content.truncate(end);
        content.push_str(&format!("\n\nReview preview truncated: {truncated}. Open the note for all fields.\n"));
        Entry{id:d.id.clone().unwrap_or_default(),path:d.path.clone(),target_id:m["target_id"].as_str().unwrap_or("").into(),status:m["workflow_status"].as_str().unwrap_or("unknown").into(),content}
    }).collect()
}
/// Explicit successors select the current review; dates alone never supersede records.
pub fn summary(r: &Records, target: &Document) -> Summary {
    let reviews: Vec<_> = r
        .active
        .iter()
        .filter(|d| {
            d.kind == KIND
                && d.metadata["target_id"].as_str() == target.id.as_deref()
                && d.metadata["workflow_status"] != "draft"
        })
        .collect();
    let paths = reviews.iter().map(|d| d.path.clone()).collect();
    let mut result = Summary {
        state: "pending".into(),
        verified: false,
        records: paths,
        problem: None,
    };
    if reviews.is_empty() {
        result.verified = if target.kind == "source" {
            target.metadata["review_status"] == "verified"
        } else {
            target.metadata["workflow_status"] == "verified"
        };
        result.state = if result.verified {
            "legacy_verified"
        } else {
            "pending"
        }
        .into();
        return result;
    }
    let evaluated = (|| -> Result<&Document> {
        let mut superseded = BTreeSet::new();
        for d in &reviews {
            check(r, d)?;
            for p in predecessors(r, d)? {
                superseded.insert(p.path.clone());
            }
        }
        let heads: Vec<_> = reviews
            .iter()
            .filter(|d| !superseded.contains(&d.path))
            .collect();
        ensure!(
            heads.len() == 1,
            "Conflicting review history; link the applicable previous records explicitly"
        );
        let d = *heads[0];
        current(r, d)?;
        Ok(d)
    })();
    match evaluated {
        Ok(d) => {
            result.state = if d.metadata["coverage"] != "full" {
                "partial".into()
            } else {
                d.metadata["outcome"].as_str().unwrap().into()
            };
            result.verified = result.state == "verified";
        }
        Err(e) => {
            result.state = "unavailable".into();
            result.problem = Some(e.to_string());
        }
    }
    result
}
