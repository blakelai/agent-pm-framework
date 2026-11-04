//! Read-only planning snapshots and explicit single-item scheduling proposals.
use crate::{
    editing::{Edit, Target},
    planning::{self, Forecast, Period},
    records,
    workspace::{Document, Workspace},
};
use anyhow::{Context, Result, ensure};
use std::collections::BTreeSet;

pub const PEOPLE: &str = "06_Delivery/People.md";
pub const ITERATIONS: &str = "06_Delivery/Iterations.md";
pub const PLANNING: &str = "06_Delivery/Planning.md";

pub struct Snapshot {
    pub loaded_at: String,
    pub periods: Vec<Period>,
    pub items: Vec<Document>,
    pub people: Vec<Vec<String>>,
    pub forecast: Option<Forecast>,
    pub problems: Vec<String>,
}

fn periods(w: &Workspace) -> Result<Vec<Period>> {
    let (_, body) = records::parse_project_note(&w.read(ITERATIONS)?)?;
    let mut ids = BTreeSet::new();
    let mut result = vec![];
    for row in planning::table(&body, &["Sprint", "Start", "End", "Goal"])? {
        let sprint = row[0].parse().context("Sprint must be an integer")?;
        let start = planning::date(&row[1])?;
        let end = planning::date(&row[2])?;
        ensure!(
            ids.insert(sprint) && start <= end,
            "Duplicate or inverted iteration"
        );
        result.push(Period {
            sprint,
            start,
            end,
            goal: row[3].clone(),
        });
    }
    result.sort_by_key(|p| p.start);
    ensure!(
        result.windows(2).all(|p| p[0].end < p[1].start),
        "Iterations overlap"
    );
    Ok(result)
}

impl Snapshot {
    /// Failure clears calculated values; independent supplied records remain visible.
    pub fn load(w: &Workspace) -> Self {
        let mut problems = vec![];
        let periods = periods(w).unwrap_or_else(|e| {
            problems.push(format!("Iterations unavailable: {e:#}"));
            vec![]
        });
        let people = (|| -> Result<_> {
            let (_, body) = records::parse_project_note(&w.read(PEOPLE)?)?;
            planning::table(
                &body,
                &[
                    "Person", "Role", "Sprint", "Project", "FTE", "Workdays", "Absence", "Focus",
                    "Reserve",
                ],
            )
        })()
        .unwrap_or_else(|e| {
            problems.push(format!("Allocation rows unavailable: {e:#}"));
            vec![]
        });
        let mut items = w.documents().unwrap_or_else(|e| {
            problems.push(format!("Project documents unavailable: {e:#}"));
            vec![]
        });
        items.retain(|d| {
            d.kind == "pbi"
                && records::MANAGED
                    .iter()
                    .any(|p| d.path.starts_with(&format!("{p}/")))
        });
        items.sort_by(|a, b| (&a.id, &a.path).cmp(&(&b.id, &b.path)));
        let forecast = match planning::calculate(w) {
            Ok(f) => Some(f),
            Err(e) => {
                problems.push(format!("Forecast unavailable: {e:#}"));
                None
            }
        };
        Self {
            loaded_at: records::timestamp(),
            periods,
            items,
            people,
            forecast,
            problems,
        }
    }
    pub fn signals(&self) -> Vec<String> {
        let mut result = self.problems.clone();
        if let Some(f) = &self.forecast {
            result.extend(f.messages.clone());
            for c in &f.caps {
                if c.effort > c.capacity {
                    result.push(format!(
                        "Sprint {} / {}: demand {:.2} exceeds capacity {:.2} by {:.2} person-days",
                        c.sprint,
                        c.role,
                        c.effort,
                        c.capacity,
                        c.effort - c.capacity
                    ));
                }
            }
            for i in &f.items {
                if i.remaining.is_none() {
                    result.push(format!(
                        "{}: effort unavailable; supply estimates before scheduling",
                        i.id
                    ));
                }
            }
        }
        for d in &self.items {
            if d.id.is_none() {
                result.push(format!("{}: missing stable PBI ID", d.path));
            }
        }
        result
    }
}

/// The user's selected target is applied only in memory. Existing editor checks
/// govern the later review/save; neither forecasts nor records are written here.
pub fn propose(
    w: &Workspace,
    path: &str,
    expected_hash: &str,
    sprint: Option<i64>,
) -> Result<Edit> {
    let mut edit = Edit::load(w, path)?;
    ensure!(
        edit.source_hash().as_deref() == Some(expected_hash),
        "PBI changed since this view was loaded; refresh before scheduling"
    );
    let d = records::read(w, path)?;
    ensure!(d.kind == "pbi", "Only PBIs can be scheduled");
    ensure!(
        !matches!(
            d.metadata["delivery_status"].as_str(),
            Some("done" | "cancelled")
        ),
        "Completed or cancelled work is historical; create or refine active work instead"
    );
    if let Some(sprint) = sprint {
        ensure!(
            periods(w)?.iter().any(|p| p.sprint == sprint),
            "Select a recorded iteration"
        );
    }
    edit.apply(
        &Target::Property("sprint".into()),
        &sprint.map(|s| s.to_string()).unwrap_or_default(),
    )?;
    Ok(edit)
}

/// Explicit before/after facts, without interpreting person-days as calendar dates.
pub(crate) fn impact(before: Result<Forecast>, after: &Forecast) -> Vec<String> {
    let mut lines = vec![format!("Proposed forecast as_of: {}", after.as_of)];
    let before = match before {
        Ok(f) => Some(f),
        Err(e) => {
            lines.push(format!("Previous forecast unavailable: {e:#}"));
            None
        }
    };
    if let Some(b) = &before
        && b.as_of != after.as_of
    {
        lines.push(format!(
            "Time basis changed: {} → {}; totals use their respective supplied availability",
            b.as_of, after.as_of
        ));
    }
    let keys = after
        .caps
        .iter()
        .map(|c| (c.sprint, c.role.as_str()))
        .chain(
            before
                .iter()
                .flat_map(|b| b.caps.iter().map(|c| (c.sprint, c.role.as_str()))),
        )
        .collect::<BTreeSet<_>>();
    for (sprint, role) in keys {
        let a = after
            .caps
            .iter()
            .find(|c| c.sprint == sprint && c.role == role);
        let b = before
            .as_ref()
            .and_then(|b| b.caps.iter().find(|c| c.sprint == sprint && c.role == role));
        if before.is_some()
            && a.map(|c| (c.capacity, c.effort)) == b.map(|c| (c.capacity, c.effort))
        {
            continue;
        }
        let show = |c: Option<&planning::Capacity>| {
            c.map(|c| {
                format!(
                    "capacity {:.2}, demand {:.2}, balance {:.2}",
                    c.capacity,
                    c.effort,
                    c.capacity - c.effort
                )
            })
            .unwrap_or_else(|| "unavailable / no allocation row".into())
        };
        lines.push(format!(
            "Sprint {sprint} / {role} (person-days): {} → {}",
            show(b),
            show(a)
        ));
    }
    for c in &after.caps {
        if c.effort > c.capacity {
            lines.push(format!(
                "Over capacity: Sprint {} / {} by {:.2} person-days",
                c.sprint,
                c.role,
                c.effort - c.capacity
            ));
        }
    }
    lines.push(
        "Iteration assignment is a proposal, not a delivery commitment or an optimized schedule."
            .into(),
    );
    lines
}
