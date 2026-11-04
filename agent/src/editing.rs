//! Human-operated project edits: validate and review before an optimistic write.
//! The Agent's draft tool deliberately does not use this commit path.
use crate::{
    delivery, planning, records, validation,
    workspace::{self, Document, Workspace},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const PEOPLE: &[&str] = &[
    "Person", "Role", "Sprint", "Project", "FTE", "Workdays", "Absence", "Focus", "Reserve",
];
const PERIODS: &[&str] = &["Sprint", "Start", "End", "Goal"];
const ESTIMATE: &[&str] = &["Role", "O", "M", "P"];
const REMAINING: &[&str] = &["Role", "Remaining O", "Remaining M", "Remaining P"];

#[derive(Clone, Copy, Debug)]
pub enum FieldType {
    Text,
    Integer,
    Number,
    Date,
    Timestamp,
    EventTimestamp,
    List,
    Choice(&'static [&'static str]),
}
impl FieldType {
    pub fn hint(self) -> String {
        match self {
            Self::Text => "Text; blank remains unknown".into(),
            Self::Integer => "Integer; blank remains unknown".into(),
            Self::Number => "Finite number; blank remains unknown, never zero".into(),
            Self::Date => "ISO date: YYYY-MM-DD; blank remains unknown".into(),
            Self::Timestamp => {
                "Actual ISO date or RFC3339 timestamp; never filled automatically".into()
            }
            Self::EventTimestamp => {
                "Actual RFC3339 timestamp with timezone; blank remains unknown".into()
            }
            Self::List => "One value or [[document link]] per line; blank is an empty list".into(),
            Self::Choice(values) => format!("Choose: {}", values.join(" / ")),
        }
    }
    fn value(self, raw: &str) -> Result<Value> {
        let s = raw.trim();
        if matches!(self, Self::List) {
            return Ok(json!(
                s.lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            ));
        }
        if s.is_empty() {
            return Ok(Value::Null);
        }
        Ok(match self {
            Self::Integer => json!(s.parse::<i64>().context("Expected an integer")?),
            Self::Number => {
                let n: f64 = s.parse().context("Expected a number")?;
                ensure!(n.is_finite(), "Number must be finite");
                json!(n)
            }
            Self::Date => {
                planning::date(s)?;
                json!(s)
            }
            Self::Timestamp => {
                ensure!(
                    planning::date(s).is_ok() || chrono::DateTime::parse_from_rfc3339(s).is_ok(),
                    "Use an actual ISO date or RFC3339 timestamp"
                );
                json!(s)
            }
            Self::EventTimestamp => {
                chrono::DateTime::parse_from_rfc3339(s)
                    .context("Supply an actual RFC3339 timestamp with timezone")?;
                json!(s)
            }
            Self::Choice(values) => {
                ensure!(values.contains(&s), "Choose one of: {}", values.join(", "));
                json!(s)
            }
            _ => json!(s),
        })
    }
}
#[derive(Clone, Debug)]
pub enum Target {
    Property(String),
    Body,
    Cell(usize, usize, usize),
    AddRow(usize),
    RemoveRow(usize, usize),
}
#[derive(Clone, Debug)]
pub struct Entry {
    pub label: String,
    pub value: String,
    pub kind: FieldType,
    pub target: Target,
}
#[derive(Clone, Debug)]
struct Table {
    columns: &'static [&'static str],
    rows: Vec<Vec<String>>,
    present: bool,
}
#[derive(Clone, Debug)]
pub struct Edit {
    pub path: String,
    original: Option<String>,
    metadata: Value,
    body: String,
    tables: Vec<Table>,
}
#[derive(Debug)]
pub struct Preview {
    pub path: String,
    pub content: String,
    pub changes: Vec<String>,
    pub warnings: Vec<String>,
    pub planning_changes: Vec<String>,
    context: String,
    before: Option<String>,
}
pub struct Saved {
    pub path: String,
    pub audit_path: Option<String>,
    pub warning: Option<String>,
}

