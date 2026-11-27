//! Active project records and the legacy semantic-hash contract.
use crate::workspace::{Document, Workspace, atomic_write, hash, note, parse_note};
use anyhow::{Context, Result, bail, ensure};
use chrono::{SecondsFormat, Utc};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const MANAGED: &[&str] = &[
    "00_Governance",
    "01_Sources/Notes",
    "01_Sources/Evidence",
    "01_Sources/Reviews",
    "02_Domain",
    "03_Systems/Repositories",
    "03_Systems/Analysis",
    "04_Requirements",
    "05_Backlog",
    "06_Delivery",
    "07_Decisions",
    "08_Verification",
    "09_Operations",
];
pub fn truth(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
        Value::Number(v) => v.as_f64() != Some(0.),
    }
}
pub fn identifier(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 80
            && id.as_bytes()[0].is_ascii_alphanumeric()
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        "IDs must contain only letters, numbers, underscores and hyphens"
    );
    Ok(())
}
pub fn link(d: &Document) -> String {
    format!("[[{}]]", d.path.strip_suffix(".md").unwrap_or(&d.path))
}
pub fn read(workspace: &Workspace, path: &str) -> Result<Document> {
    let text = workspace.read(path)?;
    let (metadata, body) = parse_project_note(&text).with_context(|| path.to_owned())?;
    Ok(Document {
        path: path.into(),
        title: metadata["title"].as_str().unwrap_or(path).into(),
        kind: metadata["type"].as_str().unwrap_or("").into(),
        id: metadata["id"].as_str().map(String::from),
        hash: hash(text.as_bytes()),
        metadata,
        body,
    })
}

// Flat Properties are a persisted contract of the existing working Vault, separate
// from the general OKF YAML reader. For example, an unquoted exponent was a string
// in the original reader; silently retyping it would invalidate existing baselines.
pub fn parse_project_note(text: &str) -> Result<(Value, String)> {
    let (_, body) = parse_note(text)?; // Reject duplicate keys and invalid YAML first.
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let head = normalized
        .strip_prefix("---\n")
        .unwrap()
        .split_once("\n---")
        .unwrap()
        .0;
    if head.trim_start().starts_with('{') {
        return Ok((serde_json::from_str(head)?, body));
    }
    let mut metadata = serde_json::Map::new();
    let mut last: Option<String> = None;
    for line in head.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(value) = trimmed
            .strip_prefix('-')
            .filter(|s| s.starts_with(char::is_whitespace))
        {
            let key = last.as_ref().context("Invalid property list")?;
            let entry = metadata.get_mut(key).unwrap();
            if entry.is_null() {
                *entry = json!([]);
            }
            entry
                .as_array_mut()
                .context("Invalid property list")?
                .push(scalar(value)?);
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .context("Use flat Properties or JSON-style nested values")?;
        ensure!(
            !key.is_empty()
                && (key.as_bytes()[0].is_ascii_alphabetic() || key.starts_with('_'))
                && key
                    .chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
                && (value.is_empty() || value.starts_with(char::is_whitespace)),
            "Use flat Properties or JSON-style nested values"
        );
        ensure!(!metadata.contains_key(key), "Duplicate property: {key}");
        metadata.insert(key.into(), scalar(value)?);
        last = Some(key.into());
    }
    Ok((Value::Object(metadata), body))
}
fn scalar(text: &str) -> Result<Value> {
    let text = text.trim();
    if text.starts_with(['"', '[', '{']) {
        return serde_json::from_str(text).context("Use JSON-style inline values");
    }
    if text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2 {
        return Ok(json!(text[1..text.len() - 1].replace("''", "'")));
    }
    let end = text.char_indices().find_map(|(i, c)| {
        (c == '#' && i > 0 && text[..i].ends_with(char::is_whitespace)).then_some(i)
    });
    let text = text[..end.unwrap_or(text.len())].trim_end();
    match text {
        "" | "null" | "~" => return Ok(Value::Null),
        "true" => return Ok(json!(true)),
        "false" => return Ok(json!(false)),
        "|" | ">" => bail!("Use flat Properties or JSON-style nested values"),
        _ => {}
    }
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let parts: Vec<_> = unsigned.split('.').collect();
    if parts.len() <= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
    {
        if parts.len() == 2 {
            let value: f64 = text.parse()?;
            ensure!(value.is_finite(), "Property number must be finite");
            return Ok(json!(value));
        }
        if let Ok(value) = text.parse::<i64>() {
            return Ok(json!(value));
        }
        return Ok(json!(
            text.parse::<u64>()
                .context("Property integer exceeds supported exact range")?
        ));
    }
    Ok(json!(text))
}

