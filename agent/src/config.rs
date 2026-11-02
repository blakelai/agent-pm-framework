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
    #[serde(default = "yes", skip_serializing_if = "enabled")]
    pub request_usage: bool,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
}
fn yes() -> bool {
    true
}
fn enabled(value: &bool) -> bool {
    *value
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
    /// Accepted only for compatibility. Interactive execution has no step cap.
    #[serde(default, skip_serializing)]
    pub max_steps: usize,
    #[serde(default)]
    pub session_limits: crate::budget::Limits,
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub mcp: BTreeMap<String, McpConfig>,
}
impl Config {
    /// Request-count limits belong to the evaluator, never interactive sessions.
    pub fn interactive_limits(&self) -> crate::budget::Limits {
        crate::budget::Limits {
            max_requests: None,
            max_total_tokens: self.session_limits.max_total_tokens,
        }
    }
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| {
            format!(
                "Cannot read provider configuration {}. Check --config and file permissions; for a new configuration, copy agent/config.example.toml and set a model.",
                path.display()
            )
        })?;
        // toml::de::Error displays the raw source line and may repeat configured values.
        // Keep the exact location and a safe failure category, never config contents.
        toml::from_str(&text)
            .map_err(|error: toml::de::Error| safe_parse_error(&text, &error))
            .with_context(|| format!("Cannot parse provider configuration {}; compare the indicated location with agent/config.example.toml", path.display()))
    }
    pub fn load(path: &Path) -> Result<Self> {
        let config = Self::read(path)?;
        (|| -> Result<()> {
            let p = config.active()?;
            if p.model.trim().is_empty() {
                bail!("Set an explicit model in the active provider profile; no model is inferred")
            }
            validate_endpoint(&p.base_url).context("Check the provider's base_url")?;
            if p.timeout_seconds == 0 {
                bail!("timeout_seconds must be positive")
            }
            Ok(())
        })()
        .with_context(|| {
            format!(
                "Validate active provider profile {:?} in {}",
                config.provider,
                path.display()
            )
        })?;
        Ok(config)
    }
    pub fn active(&self) -> Result<&ProviderConfig> {
        self.providers
            .get(&self.provider)
            .with_context(|| format!("Active provider profile {:?} does not exist; make provider match an entry in [providers]", self.provider))
    }
}
fn safe_parse_error(text: &str, error: &toml::de::Error) -> anyhow::Error {
    let missing_field = error
        .message()
        .strip_prefix("missing field `")
        .and_then(|message| message.strip_suffix('`'))
        .filter(|field| {
            matches!(
                *field,
                "provider"
                    | "providers"
                    | "protocol"
                    | "base_url"
                    | "model"
                    | "transport"
                    | "command"
                    | "url"
            )
        });
    let category = if error.message().starts_with("unknown field") {
        "Unsupported configuration field"
    } else if error.message().starts_with("missing field") {
        "Missing required configuration field"
    } else if error.message().starts_with("unknown variant") {
        "Unsupported configuration choice; check protocol and transport"
    } else if error.message().starts_with("invalid type") {
        "Incorrect configuration value type"
    } else if error.message().starts_with("duplicate") {
        "Duplicate configuration key"
    } else {
        "Invalid TOML syntax or configuration value"
    };
    let category = missing_field
        .map(|field| format!("Missing required configuration field `{field}`"))
        .unwrap_or_else(|| category.into());
    let location = error
        .span()
        .map(|span| {
            let mut offset = span.start.min(text.len());
            while !text.is_char_boundary(offset) {
                offset -= 1;
            }
            let before = &text[..offset];
            let line = before.bytes().filter(|b| *b == b'\n').count() + 1;
            let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            format!(" at line {line}, column {column}")
        })
        .unwrap_or_default();
    anyhow::anyhow!("{category}{location}; configuration contents are omitted")
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
    fn missing_config_and_invalid_active_profile_keep_path_and_cause() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("provider-config.toml");
        let error = Config::load(&path).unwrap_err();
        let report = format!("{error:#}");
        assert!(report.contains(path.to_str().unwrap()));
        assert!(report.contains("--config") && report.contains("os error 2"));
        std::fs::write(&path, "provider = \"legacy\"\nproviders = {}\n").unwrap();
        let error = Config::load(&path).unwrap_err();
        let report = format!("{error:#}");
        assert!(report.contains(path.to_str().unwrap()));
        assert!(report.contains("legacy") && report.contains("does not exist"));
    }
    #[test]
    fn config_parse_failure_identifies_file_and_location_without_echoing_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy-config.toml");
        std::fs::write(&path, "provider = \"SECRET_CONFIG_VALUE\" invalid\n").unwrap();
        let error = Config::read(&path).unwrap_err();
        let report = format!("{error:#}");
        assert!(
            report.contains(path.to_str().unwrap()),
            "Missing config path: {report}"
        );
        assert!(
            report.contains("line 1"),
            "Missing source location: {report}"
        );
        assert!(
            !report.contains("SECRET_CONFIG_VALUE"),
            "Config contents leaked: {report}"
        );
        std::fs::write(&path, "provider = \"legacy\"\n[providers.legacy]\nprotocol = \"SECRET_PROTOCOL_VALUE\"\nbase_url = \"https://example.test\"\nmodel = \"fixture\"\n").unwrap();
        let report = format!("{:#}", Config::read(&path).unwrap_err());
        assert!(report.contains("line 3"));
        assert!(report.contains("Unsupported configuration choice"));
        assert!(!report.contains("SECRET_PROTOCOL_VALUE"));
    }
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
