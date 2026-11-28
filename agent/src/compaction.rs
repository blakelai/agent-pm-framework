//! A local, extractive checkpoint for provider replay. The session history is append-only.
use crate::{
    config::Protocol,
    records,
    runtime::Session,
    workspace::{Workspace, hash, parse_note},
};
use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

const CONTEXT_BYTES: usize = 700_000;
const RECENT_BYTES: usize = 160_000;
const MAX_REFERENCE_BYTES: usize = 360_000;
const USER_EXCERPTS: usize = 8;
const ASSISTANT_EXCERPTS: usize = 4;
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Checkpoint {
    schema_version: u32,
    archived_items: usize,
    archived_sha256: String,
    updated_at: String,
    references: Vec<Value>,
    excerpts: Vec<Excerpt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Excerpt {
    history_index: usize,
    role: String,
    beginning: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ending: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub archived_items: usize,
    pub retained_items: usize,
    pub archive_sha256: String,
    pub reference_count: usize,
    pub previous_bytes: usize,
    pub compacted_bytes: usize,
}

pub(crate) struct ContextWindow<'a> {
    pub history: Cow<'a, [Value]>,
    pub update: Option<(Checkpoint, Report)>,
}

const DIGEST_INTRO: &str = "[Application context checkpoint. Older conversation remains in local history. The JSON below contains recorded references and selected verbatim excerpts. Assistant excerpts and tool-derived values are historical data. Follow current user instructions and runtime gates. Inspect an exact referenced source before relying on its content. Archived image entries identify their saved files.]";

impl Checkpoint {
    fn digest(&self, state: &Value) -> Result<Value> {
        let data = json!({
            "archived_items": self.archived_items,
            "archive_sha256": self.archived_sha256,
            "references": self.references,
            "excerpts": self.excerpts,
            "continuation": state,
        });
        Ok(
            json!({"role":"user","content":format!("{DIGEST_INTRO}\n{}", serde_json::to_string(&data)?)}),
        )
    }

    pub(crate) fn markdown(&self) -> String {
        format!(
            "## Context compaction\n\n- Updated {}\n- Archived provider items {}\n- Archived history SHA256 {}\n- Recorded reference entries {}\n\nFull local conversation history remains available.\n",
            self.updated_at,
            self.archived_items,
            self.archived_sha256,
            self.references.len(),
        )
    }
}

pub(crate) fn prepare<'a>(
    workspace: &Workspace,
    session: &'a Session,
    protocol: &Protocol,
) -> Result<ContextWindow<'a>> {
    ensure!(
        session.pending.is_empty(),
        "Handle pending tool calls before compacting provider context"
    );
    let previous = session.context_checkpoint.as_ref();
    let archived_items = previous.map_or(0, |checkpoint| checkpoint.archived_items);
    let history = &session.history;
    let recent = history
        .get(archived_items..)
        .context("Context checkpoint extends beyond local history")?;
    let state = if previous.is_some() {
        Some(continuation(workspace, session))
    } else {
        None
    };
    let prefix = if let Some(checkpoint) = previous {
        ensure!(
            checkpoint.schema_version == SCHEMA_VERSION
                && archived_items > 0
                && hash(&serde_json::to_vec(&history[..archived_items])?)
                    == checkpoint.archived_sha256,
            "Saved context checkpoint does not match the archived history"
        );
        Some(checkpoint.digest(state.as_ref().unwrap())?)
    } else {
        None
    };
    let recent_bytes = serde_json::to_vec(recent)?.len();
    let previous_bytes = recent_bytes
        + prefix.as_ref().map_or(0, |item| {
            item.to_string().len() + usize::from(!recent.is_empty())
        });
    if previous_bytes < CONTEXT_BYTES {
        return Ok(ContextWindow {
            history: match prefix {
                Some(item) => Cow::Owned(
                    std::iter::once(item)
                        .chain(recent.iter().cloned())
                        .collect(),
                ),
                None => Cow::Borrowed(recent),
            },
            update: None,
        });
    }

    let through = archived_items + safe_cut(recent, protocol)?;
    let latest_user = history.iter().rposition(|item| item["role"] == "user");
    let mut refs = References::new();
    let mut excerpts = Vec::new();
    if let Some(checkpoint) = previous {
        for entry in &checkpoint.references {
            refs.add(entry.to_string())?;
        }
        excerpts.clone_from(&checkpoint.excerpts);
    }
    for (index, item) in history
        .iter()
        .enumerate()
        .take(through)
        .skip(archived_items)
    {
        collect_item(item, &history[..index], &mut refs)?;
        if let Some(excerpt) = excerpt(index, item, latest_user == Some(index)) {
            excerpts.push(excerpt);
        }
    }
    let checkpoint = Checkpoint {
        schema_version: SCHEMA_VERSION,
        archived_items: through,
        archived_sha256: hash(&serde_json::to_vec(&history[..through])?),
        updated_at: records::timestamp(),
        references: refs
            .entries
            .into_iter()
            .map(|entry| serde_json::from_str(&entry))
            .collect::<serde_json::Result<_>>()?,
        excerpts: selected_excerpts(excerpts, latest_user),
    };
    let state = state.unwrap_or_else(|| continuation(workspace, session));
    let window = std::iter::once(checkpoint.digest(&state)?)
        .chain(history[through..].iter().cloned())
        .collect::<Vec<_>>();
    let compacted_bytes = serde_json::to_vec(&window)?.len();
    ensure!(
        compacted_bytes < CONTEXT_BYTES,
        "The active input and exact references exceed the context replay limit. Start a narrower session. Full history remains saved"
    );
    let report = Report {
        archived_items: through,
        retained_items: history.len() - through,
        archive_sha256: checkpoint.archived_sha256.clone(),
        reference_count: checkpoint.references.len(),
        previous_bytes,
        compacted_bytes,
    };
    Ok(ContextWindow {
        history: Cow::Owned(window),
        update: Some((checkpoint, report)),
    })
}