pub struct Records {
    pub workspace: Workspace,
    pub active: Vec<Document>,
    all: Vec<String>,
}
impl Records {
    pub fn load(workspace: &Workspace) -> Result<Self> {
        for folder in MANAGED {
            let path = workspace.path(folder)?;
            if !path.exists() {
                continue;
            }
            for entry in walkdir::WalkDir::new(path)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| e.file_name() != "Baselines")
            {
                ensure!(
                    !entry?.file_type().is_symlink(),
                    "Active project records cannot contain symlinks"
                );
            }
        }
        let all = workspace.documents()?;
        let mut active = vec![];
        let mut ids = BTreeSet::new();
        for folder in MANAGED {
            for d in &all {
                if !d.path.starts_with(&format!("{folder}/")) {
                    continue;
                }
                let d = read(workspace, &d.path)?;
                if d.kind == crate::reviews::KIND
                    || d.metadata["workflow_schema"] == crate::reviews::PROFILE
                {
                    ensure!(
                        d.kind == crate::reviews::KIND
                            && d.id.as_deref().is_some_and(|id| !id.is_empty()),
                        "Material reviews require type=material_review and a nonempty ID: {}",
                        d.path
                    );
                }
                if d.metadata.get("clarification_schema").is_some() {
                    ensure!(
                        d.kind == "question" && d.id.as_deref().is_some_and(|id| !id.is_empty()),
                        "Scoped clarification requires type=question and a nonempty ID: {}",
                        d.path
                    );
                }
                if !truth(&d.metadata["id"]) {
                    continue;
                }
                ensure!(d.id.is_some(), "Document id must be a string: {}", d.path);
                ensure!(
                    ids.insert(d.id.clone().unwrap()),
                    "Duplicate document ID: {}",
                    d.id.as_deref().unwrap()
                );
                active.push(d);
            }
        }
        Ok(Self {
            workspace: workspace.clone(),
            active,
            all: all.into_iter().map(|d| d.path).collect(),
        })
    }
    pub fn by_id(&self, id: &str) -> Result<&Document> {
        let found: Vec<_> = self
            .active
            .iter()
            .filter(|d| d.id.as_deref() == Some(id))
            .collect();
        ensure!(
            found.len() == 1,
            "Expected exactly one note with ID {id}, got {}",
            found.len()
        );
        Ok(found[0])
    }
    pub(crate) fn for_validation(
        workspace: &Workspace,
        active: Vec<Document>,
        paths: Vec<String>,
    ) -> Self {
        Self {
            workspace: workspace.clone(),
            active,
            all: paths,
        }
    }
    pub fn resolve_path(&self, reference: &str) -> Result<String> {
        let path = reference.strip_prefix("[[").unwrap_or(reference);
        let path = path
            .strip_suffix("]]")
            .unwrap_or(path)
            .split(['|', '#'])
            .next()
            .unwrap_or("");
        let direct = self.workspace.path(path)?;
        if direct.is_file() {
            return Ok(path.into());
        }
        if !path.ends_with(".md") && self.workspace.path(&format!("{path}.md"))?.is_file() {
            return Ok(format!("{path}.md"));
        }
        let name = format!("{path}.md");
        let matches: Vec<_> = self
            .all
            .iter()
            .filter(|d| !path.contains('/') && d.rsplit('/').next() == Some(name.as_str()))
            .collect();
        ensure!(
            matches.len() == 1,
            "Unresolved or ambiguous link: {reference}"
        );
        ensure!(
            self.workspace.path(matches[0])?.is_file(),
            "Link target is not a file: {reference}"
        );
        Ok(matches[0].clone())
    }
    pub fn resolve(&self, reference: &str) -> Result<Document> {
        read(&self.workspace, &self.resolve_path(reference)?)
    }
    pub fn targets(&self, refs: Option<&Value>, expected: &[&str]) -> Result<Vec<Document>> {
        let Some(refs) = refs else {
            return Ok(vec![]);
        };
        refs.as_array()
            .context("References must be a list")?
            .iter()
            .map(|r| {
                let reference = r.as_str().context("Reference must be a string")?;
                let d = self.resolve(reference)?;
                ensure!(
                    d.id.as_ref().is_some_and(|s| !s.is_empty())
                        && (expected.is_empty() || expected.contains(&d.kind.as_str())),
                    "Invalid reference type: {reference}"
                );
                Ok(d)
            })
            .collect()
    }
}

