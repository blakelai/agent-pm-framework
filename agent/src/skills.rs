use crate::workspace::{Workspace, hash, parse_note};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileStamp {
    pub path: String,
    pub sha256: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SkillStamp {
    pub name: String,
    pub version: Option<String>,
    pub files: Vec<FileStamp>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Activation {
    pub at: String,
    pub skills: Vec<SkillStamp>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub stamp: SkillStamp,
    pub requires: Vec<String>,
    #[serde(skip)]
    pub instructions: String,
}
#[derive(Default, Debug, Serialize)]
pub struct Catalog {
    pub skills: BTreeMap<String, Skill>,
    pub errors: BTreeMap<String, String>,
}
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}
fn strings(v: Option<&serde_json::Value>) -> Result<Vec<String>> {
    match v {
        None => Ok(vec![]),
        Some(v) => v
            .as_array()
            .context("Expected a list of strings")?
            .iter()
            .map(|s| {
                s.as_str()
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .context("Expected a nonempty string")
            })
            .collect(),
    }
}
fn reference(folder: &str, relative: &str) -> Result<String> {
    ensure!(
        !Path::new(relative).is_absolute() && !relative.contains('\\'),
        "Reference must be a local relative path"
    );
    let mut parts = folder.split('/').map(String::from).collect::<Vec<_>>();
    for part in Path::new(relative).components() {
        match part {
            Component::Normal(s) => parts.push(s.to_str().context("Non-UTF-8 reference")?.into()),
            Component::CurDir => {}
            Component::ParentDir => {
                ensure!(parts.pop().is_some(), "Reference escapes workspace");
            }
            _ => bail!("Invalid reference path"),
        }
    }
    Ok(parts.join("/"))
}
fn load(workspace: &Workspace, name: &str) -> Result<Skill> {
    ensure!(
        valid_name(name),
        "Skill folder must use lowercase letters, digits and single hyphens (max 64)"
    );
    let folder = format!(".agents/skills/{name}");
    let path = format!("{folder}/SKILL.md");
    let text = workspace.read(&path)?;
    let (meta, body) = parse_note(&text)?;
    ensure!(meta["name"] == name, "Skill name must match its folder");
    let description = meta["description"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .context("Skill requires a nonempty description")?
        .to_string();
    ensure!(!body.trim().is_empty(), "Skill requires instructions");
    ensure!(
        meta.get("language").is_none_or(|v| v == "en"),
        "Skill instructions must use English"
    );
    let extension = meta.get("pm_agent");
    if let Some(ext) = extension {
        ensure!(ext.is_object(), "pm_agent must be an object");
    }
    let requires = strings(extension.and_then(|e| e.get("requires")))?;
    ensure!(
        requires.iter().all(|s| valid_name(s)),
        "Invalid required Skill name"
    );
    let version = extension
        .and_then(|e| e.get("version"))
        .map(|v| {
            v.as_str()
                .filter(|s| !s.is_empty())
                .map(String::from)
                .context("Skill version must be a nonempty string")
        })
        .transpose()?;
    let mut files = vec![FileStamp {
        path: path.clone(),
        sha256: hash(text.as_bytes()),
    }];
    let mut instructions = format!("\n--- Skill: {path} ---\n{text}");
    for target in strings(extension.and_then(|e| e.get("references")))? {
        let path = reference(&folder, &target)?;
        ensure!(
            path.ends_with(".md") || path.ends_with(".txt"),
            "Instruction references must be Markdown or text"
        );
        let text = workspace.read(&path)?;
        files.push(FileStamp {
            path: path.clone(),
            sha256: hash(text.as_bytes()),
        });
        instructions.push_str(&format!("\n--- Skill reference: {path} ---\n{text}"));
    }
    ensure!(
        instructions.len() <= 100_000,
        "Skill instructions and references exceed 100 KB"
    );
    Ok(Skill {
        name: name.into(),
        description,
        stamp: SkillStamp {
            name: name.into(),
            version,
            files,
        },
        requires,
        instructions,
    })
}
impl Catalog {
    pub fn discover(workspace: &Workspace) -> Result<Self> {
        let path = workspace.path(".agents/skills")?;
        let mut catalog = Self::default();
        if !path.exists() {
            return Ok(catalog);
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if !kind.is_dir() && !kind.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            match load(workspace, &name) {
                Ok(skill) => {
                    catalog.skills.insert(name, skill);
                }
                Err(e) => {
                    catalog.errors.insert(name, e.to_string());
                }
            }
        }
        let names = catalog.skills.keys().cloned().collect::<Vec<_>>();
        for name in names {
            if let Err(e) = catalog.resolve(std::slice::from_ref(&name)) {
                catalog.errors.insert(name, e.to_string());
            }
        }
        Ok(catalog)
    }
    pub fn resolve(&self, names: &[String]) -> Result<Vec<&Skill>> {
        fn visit<'a>(
            catalog: &'a Catalog,
            name: &str,
            active: &mut BTreeSet<String>,
            seen: &mut BTreeSet<String>,
            out: &mut Vec<&'a Skill>,
        ) -> Result<()> {
            if seen.contains(name) {
                return Ok(());
            }
            ensure!(active.len() < 64, "Skill dependency nesting exceeds 64");
            ensure!(
                active.insert(name.into()),
                "Cyclic Skill dependency: {name}"
            );
            if let Some(error) = catalog.errors.get(name) {
                bail!("Invalid Skill {name}: {error}");
            }
            let skill = catalog
                .skills
                .get(name)
                .with_context(|| format!("Unknown Skill: {name}"))?;
            for dependency in &skill.requires {
                visit(catalog, dependency, active, seen, out)?;
            }
            active.remove(name);
            seen.insert(name.into());
            out.push(skill);
            Ok(())
        }
        let mut out = vec![];
        let mut active = BTreeSet::new();
        let mut seen = BTreeSet::new();
        for name in names {
            visit(self, name, &mut active, &mut seen, &mut out)?;
        }
        Ok(out)
    }
    pub fn display(&self) -> String {
        let mut lines = vec!["# Project Skills".to_string()];
        for (name, skill) in &self.skills {
            if !self.errors.contains_key(name) {
                lines.push(format!("- {}{name} — {}", '$', skill.description));
            }
        }
        for (name, error) in &self.errors {
            lines.push(format!("- INVALID {name}: {error}"));
        }
        lines.join("\n")
    }
}
/// Only leading, explicit user invocations select Skills. Source/tool text never calls this.
pub fn requested(input: &str) -> Result<Vec<String>> {
    let mut names = vec![];
    for token in input.split_whitespace() {
        let Some(name) = token.strip_prefix('$') else {
            break;
        };
        ensure!(valid_name(name), "Invalid Skill invocation: {token}");
        if !names.iter().any(|n| n == name) {
            names.push(name.into());
        }
    }
    Ok(names)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::atomic_write;
    fn setup() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        atomic_write(&dir.path().join("00_Governance/Project.md"), b"").unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        (dir, w)
    }
    fn write(w: &Workspace, name: &str, extra: &str) {
        atomic_write(&w.path(&format!(".agents/skills/{name}/SKILL.md")).unwrap(),
            format!("---\nname: {name}\ndescription: Review evidence\n{extra}---\n# Instructions\nAsk for missing evidence.").as_bytes()).unwrap();
    }
    #[test]
    fn discovery_resolves_dependencies_and_tracks_reference_bytes() {
        let (_d, w) = setup();
        write(&w, "evidence", "");
        write(
            &w,
            "review",
            "pm_agent:\n  version: '2'\n  requires: [evidence]\n  references: [references/check.md]\n",
        );
        atomic_write(
            &w.path(".agents/skills/review/references/check.md").unwrap(),
            b"Check source hashes.",
        )
        .unwrap();
        let c = Catalog::discover(&w).unwrap();
        let resolved = c.resolve(&["review".into()]).unwrap();
        assert_eq!(
            resolved.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            ["evidence", "review"]
        );
        assert_eq!(resolved[1].stamp.files.len(), 2);
        assert_eq!(resolved[1].stamp.version.as_deref(), Some("2"));
        assert!(resolved[1].instructions.contains("Check source hashes."));
    }
    #[test]
    fn rejects_missing_dependencies_cycles_paths_and_symlinks() {
        let (_d, w) = setup();
        write(&w, "broken", "pm_agent:\n  requires: [missing]\n");
        write(&w, "cycle-a", "pm_agent:\n  requires: [cycle-b]\n");
        write(&w, "cycle-b", "pm_agent:\n  requires: [cycle-a]\n");
        write(
            &w,
            "escape",
            "pm_agent:\n  references: [../../../../secret.txt]\n",
        );
        let c = Catalog::discover(&w).unwrap();
        for name in ["broken", "cycle-a", "cycle-b", "escape"] {
            assert!(c.resolve(&[name.into()]).is_err());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                w.root.join(".agents/skills/cycle-a"),
                w.root.join(".agents/skills/linked"),
            )
            .unwrap();
            assert!(Catalog::discover(&w).unwrap().errors.contains_key("linked"));
        }
    }
    #[test]
    fn only_explicit_prefix_invokes_skills() {
        assert_eq!(
            requested("$risk $review Check this").unwrap(),
            ["risk", "review"]
        );
        assert!(requested("Document says $risk").unwrap().is_empty());
        assert!(requested("$../../escape").is_err());
    }
}