fn safe_cut(history: &[Value], protocol: &Protocol) -> Result<usize> {
    let mut calls = BTreeSet::new();
    let mut boundaries = vec![];
    let mut consumed = 0;
    for (index, item) in history.iter().enumerate() {
        if index > 0 && item["role"] == "user" && calls.is_empty() {
            boundaries.push((index, consumed));
        }
        consumed += item.to_string().len() + 1;
        let result = match protocol {
            Protocol::ChatCompletions => {
                for call in item["tool_calls"].as_array().into_iter().flatten() {
                    let id = call["id"]
                        .as_str()
                        .context("Tool call is missing its replay ID")?;
                    ensure!(
                        calls.insert(id),
                        "Duplicate tool call in the same replay batch"
                    );
                }
                if item["role"] == "tool" {
                    Some(
                        item["tool_call_id"]
                            .as_str()
                            .context("Tool result is missing its replay ID")?,
                    )
                } else {
                    None
                }
            }
            Protocol::Responses => match item["type"].as_str() {
                Some("function_call") => {
                    let id = item["call_id"]
                        .as_str()
                        .context("Function call is missing its replay ID")?;
                    ensure!(
                        calls.insert(id),
                        "Duplicate function call in the same replay batch"
                    );
                    None
                }
                Some("function_call_output") => Some(
                    item["call_id"]
                        .as_str()
                        .context("Function result is missing its replay ID")?,
                ),
                _ => None,
            },
        };
        if let Some(id) = result {
            ensure!(
                calls.remove(id),
                "Tool result has no matching call in local history"
            );
            if calls.is_empty() {
                boundaries.push((index + 1, consumed));
            }
        }
    }
    ensure!(
        calls.is_empty(),
        "Local history has incomplete tool exchanges. Resolve the pending operation before continuing"
    );
    if history.last().is_some_and(|item| {
        item["role"] == "assistant" || item["type"] == "reasoning" || item["type"] == "message"
    }) {
        boundaries.push((history.len(), consumed));
    }
    boundaries.iter().find(|(_, bytes)| consumed - *bytes <= RECENT_BYTES)
        .or_else(|| boundaries.last())
        .map(|(index, _)| *index)
        .context("No complete history boundary fits before the active message. Start a narrower session. Full history remains saved")
}

