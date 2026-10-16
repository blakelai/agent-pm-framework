//! Deterministic rolling forecasts from the existing Markdown contract.
//! Unknown effort is represented explicitly; this module never assigns work or dates.
use crate::workspace::{Document, Workspace, atomic_write, hash, note, parse_note};
use anyhow::{Context, Result, bail, ensure};
use chrono::{NaiveDate, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::locale::Catalog;

const REPORT: &str = "06_Delivery/Plan-Report.md";
const MANAGED: &[&str] = &[
    "00_Governance",
    "01_Sources/Notes",
    "01_Sources/Evidence",
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

#[derive(Debug, Serialize)]
pub struct Period {
    pub sprint: i64,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub goal: String,
}
#[derive(Debug, Default, Serialize)]
pub struct Capacity {
    pub sprint: i64,
    pub role: String,
    pub capacity: f64,
    pub effort: f64,
    pub items: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub path: String,
    pub delivery_status: String,
    pub sprint: Option<i64>,
    pub original: Option<f64>,
    pub remaining: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct Forecast {
    pub as_of: NaiveDate,
    pub periods: Vec<Period>,
    pub caps: Vec<Capacity>,
    pub items: Vec<Item>,
    pub messages: Vec<String>,
}

fn table(body: &str, columns: &[&str]) -> Result<Vec<Vec<String>>> {
    let mut header: Option<Vec<String>> = None;
    let mut current = vec![];
    let mut matches = vec![];
    for line in body.lines().chain(std::iter::once("")) {
        let line = line.trim();
        if !line.starts_with('|') {
            if header.as_ref().is_some_and(|h| h == columns) {
                matches.push(std::mem::take(&mut current));
            }
            header = None;
            current.clear();
            continue;
        }
        let cells: Vec<String> = line
            .trim_matches('|')
            .split('|')
            .map(|s| s.trim().to_owned())
            .collect();
        if cells
            .iter()
            .all(|s| !s.is_empty() && s.chars().all(|c| matches!(c, ':' | '-' | ' ')))
        {
            continue;
        }
        if let Some(h) = &header {
            ensure!(
                cells.len() == h.len(),
                "Table row has a different number of columns than its header"
            );
            current.push(cells);
        } else {
            header = Some(cells);
        }
    }
    ensure!(
        matches.len() == 1,
        "Expected one table with columns: {columns:?}"
    );
    Ok(matches.pop().unwrap())
}
fn date(s: &str) -> Result<NaiveDate> {
    let value = NaiveDate::parse_from_str(s, "%Y-%m-%d").context("Use an ISO date YYYY-MM-DD")?;
    ensure!(
        value.format("%Y-%m-%d").to_string() == s,
        "Use an ISO date YYYY-MM-DD"
    );
    Ok(value)
}
fn finite(value: f64) -> Result<f64> {
    ensure!(
        value.is_finite(),
        "Numbers and calculated totals must be finite"
    );
    Ok(value)
}
fn number(s: &str) -> Result<f64> {
    finite(s.parse().context("Expected a numeric planning input")?)
}
fn sum(values: impl Iterator<Item = f64>) -> Result<f64> {
    finite(values.fold(0., |total, value| total + value))
}
fn effort(body: &str, remaining: bool) -> Result<BTreeMap<String, f64>> {
    let headers = if remaining {
        ["Role", "Remaining O", "Remaining M", "Remaining P"]
    } else {
        ["Role", "O", "M", "P"]
    };
    let mut result = BTreeMap::new();
    for row in table(body, &headers)? {
        let role = &row[0];
        ensure!(
            !role.is_empty() && !result.contains_key(role),
            "Missing or duplicate estimate role"
        );
        let (o, m, p) = (number(&row[1])?, number(&row[2])?, number(&row[3])?);
        ensure!(0. <= o && o <= m && m <= p, "Require 0 <= O <= M <= P");
        result.insert(role.clone(), finite((o + 4. * m + p) / 6.)?);
    }
    ensure!(!result.is_empty(), "Empty estimate");
    Ok(result)
}
fn read(workspace: &Workspace, path: &str) -> Result<(Value, String)> {
    parse_note(&workspace.read(path)?).with_context(|| path.to_owned())
}
fn optional_date(meta: &Value, key: &str) -> Result<Option<NaiveDate>> {
    match &meta[key] {
        Value::Null => Ok(None),
        Value::String(s) if s.is_empty() => Ok(None),
        Value::String(s) => Ok(Some(date(s)?)),
        _ => bail!("{key} must be an ISO date or null"),
    }
}
fn managed(d: &Document) -> bool {
    MANAGED.iter().any(|prefix| {
        d.path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
    })
}
fn dependency<'a>(reference: &str, docs: &'a [Document]) -> Result<&'a str> {
    let path = reference
        .trim_start_matches("[[")
        .trim_end_matches("]]")
        .split(['|', '#'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".md");
    // Resolve only known project PBIs; ambiguous short names cannot select an arbitrary note.
    let direct = docs.iter().find(|d| d.path.trim_end_matches(".md") == path);
    if let Some(d) = direct {
        return Ok(d.id.as_deref().unwrap());
    }
    let candidates: Vec<_> = docs
        .iter()
        .filter(|d| {
            !path.contains('/')
                && d.path.rsplit('/').next().unwrap().trim_end_matches(".md") == path
        })
        .collect();
    ensure!(
        candidates.len() == 1,
        "Unresolved or ambiguous PBI dependency: {reference}"
    );
    Ok(candidates[0].id.as_deref().unwrap())
}

/// Read and validate the current inputs without changing project records.
pub fn calculate(workspace: &Workspace) -> Result<Forecast> {
    calculate_with(workspace, &Catalog::load(workspace)?)
}
fn calculate_with(workspace: &Workspace, t: &Catalog) -> Result<Forecast> {
    // Do not silently omit linked PBI files from a capacity calculation.
    for folder in MANAGED {
        let path = workspace.path(folder)?;
        if !path.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(path).follow_links(false) {
            ensure!(
                !entry?.file_type().is_symlink(),
                "Planning inputs cannot contain symlinks"
            );
        }
    }
    let (_, body) = read(workspace, "06_Delivery/Iterations.md")?;
    let mut periods = BTreeMap::new();
    for row in table(&body, &["Sprint", "Start", "End", "Goal"])? {
        let sprint: i64 = row[0].parse().context("Sprint must be an integer")?;
        let period = Period {
            sprint,
            start: date(&row[1])?,
            end: date(&row[2])?,
            goal: row[3].clone(),
        };
        ensure!(
            period.end >= period.start && !periods.contains_key(&sprint),
            "Duplicate, inverted or overlapping iterations"
        );
        periods.insert(sprint, period);
    }
    ensure!(
        !periods.is_empty(),
        "Planning is not configured: add periods to 06_Delivery/Iterations.md"
    );
    let mut ordered: Vec<_> = periods.values().collect();
    ordered.sort_by_key(|p| p.start);
    ensure!(
        ordered.windows(2).all(|p| p[1].start > p[0].end),
        "Duplicate, inverted or overlapping iterations"
    );
    let (config, _) = read(workspace, "06_Delivery/Planning.md")?;
    let as_of = optional_date(&config, "as_of")?.context(
        "Planning is not configured: set as_of in 06_Delivery/Planning.md to YYYY-MM-DD",
    )?;
    let (project, _) = read(workspace, "00_Governance/Project.md")?;
    let project = project["id"]
        .as_str()
        .context("Project needs a stable id")?;
    let (_, body) = read(workspace, "06_Delivery/People.md")?;
    let mut caps: BTreeMap<(i64, String), Capacity> = BTreeMap::new();
    let mut allocation: BTreeMap<(String, i64), f64> = BTreeMap::new();
    let mut calendars = BTreeMap::new();
    let mut keys = BTreeSet::new();
    for row in table(
        &body,
        &[
            "Person", "Role", "Sprint", "Project", "FTE", "Workdays", "Absence", "Focus", "Reserve",
        ],
    )? {
        let sprint: i64 = row[2].parse().context("Sprint must be an integer")?;
        let person = &row[0];
        let role = &row[1];
        let key = (person.clone(), sprint, row[3].clone(), role.clone());
        ensure!(
            !person.is_empty()
                && !role.is_empty()
                && !row[3].is_empty()
                && keys.insert(key)
                && periods.contains_key(&sprint),
            "Invalid or duplicate person allocation"
        );
        let (fte, days, absence, focus, reserve) = (
            number(&row[4])?,
            number(&row[5])?,
            number(&row[6])?,
            number(&row[7])?,
            number(&row[8])?,
        );
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
        let period = &periods[&sprint];
        let window = ((period.end - period.start.max(as_of)).num_days() + 1).max(0);
        ensure!(
            days <= window as f64,
            "Remaining workdays exceed remaining calendar days"
        );
        let key = (person.clone(), sprint);
        let allocated = allocation.entry(key.clone()).or_default();
        *allocated += fte;
        ensure!(
            *allocated <= 1. + 1e-9,
            "Person overallocated across roles/projects: {person}, Sprint {sprint}"
        );
        if let Some(previous) = calendars.insert(key, days) {
            ensure!(
                previous == days,
                "Conflicting remaining workdays for same person/Sprint"
            );
        }
        if row[3] != project {
            continue;
        }
        let cap = caps
            .entry((sprint, role.clone()))
            .or_insert_with(|| Capacity {
                sprint,
                role: role.clone(),
                ..Default::default()
            });
        if period.end >= as_of {
            cap.capacity = finite(cap.capacity + (fte * days - absence) * focus * (1. - reserve))?;
        }
    }
    ensure!(!caps.is_empty(), "No allocations for current project");
    let docs: Vec<_> = workspace
        .documents()?
        .into_iter()
        .filter(|d| managed(d) && d.kind == "pbi" && d.id.is_some())
        .collect();
    let ids: BTreeMap<_, _> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| (d.id.as_deref().unwrap(), i))
        .collect();
    ensure!(ids.len() == docs.len(), "Duplicate PBI ID");
    let mut graph: Vec<Vec<usize>> = vec![vec![]; docs.len()];
    let mut items = vec![];
    let mut messages = vec![];
    for (i, d) in docs.iter().enumerate() {
        let id = d.id.as_ref().unwrap();
        let m = &d.metadata;
        let state = m["delivery_status"].as_str().unwrap_or("");
        ensure!(
            [
                "proposed",
                "ready",
                "in_progress",
                "blocked",
                "done",
                "cancelled"
            ]
            .contains(&state),
            "Invalid delivery_status: {id}"
        );
        if let Some(refs) = m.get("depends_on") {
            for reference in refs.as_array().context("depends_on must be a list")? {
                let dep = dependency(
                    reference.as_str().context("Dependency must be a link")?,
                    &docs,
                )?;
                graph[i].push(ids[dep]);
            }
        }
        let sprint = if m["sprint"].is_null() {
            None
        } else {
            Some(
                m["sprint"]
                    .as_i64()
                    .with_context(|| format!("Invalid Sprint: {id}"))?,
            )
        };
        let mut item = Item {
            id: id.clone(),
            path: d.path.clone(),
            delivery_status: state.into(),
            sprint,
            original: None,
            remaining: None,
        };
        if matches!(state, "done" | "cancelled") {
            item.remaining = Some(0.);
            items.push(item);
            continue;
        }
        if let Some(s) = sprint {
            ensure!(periods.contains_key(&s), "Invalid Sprint: {id}");
        }
        let estimates = (|| -> Result<_> {
            let initial = effort(&d.body, false)?;
            let remaining = if matches!(state, "in_progress" | "blocked") {
                effort(&d.body, true)?
            } else {
                initial.clone()
            };
            Ok((sum(initial.values().copied())?, remaining))
        })();
        let estimate = match estimates {
            Ok((initial, remaining)) => {
                item.original = Some(initial);
                item.remaining = Some(sum(remaining.values().copied())?);
                Some(remaining)
            }
            Err(e) if sprint.is_some() => return Err(e.context(id.clone())),
            Err(_) => None,
        };
        if let Some(sprint) = sprint {
            let period = &periods[&sprint];
            ensure!(
                period.end >= as_of,
                "{id}: unfinished work assigned to a past Sprint; reschedule it"
            );
            for (role, amount) in estimate.unwrap() {
                let cap = caps.get_mut(&(sprint, role.clone())).with_context(|| {
                    format!("{id}: no capacity row for {role}, Sprint {sprint}")
                })?;
                cap.effort = finite(cap.effort + amount)?;
                if amount != 0. {
                    cap.items.push(id.clone());
                }
            }
            if state == "blocked" {
                messages.push(t.render("blocked", &[("id", id.clone())])?);
            }
            if let Some(ready) = optional_date(m, "ready_after")? {
                let key = if ready > period.end {
                    Some("external_late")
                } else if ready > period.start.max(as_of) {
                    Some("external_wait")
                } else {
                    None
                };
                if let Some(key) = key {
                    messages.push(t.render(key, &[("id", id.clone())])?);
                }
            }
            if optional_date(m, "due_date")?.is_some_and(|d| period.end > d) {
                messages.push(t.render("deadline", &[("id", id.clone())])?);
            }
        }
        items.push(item);
    }
    // Iterative cycle detection avoids recursion depth depending on project size.
    let mut counts: Vec<_> = graph.iter().map(Vec::len).collect();
    let mut downstream = vec![vec![]; graph.len()];
    for (i, deps) in graph.iter().enumerate() {
        for dep in deps {
            downstream[*dep].push(i);
        }
    }
    let mut ready: VecDeque<_> = counts
        .iter()
        .enumerate()
        .filter_map(|(i, n)| (*n == 0).then_some(i))
        .collect();
    let mut visited = 0;
    while let Some(i) = ready.pop_front() {
        visited += 1;
        for next in &downstream[i] {
            counts[*next] -= 1;
            if counts[*next] == 0 {
                ready.push_back(*next);
            }
        }
    }
    ensure!(visited == graph.len(), "Cyclic PBI dependencies");
    for (i, deps) in graph.iter().enumerate() {
        let item = &items[i];
        if matches!(item.delivery_status.as_str(), "done" | "cancelled") || item.sprint.is_none() {
            continue;
        }
        for dep in deps {
            let dep = &items[*dep];
            let key = match dep.delivery_status.as_str() {
                "done" => None,
                "cancelled" => Some("dep_cancelled"),
                _ if dep.sprint.is_none() => Some("dep_unscheduled"),
                _ if dep.sprint > item.sprint => Some("dep_later"),
                _ if dep.sprint == item.sprint => Some("dep_same"),
                _ => None,
            };
            if let Some(key) = key {
                messages.push(t.render(key, &[("id", item.id.clone()), ("dep", dep.id.clone())])?);
            }
        }
    }
    Ok(Forecast {
        as_of,
        periods: periods.into_values().collect(),
        caps: caps.into_values().collect(),
        items,
        messages,
    })
}

fn render(plan: &Forecast, t: &Catalog) -> Result<String> {
    let assigned = sum(plan
        .items
        .iter()
        .filter(|i| i.sprint.is_some())
        .filter_map(|i| i.remaining))?;
    let unassigned = sum(plan
        .items
        .iter()
        .filter(|i| i.sprint.is_none())
        .filter_map(|i| i.remaining))?;
    let unknown = plan
        .items
        .iter()
        .filter(|i| i.sprint.is_none() && i.remaining.is_none())
        .count();
    let mut body = format!(
        "# {}\n\n{}\n\n",
        t.text("plan_title")?,
        t.render(
            "plan_intro",
            &[
                ("date", plan.as_of.to_string()),
                ("assigned:.2f", format!("{assigned:.2}")),
                ("unassigned:.2f", format!("{unassigned:.2}")),
                ("unknown", unknown.to_string())
            ]
        )?
    );
    if !plan
        .items
        .iter()
        .any(|i| i.sprint.is_some() && !matches!(i.delivery_status.as_str(), "done" | "cancelled"))
    {
        body.push_str(&format!("> {}\n\n", t.text("empty_schedule")?));
    }
    body.push_str(&format!(
        "{}\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n",
        t.text("plan_basis")?
    ));
    for p in &plan.periods {
        body.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            p.sprint, p.start, p.end, p.goal
        ));
    }
    body.push_str(&format!(
        "\n{}\n|---|---|---:|---:|---:|---|\n",
        t.text("capacity_header")?
    ));
    for c in &plan.caps {
        let gap = c.capacity - c.effort;
        body.push_str(&format!(
            "| {} | {} | {:.2} | {:.2} | {:.2} | {} |\n",
            c.sprint,
            c.role,
            c.effort,
            c.capacity,
            gap,
            t.text(if gap < -1e-9 { "overload" } else { "within" })?
        ));
    }
    body.push_str(&format!(
        "\n## {}\n\n{}\n|---|---|---:|---:|---|\n",
        t.text("items_title")?,
        t.text("items_header")?
    ));
    for i in &plan.items {
        body.push_str(&format!(
            "| [[{}]] | {} | {} | {} | {} |\n",
            i.path.trim_end_matches(".md"),
            i.delivery_status,
            i.original
                .map(|n| format!("{n:.2}"))
                .unwrap_or(t.text("not_counted")?),
            i.remaining
                .map(|n| format!("{n:.2}"))
                .unwrap_or(t.text("unestimated")?),
            i.sprint
                .map(|s| s.to_string())
                .unwrap_or(t.text("unscheduled")?)
        ));
    }
    let messages = if plan.messages.is_empty() {
        t.text("no_conflict")?
    } else {
        plan.messages
            .iter()
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    body.push_str(&format!(
        "\n## {}\n\n{}\n\n## {}\n\n{}",
        t.text("dependencies")?,
        messages,
        t.text("decisions")?,
        t.text("plan_decisions")?
    ));
    Ok(body)
}

