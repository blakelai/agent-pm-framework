//! Synthetic protocol observations only; no provider certification.
use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use pm_agent::{
    config::{Config, McpConfig, Protocol, ProviderConfig},
    probe::{self, State as CheckState},
    provider::{Event, Usage},
    workspace::{self, Workspace},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
enum Mode {
    Good,
    NoUsage,
    WrongText,
    WrongArgs,
    WrongReceipt,
    ExtraCalls,
    Http(u16),
    NotSse,
    Truncated,
    Slow,
}
type Requests = Arc<Mutex<Vec<Value>>>;
#[derive(Clone)]
struct Service {
    protocol: Protocol,
    mode: Mode,
    requests: Requests,
}
async fn serve(protocol: Protocol, mode: Mode) -> (String, Requests, tokio::task::JoinHandle<()>) {
    let requests = Arc::new(Mutex::new(vec![]));
    async fn handler(
        State(s): State<Service>,
        Json(body): Json<Value>,
    ) -> axum::response::Response {
        s.requests.lock().unwrap().push(body.clone());
        if let Mode::Http(code) = s.mode {
            return (StatusCode::from_u16(code).unwrap(), "SENSITIVE_ERROR_BODY").into_response();
        }
        if matches!(s.mode, Mode::Slow) {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        }
        if matches!(s.mode, Mode::NotSse) {
            return Json(json!({"secret":"SENSITIVE_NON_SSE"})).into_response();
        }
        let history = if s.protocol == Protocol::Responses {
            &body["input"]
        } else {
            &body["messages"]
        };
        let user = history
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["role"] == "user")
            .unwrap()["content"]
            .as_str()
            .unwrap();
        let result = history
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["role"] == "tool" || v["type"] == "function_call_output");
        let mut call = None;
        let text;
        if let Some(result) = result {
            let output = if s.protocol == Protocol::Responses {
                &result["output"]
            } else {
                &result["content"]
            };
            let receipt: Value = serde_json::from_str(output.as_str().unwrap()).unwrap();
            text = if matches!(s.mode, Mode::WrongReceipt) {
                "SENSITIVE_WRONG_RECEIPT".into()
            } else {
                receipt["receipt"].as_str().unwrap().into()
            };
            if s.protocol == Protocol::Responses {
                assert!(
                    history
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|v| v["type"] == "reasoning"
                            && v["encrypted_content"] == "opaque-synthetic")
                );
            }
        } else if body.get("tools").is_some() {
            let schema = if s.protocol == Protocol::Responses {
                &body["tools"][0]["parameters"]
            } else {
                &body["tools"][0]["function"]["parameters"]
            };
            let nonce = schema["properties"]["nonce"]["enum"][0].as_str().unwrap();
            let args = if matches!(s.mode, Mode::WrongArgs) {
                json!({"nonce":"WRONG","secret":"SENSITIVE_ARGS"})
            } else {
                json!({"nonce":nonce})
            };
            call = Some(args);
            text = String::new();
        } else {
            text = if matches!(s.mode, Mode::WrongText) {
                "SENSITIVE_WRONG_TEXT".into()
            } else {
                user.strip_prefix("Return exactly this text: ")
                    .unwrap()
                    .into()
            };
        }
        let mut usage = if s.protocol == Protocol::Responses {
            json!({"input_tokens":0,"output_tokens":2})
        } else {
            json!({"prompt_tokens":0,"completion_tokens":2})
        };
        if matches!(s.mode, Mode::NoUsage) {
            usage = Value::Null;
        }
        let data = if s.protocol == Protocol::Responses {
            let mut output =
                vec![json!({"type":"reasoning","encrypted_content":"opaque-synthetic"})];
            if let Some(args) = call {
                output.push(json!({"type":"function_call","call_id":"probe-call","name":"pm_probe_echo","arguments":args.to_string()}));
                if matches!(s.mode, Mode::ExtraCalls) {
                    output.push(json!({"type":"function_call","call_id":"other","name":"pm_probe_echo","arguments":"{}"}));
                }
            } else {
                output.push(json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}));
            }
            let delta = format!(
                "data: {}\n\n",
                json!({"type":"response.output_text.delta","delta":text})
            );
            if matches!(s.mode, Mode::Truncated) {
                delta
            } else {
                format!(
                    "{delta}data: {}\n\n",
                    json!({"type":"response.completed","response":{"status":"completed","output":output,"usage":usage}})
                )
            }
        } else {
            let mut delta = json!({"content":text});
            let finish = if let Some(args) = call {
                let mut calls = vec![
                    json!({"index":0,"id":"probe-call","function":{"name":"pm_probe_echo","arguments":args.to_string()}}),
                ];
                if matches!(s.mode, Mode::ExtraCalls) {
                    calls.push(json!({"index":1,"id":"other","function":{"name":"dangerous_command","arguments":"{}"}}));
                }
                delta["tool_calls"] = json!(calls);
                "tool_calls"
            } else {
                "stop"
            };
            let data = format!(
                "data: {}\n\n",
                json!({"choices":[{"delta":delta,"finish_reason":finish}],"usage":usage})
            );
            if matches!(s.mode, Mode::Truncated) {
                data
            } else {
                format!("{data}data: [DONE]\n\n")
            }
        };
        ([("content-type", "text/event-stream")], data).into_response()
    }
    let app = Router::new()
        .route("/v1/responses", post(handler))
        .route("/v1/chat/completions", post(handler))
        .with_state(Service {
            protocol,
            mode,
            requests: requests.clone(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}/v1"), requests, task)
}
fn config(url: String, protocol: Protocol) -> Config {
    Config {
        provider: "fixture".into(),
        max_steps: 32,
        providers: BTreeMap::from([(
            "fixture".into(),
            ProviderConfig {
                protocol,
                base_url: url,
                model: "fixture-model".into(),
                api_key_env: None,
                tools: true,
                request_usage: true,
                timeout_seconds: 5,
            },
        )]),
        mcp: BTreeMap::from([(
            "never-start".into(),
            McpConfig::Stdio {
                command: "/this/must/not/start".into(),
                args: vec![],
                env_names: vec![],
                allowed_tools: vec![],
            },
        )]),
    }
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    workspace::atomic_write(
        &dir.path().join("00_Governance/Project.md"),
        workspace::note(
            json!({"type":"project","id":"PROJECT-TEST","title":"Private project","language":"en"}),
            "# PRIVATE_PROJECT_SENTINEL",
        )
        .unwrap()
        .as_bytes(),
    )
    .unwrap();
    let w = Workspace::open(dir.path()).unwrap();
    (dir, w)
}
#[tokio::test]
async fn both_adapters_observe_text_calls_receipts_and_only_reported_usage() {
    for protocol in [Protocol::ChatCompletions, Protocol::Responses] {
        let (url, requests, server) = serve(protocol.clone(), Mode::Good).await;
        let cfg = config(url, protocol);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let r = probe::run(&cfg, None, &tx, &CancellationToken::new())
            .await
            .unwrap();
        assert!(r.passed());
        assert!(!r.cancelled());
        assert_eq!(requests.lock().unwrap().len(), 3);
        for check in &r.checks {
            assert_eq!(
                check.usage,
                Some(Usage {
                    input_tokens: Some(0),
                    output_tokens: Some(2),
                    total_tokens: None
                })
            );
            assert!(check.request_attempted);
            assert!(check.elapsed_ms.is_some());
        }
        assert!(r.checks[0].text_deltas > 0);
        assert!(std::iter::from_fn(|| rx.try_recv().ok()).all(|e| matches!(e, Event::Notice(_))));
        let serialized = serde_json::to_string(&r).unwrap();
        for forbidden in [
            "PM_PROBE_TEXT_",
            "PM_PROBE_RECEIPT_",
            "opaque-synthetic",
            "PRIVATE_PROJECT_SENTINEL",
            "/this/must/not/start",
        ] {
            assert!(!serialized.contains(forbidden));
        }
        server.abort();
    }
}
#[tokio::test]
async fn missing_usage_and_disabled_tools_are_unknown_not_failed_capabilities() {
    let (url, requests, server) = serve(Protocol::ChatCompletions, Mode::NoUsage).await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    let p = cfg.providers.get_mut("fixture").unwrap();
    p.tools = false;
    p.request_usage = false;
    let r = probe::run(
        &cfg,
        None,
        &tokio::sync::mpsc::unbounded_channel().0,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(r.passed());
    assert!(r.checks[0].usage.is_none());
    assert_eq!(r.checks[1].state, CheckState::Skipped);
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(requests.lock().unwrap()[0].get("stream_options").is_none());
    let o = probe::Observation {
        report: r,
        configuration_matches: Some(true),
        path: "private".into(),
    };
    assert!(o.markdown().contains("not provided"));
    assert!(o.markdown().contains("Tools disabled"));
    server.abort();
}
#[tokio::test]
async fn mismatched_calls_and_receipts_never_dispatch_any_real_tools() {
    for (mode, expected, failed) in [
        (Mode::WrongText, 1, 0),
        (Mode::WrongArgs, 2, 1),
        (Mode::ExtraCalls, 2, 1),
        (Mode::WrongReceipt, 3, 2),
    ] {
        let (url, requests, server) = serve(Protocol::ChatCompletions, mode).await;
        let r = probe::run(
            &config(url, Protocol::ChatCompletions),
            None,
            &tokio::sync::mpsc::unbounded_channel().0,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!r.passed());
        assert_eq!(r.checks[failed].state, CheckState::Failed);
        assert_eq!(requests.lock().unwrap().len(), expected);
        assert!(!serde_json::to_string(&r).unwrap().contains("SENSITIVE"));
        server.abort();
    }
}
#[tokio::test]
async fn errors_do_not_retry_fallback_or_persist_service_payloads() {
    for mode in [
        Mode::Http(401),
        Mode::Http(429),
        Mode::Http(400),
        Mode::NotSse,
        Mode::Truncated,
    ] {
        let (url, requests, server) = serve(Protocol::Responses, mode).await;
        let r = probe::run(
            &config(url, Protocol::Responses),
            None,
            &tokio::sync::mpsc::unbounded_channel().0,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert!(!r.passed());
        assert_eq!(r.checks[0].state, CheckState::Failed);
        assert_eq!(r.checks[1].state, CheckState::Skipped);
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert!(r.checks[0].usage.is_none());
        assert!(!serde_json::to_string(&r).unwrap().contains("SENSITIVE"));
        server.abort();
    }
}
#[tokio::test]
async fn cancellation_and_timeout_stop_without_a_followup_request() {
    let (url, requests, server) = serve(Protocol::ChatCompletions, Mode::Slow).await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let tx = tokio::sync::mpsc::unbounded_channel().0;
    let r = tokio::join!(probe::run(&cfg, None, &tx, &cancel), async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while requests.lock().unwrap().is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        signal.cancel();
    })
    .0
    .unwrap();
    assert!(r.cancelled());
    assert!(!r.passed());
    assert_eq!(requests.lock().unwrap().len(), 1);
    let before = requests.lock().unwrap().len();
    let r = probe::run(
        &cfg,
        None,
        &tokio::sync::mpsc::unbounded_channel().0,
        &cancel,
    )
    .await
    .unwrap();
    assert!(r.cancelled());
    assert!(!r.checks[0].request_attempted);
    assert_eq!(requests.lock().unwrap().len(), before);
    cfg.providers.get_mut("fixture").unwrap().timeout_seconds = 1;
    let r = probe::run(
        &cfg,
        None,
        &tokio::sync::mpsc::unbounded_channel().0,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(r.checks[0].state, CheckState::Failed);
    assert_eq!(requests.lock().unwrap().len(), before + 1);
    server.abort();
}
#[tokio::test]
async fn local_records_are_bound_to_config_and_cannot_reuse_corrupt_or_linked_results() {
    let (_dir, w) = fixture();
    let (url, requests, server) = serve(Protocol::ChatCompletions, Mode::Good).await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    let r = probe::run(
        &cfg,
        None,
        &tokio::sync::mpsc::unbounded_channel().0,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    let path = probe::save(&w, &r).unwrap();
    assert_eq!(
        probe::last(&w, &cfg, None).unwrap().configuration_matches,
        Some(true)
    );
    let input = w.read("00_Governance/Project.md").unwrap();
    assert!(input.contains("PRIVATE_PROJECT_SENTINEL"));
    assert!(
        !requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.to_string().contains("PRIVATE_PROJECT_SENTINEL"))
    );
    cfg.providers.get_mut("fixture").unwrap().model = "changed-model".into();
    let last = probe::last(&w, &cfg, None).unwrap();
    assert_eq!(last.configuration_matches, Some(false));
    assert!(last.markdown().contains("historical result only"));
    assert_eq!(requests.lock().unwrap().len(), 3);
    workspace::atomic_write(&w.path(&path).unwrap(), b"broken").unwrap();
    assert!(probe::last(&w, &cfg, None).is_err());
    std::fs::remove_file(w.path(&path).unwrap()).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(w.root.join("00_Governance/Project.md"), w.root.join(&path))
            .unwrap();
        assert!(probe::last(&w, &cfg, None).is_err());
        assert!(probe::save(&w, &r).is_err());
    }
    server.abort();
}
#[tokio::test]
async fn selected_profile_is_explicit_and_does_not_mutate_active_configuration() {
    let (url, requests, server) = serve(Protocol::ChatCompletions, Mode::Good).await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    let valid = cfg.providers["fixture"].clone();
    cfg.providers.get_mut("fixture").unwrap().model.clear();
    for name in ["openai", "ollama", "vllm", "cliproxyapi", "openrouter"] {
        cfg.providers.insert(name.into(), valid.clone());
    }
    let before = serde_json::to_value(&cfg).unwrap();
    let tx = tokio::sync::mpsc::unbounded_channel().0;
    assert!(
        probe::run(&cfg, None, &tx, &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(
        probe::run(&cfg, Some("missing"), &tx, &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(requests.lock().unwrap().is_empty());
    // Names do not grant capabilities; the selected configured adapter is exercised.
    let r = probe::run(&cfg, Some("openrouter"), &tx, &CancellationToken::new())
        .await
        .unwrap();
    assert!(r.passed());
    assert_eq!(r.profile, "openrouter");
    assert_eq!(serde_json::to_value(&cfg).unwrap(), before);
    server.abort();
}

#[tokio::test]
async fn missing_credentials_are_reported_without_exposing_names_or_dispatching_requests() {
    let (url, requests, server) = serve(Protocol::ChatCompletions, Mode::Good).await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    let name = format!("PM_PROBE_TEST_UNSET_{}", uuid::Uuid::new_v4().simple());
    cfg.providers.get_mut("fixture").unwrap().api_key_env = Some(name.clone());
    let r = probe::run(
        &cfg,
        None,
        &tokio::sync::mpsc::unbounded_channel().0,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(!r.passed());
    assert!(requests.lock().unwrap().is_empty());
    assert!(r.checks[0].detail.contains("missing or empty"));
    assert!(!serde_json::to_string(&r).unwrap().contains(&name));
    cfg.providers.get_mut("fixture").unwrap().timeout_seconds = 1000;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let r = probe::run(
        &cfg,
        None,
        &tokio::sync::mpsc::unbounded_channel().0,
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(r.request_timeout_seconds, 60);
    server.abort();
}
#[tokio::test]
async fn binary_probe_and_last_do_not_create_sessions_or_launch_configured_mcp() {
    let (_dir, w) = fixture();
    let (url, requests, server) = serve(Protocol::ChatCompletions, Mode::Good).await;
    let cfg = config(url, Protocol::ChatCompletions);
    let path = w.path(".pm-agent/config.toml").unwrap();
    workspace::atomic_write(&path, toml::to_string(&cfg).unwrap().as_bytes()).unwrap();
    let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .args(["--vault", w.root.to_str().unwrap(), "probe"])
        .output()
        .await
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["configuration_matches"], true);
    assert!(!w.root.join(".pm-agent/runtime.sqlite3").exists());
    let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .args(["--vault", w.root.to_str().unwrap(), "probe", "--last"])
        .output()
        .await
        .unwrap();
    assert!(result.status.success());
    assert_eq!(requests.lock().unwrap().len(), 3);
    server.abort();
}
