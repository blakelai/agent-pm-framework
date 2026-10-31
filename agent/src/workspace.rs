use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};
use walkdir::WalkDir;

pub const MAX_FILE: u64 = 2 * 1024 * 1024;
pub(crate) const DRAFT_ROOTS: &[&str] = &[
    "02_Domain/",
    "03_Systems/Analysis/",
    "04_Requirements/",
    "05_Backlog/",
    "07_Decisions/",
    "08_Verification/Cases/",
    "09_Operations/",
];
// Explicit knowledge documents outside the project folders. Never grant access
// to application code, build outputs, configuration or arbitrary sibling files.
pub(crate) const GUIDES: &[&str] = &[
    "agent/README.md",
    "agent/Implementation.md",
    "agent/Interaction.md",
    "agent/Skills.md",
    "agent/Editing.md",
    "agent/Planning-View.md",
    "agent/Clarification.md",
    "agent/Inspection.md",
    "agent/Materials.md",
    "agent/Material-Reviews.md",
    "agent/Delivery-Workflows.md",
    "agent/Automatic-Refresh.md",
    "agent/Provider-Probes.md",
    "agent/Session-Limits.md",
    "agent/Backup-and-Restore.md",
    "agent/Storage-Migrations.md",
    "agent/Installation.md",
    "agent/Evaluation.md",
    "agent/Grounding.md",
    "tests/README.md",
];
const FOLDERS: &[&str] = &[
    "00_Governance",
    "01_Sources",
    "02_Domain",
    "03_Systems",
    "04_Requirements",
    "05_Backlog",
    "06_Delivery",
    "07_Decisions",
    "08_Verification",
    "09_Operations",
    "80_Templates",
    "90_Agent",
    ".agents",
];

