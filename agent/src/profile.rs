//! Local OKF authoring checks. These check structure, not truth or human approval.
use crate::{
    records::truth,
    workspace::{MAX_FILE, Workspace},
};
use anyhow::{Context, Result, ensure};
use chrono::NaiveDate;
use regex::Regex;
use serde::Serialize;
use serde_yaml::Value;
use std::{fs, path::Path};

const PRIVATE: &[&str] = &[
    ".git",
    ".obsidian",
    "__pycache__",
    "exports",
    "target",
    ".pm-agent",
    ".venv",
];
const RAW: &[&str] = &[
    "01_Sources/Assets/",
    "01_Sources/Extracted/",
    "03_Systems/Snapshots/",
    "00_Governance/Baselines/",
];

pub(crate) fn inventory(workspace: &Workspace) -> Result<Vec<String>> {
    let mut paths = vec![];
    for entry in walkdir::WalkDir::new(&workspace.root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0 || !PRIVATE.contains(&e.file_name().to_string_lossy().as_ref())
        })
    {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name == ".env" || name.starts_with(".env.") && name != ".env.example" {
            continue;
        }
        paths.push(
            entry
                .path()
                .strip_prefix(&workspace.root)?
                .to_string_lossy()
                .into_owned(),
        );
    }
    paths.sort();
    Ok(paths)
}
pub(crate) fn read_text(workspace: &Workspace, path: &str) -> Result<String> {
    let file = workspace.path(path)?;
    let meta = fs::metadata(&file)?;
    ensure!(
        meta.is_file() && meta.len() <= MAX_FILE,
        "Expected a regular UTF-8 document of at most 2 MiB"
    );
    fs::read_to_string(file).context("Cannot read UTF-8 document")
}
fn yaml(text: &str) -> Result<(Option<Value>, String)> {
    let normalized = text.replace("\r\n", "\n");
    let header = Regex::new(r"\A---\n(.*?)\n---(?:\n|\z)").unwrap();
    // Dot-all permits multi-line YAML; an absent complete header is not concept metadata.
    let header = regex::RegexBuilder::new(header.as_str())
        .dot_matches_new_line(true)
        .build()?;
    let Some(c) = header.captures(&normalized) else {
        return Ok((None, normalized));
    };
    let meta: Value =
        serde_yaml::from_str(&c[1]).context("Invalid or duplicate YAML frontmatter")?;
    ensure!(meta.is_mapping(), "Frontmatter must be a mapping");
    Ok((Some(meta), normalized[c.get(0).unwrap().end()..].into()))
}
#[derive(Debug, Serialize)]
pub struct Profile {
    pub okf_version: &'static str,
    pub documents_checked: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub conformant: bool,
    pub scope: &'static str,
}
pub fn inspect_authored(workspace: &Workspace) -> Result<Profile> {
    let paths = inventory(workspace)?;
    inspect_paths(workspace, &paths)
}
pub(crate) fn inspect_paths(workspace: &Workspace, paths: &[String]) -> Result<Profile> {
    let heading = Regex::new(r"(?m)^#{1,6} .+")?;
    let links = Regex::new(r"(?m)^[-*] \[[^\]]+\]\([^)]+\)")?;
    let dates = Regex::new(r"(?m)^## (.+)$")?;
    let date_heading = Regex::new(r"(?m)^## \d{4}-\d{2}-\d{2}$")?;
    let mut output = Profile {
        okf_version: "0.2",
        documents_checked: 0,
        errors: vec![],
        warnings: vec![],
        conformant: false,
        scope: "project profile",
    };
    for path in paths.iter().filter(|p| {
        p.ends_with(".md")
            && !RAW.iter().any(|r| p.starts_with(r))
            && !Path::new(p)
                .components()
                .any(|c| c.as_os_str() == ".agents")
    }) {
        output.documents_checked += 1;
        let checked = (|| -> Result<()> {
            let (meta, body) = yaml(&read_text(workspace, path)?)?;
            match Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
            {
                "index.md" => {
                    if let Some(meta) = &meta {
                        ensure!(
                            path == "index.md"
                                && meta
                                    .as_mapping()
                                    .unwrap()
                                    .keys()
                                    .all(|k| k.as_str() == Some("okf_version")),
                            "Only root index.md may have frontmatter, containing okf_version"
                        );
                    }
                    ensure!(heading.is_match(&body), "Index needs a section heading");
                    ensure!(links.is_match(&body), "Index needs Markdown list links");
                }
                "log.md" => {
                    ensure!(meta.is_none(), "log.md must not carry concept frontmatter");
                    for h in dates.captures_iter(&body) {
                        NaiveDate::parse_from_str(&h[1], "%Y-%m-%d").context("Invalid log date")?;
                    }
                    ensure!(
                        date_heading.is_match(&body),
                        "Log needs date-grouped entries"
                    );
                }
                _ => {
                    let meta =
                        meta.context("Concept needs YAML frontmatter with a nonempty string type")?;
                    ensure!(
                        meta["type"].as_str().is_some_and(|s| !s.trim().is_empty()),
                        "Concept needs YAML frontmatter with a nonempty string type"
                    );
                    ensure!(
                        meta["title"].as_str().is_some(),
                        "This project profile requires a title"
                    );
                    let map = meta.as_mapping().unwrap();
                    if map.contains_key(Value::String("status".into()))
                        && ![Some("draft"), Some("stable"), Some("deprecated")]
                            .contains(&meta["status"].as_str())
                    {
                        output.warnings.push(format!("{path}: use workflow_status for project states; status is the OKF lifecycle"));
                    }
                    if map.contains_key(Value::String("sources".into()))
                        && !meta["sources"].as_sequence().is_some_and(|values| {
                            values.iter().all(|v| {
                                v.is_mapping()
                                    && serde_json::to_value(&v["resource"]).is_ok_and(|v| truth(&v))
                            })
                        })
                    {
                        output.warnings.push(format!(
                            "{path}: optional sources entries should contain resource"
                        ));
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = checked {
            output.errors.push(format!("{path}: {error:#}"));
        }
    }
    output.conformant = output.documents_checked > 0 && output.errors.is_empty();
    Ok(output)
}