/// Generate a report and the same tool envelope as the PM helper. No Python process is started.
pub fn run(workspace: &Workspace, args: &[String]) -> Result<Value> {
    if args == ["--help"] {
        return Ok(
            json!({"exit_code":0,"successful":true,"stdout":"plan: Recalculate effort and capacity from Markdown tables. No arguments.\n","stderr":"","reports":[],"engine":"rust"}),
        );
    }
    ensure!(args.is_empty(), "plan accepts no arguments (use --help)");
    // Invalid or unavailable translations leave the previous report intact.
    let catalog = Catalog::load(workspace)?;
    let title = catalog.text("plan_title")?;
    let result =
        calculate_with(workspace, &catalog).and_then(|plan| Ok((render(&plan, &catalog)?, plan)));
    let (body, status, error, forecast) = match result {
        Ok((body, plan)) => (body, "forecast", String::new(), Some(plan)),
        Err(e) => {
            let error = format!("{e:#}");
            (
                format!(
                    "# {title}\n\n{}",
                    catalog.render("plan_failed", &[("error", error.clone())])?
                ),
                "blocked",
                error,
                None,
            )
        }
    };
    let content = note(
        json!({"id":"REPORT-PLAN","type":"report","title":title,"language":catalog.language,"workflow_status":status,"generated_at":Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)}),
        &body,
    )?;
    atomic_write(&workspace.path(REPORT)?, content.as_bytes())?;
    Ok(
        json!({"exit_code":if error.is_empty() {0} else {2}, "successful":error.is_empty(), "stdout":format!("{}\n",workspace.path(REPORT)?.display()), "stderr":error, "reports":[{"path":REPORT,"sha256":hash(content.as_bytes()),"content":content}], "forecast":forecast,"engine":"rust"}),
    )
}
