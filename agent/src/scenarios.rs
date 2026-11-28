//! Compare alternative sprint assignments and apply a freshly reviewed batch.
use crate::{
    editing::{self, ReviewedMove},
    planning::{Forecast, PinnedInputs},
    records, schedule, sources,
    workspace::{Workspace, WorkspaceLock, atomic_create, atomic_write, hash, note},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

const VERSION: u32 = 1;
const REPORTS: &str = ".pm-agent/planning-scenarios";
const JOURNAL: &str = ".pm-agent/planning-scenario-apply.json";
const MAX_MOVES: usize = 20;
const MAX_REPORT: u64 = 16 * 1024 * 1024;
const MAX_JOURNAL: u64 = 192 * 1024 * 1024;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    scenarios: Vec<Variant>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Variant {
    id: String,
    moves: Vec<Assignment>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Assignment {
    pbi: String,
    #[serde(deserialize_with = "destination")]
    sprint: Option<i64>,
}

fn destination<'de, D: Deserializer<'de>>(input: D) -> Result<Option<i64>, D::Error> {
    Option::<i64>::deserialize(input)
}

impl Spec {
    pub fn read(path: &Path) -> Result<Self> {
        let spec: Self = serde_json::from_slice(&sources::bytes(path, 128 * 1024)?)
            .context("Invalid scenario specification")?;
        spec.validate()?;
        Ok(spec)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            (2..=8).contains(&self.scenarios.len()),
            "Supply two through eight alternatives"
        );
        let mut ids = BTreeSet::new();
        for variant in &self.scenarios {
            records::identifier(&variant.id)?;
            ensure!(
                ids.insert(&variant.id),
                "Duplicate scenario ID: {}",
                variant.id
            );
            variant.assignments()?;
        }
        Ok(())
    }
}

impl Variant {
    fn assignments(&self) -> Result<BTreeMap<String, Option<i64>>> {
        ensure!(
            (1..=MAX_MOVES).contains(&self.moves.len()),
            "Scenario {} needs one through {MAX_MOVES} moves",
            self.id
        );
        let mut moves = BTreeMap::new();
        for change in &self.moves {
            records::identifier(&change.pbi)?;
            ensure!(
                moves.insert(change.pbi.clone(), change.sprint).is_none(),
                "Duplicate PBI in scenario {}: {}",
                self.id,
                change.pbi
            );
        }
        Ok(moves)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    Forecast { forecast: Forecast },
    Blocked { error: String },
}

impl Outcome {
    fn new(result: Result<Forecast>) -> Self {
        match result {
            Ok(forecast) => Self::Forecast { forecast },
            Err(error) => Self::Blocked {
                error: format!("{error:#}"),
            },
        }
    }

    fn forecast(&self) -> Result<&Forecast> {
        match self {
            Self::Forecast { forecast } => Ok(forecast),
            Self::Blocked { error } => bail!("{error}"),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Comparison {
    schema_version: u32,
    pub id: String,
    pub simulation: bool,
    pub created_at: String,
    pub input_sha256: String,
    pub inputs: BTreeMap<String, String>,
    pub current_schedule: Outcome,
    pub scenarios: BTreeMap<String, Outcome>,
    spec: Spec,
}

pub fn compare(w: &Workspace, lock: &WorkspaceLock, id: &str, spec: Spec) -> Result<Comparison> {
    lock.verify(w)?;
    require_no_pending_apply(w)?;
    let path = comparison_path(id)?;
    spec.validate()?;
    let pinned = PinnedInputs::capture(w)?;
    let current_schedule = Outcome::new(pinned.forecast(&BTreeMap::new()));
    let mut scenarios = BTreeMap::new();
    for variant in &spec.scenarios {
        let result = (|| {
            let moves = variant.assignments()?;
            check_moves(&pinned, &moves)?;
            pinned.forecast(&moves)
        })();
        scenarios.insert(variant.id.clone(), Outcome::new(result));
    }
    ensure!(
        PinnedInputs::capture(w)?.pins == pinned.pins,
        "Planning inputs changed during comparison. Run it again"
    );
    let comparison = Comparison {
        schema_version: VERSION,
        id: id.into(),
        simulation: true,
        created_at: records::timestamp(),
        input_sha256: fingerprint(&pinned.pins)?,
        inputs: pinned.pins,
        current_schedule,
        scenarios,
        spec,
    };
    let text = serde_json::to_vec(&comparison)?;
    ensure!(
        text.len() as u64 <= MAX_REPORT,
        "Scenario comparison is larger than 16 MiB"
    );
    atomic_create(&w.path(&path)?, &text)?;
    Ok(comparison)
}

fn fingerprint(value: &impl Serialize) -> Result<String> {
    Ok(hash(&serde_json::to_vec(value)?))
}

fn comparison_path(id: &str) -> Result<String> {
    records::identifier(id)?;
    Ok(format!("{REPORTS}/{id}.json"))
}

pub fn load(w: &Workspace, id: &str) -> Result<Comparison> {
    let report: Comparison = serde_json::from_slice(&sources::bytes(
        &w.path(&comparison_path(id)?)?,
        MAX_REPORT,
    )?)?;
    ensure!(
        report.schema_version == VERSION
            && report.id == id
            && report.simulation
            && report.input_sha256 == fingerprint(&report.inputs)?,
        "Saved scenario comparison is invalid"
    );
    report.spec.validate()?;
    ensure!(
        report.scenarios.keys().eq(report
            .spec
            .scenarios
            .iter()
            .map(|s| &s.id)
            .collect::<BTreeSet<_>>()),
        "Scenario results do not match the saved alternatives"
    );
    Ok(report)
}

pub fn list(w: &Workspace) -> Result<Vec<String>> {
    let path = w.path(REPORTS)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut ids = vec![];
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && let Some(name) = entry.path().file_stem().and_then(|s| s.to_str())
            && entry.path().extension().is_some_and(|ext| ext == "json")
            && records::identifier(name).is_ok()
        {
            ids.push(name.to_owned());
        }
    }
    ids.sort();
    Ok(ids)
}

fn check_moves(pinned: &PinnedInputs, moves: &BTreeMap<String, Option<i64>>) -> Result<()> {
    for (id, sprint) in moves {
        let item = item(pinned, id)?;
        ensure!(
            item.path.starts_with("05_Backlog/") && item.path.ends_with(".md"),
            "Scenario moves require an editable PBI under 05_Backlog: {id}"
        );
        ensure!(
            !matches!(
                item.metadata["delivery_status"].as_str(),
                Some("done" | "cancelled")
            ),
            "Completed or cancelled work is historical: {id}"
        );
        let previous = item.metadata["sprint"].as_i64();
        ensure!(
            item.metadata["sprint"].is_null() || previous.is_some(),
            "Invalid recorded sprint: {id}"
        );
        ensure!(
            previous != *sprint,
            "Scenario destination already matches the recorded sprint: {id}"
        );
    }
    Ok(())
}

fn item<'a>(pinned: &'a PinnedInputs, id: &str) -> Result<&'a crate::workspace::Document> {
    let mut matches = pinned.items.iter().filter(|d| d.id.as_deref() == Some(id));
    let first = matches
        .next()
        .with_context(|| format!("No active PBI with ID {id}"))?;
    ensure!(matches.next().is_none(), "Duplicate PBI ID: {id}");
    Ok(first)
}

impl Comparison {
    pub fn describe(&self, selected: &str) -> Result<String> {
        let outcome = self.scenarios.get(selected).context("Unknown scenario")?;
        let mut lines = vec![format!(
            "# Simulation · {} / {selected}\n\nPinned planning inputs {}\n",
            self.id, self.input_sha256
        )];
        for change in &self
            .spec
            .scenarios
            .iter()
            .find(|s| s.id == selected)
            .context("Missing scenario specification")?
            .moves
        {
            lines.push(format!(
                "- {} → {}",
                change.pbi,
                sprint_label(change.sprint)
            ));
        }
        if let Ok(after) = outcome.forecast() {
            lines.push("\n## Capacity and forecast impact\n".into());
            lines.extend(impact(&self.current_schedule, after));
        } else if let Outcome::Blocked { error } = outcome {
            lines.push(format!("\nForecast blocked. {error}"));
        }
        Ok(lines.join("\n"))
    }
}

fn sprint_label(sprint: Option<i64>) -> String {
    sprint
        .map(|value| format!("Sprint {value}"))
        .unwrap_or_else(|| "Unscheduled".into())
}

fn impact(before: &Outcome, after: &Forecast) -> Vec<String> {
    let mut lines = schedule::impact(before.forecast(), after);
    let count = |f: &Forecast| {
        f.items
            .iter()
            .filter(|item| {
                item.sprint.is_none()
                    && !matches!(item.delivery_status.as_str(), "done" | "cancelled")
            })
            .count()
    };
    lines.push(format!(
        "Unscheduled active PBIs: {} → {}",
        before
            .forecast()
            .map(|f| count(f).to_string())
            .unwrap_or_else(|_| "unavailable".into()),
        count(after)
    ));
    lines.push("\n### Forecast and dependency signals\n".into());
    if let Ok(old) = before.forecast() {
        for message in old
            .messages
            .iter()
            .filter(|message| !after.messages.contains(message))
        {
            lines.push(format!("- Cleared · {message}"));
        }
    }
    for message in &after.messages {
        let status = if before
            .forecast()
            .is_ok_and(|old| old.messages.contains(message))
        {
            "Existing"
        } else {
            "Projected"
        };
        lines.push(format!("- {status} · {message}"));
    }
    if after.messages.is_empty() {
        lines.push("No forecast or dependency signals in this simulation.".into());
    }
    lines
}

#[derive(Debug, Serialize)]
pub struct MovePreview {
    pub path: String,
    pbi: String,
    before: Option<i64>,
    after: Option<i64>,
    before_sha256: String,
    after_sha256: String,
    warnings: Vec<String>,
}

#[derive(Serialize)]
struct Proposal {
    comparison: String,
    scenario: String,
    input_sha256: String,
    validation_inputs_sha256: String,
    moves: Vec<MovePreview>,
    impact: Vec<String>,
}

#[derive(Serialize)]
pub struct Review {
    pub review_sha256: String,
    pub simulation: bool,
    proposal: Proposal,
    #[serde(skip)]
    edits: Vec<ReviewedMove>,
}

impl Review {
    pub fn describe(&self) -> String {
        let p = &self.proposal;
        let mut lines = vec![format!(
            "# Review application · {} / {}\n\nPlanning basis {}\n\nReview SHA256 {}\n",
            p.comparison, p.scenario, p.input_sha256, self.review_sha256
        )];
        for change in &p.moves {
            lines.push(format!(
                "- {} · {} → {} · [[{}]]",
                change.pbi,
                sprint_label(change.before),
                sprint_label(change.after),
                change.path
            ));
            lines.extend(
                change
                    .warnings
                    .iter()
                    .map(|warning| format!("  - {warning}")),
            );
        }
        lines.push("\n## Simulated impact\n".into());
        lines.extend(p.impact.clone());
        lines.join("\n")
    }
}

pub fn review(w: &Workspace, batch: &str, selected: &str) -> Result<Review> {
    require_no_pending_apply(w)?;
    let comparison = load(w, batch)?;
    let expected = comparison
        .scenarios
        .get(selected)
        .context("Unknown scenario")?
        .forecast()?;
    let moves = comparison
        .spec
        .scenarios
        .iter()
        .find(|s| s.id == selected)
        .context("Missing scenario specification")?
        .assignments()?;
    let pinned = PinnedInputs::capture(w)?;
    ensure!(
        comparison.inputs == pinned.pins,
        "Planning inputs changed since simulation. Create a new comparison before reviewing"
    );
    let validation_inputs_sha256 = editing::context(w)?;
    check_moves(&pinned, &moves)?;
    let mut edits = vec![];
    let mut previews = vec![];
    for (id, target) in moves {
        let doc = item(&pinned, &id)?;
        let edit = schedule::propose(w, &doc.path, &doc.hash, target)?
            .review_move(w)
            .with_context(|| format!("Scheduling checks failed for {id}"))?;
        previews.push(MovePreview {
            path: edit.path.clone(),
            pbi: id,
            before: doc.metadata["sprint"].as_i64(),
            after: target,
            before_sha256: doc.hash.clone(),
            after_sha256: hash(edit.after.as_bytes()),
            warnings: edit.warnings.clone(),
        });
        edits.push(edit);
    }
    let changes = edits
        .iter()
        .map(|e| (e.path.as_str(), e.after.as_str()))
        .collect();
    let after = pinned.forecast_edits(&changes)?;
    ensure!(
        fingerprint(&after)? == fingerprint(expected)?,
        "Reviewed forecast differs from the saved simulation. Create a new comparison"
    );
    ensure!(
        PinnedInputs::capture(w)?.pins == pinned.pins
            && editing::context(w)? == validation_inputs_sha256,
        "Planning or validation inputs changed during review. Review again"
    );
    let proposal = Proposal {
        comparison: batch.into(),
        scenario: selected.into(),
        input_sha256: comparison.input_sha256,
        validation_inputs_sha256,
        moves: previews,
        impact: impact(&comparison.current_schedule, &after),
    };
    Ok(Review {
        review_sha256: fingerprint(&proposal)?,
        simulation: true,
        proposal,
        edits,
    })
}

#[derive(Deserialize, Serialize)]
struct Journal {
    schema_version: u32,
    audit_path: String,
    audit_sha256: String,
    entries: Vec<WriteEntry>,
}

#[derive(Deserialize, Serialize)]
struct WriteEntry {
    path: String,
    before: String,
    after: String,
}

pub(crate) fn require_no_pending_apply(w: &Workspace) -> Result<()> {
    ensure!(
        !w.path(JOURNAL)?.try_exists()?,
        "A scenario application is interrupted. Use scenario recover to restore a consistent schedule"
    );
    Ok(())
}

pub fn apply(
    w: &Workspace,
    lock: &WorkspaceLock,
    batch: &str,
    selected: &str,
    expected: &str,
) -> Result<Value> {
    lock.verify(w)?;
    let review = review(w, batch, selected)?;
    ensure!(
        review.review_sha256 == expected,
        "Scenario review changed. Inspect a fresh review before applying"
    );
    let audit_path = format!("90_Agent/Runs/EDIT-SCENARIO-{}.md", uuid::Uuid::new_v4());
    let audit = note(
        json!({"type":"agent_run","title":"Planning scenario application","language":"en","workflow_status":"completed"}),
        &format!(
            "# Planning scenario application\n\nApplied the reviewed sprint assignments on {}.\n\n{}\n\nForecast and dependency effects above are simulations. Run pm plan to recalculate the current report.\n",
            records::timestamp(),
            review.describe()
        ),
    )?;
    ensure!(
        audit.len() as u64 <= crate::workspace::MAX_FILE,
        "Scenario audit exceeds 2 MiB"
    );
    let journal = Journal {
        schema_version: VERSION,
        audit_path: audit_path.clone(),
        audit_sha256: hash(audit.as_bytes()),
        entries: review
            .edits
            .into_iter()
            .map(|e| WriteEntry {
                path: e.path,
                before: e.before,
                after: e.after,
            })
            .collect(),
    };
    let data = serde_json::to_vec(&journal)?;
    ensure!(
        data.len() as u64 <= MAX_JOURNAL,
        "Scenario batch recovery data is too large"
    );
    let journal_path = w.path(JOURNAL)?;
    atomic_create(&journal_path, &data)?;
    let result = (|| -> Result<()> {
        let mut inputs = PinnedInputs::capture(w)?.pins;
        ensure!(
            fingerprint(&inputs)? == review.proposal.input_sha256
                && editing::context(w)? == review.proposal.validation_inputs_sha256,
            "Planning or validation inputs changed before saving"
        );
        for entry in &journal.entries {
            ensure!(
                w.read(&entry.path)? == entry.before,
                "PBI changed before save: {}",
                entry.path
            );
            atomic_write(&w.path(&entry.path)?, entry.after.as_bytes())?;
            inputs.insert(entry.path.clone(), hash(entry.after.as_bytes()));
        }
        ensure!(
            PinnedInputs::capture(w)?.pins == inputs,
            "Planning inputs changed during saving"
        );
        atomic_create(&w.path(&audit_path)?, audit.as_bytes())
    })();
    if let Err(error) = result {
        return match rollback(w, &journal) {
            Ok(()) => Err(error.context("Scenario application was rolled back")),
            Err(recovery) => Err(error.context(format!(
                "Recovery is pending. Run scenario recover. {recovery:#}"
            ))),
        };
    }
    let warning = fs::remove_file(journal_path)
        .err()
        .map(|e| format!("Scenario committed. Run scenario recover to clear its journal. {e}"));
    Ok(
        json!({"comparison":batch,"scenario":selected,"status":"applied","review_sha256":expected,
        "audit_path":audit_path,"changed":journal.entries.iter().map(|e| &e.path).collect::<Vec<_>>(),"warning":warning}),
    )
}

pub fn recover(w: &Workspace, lock: &WorkspaceLock) -> Result<Value> {
    lock.verify(w)?;
    let path = w.path(JOURNAL)?;
    let journal: Journal = serde_json::from_slice(&sources::bytes(&path, MAX_JOURNAL)?)?;
    validate_journal(w, &journal)?;
    if w.path(&journal.audit_path)?.try_exists()? {
        ensure!(
            hash(w.read(&journal.audit_path)?.as_bytes()) == journal.audit_sha256,
            "Scenario commit audit changed. Reconcile the interrupted application before clearing its journal"
        );
        fs::remove_file(path)?;
        return Ok(json!({"status":"applied","audit_path":journal.audit_path}));
    }
    rollback(w, &journal)?;
    Ok(json!({"status":"rolled_back"}))
}

fn validate_journal(w: &Workspace, journal: &Journal) -> Result<()> {
    ensure!(
        journal.schema_version == VERSION && (1..=MAX_MOVES).contains(&journal.entries.len()),
        "Invalid scenario journal"
    );
    let suffix = journal
        .audit_path
        .strip_prefix("90_Agent/Runs/EDIT-SCENARIO-")
        .and_then(|s| s.strip_suffix(".md"))
        .context("Invalid scenario audit path")?;
    uuid::Uuid::parse_str(suffix)?;
    let mut paths = BTreeSet::new();
    for entry in &journal.entries {
        w.path(&entry.path)?;
        ensure!(
            entry.path.starts_with("05_Backlog/")
                && entry.path.ends_with(".md")
                && paths.insert(&entry.path),
            "Invalid PBI path in scenario journal"
        );
        let (mut before, old_body) = records::parse_project_note(&entry.before)?;
        let (after, new_body) = records::parse_project_note(&entry.after)?;
        ensure!(
            !entry
                .path
                .split('/')
                .any(|part| matches!(part, "Assets" | "Snapshots" | "Baselines"))
                && !matches!(before["doc_status"].as_str(), Some("approved" | "retired"))
                && !matches!(
                    before["delivery_status"].as_str(),
                    Some("done" | "cancelled")
                )
                && (after["sprint"].is_null() || after["sprint"].as_i64().is_some())
                && before["sprint"] != after["sprint"],
            "Invalid sprint destination or historical PBI in scenario journal"
        );
        before["sprint"] = after["sprint"].clone();
        ensure!(
            before["type"] == "pbi" && before == after && old_body == new_body,
            "Invalid sprint edit in scenario journal"
        );
    }
    Ok(())
}

fn rollback(w: &Workspace, journal: &Journal) -> Result<()> {
    for entry in &journal.entries {
        let current = w.read(&entry.path)?;
        ensure!(
            current == entry.before || current == entry.after,
            "PBI changed outside the scenario application: {}",
            entry.path
        );
    }
    for entry in journal.entries.iter().rev() {
        let current = w.read(&entry.path)?;
        if current != entry.before {
            ensure!(
                current == entry.after,
                "PBI changed during recovery: {}",
                entry.path
            );
            atomic_write(&w.path(&entry.path)?, entry.before.as_bytes())?;
        }
    }
    fs::remove_file(w.path(JOURNAL)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_rolls_back_partial_writes_and_keeps_a_committed_batch() {
        let fixture = tempfile::tempdir().unwrap();
        let w = Workspace {
            root: fixture.path().to_owned(),
        };
        let lock = WorkspaceLock::acquire(&w).unwrap();
        let audit = note(
            json!({"type":"agent_run","title":"Synthetic","language":"en"}),
            "# Synthetic audit",
        )
        .unwrap();
        let journal = Journal {
            schema_version: VERSION,
            audit_path: format!("90_Agent/Runs/EDIT-SCENARIO-{}.md", uuid::Uuid::new_v4()),
            audit_sha256: hash(audit.as_bytes()),
            entries: ["PBI-A", "PBI-B"]
                .into_iter()
                .map(|id| {
                    let before =
                        note(json!({"id":id,"type":"pbi","sprint":1}), "# Synthetic").unwrap();
                    WriteEntry {
                        path: format!("05_Backlog/{id}.md"),
                        after: before.replace("sprint: 1", "sprint: 2"),
                        before,
                    }
                })
                .collect(),
        };
        let first = &journal.entries[0];
        let second = &journal.entries[1];
        let journal_path = w.path(JOURNAL).unwrap();
        let data = serde_json::to_vec(&journal).unwrap();
        atomic_create(&journal_path, &data).unwrap();
        atomic_write(&w.path(&first.path).unwrap(), first.after.as_bytes()).unwrap();
        atomic_write(&w.path(&second.path).unwrap(), b"External edit").unwrap();
        assert!(require_no_pending_apply(&w).is_err());
        assert!(
            crate::planning::run(&w, &[])
                .unwrap_err()
                .to_string()
                .contains("scenario recover")
        );
        let backup = crate::backup::create_locked(
            &w,
            &fixture.path().join("backup.zip"),
            false,
            &lock,
            &tokio_util::sync::CancellationToken::new(),
        );
        assert!(backup.is_err());
        assert!(
            recover(&w, &lock)
                .unwrap_err()
                .to_string()
                .contains("changed outside")
        );
        assert_eq!(w.read(&first.path).unwrap(), first.after);
        assert_eq!(w.read(&second.path).unwrap(), "External edit");

        atomic_write(&w.path(&second.path).unwrap(), second.before.as_bytes()).unwrap();
        assert_eq!(recover(&w, &lock).unwrap()["status"], "rolled_back");
        assert_eq!(w.read(&first.path).unwrap(), first.before);
        assert_eq!(w.read(&second.path).unwrap(), second.before);
        assert!(!journal_path.exists());

        atomic_create(&journal_path, &data).unwrap();
        for entry in &journal.entries {
            atomic_write(&w.path(&entry.path).unwrap(), entry.after.as_bytes()).unwrap();
        }
        atomic_create(&w.path(&journal.audit_path).unwrap(), audit.as_bytes()).unwrap();
        assert_eq!(recover(&w, &lock).unwrap()["status"], "applied");
        assert_eq!(w.read(&first.path).unwrap(), first.after);
        assert_eq!(w.read(&second.path).unwrap(), second.after);
        assert!(!journal_path.exists());
    }
}
