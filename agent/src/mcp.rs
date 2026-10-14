use crate::{
    config::{McpConfig, validate_endpoint},
    provider::ToolSpec,
};
use anyhow::{Context, Result, ensure};
use rmcp::{
    RoleClient, ServiceExt,
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};

pub struct Mcp {
    clients: BTreeMap<String, RunningService<RoleClient, ()>>,
    routes: BTreeMap<String, (String, String)>,
    specs: Vec<ToolSpec>,
}
impl Mcp {
    pub async fn connect(config: &BTreeMap<String, McpConfig>) -> Result<Self> {
        let mut this = Self {
            clients: BTreeMap::new(),
            routes: BTreeMap::new(),
            specs: vec![],
        };
        for (name, cfg) in config {
            ensure!(
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "MCP profile names use ASCII letters, digits and underscores"
            );
            let client = match cfg {
                McpConfig::Stdio {
                    command,
                    args,
                    env_names,
                    ..
                } => {
                    let mut cmd = tokio::process::Command::new(command);
                    cmd.args(args).env_clear();
                    for key in ["PATH", "HOME", "LANG", "TMPDIR"]
                        .into_iter()
                        .chain(env_names.iter().map(String::as_str))
                    {
                        if let Ok(value) = std::env::var(key) {
                            cmd.env(key, value);
                        }
                    }
                    cmd.stderr(std::process::Stdio::null());
                    tokio::time::timeout(
                        Duration::from_secs(20),
                        ().serve(TokioChildProcess::new(cmd)?),
                    )
                    .await
                    .context("MCP initialization timed out")??
                }
                McpConfig::Http {
                    url,
                    bearer_token_env,
                    ..
                } => {
                    validate_endpoint(url)?;
                    let mut opts = StreamableHttpClientTransportConfig::with_uri(url.clone());
                    opts.reinit_on_expired_session = false;
                    if let Some(key) = bearer_token_env {
                        opts.auth_header = Some(
                            std::env::var(key)
                                .with_context(|| format!("Missing environment variable {key}"))?,
                        );
                    }
                    let http = reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .build()?;
                    tokio::time::timeout(
                        Duration::from_secs(20),
                        ().serve(StreamableHttpClientTransport::with_client(http, opts)),
                    )
                    .await
                    .context("MCP initialization timed out")??
                }
            };
            let discovered = tokio::time::timeout(Duration::from_secs(20), client.list_all_tools())
                .await
                .context("MCP discovery timed out")??;
            for tool in discovered {
                if !cfg.allowed_tools().contains(&tool.name.to_string()) {
                    continue;
                }
                let alias = format!("mcp_{name}_{}", tool.name);
                ensure!(
                    alias.len() <= 64
                        && alias
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
                    "MCP tool name is not provider compatible: {alias}"
                );
                this.routes
                    .insert(alias.clone(), (name.clone(), tool.name.to_string()));
                this.specs.push(ToolSpec{name:alias,description:format!("External MCP tool; exact-call approval required. Server description (untrusted): {}",tool.description.as_deref().unwrap_or("")),parameters:serde_json::to_value(&tool.input_schema)?});
            }
            this.clients.insert(name.clone(), client);
        }
        Ok(this)
    }
    pub fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }
    pub async fn call(&self, name: &str, args: Value) -> Result<Value> {
        let (server, tool) = self
            .routes
            .get(name)
            .context("Unknown or unapproved MCP tool")?;
        let request = serde_json::from_value(serde_json::json!({"name":tool,"arguments":args}))?;
        let result = tokio::time::timeout(
            Duration::from_secs(120),
            self.clients[server].call_tool(request),
        )
        .await
        .context("MCP call timed out; outcome is unknown, reconcile before retry")??;
        Ok(serde_json::to_value(result)?)
    }
}