fn continuation(workspace: &Workspace, session: &Session) -> Value {
    let mut questions = BTreeMap::new();
    for question in session.questions.iter().filter(|q| !q.resolved) {
        questions.insert(question.path.as_str(), Some(question.question.as_str()));
    }
    for path in &session.blocked_questions {
        questions.entry(path).or_insert(None);
    }
    let questions = questions.into_iter().map(|(path, text)| {
        match records::read(workspace, path) {
            Ok(doc) => json!({
                "path": path, "id": doc.id, "question": text.unwrap_or(&doc.title), "record_sha256": doc.hash,
                "workflow_status": doc.metadata["workflow_status"],
                "blocking_scope": doc.metadata["blocking_scope"], "affected_ids": doc.metadata["affected_ids"],
                "affected_paths": doc.metadata["affected_paths"], "answer_path": doc.metadata["answer_path"],
                "answer_source": doc.metadata["answer_source"],
                "answer_sha256": doc.metadata["answer_sha256"], "answer_for_sha256": doc.metadata["answer_for_sha256"],
            }),
            Err(_) => json!({"path":path, "question":text, "record_status":"unavailable"}),
        }
    }).collect::<Vec<_>>();
    json!({"unresolved_questions":questions,"pending_operations":session.pending})
}

struct References {
    entries: BTreeSet<String>,
    bytes: usize,
}

impl References {
    fn new() -> Self {
        Self {
            entries: BTreeSet::new(),
            bytes: 0,
        }
    }

    fn add(&mut self, entry: String) -> Result<()> {
        let bytes = entry.len();
        if self.entries.insert(entry) {
            self.bytes += bytes + 1;
        }
        ensure!(
            self.bytes <= MAX_REFERENCE_BYTES,
            "The exact reference inventory exceeds the compaction limit. Start a narrower session. Full history remains saved"
        );
        Ok(())
    }

    fn visit(&mut self, value: &Value, inherited: &Map<String, Value>) -> Result<()> {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.visit(item, inherited)?;
                }
            }
            Value::Object(object) => {
                let mut fields = inherited.clone();
                for (name, value) in object {
                    if let Some(value) = reference_field(name, value) {
                        fields.insert(name.clone(), value);
                    }
                }
                if fields != *inherited && fields.keys().any(|name| is_anchor(name)) {
                    self.add(serde_json::to_string(&fields)?)?;
                }
                for (name, value) in object {
                    match value {
                        Value::Object(_) | Value::Array(_) if name != "encrypted_content" => {
                            self.visit(value, &fields)?
                        }
                        Value::String(text)
                            if matches!(name.as_str(), "content" | "text" | "stdout") =>
                        {
                            self.text(text)?;
                            if name == "content" {
                                let start = text.trim_start_matches('\u{feff}');
                                if (start.starts_with("---\n") || start.starts_with("---\r\n"))
                                    && let Ok((metadata, _)) = parse_note(text)
                                {
                                    self.visit(&metadata, &fields)?;
                                } else if start.starts_with('{')
                                    && (fields.contains_key("path")
                                        || fields.contains_key("manifest"))
                                    && let Ok(value) = serde_json::from_str::<Value>(text)
                                {
                                    self.visit(&value, &fields)?;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Value::String(text) => self.text(text)?,
            _ => {}
        }
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<()> {
        static LOCATOR: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(
            r"\[Application source locator: ([^;\r\n]+); SHA256: ([a-fA-F0-9]{64}); answer target: ([^\]\r\n]+)\]"
        ).unwrap()
        });
        static PIN_PATH: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?:03_Systems/Snapshots|00_Governance/Baselines)/[a-zA-Z0-9_-]+").unwrap()
        });
        for source in LOCATOR.captures_iter(text) {
            self.add(
                json!({"path":&source[1],"sha256":&source[2],"answer_target":&source[3]})
                    .to_string(),
            )?;
        }
        for path in PIN_PATH.find_iter(text) {
            self.add(json!({"path":path.as_str()}).to_string())?;
        }
        Ok(())
    }
}

fn is_anchor(name: &str) -> bool {
    name.ends_with("sha256")
        || matches!(
            name,
            "snapshot_id"
                | "snapshot"
                | "latest_snapshot"
                | "old_snapshot"
                | "new_snapshot"
                | "baseline_id"
                | "baseline"
                | "manifest"
                | "question_path"
                | "requirement_versions"
                | "source_id"
                | "evidence_id"
                | "correction_id"
                | "status"
                | "fingerprint"
                | "expected_hash"
        )
}

