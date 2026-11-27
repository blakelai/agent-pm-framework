//! Explicit, isolated provider observations. Never executes project or MCP tools.
use crate::{
    config::{Config, Protocol, ProviderConfig, validate_endpoint},
    provider::{Completion, Event, Provider, ToolSpec, Usage},
    workspace::{self, Workspace},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Instant;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_util::sync::CancellationToken;

const SYSTEM: &str = "You are performing an isolated protocol capability check. Use only the exact synthetic instructions below. Do not use external knowledge or other tools. Return only the requested marker. After a tool result, return its receipt field exactly, without calling another tool.";
const TOOL: &str = "pm_probe_echo";
const MAX_SECONDS: u64 = 60;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Passed,
    Failed,
    Skipped,
    Cancelled,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub state: State,
    pub detail: String,
    pub request_attempted: bool,
    pub elapsed_ms: Option<u64>,
    pub text_deltas: usize,
    pub usage: Option<Usage>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub schema_version: u32,
    pub id: String,
    pub started_at: String,
    pub finished_at: String,
    pub profile: String,
    pub configured_model: String,
    pub protocol: Protocol,
    pub configuration_sha256: String,
    pub tools_configured: bool,
    pub request_usage_configured: bool,
    pub request_timeout_seconds: u64,
    pub checks: Vec<Check>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Observation {
    pub report: Report,
    /// None means the current selected configuration could not be read.
    pub configuration_matches: Option<bool>,
    pub path: String,
}
impl Report {
    pub fn passed(&self) -> bool {
        self.checks
            .first()
            .is_some_and(|c| c.state == State::Passed)
            && (!self.tools_configured || self.checks.iter().all(|c| c.state == State::Passed))
    }
    pub fn cancelled(&self) -> bool {
        self.checks.iter().any(|c| c.state == State::Cancelled)
    }
}
fn fingerprint(p: &ProviderConfig) -> Result<String> {
    Ok(workspace::hash(serde_json::to_string(p)?.as_bytes()))
}
fn profile<'a>(config: &'a Config, name: Option<&'a str>) -> Result<(&'a str, &'a ProviderConfig)> {
    let name = name.unwrap_or(&config.provider);
    Ok((
        name,
        config
            .providers
            .get(name)
            .context("Provider profile does not exist")?,
    ))
}
fn location(name: &str) -> String {
    format!(".pm-agent/probes/{}.json", workspace::hash(name.as_bytes()))
}
fn empty(name: &str, detail: &str) -> Check {
    Check {
        name: name.into(),
        state: State::Skipped,
        detail: detail.into(),
        request_attempted: false,
        elapsed_ms: None,
        text_deltas: 0,
        usage: None,
    }
}
fn failure(error: &anyhow::Error) -> String {
    // Only locally classified diagnostics survive; never copy service bodies,
    // arbitrary tool arguments, generated text, URLs or credential values.
    let message = error.to_string();
    if let Some(code) = message
        .strip_prefix("Provider HTTP ")
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse::<u16>().ok())
    {
        return format!(
            "HTTP {code}. Check endpoint, authentication, model and provider logs. Not retried."
        );
    }
    if message.starts_with("Set ") || message == "Configured API key is empty" {
        return "Configured API key environment variable is missing or empty.".into();
    }
    if message.contains("SSE") {
        return "The response did not satisfy the expected SSE protocol. Check the selected adapter and provider logs.".into();
    }
    if message.contains("completion")
        || message.contains("finish reason")
        || message.contains("incomplete")
    {
        return "The stream did not complete successfully. Partial output is not accepted.".into();
    }
    "Request failed or was interrupted. Check provider configuration and logs. No response body was retained; not retried.".into()
}