/// Python's original sorted JSON representation is part of persisted baseline identity.
/// Retain spaces, Unicode and float spelling rather than replacing it with compact JSON.
pub fn canonical_json(value: &Value) -> Result<String> {
    match value {
        Value::Array(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        )),
        Value::Object(values) => {
            let mut keys: Vec<_> = values.keys().collect();
            keys.sort();
            Ok(format!(
                "{{{}}}",
                keys.into_iter()
                    .map(|k| Ok(format!(
                        "{}: {}",
                        serde_json::to_string(k)?,
                        canonical_json(&values[k])?
                    )))
                    .collect::<Result<Vec<_>>>()?
                    .join(", ")
            ))
        }
        Value::Number(n) if n.is_f64() => {
            let f = n.as_f64().context("Invalid number")?;
            ensure!(f.is_finite(), "Semantic hashes require finite numbers");
            let text = format!("{f:?}");
            if let Some((mantissa, exp)) = text.split_once('e') {
                let exp: i32 = exp.parse()?;
                Ok(format!(
                    "{mantissa}e{}{exp:02}",
                    if exp < 0 { "-" } else { "+" },
                    exp = exp.abs()
                ))
            } else {
                Ok(text)
            }
        }
        _ => Ok(serde_json::to_string(value)?),
    }
}
pub fn content_hash(meta: &Value, body: &str) -> Result<String> {
    let mut properties = meta
        .as_object()
        .context("Properties must be an object")?
        .clone();
    for key in [
        "doc_status",
        "baseline_id",
        "reviewed_by",
        "reviewed_at",
        "decision",
    ] {
        properties.remove(key);
    }
    Ok(hash(
        canonical_json(&json!({"properties":properties,"body":body.trim()}))?.as_bytes(),
    ))
}
pub fn fingerprint(d: &Document) -> Result<String> {
    let id = d.id.as_deref().context("Document needs an id")?;
    let version = match &d.metadata["version"] {
        Value::Null => "None".into(),
        Value::String(s) => s.clone(),
        v => v.to_string(),
    };
    Ok(format!(
        "{id}@{version}:{}",
        content_hash(&d.metadata, &d.body)?
    ))
}
pub fn timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
pub fn write_report(workspace: &Workspace, path: &str, id: &str, body: &str) -> Result<Value> {
    let title = body
        .lines()
        .find_map(|s| s.strip_prefix("# "))
        .context("Report needs a title")?;
    let content = note(
        json!({"id":id,"type":"report","title":title,"language":"en","generated_at":timestamp()}),
        body,
    )?;
    atomic_write(&workspace.path(path)?, content.as_bytes())?;
    Ok(json!({"path":path,"sha256":hash(content.as_bytes()),"content":content}))
}
pub fn wiki_refs(text: &str) -> Vec<&str> {
    let mut refs = vec![];
    let mut rest = text;
    while let Some((_, tail)) = rest.split_once("[[") {
        let Some((reference, tail)) = tail.split_once("]]") else {
            break;
        };
        if !reference.is_empty() {
            refs.push(reference);
        }
        rest = tail;
    }
    refs
}
pub fn string(value: &Value, key: &str) -> Result<String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .with_context(|| format!("Missing string: {key}"))
}
pub fn positive_version(meta: &Value) -> Result<i64> {
    let version = meta["version"]
        .as_i64()
        .context("Positive integer document version required")?;
    ensure!(version > 0, "Positive integer document version required");
    Ok(version)
}
