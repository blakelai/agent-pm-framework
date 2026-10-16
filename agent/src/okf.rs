//! Portable OKF bundles; raw attachments stay byte-identical, active records stay untouched.
use crate::{
    profile,
    records::{self, Records},
    sources,
    workspace::{Workspace, atomic_write, note, parse_note},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

fn normalized(path: &Path) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                ensure!(result.pop(), "Path escapes filesystem root");
            }
            Component::CurDir => {}
            _ => result.push(part.as_os_str()),
        }
    }
    Ok(result)
}
fn destination(workspace: &Workspace, raw: &str) -> Result<PathBuf> {
    let path = sources::expanded(raw)?;
    let path = normalized(&if path.is_absolute() {
        path
    } else {
        workspace.root.join(path)
    })?;
    let mut existing = path.clone();
    let mut tail = vec![];
    while !existing.exists() {
        tail.push(
            existing
                .file_name()
                .context("Invalid export destination")?
                .to_owned(),
        );
        ensure!(existing.pop(), "Invalid export destination");
    }
    let mut output = existing.canonicalize()?;
    for component in tail.iter().rev() {
        output.push(component);
    }
    ensure!(
        !output.starts_with(&workspace.root) && !workspace.root.starts_with(&output),
        "Export to a separate directory outside the working Vault"
    );
    ensure!(
        !output.exists(),
        "Export destination exists; use a new directory to preserve history"
    );
    Ok(output)
}
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/._-~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
fn decode(text: &str) -> String {
    let mut out = vec![];
    let mut i = 0;
    let b = text.as_bytes();
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            let digit = |c: u8| (c as char).to_digit(16).unwrap() as u8;
            out.push(digit(b[i + 1]) * 16 + digit(b[i + 2]));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
struct Links<'a> {
    workspace: &'a Workspace,
    records: Records,
    mapping: &'a BTreeMap<String, String>,
}
impl Links<'_> {
    fn href(&self, original: &str, target: &str) -> String {
        let target = decode(target);
        if target.starts_with('#') || reqwest::Url::parse(&target).is_ok() {
            return target;
        }
        let (file, anchor) = target
            .split_once('#')
            .map_or((target.as_str(), None), |(p, a)| (p, Some(a)));
        let source = if file.starts_with('/') {
            self.workspace.root.join(file.trim_start_matches('/'))
        } else {
            self.workspace
                .root
                .join(original)
                .parent()
                .unwrap()
                .join(file)
        };
        let Ok(source) = normalized(&source) else {
            return target;
        };
        let Ok(relative) = source.strip_prefix(&self.workspace.root) else {
            return target;
        };
        let Some(relative) = relative.to_str() else {
            return target;
        };
        let Some(destination) = self.mapping.get(relative) else {
            return target;
        };
        let origin = Path::new(&self.mapping[original]).parent().unwrap();
        let from = origin.components().collect::<Vec<_>>();
        let to = Path::new(destination).components().collect::<Vec<_>>();
        let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
        let mut path = PathBuf::new();
        for _ in common..from.len() {
            path.push("..");
        }
        for component in &to[common..] {
            path.push(component);
        }
        format!(
            "{}{}",
            encode(&path.to_string_lossy()),
            anchor
                .map(|a| format!("#{}", a.trim_start_matches('^')))
                .unwrap_or_default()
        )
    }
    fn prose(&self, original: &str, text: &str) -> Result<String> {
        let md = regex::Regex::new(r"(!?)\[([^\]]+)\]\(([^)]+)\)")?;
        let text = md.replace_all(text, |c: &regex::Captures<'_>| {
            if &c[1] == "!" {
                c[0].to_owned()
            } else {
                format!("[{}]({})", &c[2], self.href(original, &c[3]))
            }
        });
        let wiki = regex::Regex::new(r"!?\[\[([^\]]+)\]\]")?;
        let text = wiki.replace_all(&text, |c: &regex::Captures<'_>| {
            let raw = &c[1];
            let (target, alias) = raw
                .split_once('|')
                .map_or((raw, None), |(t, a)| (t, Some(a)));
            let label = alias.unwrap_or_else(|| {
                target
                    .split('#')
                    .next()
                    .unwrap()
                    .rsplit('/')
                    .next()
                    .unwrap()
            });
            match self.records.resolve_path(raw) {
                Ok(path) => {
                    let anchor = target
                        .split_once('#')
                        .map(|(_, a)| format!("#{a}"))
                        .unwrap_or_default();
                    format!(
                        "[{label}]({})",
                        self.href(original, &format!("/{path}{anchor}"))
                    )
                }
                Err(_) => c[0].to_owned(),
            }
        });
        Ok(regex::Regex::new(r#"(?m)\^([A-Za-z0-9_-]+)\s*$"#)?
            .replace_all(&text, "<a id=\"$1\"></a>")
            .into_owned())
    }
    fn body(&self, original: &str, body: &str) -> Result<String> {
        // Preserve even an unfinished code fence verbatim; it is an example, not a graph edge.
        let mut output = String::new();
        let mut prose = String::new();
        let mut fence: Option<(char, usize)> = None;
        for line in body.split_inclusive('\n') {
            let trimmed = line.trim_start();
            let marker = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
            let count = marker
                .map(|c| trimmed.chars().take_while(|x| *x == c).count())
                .unwrap_or(0);
            if let Some((ch, len)) = fence {
                output.push_str(line);
                if marker == Some(ch) && count >= len && trimmed[count..].trim().is_empty() {
                    fence = None;
                }
            } else if count >= 3 {
                output.push_str(&self.prose(original, &prose)?);
                prose.clear();
                output.push_str(line);
                fence = Some((marker.unwrap(), count));
            } else {
                prose.push_str(line);
            }
        }
        output.push_str(&self.prose(original, &prose)?);
        Ok(output)
    }
}
pub fn export(workspace: &Workspace, raw_output: &str) -> Result<Value> {
    let output = destination(workspace, raw_output)?;
    let inventory = profile::inventory(workspace)?;
    let check = profile::inspect_paths(workspace, &inventory)?;
    ensure!(
        check.errors.is_empty(),
        "Fix authored document profile before export: {:?}",
        check.errors
    );
    // Reject links before copying any attachment, including links in otherwise omitted directories.
    for path in &inventory {
        workspace.path(path)?;
    }
    let authored = inventory
        .iter()
        .filter(|p| profile::authored(p))
        .cloned()
        .collect::<Vec<_>>();
    let skills = inventory
        .iter()
        .filter(|p| {
            p.starts_with(".agents/skills/")
                && p.ends_with("/SKILL.md")
                && p.split('/').count() == 4
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut mapping: BTreeMap<String, String> =
        authored.iter().map(|p| (p.clone(), p.clone())).collect();
    for path in &skills {
        let name = Path::new(path)
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy();
        let dest = format!("90_Agent/Skills/{name}.md");
        ensure!(
            !mapping.values().any(|p| p == &dest),
            "Skill export conflicts with authored document: {dest}"
        );
        mapping.insert(path.clone(), dest);
    }
    let concepts = mapping.keys().cloned().collect::<BTreeSet<_>>();
    for path in &inventory {
        if concepts.contains(path) || path.starts_with(".agents/") || path.ends_with(".pyc") {
            continue;
        }
        let dest = if path.ends_with(".md") {
            format!("{path}.txt")
        } else {
            path.clone()
        };
        ensure!(
            !mapping.values().any(|p| p == &dest),
            "Export attachment path collision: {dest}"
        );
        mapping.insert(path.clone(), dest);
    }
    fs::create_dir_all(output.parent().context("Missing export parent")?)?;
    let stage = tempfile::tempdir_in(output.parent().unwrap())?;
    let bundle = Workspace {
        root: stage.path().to_owned(),
    };
    for (source, dest) in &mapping {
        if concepts.contains(source) {
            continue;
        }
        let dest = bundle.path(dest)?;
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(workspace.path(source)?, dest)?;
    }
    let links = Links {
        workspace,
        records: Records::for_validation(workspace, vec![], inventory.clone()),
        mapping: &mapping,
    };
    let mut index = String::from("---\nokf_version: \"0.2\"\n---\n\n# Project knowledge\n\n");
    for original in authored.iter().chain(&skills) {
        let source = profile::read_text(workspace, original)?;
        let dest = bundle.path(&mapping[original])?;
        if matches!(
            Path::new(original).file_name().and_then(|n| n.to_str()),
            Some("index.md" | "log.md")
        ) {
            atomic_write(&dest, source.as_bytes())?;
            continue;
        }
        let (mut meta, mut body) = if skills.contains(original) {
            parse_note(&source)?
        } else {
            records::parse_project_note(&source)?
        };
        let label = meta["title"]
            .as_str()
            .or(meta["name"].as_str())
            .unwrap_or(original)
            .to_owned();
        let description = meta["description"]
            .as_str()
            .or(meta["type"].as_str())
            .unwrap_or("Agent Skill")
            .to_owned();
        if skills.contains(original) {
            let name = records::string(&meta, "name")?;
            let description = records::string(&meta, "description")?;
            meta = json!({"type":"Agent Skill","title":name,"description":description,"language":"en","skill_name":name});
            body = format!(
                "> Knowledge representation of a Skill. Use the working Vault for native Skill invocation.\n\n{body}"
            );
        }
        body = links.body(original, &body)?;
        let mut refs = BTreeSet::new();
        for raw in records::wiki_refs(&serde_json::to_string(&meta)?) {
            if let Ok(target) = links.records.resolve_path(raw) {
                refs.insert(format!(
                    "[{}]({})",
                    Path::new(&target).file_stem().unwrap().to_string_lossy(),
                    links.href(original, &format!("/{target}"))
                ));
            }
        }
        if !refs.is_empty() {
            body.push_str(&format!(
                "\n\n## Related records\n\n{}\n",
                refs.iter()
                    .map(|r| format!("- {r}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        atomic_write(&dest, note(meta, &body)?.as_bytes())?;
        index.push_str(&format!(
            "- [{label}]({}) - {description}\n",
            encode(&mapping[original])
        ));
    }
    atomic_write(&bundle.path("index.md")?, index.as_bytes())?;
    atomic_write(&bundle.path("export-map.json")?,serde_json::to_vec_pretty(&json!({"created_at":records::timestamp(),"source_spec":"https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md","paths":mapping,"note":"Raw .md attachments are exact bytes with a .txt suffix; native Skill adapters are exported as concepts."}))?.as_slice())?;
    let result = profile::check_bundle(&bundle.root)?;
    ensure!(
        result.conformant,
        "Export failed structural validation: {:?}",
        result.errors
    );
    atomic_write(
        &bundle.path("validation.json")?,
        serde_json::to_vec_pretty(&result)?.as_slice(),
    )?;
    // Reserve a fresh destination; never replace an earlier published bundle.
    fs::create_dir(&output)
        .context("Export destination was created during export; use a new directory")?;
    if let Err(error) = fs::rename(stage.path(), &output) {
        let _ = fs::remove_dir(&output);
        return Err(error.into());
    }
    Ok(serde_json::to_value(result)?)
}