fn reference_field(name: &str, value: &Value) -> Option<Value> {
    if name == "requirement_versions" {
        return value
            .as_array()
            .filter(|items| items.iter().all(Value::is_string))
            .map(|_| value.clone());
    }
    let relevant = name != "status" && is_anchor(name)
        || matches!(
            name,
            "path"
                | "source_path"
                | "source_note"
                | "resource"
                | "asset"
                | "id"
                | "kind"
                | "type"
                | "repo_id"
                | "revision"
                | "old_revision"
                | "new_revision"
                | "locator"
                | "assertion_type"
                | "old_path"
                | "new_path"
                | "baseline_version"
                | "current_version"
                | "version"
                | "approval_decision"
                | "dirty"
                | "old_dirty"
                | "new_dirty"
                | "record_id"
                | "bytes"
                | "width"
                | "height"
                | "answer_path"
        );
    if name == "status" {
        return value
            .as_str()
            .filter(|status| {
                matches!(
                    *status,
                    "reconciled_by_user"
                        | "superseded_by_correction"
                        | "draft_saved"
                        | "blocked_by_clarification"
                )
            })
            .map(|_| value.clone());
    }
    (relevant && (value.is_string() || value.is_number() || value.is_boolean()))
        .then(|| value.clone())
}

fn collect_item(item: &Value, preceding: &[Value], refs: &mut References) -> Result<()> {
    let empty = Map::new();
    if item["type"] == "reasoning" {
        return Ok(());
    }
    let result = if item["role"] == "tool" {
        item["content"].as_str()
    } else if item["type"] == "function_call_output" {
        item["output"].as_str()
    } else {
        None
    };
    if let Some(text) = result {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            let mut call = Map::new();
            if matches!(
                value["status"].as_str(),
                Some("reconciled_by_user" | "superseded_by_correction")
            ) && let Some(id) = item["call_id"]
                .as_str()
                .or_else(|| item["tool_call_id"].as_str())
            {
                call.insert("call_id".into(), json!(id));
                if let Some(operation) = preceding
                    .iter()
                    .rev()
                    .find_map(|entry| operation_for(entry, id))
                {
                    call.insert("operation".into(), operation);
                }
            }
            if value["status"] == "reconciled_by_user" {
                for key in ["observed_result", "source"] {
                    if let Some(text) = value[key].as_str() {
                        call.insert(key.into(), Value::String(text.into()));
                    }
                }
            }
            refs.visit(&value, &call)?;
        } else {
            refs.text(text)?;
        }
        return Ok(());
    }
    if item["type"] == "function_call" {
        return collect_arguments(&item["arguments"], refs);
    }
    for call in item["tool_calls"].as_array().into_iter().flatten() {
        collect_arguments(&call["function"]["arguments"], refs)?;
    }
    if matches!(item["role"].as_str(), Some("user" | "assistant")) {
        refs.visit(&item["content"], &empty)?;
    }
    Ok(())
}

fn collect_arguments(arguments: &Value, refs: &mut References) -> Result<()> {
    if let Some(text) = arguments.as_str()
        && let Ok(value) = serde_json::from_str::<Value>(text)
    {
        refs.visit(&value, &Map::new())?;
    }
    Ok(())
}

fn operation_for(item: &Value, id: &str) -> Option<Value> {
    let function = if item["type"] == "function_call" && item["call_id"] == id {
        item
    } else {
        &item["tool_calls"]
            .as_array()?
            .iter()
            .find(|call| call["id"] == id)?["function"]
    };
    Some(json!({"name":function["name"],"arguments":function["arguments"]}))
}

fn excerpt(index: usize, item: &Value, latest_user: bool) -> Option<Excerpt> {
    let role = item["role"]
        .as_str()
        .filter(|role| matches!(*role, "user" | "assistant"))?;
    let content = &item["content"];
    let text = content.as_str().map(String::from).unwrap_or_else(|| {
        content
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| {
                matches!(
                    p["type"].as_str(),
                    Some("text" | "input_text" | "output_text")
                )
            })
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    });
    if text.trim().is_empty() {
        return None;
    }
    let limit = if latest_user {
        usize::MAX
    } else if role == "user" {
        4_000
    } else {
        1_000
    };
    let (beginning, ending) = if text.len() <= limit {
        (text, None)
    } else {
        let end = text.floor_char_boundary(limit * 3 / 4);
        let start = text.ceil_char_boundary(text.len() - limit / 4);
        (text[..end].into(), Some(text[start..].into()))
    };
    Some(Excerpt {
        history_index: index,
        role: role.into(),
        beginning,
        ending,
    })
}