fn fields(kind: &str) -> Vec<(&'static str, FieldType)> {
    use FieldType::*;
    let mut result = vec![("title", Text), ("owner", Text)];
    result.extend(delivery::fields(kind));
    if kind == crate::reviews::KIND {
        result.extend(crate::reviews::fields());
    }
    match kind {
        "pbi" => result.extend([
            (
                "work_type",
                Choice(&["feature", "bug", "enabler", "spike", "migration"]),
            ),
            (
                "delivery_status",
                Choice(&[
                    "proposed",
                    "ready",
                    "in_progress",
                    "blocked",
                    "done",
                    "cancelled",
                ]),
            ),
            ("sprint", Integer),
            ("contexts", List),
            ("parents", List),
            ("satisfies", List),
            ("origins", List),
            ("evidence", List),
            ("depends_on", List),
            ("ready_after", Date),
            ("due_date", Date),
            ("research_question", Text),
            ("timebox_days", Number),
            ("baseline_id", Text),
            ("ready_by", Text),
            ("ready_at", Timestamp),
            ("readiness_evidence", List),
            ("ac_coverage", List),
            ("acceptance_evidence", List),
            ("dod_evidence", List),
        ]),
        "risk" => result.extend([
            ("workflow_status", Choice(&["open", "closed"])),
            ("due_date", Date),
            ("evidence", List),
        ]),
        "decision" => result.extend([
            (
                "workflow_status",
                Choice(&["draft", "accepted", "rejected", "rescinded"]),
            ),
            ("contexts", List),
            ("evidence", List),
            ("decided_by", Text),
            ("decided_at", Timestamp),
            ("decision_evidence", List),
        ]),
        _ => {}
    }
    result
}
fn display(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(v) => v.iter().map(display).collect::<Vec<_>>().join("\n"),
        _ => v.to_string(),
    }
}
fn table_range(body: &str, columns: &[&str]) -> Result<Option<std::ops::Range<usize>>> {
    let mut start = 0;
    let mut found = None;
    let mut fenced = false;
    let mut lines = body.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
        }
        let cells = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect::<Vec<_>>();
        if !fenced && line.trim_start().starts_with('|') && cells == columns {
            ensure!(found.is_none(), "Multiple tables with columns {columns:?}");
            let begin = start;
            start += line.len();
            while lines
                .peek()
                .is_some_and(|s| s.trim_start().starts_with('|'))
            {
                start += lines.next().unwrap().len();
            }
            found = Some(begin..start);
        } else {
            start += line.len();
        }
    }
    Ok(found)
}
fn tables(path: &str, kind: &str, body: &str) -> Result<Vec<Table>> {
    let columns = match path {
        "06_Delivery/People.md" => vec![PEOPLE],
        "06_Delivery/Iterations.md" => vec![PERIODS],
        _ if kind == "pbi" => vec![ESTIMATE, REMAINING],
        _ => vec![],
    };
    columns
        .into_iter()
        .map(|columns| {
            let range = table_range(body, columns)?;
            let rows = range
                .as_ref()
                .map(|r| planning::table(&body[r.clone()], columns))
                .transpose()?
                .unwrap_or_default();
            Ok(Table {
                columns,
                rows,
                present: range.is_some(),
            })
        })
        .collect()
}
fn editable(path: &str, m: &Value) -> Result<()> {
    let allowed = matches!(
        path,
        "06_Delivery/People.md" | "06_Delivery/Iterations.md" | "06_Delivery/Planning.md"
    ) || path.starts_with("05_Backlog/") && m["type"] == "pbi"
        || path.starts_with(crate::reviews::FOLDER)
            && m["type"] == crate::reviews::KIND
            && m["workflow_schema"] == crate::reviews::PROFILE
        || path.starts_with("07_Decisions/")
            && matches!(m["type"].as_str(), Some("risk" | "decision"))
        || delivery::folder(m["type"].as_str().unwrap_or(""))
            .is_some_and(|folder| path.starts_with(folder))
            && m["workflow_schema"] == delivery::PROFILE;
    ensure!(
        allowed && path.ends_with(".md"),
        "Structured editing supports planning, PBIs, risks, decisions, delivery-v1 and material-review-v1 records in their project folders"
    );
    ensure!(
        !path
            .split('/')
            .any(|s| matches!(s, "Baselines" | "Snapshots" | "Assets")),
        "Historical evidence is read-only"
    );
    ensure!(
        m["doc_status"] != "approved" && m["doc_status"] != "retired",
        "Approved or retired records require a new version and Change"
    );
    ensure!(
        m["decision_type"] != "baseline_approval",
        "Use the baseline decision procedure for baseline approvals"
    );
    Ok(())
}
impl Edit {
    pub(crate) fn source_hash(&self) -> Option<String> {
        self.original
            .as_ref()
            .map(|s| workspace::hash(s.as_bytes()))
    }
    pub fn load(w: &Workspace, path: &str) -> Result<Self> {
        let original = w.read(path)?;
        let (metadata, body) = records::parse_project_note(&original)?;
        editable(path, &metadata)?;
        ensure!(
            metadata["type"] != crate::reviews::KIND || metadata["workflow_status"] == "draft",
            "Recorded material reviews are historical; create a new review linked through previous_records"
        );
        let tables = tables(path, metadata["type"].as_str().unwrap_or(""), &body)?;
        ensure!(
            metadata["type"] != "decision" || metadata["workflow_status"] == "draft",
            "Recorded decisions are historical; create a new decision linked to the previous record"
        );
        ensure!(
            !delivery::historical(&metadata),
            "Recorded delivery outcomes are historical; create a new record linked through previous_records"
        );
        Ok(Self {
            path: path.into(),
            original: Some(original),
            metadata,
            body,
            tables,
        })
    }
    pub fn material_review(w: &Workspace, id: &str, expected_hash: &str) -> Result<Self> {
        Ok(Self {
            path: crate::reviews::FOLDER.into(),
            original: None,
            metadata: crate::reviews::prepare(w, id, expected_hash)?,
            body: String::new(),
            tables: vec![],
        })
    }
    pub fn description(&self) -> String {
        if self.metadata["type"] == crate::reviews::KIND {
            format!(
                "{} · target {} · source {}",
                self.path,
                self.metadata["target_id"].as_str().unwrap_or("?"),
                self.metadata["source_id"].as_str().unwrap_or("?")
            )
        } else {
            self.path.clone()
        }
    }
    pub fn create(w: &Workspace, kind: &str) -> Result<Self> {
        ensure!(
            ["pbi", "risk", "decision"].contains(&kind) || delivery::KINDS.contains(&kind),
            "Unsupported record type"
        );
        let mut metadata =
            json!({"id":null,"type":kind,"title":null,"language":w.language()?,"owner":null});
        if kind == "pbi" {
            metadata["delivery_status"] = json!("proposed");
            metadata["doc_status"] = json!("draft");
        } else {
            metadata["workflow_status"] = json!(if kind == "risk" { "open" } else { "draft" });
        }
        if delivery::KINDS.contains(&kind) {
            metadata["workflow_schema"] = json!(delivery::PROFILE);
        }
        let path = if let Some(folder) = delivery::folder(kind) {
            folder
        } else if kind == "pbi" {
            "05_Backlog/"
        } else {
            "07_Decisions/"
        };
        let tables = tables(path, kind, "")?;
        Ok(Self {
            path: path.into(),
            original: None,
            metadata,
            body: String::new(),
            tables,
        })
    }
    pub fn entries(&self) -> Vec<Entry> {
        let mut properties = if self.path == "06_Delivery/Planning.md" {
            vec![("as_of", FieldType::Date)]
        } else if matches!(
            self.path.as_str(),
            "06_Delivery/People.md" | "06_Delivery/Iterations.md"
        ) {
            vec![]
        } else {
            fields(self.metadata["type"].as_str().unwrap_or(""))
        };
        if self.original.is_none() {
            properties.insert(0, ("id", FieldType::Text));
        }
        let mut entries = properties
            .into_iter()
            .map(|(key, kind)| Entry {
                label: key.into(),
                value: display(&self.metadata[key]),
                kind,
                target: Target::Property(key.into()),
            })
            .collect::<Vec<_>>();
        if !matches!(
            self.path.as_str(),
            "06_Delivery/People.md" | "06_Delivery/Iterations.md" | "06_Delivery/Planning.md"
        ) {
            entries.push(Entry {
                label: "Document content (Markdown)".into(),
                value: self.render_body().unwrap_or_else(|_| self.body.clone()),
                kind: FieldType::Text,
                target: Target::Body,
            });
        }
        for (t, table) in self.tables.iter().enumerate() {
            let label = table.columns.join(" / ");
            for (r, row) in table.rows.iter().enumerate() {
                for (c, name) in table.columns.iter().enumerate() {
                    let kind = match *name {
                        "Sprint" => FieldType::Integer,
                        "Start" | "End" => FieldType::Date,
                        "Person" | "Role" | "Project" | "Goal" => FieldType::Text,
                        _ => FieldType::Number,
                    };
                    entries.push(Entry {
                        label: format!("{} row {} · {name}", table.columns[1], r + 1),
                        value: row[c].clone(),
                        kind,
                        target: Target::Cell(t, r, c),
                    });
                }
                entries.push(Entry {
                    label: format!("Remove row {} · {label}", r + 1),
                    value: String::new(),
                    kind: FieldType::Text,
                    target: Target::RemoveRow(t, r),
                });
            }
            entries.push(Entry {
                label: format!("Add row · {label}"),
                value: String::new(),
                kind: FieldType::Text,
                target: Target::AddRow(t),
            });
        }
        entries
    }
    pub fn apply(&mut self, target: &Target, raw: &str) -> Result<()> {
        match target {
            Target::Property(key) => {
                let entry = self
                    .entries()
                    .into_iter()
                    .find(|e| matches!(&e.target, Target::Property(k) if k == key))
                    .context("Field is not editable")?;
                let value = entry.kind.value(raw)?;
                if key == "id" {
                    records::identifier(value.as_str().context("Provide a stable ID")?)?;
                }
                self.metadata[key] = value;
            }
            Target::Cell(t, r, c) => {
                let table = self.tables.get_mut(*t).context("Unknown table")?;
                ensure!(
                    !raw.contains(['|', '\n', '\r']),
                    "Table cells cannot contain pipes or newlines"
                );
                *table
                    .rows
                    .get_mut(*r)
                    .and_then(|r| r.get_mut(*c))
                    .context("Unknown table cell")? = raw.trim().into();
            }
            Target::AddRow(t) => {
                let table = self.tables.get_mut(*t).context("Unknown table")?;
                table.rows.push(vec![String::new(); table.columns.len()]);
                table.present = true;
            }
            Target::RemoveRow(t, r) => {
                let table = self.tables.get_mut(*t).context("Unknown table")?;
                ensure!(*r < table.rows.len(), "Unknown row");
                table.rows.remove(*r);
            }
            Target::Body => {
                let tables = tables(
                    &self.path,
                    self.metadata["type"].as_str().unwrap_or(""),
                    raw,
                )?;
                self.body = raw.into();
                self.tables = tables;
            }
        }
        Ok(())
    }
    fn render_body(&self) -> Result<String> {
        let mut body = self.body.clone();
        for table in &self.tables {
            if !table.present && table.rows.is_empty() {
                continue;
            }
            let range = table_range(&body, table.columns)?;
            let mut rendered = format!(
                "| {} |\n| {} |\n",
                table.columns.join(" | "),
                vec!["---"; table.columns.len()].join(" | ")
            );
            for row in &table.rows {
                rendered.push_str(&format!("| {} |\n", row.join(" | ")));
            }
            if let Some(range) = range {
                body.replace_range(range, &rendered);
            } else {
                body.push_str(&format!("\n\n{rendered}"));
            }
        }
        Ok(body)
    }
    fn candidate(&self, w: &Workspace) -> Result<(Document, String)> {
        let mut path = self.path.clone();
        let m = &self.metadata;
        if self.original.is_none() {
            let id = records::string(m, "id")?;
            records::identifier(&id)?;
            path.push_str(&format!("{id}.md"));
        }
        editable(&path, m)?;
        w.path(&path)?;
        let body = self.render_body()?;
        ensure!(
            body.trim_start().starts_with("# "),
            "Provide a document heading and actual content; no placeholder content is generated"
        );
        for key in ["type", "title", "language"] {
            records::string(m, key)?;
        }
        if !matches!(
            path.as_str(),
            "06_Delivery/People.md" | "06_Delivery/Iterations.md" | "06_Delivery/Planning.md"
        ) {
            ensure!(
                m["language"] == w.language()?,
                "Document language differs from Settings; handle the language migration explicitly"
            );
        }
        for (key, kind) in fields(m["type"].as_str().unwrap_or("")) {
            if m.get(key).is_some() {
                let parsed = kind
                    .value(&display(&m[key]))
                    .with_context(|| key.to_owned())?;
                ensure!(
                    parsed == m[key]
                        || matches!(kind, FieldType::Number)
                            && parsed.as_f64().is_some()
                            && parsed.as_f64() == m[key].as_f64(),
                    "Invalid field type or surrounding whitespace: {key}"
                );
            }
        }
        if matches!(m["type"].as_str(), Some("risk" | "decision")) {
            records::string(m, "workflow_status")?;
        }
        let content = workspace::note(m.clone(), &body)?;
        ensure!(
            content.len() <= workspace::MAX_FILE as usize,
            "Document exceeds 2 MiB"
        );
        Ok((
            Document {
                path,
                title: m["title"].as_str().unwrap().into(),
                kind: m["type"].as_str().unwrap().into(),
                id: m["id"].as_str().map(String::from),
                hash: workspace::hash(content.as_bytes()),
                metadata: m.clone(),
                body,
            },
            content,
        ))
    }
    pub fn preview(&self, w: &Workspace) -> Result<Preview> {
        let (d, content) = self.candidate(w)?;
        unchanged(w, &d.path, self.original.as_deref())?;
        let mut warnings = vec![];
        let mut planning_changes = vec![];
        validate_tables(&self.tables)?;
        if let Some(original) = &self.original {
            let (old, old_body) = records::parse_project_note(original)?;
            ensure!(
                old["id"] == d.metadata["id"] && old["type"] == d.metadata["type"],
                "Record identity cannot change"
            );
            if old["type"] == "release"
                && old["workflow_schema"] == delivery::PROFILE
                && old["workflow_status"] == "ready"
            {
                let mut basis = d.metadata.clone();
                for key in [
                    "workflow_status",
                    "released_by",
                    "released_at",
                    "release_evidence",
                ] {
                    if let Some(value) = old.get(key) {
                        basis[key] = value.clone();
                    } else {
                        basis.as_object_mut().unwrap().remove(key);
                    }
                }
                ensure!(
                    basis == old
                        && old_body.trim() == d.body.trim()
                        && matches!(
                            d.metadata["workflow_status"].as_str(),
                            Some("ready" | "released")
                        ),
                    "A ready release preserves its reviewed scope and go decision; record only the release event here, or create a linked new record for a changed scope or no-go decision"
                );
            }
            if old["type"] == "decision" && old["workflow_status"] != "draft" {
                ensure!(
                    old == d.metadata && original == &content,
                    "Recorded decisions are historical; create a new decision with links to the prior record"
                );
            }
            if old["type"] == "pbi" && old["delivery_status"] != "proposed" {
                ensure!(
                    old["work_type"] == d.metadata["work_type"],
                    "Work type cannot change after readiness; refine a proposed item through the Change procedure"
                );
            }
            if old["type"] == "pbi"
                && old["delivery_status"] != "proposed"
                && records::truth(&old["baseline_id"])
            {
                ensure!(
                    old["baseline_id"] == d.metadata["baseline_id"],
                    "Execution baseline cannot be replaced in this editor; use the Change procedure"
                );
            }
        }
        let record_set = records::Records::load(w)?;
        if d.kind == crate::reviews::KIND {
            crate::reviews::check(&record_set, &d)?;
            crate::reviews::current(&record_set, &d)?;
            warnings.push("This records supplied review facts, not an authenticated identity or business approval. Partial and follow-up reviews do not clear source-review gates. Existing source/evidence bytes remain unchanged.".into());
        }
        if d.kind == "decision" && d.metadata["workflow_status"] != "draft" {
            for key in ["decided_by", "decided_at"] {
                records::string(&d.metadata, key)?;
            }
            require_links(&record_set, &d.metadata, "decision_evidence")?;
        }
        if d.kind == "risk" && d.metadata["workflow_status"] == "closed" {
            require_links(&record_set, &d.metadata, "evidence")?;
        }
        if d.kind == "pbi" {
            for key in ["readiness_evidence", "acceptance_evidence", "dod_evidence"] {
                if records::truth(&d.metadata[key]) {
                    require_links(&record_set, &d.metadata, key)?;
                }
            }
            if d.metadata["sprint"].is_number()
                && !matches!(
                    d.metadata["delivery_status"].as_str(),
                    Some("done" | "cancelled")
                )
            {
                let table = self
                    .tables
                    .iter()
                    .find(|t| t.columns == ESTIMATE)
                    .context("Add estimates for scheduled work")?;
                ensure!(
                    !table.rows.is_empty()
                        && table
                            .rows
                            .iter()
                            .all(|r| r[1..].iter().all(|s| !s.is_empty())),
                    "Scheduled work needs complete original O/M/P estimates"
                );
                if matches!(
                    d.metadata["delivery_status"].as_str(),
                    Some("in_progress" | "blocked")
                ) {
                    let table = self
                        .tables
                        .iter()
                        .find(|t| t.columns == REMAINING)
                        .context("Add remaining estimates")?;
                    ensure!(
                        !table.rows.is_empty()
                            && table
                                .rows
                                .iter()
                                .all(|r| r[1..].iter().all(|s| !s.is_empty())),
                        "In-progress scheduled work needs complete remaining O/M/P estimates"
                    );
                }
            }
        }
        for f in validation::inspect_candidate(w, &d)? {
            ensure!(f.level != "ERROR", "{}: {}", f.code, f.message);
            warnings.push(format!("{}: {}", f.code, f.message));
        }
        if matches!(
            d.path.as_str(),
            "06_Delivery/People.md" | "06_Delivery/Iterations.md" | "06_Delivery/Planning.md"
        ) || d.kind == "pbi"
        {
            check_period_references(w, &d)?;
            match planning::preview(w, &d.path, &content) {
                Ok(plan) => {
                    planning_changes = crate::schedule::impact(planning::calculate(w), &plan);
                    warnings.extend(plan.messages);
                }
                Err(e) if e.downcast_ref::<planning::Incomplete>().is_some() => {
                    warnings.push(format!("Forecast unavailable: {e}"))
                }
                Err(e) => return Err(e.context("Planning check failed")),
            }
        }
        let changes = differences(self.original.as_deref(), &content)?;
        ensure!(!changes.is_empty(), "No changes to save");
        Ok(Preview {
            path: d.path,
            content,
            changes,
            warnings,
            planning_changes,
            context: context(w)?,
            before: self.original.clone(),
        })
    }
    pub fn save(&self, w: &Workspace, reviewed: &Preview) -> Result<Saved> {
        let current = self.preview(w)?;
        ensure!(
            current.context == reviewed.context
                && current.content == reviewed.content
                && current.path == reviewed.path
                && current.warnings == reviewed.warnings
                && current.planning_changes == reviewed.planning_changes,
            "Project inputs changed after preview; review the updated changes before saving"
        );
        unchanged(w, &current.path, current.before.as_deref())?;
        workspace::atomic_write(&w.path(&current.path)?, current.content.as_bytes())?;
        let audit_path = format!("90_Agent/Runs/EDIT-{}.md", uuid::Uuid::new_v4());
        let body = format!(
            "# Project edit record\n\n- Initiated through the local TUI; no user identity is inferred.\n- Project language: {}\n- Agent record language: en\n- Recorded at: {}\n- Document: [[{}]]\n- Before SHA256: {}\n- After SHA256: {}\n\n## Reviewed changes\n\n{}\n\n## Validation notes\n\n{}\n\nStructural checks do not establish business approval, semantic correctness or actual test execution. Forecast reports remain historical until recalculated.\n",
            w.language()?,
            records::timestamp(),
            current.path,
            current
                .before
                .as_deref()
                .map(|s| workspace::hash(s.as_bytes()))
                .unwrap_or_else(|| "new document".into()),
            workspace::hash(current.content.as_bytes()),
            current.changes.join("\n\n"),
            current
                .warnings
                .iter()
                .chain(&current.planning_changes)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n\n")
        );
        let result = workspace::note(json!({"type":"agent_run","title":"Project edit record","language":"en","workflow_status":"completed"}),&body).and_then(|s|workspace::atomic_write(&w.path(&audit_path)?,s.as_bytes()));
        Ok(match result {
            Ok(()) => Saved {
                path: current.path,
                audit_path: Some(audit_path),
                warning: None,
            },
            Err(e) => Saved {
                path: current.path,
                audit_path: None,
                warning: Some(format!(
                    "Document was saved, but its audit record failed: {e}. Do not repeat the save."
                )),
            },
        })
    }
}
fn unchanged(w: &Workspace, path: &str, original: Option<&str>) -> Result<()> {
    match original {
        Some(old) => ensure!(
            w.read(path)? == old,
            "Document changed outside this editor; keep your draft and reopen after reconciling"
        ),
        None => ensure!(
            !w.path(path)?.exists(),
            "A document already exists at this path"
        ),
    }
    Ok(())
}
fn require_links(records: &records::Records, m: &Value, key: &str) -> Result<()> {
    ensure!(
        !records.targets(m.get(key), &[])?.is_empty(),
        "Provide actual {key} document links"
    );
    Ok(())
}
fn context(w: &Workspace) -> Result<String> {
    let values = w
        .documents()?
        .into_iter()
        .map(|d| (d.path, d.hash))
        .collect::<BTreeMap<_, _>>();
    Ok(workspace::hash(serde_json::to_string(&values)?.as_bytes()))
}
fn differences(before: Option<&str>, after: &str) -> Result<Vec<String>> {
    let (new, body) = records::parse_project_note(after)?;
    let (old, old_body) = before
        .map(records::parse_project_note)
        .transpose()?
        .unwrap_or((json!({}), String::new()));
    let mut changes = vec![];
    let keys = old
        .as_object()
        .unwrap()
        .keys()
        .chain(new.as_object().unwrap().keys())
        .collect::<BTreeSet<_>>();
    for key in keys {
        if old.get(key) != new.get(key) {
            changes.push(format!(
                "{key}: {} → {}",
                old.get(key)
                    .map(Value::to_string)
                    .unwrap_or_else(|| "(absent)".into()),
                new.get(key)
                    .map(Value::to_string)
                    .unwrap_or_else(|| "(absent)".into())
            ));
        }
    }
    if old_body != body {
        let a = old_body.lines().collect::<Vec<_>>();
        let b = body.lines().collect::<Vec<_>>();
        let prefix = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
        let suffix = a[prefix..]
            .iter()
            .rev()
            .zip(b[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        changes.push(format!(
            "Body from line {}:\n{}\n{}",
            prefix + 1,
            a[prefix..a.len() - suffix]
                .iter()
                .map(|s| format!("- {s}"))
                .collect::<Vec<_>>()
                .join("\n"),
            b[prefix..b.len() - suffix]
                .iter()
                .map(|s| format!("+ {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    Ok(changes)
}
fn validate_tables(tables: &[Table]) -> Result<()> {
    for table in tables {
        let mut keys = BTreeSet::new();
        let mut periods = vec![];
        let mut allocations: BTreeMap<(&str, i64), (f64, f64)> = BTreeMap::new();
        for row in &table.rows {
            let num = |c: usize| -> Result<f64> {
                let n: f64 = row[c]
                    .parse()
                    .with_context(|| format!("{} needs an explicit number", table.columns[c]))?;
                ensure!(n.is_finite(), "Numbers must be finite");
                Ok(n)
            };
            if table.columns == PERIODS {
                let sprint: i64 = row[0].parse().context("Sprint must be an integer")?;
                let start = planning::date(&row[1])?;
                let end = planning::date(&row[2])?;
                ensure!(
                    keys.insert(sprint.to_string()) && end >= start,
                    "Duplicate or inverted iteration"
                );
                periods.push((start, end));
            } else if table.columns == PEOPLE {
                ensure!(
                    [0, 1, 3].iter().all(|c| !row[*c].trim().is_empty()),
                    "Person, Role and Project are required"
                );
                let sprint: i64 = row[2].parse().context("Sprint must be an integer")?;
                ensure!(
                    keys.insert(format!("{}\0{}\0{}\0{}", row[0], sprint, row[3], row[1])),
                    "Duplicate person allocation"
                );
                let (fte, days, absence, focus, reserve) =
                    (num(4)?, num(5)?, num(6)?, num(7)?, num(8)?);
                ensure!(
                    (0. ..=1.).contains(&fte)
                        && days >= 0.
                        && absence >= 0.
                        && absence <= fte * days
                        && focus > 0.
                        && focus <= 1.
                        && (0. ..1.).contains(&reserve),
                    "Capacity inputs outside allowed range"
                );
                let entry = allocations.entry((&row[0], sprint)).or_insert((0., days));
                entry.0 += fte;
                ensure!(
                    entry.0 <= 1. + 1e-9 && entry.1 == days,
                    "Person is overallocated or has conflicting remaining workdays"
                );
            } else {
                ensure!(
                    !row[0].is_empty() && keys.insert(row[0].clone()),
                    "Estimate role is missing or duplicated"
                );
                if row[1..].iter().all(String::is_empty) {
                    continue;
                }
                let (o, m, p) = (num(1)?, num(2)?, num(3)?);
                ensure!(0. <= o && o <= m && m <= p, "Require 0 <= O <= M <= P");
            }
        }
        periods.sort();
        ensure!(
            periods.windows(2).all(|p| p[1].0 > p[0].1),
            "Iterations overlap"
        );
    }
    Ok(())
}

// These references are checkable even before as_of or capacity is supplied.
fn check_period_references(w: &Workspace, candidate: &Document) -> Result<()> {
    let body = |path: &str| -> Result<String> {
        if path == candidate.path {
            Ok(candidate.body.clone())
        } else {
            Ok(records::parse_project_note(&w.read(path)?)?.1)
        }
    };
    let periods = planning::table(&body("06_Delivery/Iterations.md")?, PERIODS)?
        .iter()
        .map(|row| row[0].parse::<i64>().context("Sprint must be an integer"))
        .collect::<Result<BTreeSet<_>>>()?;
    for row in planning::table(&body("06_Delivery/People.md")?, PEOPLE)? {
        let sprint: i64 = row[2].parse().context("Sprint must be an integer")?;
        ensure!(
            periods.contains(&sprint),
            "Person allocation refers to missing Sprint {sprint}"
        );
    }
    let mut docs = w.documents()?;
    docs.retain(|d| d.path != candidate.path);
    docs.push(candidate.clone());
    for d in docs.iter().filter(|d| {
        d.kind == "pbi"
            && records::MANAGED
                .iter()
                .any(|folder| d.path.starts_with(&format!("{folder}/")))
            && !matches!(
                d.metadata["delivery_status"].as_str(),
                Some("done" | "cancelled")
            )
    }) {
        if !d.metadata["sprint"].is_null() {
            let sprint = d.metadata["sprint"]
                .as_i64()
                .context("Sprint must be an integer")?;
            ensure!(
                periods.contains(&sprint),
                "{} refers to missing Sprint {sprint}",
                d.path
            );
        }
    }
    Ok(())
}
