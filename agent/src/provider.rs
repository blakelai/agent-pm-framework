use crate::config::{Protocol, ProviderConfig, validate_endpoint};
use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub enum Event {
    Text(String),
    Notice(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Call {
    pub id: String,
    pub name: String,
    pub arguments: String,
}
#[derive(Debug)]
pub struct Completion {
    pub text: String,
    pub calls: Vec<Call>,
    pub items: Vec<Value>,
}

pub struct Provider {
    config: ProviderConfig,
    client: reqwest::Client,
}
impl Provider {
    pub fn new(config: ProviderConfig) -> Result<Self> {
        validate_endpoint(&config.base_url)?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()?;
        Ok(Self { config, client })
    }
    pub fn request_body(&self, system: &str, history: &[Value], tools: &[ToolSpec]) -> Value {
        let mut body = match self.config.protocol {
            Protocol::Responses => {
                json!({"model":self.config.model,"instructions":system,"input":history,"store":false,"stream":true,"include":["reasoning.encrypted_content"]})
            }
            Protocol::ChatCompletions => {
                let mut messages = vec![json!({"role":"system","content":system})];
                messages.extend_from_slice(history);
                json!({"model":self.config.model,"messages":messages,"stream":true})
            }
        };
        if self.config.tools && !tools.is_empty() {
            body["tools"]=json!(tools.iter().map(|t| match self.config.protocol {
                Protocol::Responses => json!({"type":"function","name":t.name,"description":t.description,"parameters":t.parameters,"strict":false}),
                Protocol::ChatCompletions => json!({"type":"function","function":{"name":t.name,"description":t.description,"parameters":t.parameters}}),
            }).collect::<Vec<_>>());
            body["parallel_tool_calls"] = json!(false);
        }
        body
    }
    pub fn append_completion(&self, history: &mut Vec<Value>, c: &Completion) {
        match self.config.protocol {
            Protocol::Responses => history.extend(c.items.clone()),
            Protocol::ChatCompletions => {
                let mut message = json!({"role":"assistant","content":c.text});
                if !c.calls.is_empty() {
                    message["tool_calls"]=json!(c.calls.iter().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments}})).collect::<Vec<_>>());
                }
                history.push(message);
            }
        }
    }
    pub fn append_tool(&self, history: &mut Vec<Value>, call: &Call, result: &str) {
        history.push(match self.config.protocol {
            Protocol::Responses => {
                json!({"type":"function_call_output","call_id":call.id,"output":result})
            }
            Protocol::ChatCompletions => {
                json!({"role":"tool","tool_call_id":call.id,"content":result})
            }
        });
    }
    pub async fn complete(
        &self,
        system: &str,
        history: &[Value],
        tools: &[ToolSpec],
        events: &UnboundedSender<Event>,
        cancel: &CancellationToken,
    ) -> Result<Completion> {
        let endpoint = match self.config.protocol {
            Protocol::Responses => "responses",
            Protocol::ChatCompletions => "chat/completions",
        };
        let mut req = self
            .client
            .post(format!(
                "{}/{endpoint}",
                self.config.base_url.trim_end_matches('/')
            ))
            .json(&self.request_body(system, history, tools));
        if let Some(name) = &self.config.api_key_env {
            let key = std::env::var(name).with_context(|| {
                format!("Set {name} in the process environment; do not paste keys into chat")
            })?;
            ensure!(!key.trim().is_empty(), "Configured API key is empty");
            req = req.bearer_auth(key);
        }
        let response = tokio::select! { _=cancel.cancelled()=>bail!("Cancelled"), r=req.send()=>r.context("Provider connection failed")? };
        // Do not persist server error bodies: a gateway may echo credentials or prompts.
        ensure!(
            response.status().is_success(),
            "Provider HTTP {}; check endpoint, model, key and provider logs. Request was not retried.",
            response.status()
        );
        ensure!(
            response
                .headers()
                .get("content-type")
                .is_some_and(|v| v.to_str().unwrap_or("").contains("text/event-stream")),
            "Provider did not return an SSE stream; verify protocol support"
        );
        let mut bytes = response.bytes_stream();
        let mut sse = Sse::default();
        let mut decoded = Decoder::new(self.config.protocol.clone());
        let mut size = 0;
        loop {
            let part = tokio::select! { _=cancel.cancelled()=>bail!("Cancelled"), item=bytes.next()=>item };
            let Some(part) = part else {
                break;
            };
            let part =
                part.context("Provider stream interrupted; partial output is not completed")?;
            size += part.len();
            ensure!(size <= 16 * 1024 * 1024, "Provider output exceeded 16 MiB");
            for data in sse.feed(&part)? {
                let text = decoded.accept(&data)?;
                if !text.is_empty() {
                    let _ = events.send(Event::Text(text));
                }
            }
            if decoded.finished {
                break;
            }
        }
        let completion = decoded.finish()?;
        ensure!(
            self.config.tools || completion.calls.is_empty(),
            "Provider emitted tool calls while tools are disabled"
        );
        Ok(completion)
    }
}

