use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    Responses,
    ChatCompletions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub protocol: Protocol,
    pub base_url: String,
    pub model: String,
    pub api_key_env: Option<String>,
    #[serde(default = "yes")]
    pub tools: bool,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
}
fn yes() -> bool {
    true
}
fn timeout() -> u64 {
    120
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpConfig {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env_names: Vec<String>,
        #[serde(default)]
        allowed_tools: Vec<String>,
    },
    Http {
        url: String,
        bearer_token_env: Option<String>,
        #[serde(default)]
        allowed_tools: Vec<String>,
    },
}
impl McpConfig {
    pub fn allowed_tools(&self) -> &[String] {
        match self {
            Self::Stdio { allowed_tools, .. } | Self::Http { allowed_tools, .. } => allowed_tools,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub provider: String,
    #[serde(default = "steps")]
    pub max_steps: usize,
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub mcp: BTreeMap<String, McpConfig>,
}
fn steps() -> usize {
    32
}

impl Config {
    pub fn read(path: &Path) -> Result<Self> {
        let config: Self = toml::from_str(&std::fs::read_to_string(path).with_context(|| {
            format!(
                "Read config {}. Copy agent/config.example.toml and set a model.",
                path.display()
            )
        })?)?;
        Ok(config)
    }
    pub fn load(path: &Path) -> Result<Self> {
        let config = Self::read(path)?;
        let p = config.active()?;
        if p.model.trim().is_empty() {
            bail!("Set an explicit model in the active provider profile; no model is inferred")
        }
        validate_endpoint(&p.base_url)?;
        if p.timeout_seconds == 0 || config.max_steps == 0 || config.max_steps > 100 {
            bail!("Timeout must be positive and max_steps must be in 1..=100")
        }
        Ok(config)
    }
    pub fn active(&self) -> Result<&ProviderConfig> {
        self.providers
            .get(&self.provider)
            .context("Active provider profile does not exist")
    }
}
pub fn validate_endpoint(value: &str) -> Result<()> {
    let u = reqwest::Url::parse(value)?;
    if !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        bail!("Endpoint must not contain credentials, query or fragment")
    }
    let local = matches!(
        u.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if u.scheme() != "https" && !(u.scheme() == "http" && local) {
        bail!("Use HTTPS, or HTTP on loopback only")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_credentials_and_cleartext_remote_are_rejected() {
        for url in [
            "https://secret@example.com/v1",
            "http://example.com",
            "https://x/v1?key=secret",
            "file:///tmp/x",
        ] {
            assert!(validate_endpoint(url).is_err());
        }
        assert!(validate_endpoint("http://127.0.0.1:11434/v1").is_ok());
    }
}