async fn request(
    provider: &Provider,
    history: &[Value],
    tools: &[ToolSpec],
    check: &mut Check,
    cancel: &CancellationToken,
) -> Option<Completion> {
    if cancel.is_cancelled() {
        check.state = State::Cancelled;
        check.detail = "Cancelled before request dispatch.".into();
        return None;
    }
    check.request_attempted = true;
    let started = Instant::now();
    let (tx, mut rx) = unbounded_channel();
    // Drain streaming text as it arrives; retain only observations, never payloads.
    let call = provider.complete(SYSTEM, history, tools, &tx, cancel);
    tokio::pin!(call);
    let result = loop {
        tokio::select! {
            result=&mut call=>break result,
            Some(event)=rx.recv()=>if matches!(event,Event::Text(_)) {check.text_deltas+=1;},
        }
    };
    while let Ok(event) = rx.try_recv() {
        if matches!(event, Event::Text(_)) {
            check.text_deltas += 1;
        }
    }
    check.elapsed_ms = Some(started.elapsed().as_millis().min(u64::MAX as u128) as u64);
    match result {
        Ok(c) => {
            check.usage = c.usage.clone();
            Some(c)
        }
        Err(e) => {
            check.state = if cancel.is_cancelled() {
                State::Cancelled
            } else {
                State::Failed
            };
            check.detail = if cancel.is_cancelled() {
                "Cancelled; the service may still have processed the request. No retry was made."
                    .into()
            } else {
                failure(&e)
            };
            None
        }
    }
}
fn observed(check: &mut Check, yes: bool, pass: &str, fail: &str) {
    check.state = if yes { State::Passed } else { State::Failed };
    check.detail = if yes { pass } else { fail }.into();
}

/// At most three synthetic requests using the selected adapter. No workspace access.
pub async fn run(
    config: &Config,
    name: Option<&str>,
    events: &UnboundedSender<Event>,
    cancel: &CancellationToken,
) -> Result<Report> {
    let (name, p) = profile(config, name)?;
    ensure!(
        !p.model.trim().is_empty(),
        "Set an explicit model; no model is inferred"
    );
    validate_endpoint(&p.base_url)?;
    ensure!(p.timeout_seconds > 0, "Timeout must be positive");
    let mut selected = p.clone();
    selected.timeout_seconds = selected.timeout_seconds.min(MAX_SECONDS);
    let provider = Provider::new(selected.clone())?;
    let mut report = Report {
        schema_version: 1,
        id: format!("PROBE-{}", uuid::Uuid::new_v4()),
        started_at: crate::records::timestamp(),
        finished_at: String::new(),
        profile: name.into(),
        configured_model: p.model.clone(),
        protocol: p.protocol.clone(),
        configuration_sha256: fingerprint(p)?,
        tools_configured: p.tools,
        request_usage_configured: p.request_usage,
        request_timeout_seconds: selected.timeout_seconds,
        checks: vec![
            empty("text_stream", "Not reached."),
            empty(
                "tool_call",
                if p.tools {
                    "Text check must pass first."
                } else {
                    "Tools disabled in the selected configuration; capability not tested."
                },
            ),
            empty(
                "tool_result",
                if p.tools {
                    "A valid synthetic tool call is required first."
                } else {
                    "Tools disabled in the selected configuration; capability not tested."
                },
            ),
        ],
    };
    let _=events.send(Event::Notice(format!("Provider probe: {name}; synthetic data only; at most {} requests, each at most {} seconds. Service charges may apply.",if p.tools {3} else {1},selected.timeout_seconds)));
    let marker = format!("PM_PROBE_TEXT_{}", uuid::Uuid::new_v4().simple());
    let history =
        vec![json!({"role":"user","content":format!("Return exactly this text: {marker}")})];
    let _ = events.send(Event::Notice("Probe 1/3: completed text stream.".into()));
    if let Some(c) = request(&provider, &history, &[], &mut report.checks[0], cancel).await {
        let yes = c.calls.is_empty() && c.text.trim() == marker && report.checks[0].text_deltas > 0;
        observed(
            &mut report.checks[0],
            yes,
            "Completed SSE text matched the synthetic marker.",
            "The expected text stream was not observed; this does not establish permanent lack of support.",
        );
        if yes && p.tools {
            let nonce = uuid::Uuid::new_v4().to_string();
            let spec = ToolSpec {
                name: TOOL.into(),
                description:
                    "Synthetic echo; the application returns a receipt without external effects."
                        .into(),
                parameters: json!({"type":"object","properties":{"nonce":{"type":"string","enum":[nonce]}},"required":["nonce"],"additionalProperties":false}),
            };
            let mut history = vec![
                json!({"role":"user","content":format!("Call {TOOL} exactly once with nonce {nonce}. After the tool result, return only its receipt field.")}),
            ];
            let _ = events.send(Event::Notice("Probe 2/3: synthetic function call.".into()));
            if let Some(c) = request(
                &provider,
                &history,
                std::slice::from_ref(&spec),
                &mut report.checks[1],
                cancel,
            )
            .await
            {
                let valid = c.calls.len() == 1
                    && c.calls[0].name == TOOL
                    && serde_json::from_str::<Value>(&c.calls[0].arguments)
                        .ok()
                        .is_some_and(|a| {
                            crate::tools::validate_arguments(&spec.parameters, &a).is_ok()
                        });
                observed(
                    &mut report.checks[1],
                    valid,
                    "One function call matched the exact synthetic schema.",
                    "Expected one matching synthetic function call; no returned calls were executed.",
                );
                if valid {
                    provider.append_completion(&mut history, &c);
                    let receipt = format!("PM_PROBE_RECEIPT_{}", uuid::Uuid::new_v4().simple());
                    provider.append_tool(
                        &mut history,
                        &c.calls[0],
                        &json!({"receipt":receipt}).to_string(),
                    );
                    let _ = events.send(Event::Notice(
                        "Probe 3/3: synthetic tool result continuation.".into(),
                    ));
                    if let Some(c) = request(
                        &provider,
                        &history,
                        std::slice::from_ref(&spec),
                        &mut report.checks[2],
                        cancel,
                    )
                    .await
                    {
                        observed(
                            &mut report.checks[2],
                            c.calls.is_empty() && c.text.trim() == receipt,
                            "Continuation returned the receipt supplied only in the synthetic tool result.",
                            "The expected tool-result receipt was not observed; no additional calls were executed.",
                        );
                    }
                }
            }
        }
    }
    report.finished_at = crate::records::timestamp();
    Ok(report)
}