fn selected_excerpts(excerpts: Vec<Excerpt>, latest_user: Option<usize>) -> Vec<Excerpt> {
    let mut indices = BTreeSet::new();
    for (role, limit) in [("user", USER_EXCERPTS), ("assistant", ASSISTANT_EXCERPTS)] {
        indices.extend(
            excerpts
                .iter()
                .rev()
                .filter(|e| e.role == role)
                .take(limit)
                .map(|e| e.history_index),
        );
    }
    if let Some(first) = excerpts.iter().find(|e| e.role == "user") {
        indices.insert(first.history_index);
    }
    excerpts
        .into_iter()
        .filter(|e| indices.contains(&e.history_index))
        .map(|mut excerpt| {
            if excerpt.role == "user"
                && latest_user != Some(excerpt.history_index)
                && excerpt.ending.is_none()
                && excerpt.beginning.len() > 4_000
            {
                let end = excerpt.beginning.floor_char_boundary(3_000);
                let start = excerpt
                    .beginning
                    .ceil_char_boundary(excerpt.beginning.len() - 1_000);
                excerpt.ending = Some(excerpt.beginning[start..].into());
                excerpt.beginning.truncate(end);
            }
            excerpt
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutoff_does_not_detach_parallel_tool_results_or_archive_the_latest_input() {
        let mut history = vec![
            json!({"role":"user","content":"Initial request"}),
            json!({"role":"assistant","tool_calls":[{"id":"call-a"},{"id":"call-b"}]}),
            json!({"role":"tool","tool_call_id":"call-a","content":"ok"}),
        ];
        assert!(safe_cut(&history, &Protocol::ChatCompletions).is_err());
        history.push(json!({"role":"tool","tool_call_id":"call-b","content":"ok"}));
        history.push(json!({"role":"user","content":"x".repeat(RECENT_BYTES + 1)}));
        assert_eq!(safe_cut(&history, &Protocol::ChatCompletions).unwrap(), 4);
    }

    #[test]
    fn reference_limits_refuse_excess_bytes_and_excerpts_preserve_unicode_boundaries() {
        let mut refs = References::new();
        let pin = json!({"source_path":"x".repeat(MAX_REFERENCE_BYTES / 2),"source_sha256":"hash"})
            .to_string();
        refs.add(pin.clone()).unwrap();
        refs.add(pin).unwrap();
        assert_eq!(refs.entries.len(), 1);
        assert!(refs.add("x".repeat(MAX_REFERENCE_BYTES / 2)).is_err());
        let original = "中😀".repeat(1000);
        let item = json!({"role":"user","content":original});
        let short = excerpt(1, &item, false).unwrap();
        assert!(
            original.starts_with(&short.beginning)
                && original.ends_with(short.ending.as_ref().unwrap())
        );
        assert_eq!(excerpt(1, &item, true).unwrap().beginning, original);
    }

    #[test]
    fn saved_checkpoint_rejects_a_changed_archived_prefix() {
        let fixture = tempfile::tempdir().unwrap();
        let workspace = Workspace {
            root: fixture.path().to_owned(),
        };
        let mut session: Session = serde_json::from_value(json!({
            "id":"synthetic","status":"ready","provider_fingerprint":"fixture","history":[
                {"role":"user","content":"First message"},
                {"role":"assistant","content":"old ".repeat(175_000)},
                {"role":"user","content":"Continue work"}
            ],"pending":[],"questions":[],"last_output":"","error":null,"stage":"workflow"
        }))
        .unwrap();
        let protocol = Protocol::ChatCompletions;
        let (checkpoint, _) = prepare(&workspace, &session, &protocol)
            .unwrap()
            .update
            .unwrap();
        session.context_checkpoint = Some(checkpoint);
        assert!(
            prepare(&workspace, &session, &protocol)
                .unwrap()
                .update
                .is_none()
        );
        session.history[0]["content"] = json!("Different message");
        let error = prepare(&workspace, &session, &protocol).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("does not match the archived history")
        );
    }
}