#[derive(Clone, Debug)]
pub struct Workspace {
    pub root: PathBuf,
}
/// Shared ownership of one process-level writer lock, including background recovery work.
#[derive(Clone)]
pub struct WorkspaceLock {
    root: PathBuf,
    _file: std::sync::Arc<WriterLease>,
}
struct WriterLease(fs::File);
impl Drop for WriterLease {
    fn drop(&mut self) {
        // Release when the last guard finishes, even if a concurrently starting
        // subprocess temporarily retains a duplicate descriptor before exec.
        let _ = self.0.unlock();
    }
}
impl WorkspaceLock {
    pub fn acquire(workspace: &Workspace) -> Result<Self> {
        let state = workspace.path(".pm-agent")?;
        fs::create_dir_all(&state)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(workspace.path(".pm-agent/workspace.lock")?)?;
        file.try_lock()
            .context("Close other PM Agent processes before opening another writer")?;
        Ok(Self {
            root: workspace.root.clone(),
            _file: std::sync::Arc::new(WriterLease(file)),
        })
    }
    pub(crate) fn verify(&self, workspace: &Workspace) -> Result<()> {
        ensure!(
            self.root == workspace.root,
            "Workspace lock belongs to another Vault"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub path: String,
    pub title: String,
    pub kind: String,
    pub id: Option<String>,
    pub hash: String,
    pub metadata: Value,
    pub body: String,
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn parse_note(text: &str) -> Result<(Value, String)> {
    let normalized = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .context("Knowledge document requires YAML frontmatter")?;
    let (head, body) = rest
        .split_once("\n---\n")
        .or_else(|| rest.strip_suffix("\n---").map(|head| (head, "")))
        .context("Unclosed YAML frontmatter")?;
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(head).context("Invalid or duplicate YAML properties")?;
    let meta: Value = serde_json::to_value(yaml)?;
    ensure!(meta.is_object(), "Properties must be an object");
    Ok((meta, body.trim_start().to_string()))
}
pub fn note(meta: Value, body: &str) -> Result<String> {
    // JSON scalar syntax matches the existing Python/Obsidian property contract.
    let object = meta.as_object().context("Properties must be an object")?;
    let mut text = String::from("---\n");
    for (key, value) in object {
        text.push_str(&format!("{key}: {}\n", serde_json::to_string(value)?));
    }
    text.push_str("---\n\n");
    text.push_str(body.trim());
    text.push('\n');
    Ok(text)
}

impl Workspace {
    pub fn open(root: &Path) -> Result<Self> {
        let root = root.canonicalize().context("Workspace does not exist")?;
        ensure!(
            root.join("00_Governance/Project.md").is_file(),
            "Expected an Agent PM workspace containing 00_Governance/Project.md"
        );
        Ok(Self { root })
    }
    pub fn path(&self, relative: &str) -> Result<PathBuf> {
        let rel = Path::new(relative);
        ensure!(
            !relative.is_empty() && !rel.is_absolute(),
            "Use a nonempty workspace-relative path"
        );
        let mut p = self.root.clone();
        for component in rel.components() {
            match component {
                Component::Normal(x) => p.push(x),
                _ => bail!("Path traversal is not allowed"),
            }
            if let Ok(m) = fs::symlink_metadata(&p) {
                ensure!(
                    !m.file_type().is_symlink(),
                    "Symlinks are not permitted in workspace operations"
                );
            }
        }
        Ok(p)
    }
    pub fn read(&self, relative: &str) -> Result<String> {
        let first = relative.split('/').next().unwrap_or("");
        ensure!(
            FOLDERS.contains(&first)
                || GUIDES.contains(&relative)
                || matches!(
                    relative,
                    "AGENTS.md"
                        | "README.md"
                        | "HOME.md"
                        | "References.md"
                        | "tools/README.md"
                        | "CONTEXT-MAP.md"
                        | "index.md"
                        | "log.md"
                ),
            "Path is outside project knowledge scope"
        );
        let path = self.path(relative)?;
        ensure!(
            fs::metadata(&path)?.len() <= MAX_FILE,
            "File exceeds the 2 MiB text limit; select a smaller source excerpt"
        );
        fs::read_to_string(&path).with_context(|| format!("Cannot read UTF-8 text: {relative}; request a reviewed extraction or open the original"))
    }
    pub fn documents(&self) -> Result<Vec<Document>> {
        let mut documents = vec![];
        for entry in WalkDir::new(&self.root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                e.depth() == 0
                    || if e.depth() == 1 {
                        FOLDERS.contains(&name.as_ref())
                            || matches!(name.as_ref(), "agent" | "tests")
                            || e.file_type().is_file()
                    } else {
                        let relative = e.path().strip_prefix(&self.root).unwrap();
                        if relative.starts_with("agent") || relative.starts_with("tests") {
                            GUIDES.contains(&relative.to_string_lossy().as_ref())
                        } else {
                            !matches!(
                                name.as_ref(),
                                "Assets"
                                    | "Snapshots"
                                    | "Baselines"
                                    | "Inbox"
                                    | "Extracted"
                                    | "__pycache__"
                            )
                        }
                    }
            })
        {
            let e = entry?;
            if !e.file_type().is_file() || e.path().extension().is_none_or(|x| x != "md") {
                continue;
            }
            let relative = e
                .path()
                .strip_prefix(&self.root)?
                .to_string_lossy()
                .replace('\\', "/");
            let text = self.read(&relative)?;
            let (metadata, body) = if matches!(relative.as_str(), "index.md" | "log.md")
                && !text.starts_with("---\n")
            {
                (json!({"type":"Navigation","title":relative}), text.clone())
            } else {
                match parse_note(&text) {
                    Ok(note) => note,
                    Err(error) if relative.starts_with(".agents/skills/") => (
                        json!({"type":"Invalid Skill","title":relative,"error":error.to_string()}),
                        text.clone(),
                    ),
                    Err(error) => return Err(error).with_context(|| relative.clone()),
                }
            };
            documents.push(Document {
                title: metadata["title"]
                    .as_str()
                    .or(metadata["name"].as_str())
                    .unwrap_or(&relative)
                    .into(),
                kind: metadata["type"].as_str().unwrap_or("Skill").into(),
                id: metadata["id"].as_str().map(String::from),
                hash: hash(text.as_bytes()),
                metadata,
                body,
                path: relative,
            });
        }
        documents.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(documents)
    }
    pub fn language(&self) -> Result<String> {
        let (m, _) = parse_note(&self.read("00_Governance/Settings.md")?)?;
        ensure!(m["agent_language"] == "en", "Agent language must be en");
        Ok(m["document_language"]
            .as_str()
            .filter(|x| !x.trim().is_empty())
            .context("Missing document_language")?
            .into())
    }
    pub fn save_draft(
        &self,
        relative: &str,
        content: &str,
        expected_hash: Option<&str>,
        citations: &[Value],
    ) -> Result<String> {
        let output = self.prepare_draft(relative, content, expected_hash, citations)?;
        atomic_write(&self.path(relative)?, output.as_bytes())?;
        Ok(hash(output.as_bytes()))
    }
    /// Check the complete write contract without changing a file.
    pub(crate) fn prepare_draft(
        &self,
        relative: &str,
        content: &str,
        expected_hash: Option<&str>,
        citations: &[Value],
    ) -> Result<String> {
        ensure!(
            DRAFT_ROOTS.iter().any(|p| relative.starts_with(p)),
            "Agent writes are restricted to project drafts under: {}. Choose a permitted Markdown path; use PM tools for controlled operations.",
            DRAFT_ROOTS.join(", ")
        );
        ensure!(
            relative.ends_with(".md") && content.len() <= MAX_FILE as usize,
            "Expected a Markdown draft under 2 MiB"
        );
        let (m, body) = parse_note(content)?;
        ensure!(
            m["type"] != "question" && m.get("clarification_schema").is_none(),
            "Use ask_clarification to surface questions to the user"
        );
        for key in ["type", "title", "id", "language"] {
            ensure!(
                m[key].as_str().is_some_and(|v| !v.trim().is_empty()),
                "Missing required property: {key}"
            );
        }
        ensure!(
            m["language"] == self.language()?,
            "Draft language must match project Settings"
        );
        ensure!(body.starts_with("# "), "Draft needs a readable heading");
        ensure!(
            m["doc_status"] == "draft",
            "Agent document writes must be drafts: set doc_status to draft. For PBIs, delivery_status proposed is a separate field."
        );
        for key in [
            "verified",
            "reviewed_by",
            "reviewed_at",
            "decision",
            "baseline_id",
        ] {
            ensure!(
                m.get(key).is_none_or(Value::is_null),
                "Use governed PM procedures for {key}"
            );
        }
        ensure!(
            m.get("delivery_status")
                .is_none_or(|v| v.is_null() || v == "proposed"),
            "Agent drafts cannot advance delivery status"
        );
        ensure!(
            m.get("workflow_status")
                .is_none_or(|v| v.is_null() || v == "draft" || v == "open"),
            "Agent drafts cannot approve workflow outcomes"
        );
        ensure!(
            !citations.is_empty(),
            "At least one traceable source is required; ask for missing information"
        );
        for citation in citations {
            let path = citation["path"].as_str().context("Citation needs path")?;
            let current = self.read(path)?;
            ensure!(
                citation["sha256"] == hash(current.as_bytes()),
                "Source changed or hash missing: {path}; reread and review the draft"
            );
        }
        let dest = self.path(relative)?;
        if dest.exists() {
            let old = fs::read_to_string(&dest)?;
            let (meta, _) = parse_note(&old)?;
            ensure!(
                meta["type"] != "question" && meta.get("clarification_schema").is_none(),
                "Question scope and responses cannot be overwritten through write_draft"
            );
            ensure!(
                meta["doc_status"] == "draft",
                "Only existing draft documents can be edited by the Agent"
            );
            ensure!(
                meta.get("baseline_id").is_none_or(Value::is_null),
                "Pinned content requires a new versioned draft"
            );
            ensure!(
                expected_hash == Some(hash(old.as_bytes()).as_str()),
                "Document changed; refresh and reconcile before writing"
            );
        } else {
            ensure!(
                expected_hash.is_none(),
                "Expected document no longer exists"
            );
        }
        let id = m["id"].as_str().unwrap();
        ensure!(
            !self
                .documents()?
                .iter()
                .any(|d| d.path != relative && d.id.as_deref() == Some(id)),
            "Duplicate document ID: {id}"
        );
        let mut properties = m.as_object().unwrap().clone();
        properties.insert(
            "evidence_paths".into(),
            json!(
                citations
                    .iter()
                    .filter_map(|c| c["path"].as_str())
                    .collect::<Vec<_>>()
            ),
        );
        properties.insert(
            "sources".into(),
            json!(
                citations
                    .iter()
                    .map(|c| json!({"resource": c["path"], "sha256": c["sha256"]}))
                    .collect::<Vec<_>>()
            ),
        );
        let output = note(Value::Object(properties), &body)?;
        crate::clarification::ensure_draft(self, relative, &output, citations)?;
        Ok(output)
    }
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Missing parent")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn workspace() -> (tempfile::TempDir, Workspace) {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("00_Governance")).unwrap();
        fs::write(
            d.path().join("00_Governance/Project.md"),
            note(
                json!({"type":"project","title":"Test","language":"en"}),
                "# Test",
            )
            .unwrap(),
        )
        .unwrap();
        fs::write(d.path().join("00_Governance/Settings.md"), note(json!({"type":"settings","title":"Settings","document_language":"en","agent_language":"en"}), "# Settings").unwrap()).unwrap();
        let w = Workspace::open(d.path()).unwrap();
        (d, w)
    }
    #[test]
    fn writer_lock_release_follows_guard_ownership_even_with_a_duplicate_descriptor() {
        let (_dir, w) = workspace();
        let lock = WorkspaceLock::acquire(&w).unwrap();
        // A subprocess starting on another thread can briefly inherit this open file.
        let descriptor = lock._file.0.try_clone().unwrap();
        let worker = lock.clone();
        drop(lock);
        assert!(WorkspaceLock::acquire(&w).is_err());
        drop(worker);
        let next = WorkspaceLock::acquire(&w).unwrap();
        drop(descriptor);
        assert!(WorkspaceLock::acquire(&w).is_err());
        drop(next);
        assert!(WorkspaceLock::acquire(&w).is_ok());
    }
    #[test]
    fn rejects_escape_and_symlinks() {
        let (_d, w) = workspace();
        assert!(w.path("../secret").is_err());
        assert!(w.path("/tmp/secret").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", w.root.join("escape")).unwrap();
            assert!(w.path("escape/x").is_err());
        }
        assert!(w.read(".pm-agent/config.toml").is_err());
    }
    #[test]
    fn rejects_stale_source_and_overwriting_external_edit() {
        let (_d, w) = workspace();
        let source = w.read("00_Governance/Project.md").unwrap();
        let citation = json!({"path":"00_Governance/Project.md","sha256":hash(source.as_bytes())});
        let draft = note(
            json!({"type":"prd","title":"Draft","id":"PRD-1","language":"en","doc_status":"draft"}),
            "# Draft",
        )
        .unwrap();
        let h = w
            .save_draft(
                "04_Requirements/PRD/PRD-1.md",
                &draft,
                None,
                std::slice::from_ref(&citation),
            )
            .unwrap();
        assert!(
            w.save_draft(
                "04_Requirements/PRD/PRD-1.md",
                &draft,
                Some("old"),
                std::slice::from_ref(&citation)
            )
            .is_err()
        );
        assert!(
            w.save_draft(
                "04_Requirements/PRD/PRD-1.md",
                &draft,
                Some(&h),
                &[citation]
            )
            .is_ok()
        );
        assert!(
            w.save_draft(
                "04_Requirements/PRD/PRD-2.md",
                &draft,
                None,
                &[json!({"path":"00_Governance/Project.md","sha256":"old"})]
            )
            .is_err()
        );
    }
    #[test]
    fn duplicate_yaml_is_an_error() {
        assert!(parse_note("---\ntype: x\ntype: y\n---\n# X").is_err());
    }
}
