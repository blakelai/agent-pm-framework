//! Operator tool permissions. Grants authorize execution, never business approval.
use crate::{
    config::McpConfig,
    provider::{Call, ToolSpec},
    workspace::{MAX_FILE, atomic_write, hash},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Once,
    Conversation,
    Always,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grant {
    pub tool: String,
    pub operation: String,
    pub binding_sha256: String,
}
impl Grant {
    pub fn pm(operation: &str, spec: &ToolSpec) -> Result<Self> {
        ensure!(
            matches!(operation, "settings" | "ingest" | "repo-add" | "okf-export"),
            "This PM operation does not require a reusable approval"
        );
        Ok(Self { tool: "pm".into(), operation: operation.into(), binding_sha256: hash(serde_json::to_vec(&json!({"version":1,"tool":"pm","operation":operation,"schema":spec.parameters}))?.as_slice()) })
    }
    pub fn mcp(
        alias: &str,
        server: &str,
        tool: &str,
        config: &McpConfig,
        spec: &ToolSpec,
    ) -> Result<Self> {
        Ok(Self { tool:alias.into(), operation:format!("{server}/{tool}"), binding_sha256:hash(serde_json::to_vec(&json!({"version":1,"server":server,"tool":tool,"config":config,"schema":spec.parameters}))?.as_slice()) })
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            (self.tool == "pm" || self.tool.starts_with("mcp_"))
                && self.tool.len() <= 64
                && self
                    .tool
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
                && !self.operation.is_empty()
                && self.binding_sha256.len() == 64
                && self.binding_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid global tool approval"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub call_id: String,
    pub decision: Decision,
    pub grant: Option<Grant>,
    pub at: String,
}

/// Denial needs only local saved state; no provider configuration or MCP connection.
pub fn deny(
    workspace: &crate::workspace::Workspace,
    store: &crate::store::Store,
    session: &mut crate::runtime::Session,
    expected: &Call,
) -> Result<()> {
    use crate::{
        runtime::Status,
        workspace::{note, parse_note},
    };
    ensure!(
        session.status == Status::WaitingApproval && session.pending.first() == Some(expected),
        "Pending tool changed; review the current approval request"
    );
    for (index, call) in session.pending.drain(..).enumerate() {
        let result = if index == 0 {
            json!({"status":"denied_by_user","instruction":"The user stopped this response. Do not retry this action through another tool."})
        } else {
            json!({"status":"cancelled_by_user","instruction":"The user stopped this response before this call was dispatched."})
        };
        let responses = session
            .history
            .iter()
            .any(|item| item["type"] == "function_call" && item["call_id"] == call.id);
        session.history.push(if responses {
            json!({"type":"function_call_output","call_id":call.id,"output":result.to_string()})
        } else {
            json!({"role":"tool","tool_call_id":call.id,"content":result.to_string()})
        });
    }
    session.status = Status::Cancelled;
    session.pending_approval = None;
    session.error = None;
    session.approval_decisions.push(Record {
        call_id: expected.id.clone(),
        decision: Decision::Deny,
        grant: None,
        at: chrono::Utc::now().to_rfc3339(),
    });
    store.save(&session.id, session)?;
    let path = workspace.path(&format!("90_Agent/Runs/RUN-{}.md", session.id))?;
    let (mut meta, mut body) = if path.exists() {
        parse_note(&fs::read_to_string(&path)?)?
    } else {
        (
            json!({"type":"agent_run","title":"PM Agent execution record","language":"en","id":format!("RUN-{}",session.id),"stage":session.stage}),
            format!("# PM Agent execution record\n\n- Session: {}\n", session.id),
        )
    };
    meta["workflow_status"] = json!("cancelled");
    body.push_str(&format!("\n\n## Tool approval denied\n\nThe user denied tool {} (call {}). The response stopped without dispatching this call or requesting another model response. Remaining pending calls were cancelled. Exact history remains in the private session database.\n", expected.name,expected.id));
    atomic_write(&path, note(meta, &body)?.as_bytes())
}

pub fn scope(call: &Call) -> String {
    let args: serde_json::Value = serde_json::from_str(&call.arguments).unwrap_or_default();
    if call.name == "pm" {
        format!("pm {}", args["command"].as_str().unwrap_or("operation"))
    } else {
        call.name.clone()
    }
}
pub fn reason(call: &Call) -> &'static str {
    if call.name.starts_with("mcp_") {
        return "This external MCP tool requires your permission to run.";
    }
    let args: serde_json::Value = serde_json::from_str(&call.arguments).unwrap_or_default();
    match args["command"].as_str() {
        Some("settings") => "This operation changes project settings or document language.",
        Some("ingest") => {
            "This operation imports material from outside registered workspace assets."
        }
        Some("repo-add") => "This operation registers an external repository source.",
        Some("okf-export") => "This operation writes a knowledge bundle outside the Vault.",
        _ => "This tool requires your permission to run.",
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    #[serde(default = "version")]
    version: u32,
    #[serde(default)]
    tool_approvals: Vec<Grant>,
}
fn version() -> u32 {
    1
}

/// Resolves the user file lazily: ordinary local operations need no home directory.
#[derive(Default)]
pub struct GlobalApprovals {
    path: Option<PathBuf>,
    disabled: bool,
}
impl GlobalApprovals {
    pub fn at(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            disabled: false,
        }
    }
    pub fn disabled() -> Self {
        Self {
            path: None,
            disabled: true,
        }
    }
    pub fn path(&self) -> Result<PathBuf> {
        let path = if let Some(path) = &self.path {
            path.clone()
        } else if let Some(path) = std::env::var_os("PM_AGENT_GLOBAL_CONFIG") {
            PathBuf::from(path)
        } else {
            let base = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
                })
                .context(
                    "Set PM_AGENT_GLOBAL_CONFIG to an absolute file path for global approvals",
                )?;
            base.join("pm-agent/config.toml")
        };
        ensure!(
            path.is_absolute(),
            "Global approval configuration must use an absolute path"
        );
        Ok(path)
    }
    pub fn allows(&self, grant: &Grant) -> Result<bool> {
        if self.disabled {
            return Ok(false);
        }
        Ok(Self::read(&self.path()?)?.tool_approvals.contains(grant))
    }
    pub fn allow(&self, grant: &Grant) -> Result<()> {
        ensure!(
            !self.disabled,
            "Global approvals are disabled for isolated evaluations"
        );
        grant.validate()?;
        let path = self.path()?;
        let parent = path
            .parent()
            .context("Global configuration needs a parent directory")?;
        fs::create_dir_all(parent)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("toml.lock"))?;
        lock.try_lock()
            .context("Global settings are being updated; retry the approval")?;
        let mut settings = Self::read(&path)?;
        if !settings.tool_approvals.contains(grant) {
            settings.tool_approvals.push(grant.clone());
        }
        settings.version = version();
        atomic_write(&path, toml::to_string_pretty(&settings)?.as_bytes())
            .with_context(|| format!("Cannot save global tool approval: {}", path.display()))
    }
    fn read(path: &Path) -> Result<Settings> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings {
                    version: version(),
                    tool_approvals: vec![],
                });
            }
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("Cannot read global approval settings: {}", path.display())
                });
            }
        };
        ensure!(
            bytes.len() as u64 <= MAX_FILE,
            "Global approval configuration exceeds the text limit"
        );
        let text = std::str::from_utf8(&bytes).context("Global approval settings must be UTF-8")?;
        let settings: Settings = toml::from_str(text).map_err(|_| {
            anyhow::anyhow!(
                "Invalid global approval settings: {}; configuration contents are omitted",
                path.display()
            )
        })?;
        ensure!(
            settings.version == version(),
            "Unsupported global approval configuration version"
        );
        for grant in &settings.tool_approvals {
            grant.validate()?;
        }
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_grants_preserve_existing_permissions_and_reject_changed_bindings_or_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let file = GlobalApprovals::at(path.clone());
        let spec = crate::tools::specs()
            .into_iter()
            .find(|s| s.name == "pm")
            .unwrap();
        let first = Grant::pm("settings", &spec).unwrap();
        let second = Grant::pm("ingest", &spec).unwrap();
        assert!(!file.allows(&first).unwrap());
        file.allow(&first).unwrap();
        file.allow(&second).unwrap();
        file.allow(&first).unwrap();
        let fresh = GlobalApprovals::at(path.clone());
        assert!(fresh.allows(&first).unwrap());
        assert!(fresh.allows(&second).unwrap());
        assert_eq!(
            GlobalApprovals::read(&path).unwrap().tool_approvals.len(),
            2
        );
        let mut changed = first.clone();
        changed.binding_sha256 = "0".repeat(64);
        assert!(!fresh.allows(&changed).unwrap());
        fs::write(&path, "secret = 'SYNTHETIC_SECRET'\n").unwrap();
        assert!(!format!("{:#}", fresh.allow(&first).unwrap_err()).contains("SYNTHETIC_SECRET"));
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "secret = 'SYNTHETIC_SECRET'\n"
        );
    }
    #[test]
    fn mcp_grants_change_with_server_or_schema_and_cannot_be_wildcards() {
        let mut spec = ToolSpec {
            name: "mcp_docs_search".into(),
            description: "Synthetic".into(),
            parameters: json!({"type":"object"}),
        };
        let first = McpConfig::Http {
            url: "https://example.test/mcp".into(),
            bearer_token_env: None,
            allowed_tools: None,
        };
        let second = McpConfig::Http {
            url: "https://other.test/mcp".into(),
            bearer_token_env: None,
            allowed_tools: None,
        };
        let grant = Grant::mcp("mcp_docs_search", "docs", "search", &first, &spec).unwrap();
        assert_ne!(
            grant,
            Grant::mcp("mcp_docs_search", "docs", "search", &second, &spec).unwrap()
        );
        spec.parameters = json!({"type":"object","required":["query"]});
        assert_ne!(
            grant,
            Grant::mcp("mcp_docs_search", "docs", "search", &first, &spec).unwrap()
        );
        let mut wildcard = grant.clone();
        wildcard.tool = "mcp_*".into();
        assert!(wildcard.validate().is_err());
    }
}