#[derive(Default)]
pub struct Sse {
    buffer: Vec<u8>,
}
impl Sse {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        self.buffer.extend_from_slice(bytes);
        ensure!(self.buffer.len() < 8 * 1024 * 1024, "SSE event too large");
        let mut output = vec![];
        loop {
            let lf = self
                .buffer
                .windows(2)
                .position(|s| s == b"\n\n")
                .map(|p| (p, 2));
            let crlf = self
                .buffer
                .windows(4)
                .position(|s| s == b"\r\n\r\n")
                .map(|p| (p, 4));
            let Some((pos, n)) = lf.into_iter().chain(crlf).min_by_key(|(p, _)| *p) else {
                break;
            };
            let raw = self.buffer.drain(..pos + n).collect::<Vec<_>>();
            let text = std::str::from_utf8(&raw).context("Invalid UTF-8 SSE event")?;
            let data = text
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("data:")
                        .map(|s| s.strip_prefix(' ').unwrap_or(s))
                })
                .collect::<Vec<_>>()
                .join("\n");
            if !data.is_empty() {
                output.push(data);
            }
        }
        Ok(output)
    }
}
pub struct Decoder {
    protocol: Protocol,
    text: String,
    calls: BTreeMap<usize, Call>,
    items: Vec<Value>,
    finished: bool,
    finish_reason: Option<String>,
}
impl Decoder {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            text: String::new(),
            calls: BTreeMap::new(),
            items: vec![],
            finished: false,
            finish_reason: None,
        }
    }
    pub fn accept(&mut self, data: &str) -> Result<String> {
        if data == "[DONE]" {
            ensure!(
                self.protocol == Protocol::ChatCompletions,
                "Unexpected stream terminator"
            );
            self.finished = true;
            return Ok(String::new());
        }
        let event: Value = serde_json::from_str(data).context("Malformed provider SSE JSON")?;
        ensure!(
            event.get("error").is_none(),
            "Provider stream returned an error; inspect provider logs"
        );
        let mut delta = String::new();
        match self.protocol {
            Protocol::Responses => match event["type"].as_str().unwrap_or("") {
                "response.output_text.delta" => {
                    delta = event["delta"].as_str().unwrap_or("").into();
                }
                "response.completed" => {
                    ensure!(
                        event["response"]["status"] == "completed",
                        "Response did not complete"
                    );
                    self.items = event["response"]["output"]
                        .as_array()
                        .context("Completed response has no output")?
                        .clone();
                    for (i, item) in self.items.iter().enumerate() {
                        if item["type"] == "function_call" {
                            self.calls.insert(
                                i,
                                Call {
                                    id: item["call_id"].as_str().context("Missing call_id")?.into(),
                                    name: item["name"]
                                        .as_str()
                                        .context("Missing tool name")?
                                        .into(),
                                    arguments: item["arguments"]
                                        .as_str()
                                        .context("Missing tool arguments")?
                                        .into(),
                                },
                            );
                        }
                    }
                    self.finished = true;
                }
                "response.failed" | "response.incomplete" | "error" => {
                    bail!("Provider response failed or was incomplete; no tools were executed")
                }
                _ => {}
            },
            Protocol::ChatCompletions => {
                if let Some(choice) = event["choices"].as_array().and_then(|a| a.first()) {
                    delta = choice["delta"]["content"].as_str().unwrap_or("").into();
                    if let Some(reason) = choice["finish_reason"].as_str() {
                        self.finish_reason = Some(reason.into());
                    }
                    if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
                        for call in calls {
                            let index =
                                call["index"].as_u64().context("Missing tool call index")? as usize;
                            let c = self.calls.entry(index).or_insert_with(|| Call {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                            });
                            if let Some(id) = call["id"].as_str() {
                                c.id.push_str(id);
                            }
                            if let Some(name) = call["function"]["name"].as_str() {
                                c.name.push_str(name);
                            }
                            if let Some(args) = call["function"]["arguments"].as_str() {
                                c.arguments.push_str(args);
                            }
                        }
                    }
                }
            }
        }
        self.text.push_str(&delta);
        Ok(delta)
    }
    pub fn finish(self) -> Result<Completion> {
        ensure!(
            self.finished,
            "Provider stream ended before completion; partial output is not accepted"
        );
        if self.protocol == Protocol::ChatCompletions {
            ensure!(
                matches!(self.finish_reason.as_deref(), Some("stop" | "tool_calls")),
                "Provider stopped without a successful finish reason"
            );
        }
        let calls = self.calls.into_values().collect::<Vec<_>>();
        let mut ids = std::collections::HashSet::new();
        for c in &calls {
            ensure!(
                !c.id.is_empty() && !c.name.is_empty() && ids.insert(&c.id),
                "Malformed or duplicate tool call"
            );
        }
        Ok(Completion {
            text: self.text,
            calls,
            items: self.items,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sse_handles_byte_fragmentation_and_crlf() {
        let mut s = Sse::default();
        let mut events = vec![];
        for b in ":heartbeat\r\n\r\ndata: 中文\r\n\r\ndata: x\ndata: y\n\n".as_bytes() {
            events.extend(s.feed(&[*b]).unwrap());
        }
        assert_eq!(events, vec!["中文", "x\ny"]);
    }
    #[test]
    fn fragmented_tool_calls_and_truncation() {
        let mut d = Decoder::new(Protocol::ChatCompletions);
        d.accept(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read","arguments":"{\"path\":"}}]}}]}).to_string()).unwrap();
        d.accept(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"x\"}"}}]},"finish_reason":"tool_calls"}]}).to_string()).unwrap();
        d.accept("[DONE]").unwrap();
        assert_eq!(d.finish().unwrap().calls[0].arguments, "{\"path\":\"x\"}");
        let mut d = Decoder::new(Protocol::ChatCompletions);
        d.accept("{\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}")
            .unwrap();
        assert!(d.finish().is_err());
    }
    #[test]
    fn responses_retains_reasoning_for_stateless_followup() {
        let mut d = Decoder::new(Protocol::Responses);
        d.accept(&json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"reasoning","encrypted_content":"opaque"},{"type":"function_call","call_id":"c1","name":"read","arguments":"{}"}]}}).to_string()).unwrap();
        let c = d.finish().unwrap();
        assert_eq!(c.items[0]["encrypted_content"], "opaque");
        assert_eq!(c.calls.len(), 1);
    }
}
