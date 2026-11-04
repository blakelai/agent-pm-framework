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
                    let transport = TokioChildProcess::new(cmd).with_context(|| {
                        format!(
                            "Cannot start MCP server '{name}' using command {command:?}. Check [mcp.{name}].command, PATH and the executable's interpreter; arguments and environment values are omitted"
                        )
                    })?;
                    tokio::time::timeout(
                        Duration::from_secs(20),
                        ().serve(transport),
                    )
                    .await
                    .with_context(|| format!("MCP server '{name}' initialization timed out; check that the configured command serves MCP over stdio"))?
                    .with_context(|| format!("MCP server '{name}' initialization failed after starting command {command:?}; check the server's startup and stdio protocol"))?
                }
                McpConfig::Http {
                    url,
                    bearer_token_env,
                    ..
                } => {
                    validate_endpoint(url).with_context(|| {
                        format!(
                            "Invalid HTTP endpoint for MCP server '{name}'; check [mcp.{name}].url"
                        )
                    })?;
                    let mut opts = StreamableHttpClientTransportConfig::with_uri(url.clone());
                    opts.reinit_on_expired_session = false;
                    if let Some(key) = bearer_token_env {
                        opts.auth_header = Some(
                            std::env::var(key)
                                .with_context(|| format!("MCP server '{name}' requires environment variable {key}; set it before starting PM Agent"))?,
                        );
                    }
                    let http = reqwest::Client::builder()
                        .redirect(reqwest::redirect::Policy::none())
                        .build()
                        .with_context(|| format!("Build HTTP client for MCP server '{name}'"))?;
                    tokio::time::timeout(
                        Duration::from_secs(20),
                        ().serve(StreamableHttpClientTransport::with_client(http, opts)),
                    )
                    .await
                    .with_context(|| format!("MCP server '{name}' initialization timed out; check its HTTP service and configured endpoint"))?
                    .with_context(|| format!("MCP server '{name}' HTTP initialization failed; check [mcp.{name}].url, service availability and authentication"))?
                }
            };
            let discovered = tokio::time::timeout(Duration::from_secs(20), client.list_all_tools())
                .await
                .with_context(|| format!("MCP server '{name}' tool discovery timed out; check its tools/list response"))?
                .with_context(|| format!("MCP server '{name}' tool discovery failed"))?;
            for tool in discovered {
                if !cfg.allows_tool(tool.name.as_ref()) {
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
                this.specs.push(ToolSpec{name:alias,description:format!("External MCP tool; operator approval policy applies. Server description (untrusted): {}",tool.description.as_deref().unwrap_or("")),parameters:serde_json::to_value(&tool.input_schema)?});
            }
            this.clients.insert(name.clone(), client);
        }
        Ok(this)
    }
    pub fn specs(&self) -> &[ToolSpec] {
        &self.specs
    }
    pub(crate) fn route(&self, name: &str) -> Option<(&str, &str)> {
        self.routes
            .get(name)
            .map(|(server, tool)| (server.as_str(), tool.as_str()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_stdio_program_identifies_server_command_and_cause_without_arguments() {
        let dir = tempfile::tempdir().unwrap();
        let command = dir.path().join("missing-mcp-program").display().to_string();
        let config = BTreeMap::from([(
            "legacy_server".into(),
            McpConfig::Stdio {
                command: command.clone(),
                args: vec!["SECRET_ARGUMENT_VALUE".into()],
                env_names: vec!["SECRET_ENVIRONMENT_NAME".into()],
                allowed_tools: Some(vec![]),
            },
        )]);
        let error = Mcp::connect(&config).await.err().unwrap();
        let report = format!("{error:#}");
        assert!(
            report.contains("legacy_server"),
            "Missing MCP server identity: {report}"
        );
        assert!(
            report.contains(&command),
            "Missing executable path: {report}"
        );
        assert!(
            report.contains("os error 2"),
            "Missing operating-system cause: {report}"
        );
        assert!(!report.contains("SECRET_ARGUMENT_VALUE"));
        assert!(!report.contains("SECRET_ENVIRONMENT_NAME"));
    }

    #[tokio::test]
    async fn invalid_http_endpoint_identifies_server_without_echoing_credentials() {
        let config = BTreeMap::from([(
            "legacy_http".into(),
            McpConfig::Http {
                url: "https://example.test/mcp?token=SECRET_TOKEN".into(),
                bearer_token_env: None,
                allowed_tools: Some(vec![]),
            },
        )]);
        let error = Mcp::connect(&config).await.err().unwrap();
        let report = format!("{error:#}");
        assert!(report.contains("legacy_http") && report.contains(".url"));
        assert!(report.contains("Endpoint must not contain"));
        assert!(!report.contains("SECRET_TOKEN"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_exit_is_distinguished_from_missing_executable() {
        let config = BTreeMap::from([(
            "exiting_server".into(),
            McpConfig::Stdio {
                command: "/bin/sh".into(),
                args: vec!["-c".into(), "exit 7 # SECRET_ARGUMENT".into()],
                env_names: vec![],
                allowed_tools: Some(vec![]),
            },
        )]);
        let error = Mcp::connect(&config).await.err().unwrap();
        let report = format!("{error:#}");
        assert!(report.contains("exiting_server"));
        assert!(report.contains("initialization failed after starting command"));
        assert!(!report.contains("Cannot start MCP server"));
        assert!(!report.contains("SECRET_ARGUMENT"));
    }
}