pub fn save(w: &Workspace, report: &Report) -> Result<String> {
    let path = location(&report.profile);
    workspace::atomic_write(&w.path(&path)?, &serde_json::to_vec_pretty(report)?)?;
    Ok(path)
}
pub fn last(w: &Workspace, config: &Config, name: Option<&str>) -> Result<Observation> {
    let (name, p) = profile(config, name)?;
    let path = location(name);
    let file = w.path(&path)?;
    ensure!(
        file.metadata()
            .context("No saved probe for this profile; run an explicit probe first")?
            .len()
            <= workspace::MAX_FILE,
        "Probe record exceeds 2 MiB"
    );
    let report: Report = serde_json::from_slice(&std::fs::read(file)?)
        .context("Saved probe is unreadable; run a new explicit probe")?;
    ensure!(
        report.schema_version == 1
            && report.profile == name
            && report.checks.len() == 3
            && report.checks.iter().map(|c| c.name.as_str()).eq([
                "text_stream",
                "tool_call",
                "tool_result"
            ]),
        "Unsupported or inconsistent probe record; run a new explicit probe"
    );
    Ok(Observation {
        configuration_matches: Some(report.configuration_sha256 == fingerprint(p)?),
        report,
        path,
    })
}
pub fn current(path: &std::path::Path, report: Report, saved: String) -> Observation {
    let matches = Config::read(path)
        .ok()
        .and_then(|c| {
            c.providers
                .get(&report.profile)
                .and_then(|p| fingerprint(p).ok())
        })
        .map(|f| f == report.configuration_sha256);
    Observation {
        report,
        configuration_matches: matches,
        path: saved,
    }
}
impl Observation {
    pub fn markdown(&self) -> String {
        let r = &self.report;
        let mut text = format!(
            "# Provider probe\n\n- Profile: {}\n- Configured model: {}\n- Protocol: {:?}\n- Recorded: {}\n- Record: {}\n- Configuration match: {}\n\n",
            r.profile,
            r.configured_model,
            r.protocol,
            r.finished_at,
            self.path,
            match self.configuration_matches {
                Some(true) => "matches the currently loaded profile",
                Some(false) => "CHANGED; historical result only",
                None => "unavailable; historical result only",
            }
        );
        text.push_str("| Check | Observation | Detail |\n|---|---|---|\n");
        for c in &r.checks {
            text.push_str(&format!("| {} | {:?} | {} |\n", c.name, c.state, c.detail));
        }
        text.push_str("\n## Service-reported usage per request\n\n| Request | Input | Output | Total |\n|---|---|---|---|\n");
        for c in r.checks.iter().filter(|c| c.request_attempted) {
            let u = c.usage.clone().unwrap_or_default();
            let value = |v: Option<u64>| v.map(|n| n.to_string()).unwrap_or("not provided".into());
            text.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                c.name,
                value(u.input_tokens),
                value(u.output_tokens),
                value(u.total_tokens)
            ));
        }
        text.push_str("\nThese are observations from one run, not provider certification or a context/cost estimate. The configured model label does not prove a gateway's actual route. No project content or MCP tools were used. Profile settings and Agent sessions are unchanged. The latest local record replaces the previous probe for this profile.\n");
        text
    }
}
