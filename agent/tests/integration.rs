use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use pm_agent::{
    config::{Config, McpConfig, Protocol, ProviderConfig},
    mcp::Mcp,
    provider::{Event, Provider},
    runtime::{Runtime, Session, Status},
    tools,
    workspace::{Workspace, hash, note},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

type Queue = Arc<Mutex<VecDeque<String>>>;

#[tokio::test]
async fn explicit_custom_skill_is_injected_audited_and_usage_is_service_reported() {
    let (_dir, w) = fixture();
    let skill = ".agents/skills/risk-review/SKILL.md";
    let reference = ".agents/skills/risk-review/references/check.md";
    pm_agent::workspace::atomic_write(&w.path(skill).unwrap(),b"---\nname: risk-review\ndescription: Review supplied risk evidence\npm_agent:\n  version: '1.0'\n  references: [references/check.md]\n---\n# Risk review\nCUSTOM_SKILL_SENTINEL. Ask for absent evidence.").unwrap();
    pm_agent::workspace::atomic_write(&w.path(reference).unwrap(), b"REFERENCE_SENTINEL").unwrap();
    let response=concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Inspected.\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":44,\"completion_tokens\":5,\"total_tokens\":49}}\n\n",
        "data: [DONE]\n\n"
    ).to_string();
    let (url, _, requests, server) =
        server(vec![response, chat("No usage supplied.", vec![])]).await;
    let cfg = config(url, Protocol::ChatCompletions);
    let runtime = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    runtime
        .input(&mut s, "$risk-review Review the supplied risks.")
        .unwrap();
    let (tx, _) = events();
    runtime
        .run(&mut s, None, &tx, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed);
    let request = requests.lock().unwrap()[0].clone();
    assert_eq!(request["stream_options"]["include_usage"], true);
    assert!(
        request["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("CUSTOM_SKILL_SENTINEL")
    );
    assert!(
        request["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("REFERENCE_SENTINEL")
    );
    assert_eq!(s.usage.as_ref().unwrap().input_tokens, Some(44));
    assert_eq!(s.selected_skills, ["risk-review"]);
    let loaded: Session = runtime.store.load(&s.id).unwrap();
    let stamp = loaded.skill_history[0]
        .skills
        .iter()
        .find(|skill| skill.name == "risk-review")
        .unwrap();
    assert_eq!(stamp.version.as_deref(), Some("1.0"));
    assert_eq!(stamp.files[1].sha256, hash(b"REFERENCE_SENTINEL"));
    assert!(
        w.read(&format!("90_Agent/Runs/RUN-{}.md", s.id))
            .unwrap()
            .contains(&stamp.files[1].sha256)
    );
    runtime
        .input(&mut s, "Continue with evidence only.")
        .unwrap();
    runtime
        .run(&mut s, None, &tx, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.usage, None);
    server.abort();
}

#[tokio::test]
async fn changed_skill_blocks_pending_tool_and_missing_skill_sends_no_request() {
    let (_dir, w) = fixture();
    let (url, _, requests, server) = server(vec![chat(
        "",
        vec![call(
            0,
            "import",
            "pm",
            json!({"command":"repo-add","args":["--help","unexpected"]}),
        )],
    )])
    .await;
    let cfg = config(url, Protocol::ChatCompletions);
    let runtime = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    assert!(runtime.input(&mut s, "$does-not-exist Review").is_err());
    assert!(requests.lock().unwrap().is_empty());
    runtime
        .input(&mut s, "Inspect the current project.")
        .unwrap();
    let (tx, _) = events();
    runtime
        .run(&mut s, None, &tx, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::WaitingApproval);
    let path = ".agents/skills/pm-prd/SKILL.md";
    let mut text = w.read(path).unwrap();
    text.push_str("\nChanged instructions.\n");
    pm_agent::workspace::atomic_write(&w.path(path).unwrap(), text.as_bytes()).unwrap();
    let approved = s.pending[0].id.clone();
    runtime.decision(&mut s, true).unwrap();
    runtime
        .run(&mut s, Some(approved), &tx, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Failed);
    assert!(s.error.as_ref().unwrap().contains("Skills changed"));
    assert_eq!(requests.lock().unwrap().len(), 1);
    server.abort();
}

#[test]
fn invalid_skill_is_visible_without_breaking_document_browsing() {
    let (_dir, w) = fixture();
    pm_agent::workspace::atomic_write(
        &w.path(".agents/skills/broken/SKILL.md").unwrap(),
        b"missing metadata",
    )
    .unwrap();
    assert!(
        w.documents()
            .unwrap()
            .iter()
            .any(|d| d.kind == "Invalid Skill")
    );
    let c = pm_agent::skills::Catalog::discover(&w).unwrap();
    assert!(c.errors.contains_key("broken"));
    assert!(c.resolve(&["pm-brd".into()]).is_ok());
}

type Requests = Arc<Mutex<Vec<Value>>>;
async fn server(responses: Vec<String>) -> (String, Queue, Requests, tokio::task::JoinHandle<()>) {
    let queue = Arc::new(Mutex::new(VecDeque::from(responses)));
    let requests = Arc::new(Mutex::new(vec![]));
    async fn handler(
        State((q, r)): State<(Queue, Requests)>,
        Json(input): Json<Value>,
    ) -> impl IntoResponse {
        r.lock().unwrap().push(input);
        let data = q
            .lock()
            .unwrap()
            .pop_front()
            .expect("Unexpected provider request");
        ([("content-type", "text/event-stream")], data)
    }
    let app = Router::new()
        .route("/v1/chat/completions", post(handler))
        .route("/v1/responses", post(handler))
        .with_state((queue.clone(), requests.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}/v1"), queue, requests, task)
}
fn chat(text: &str, calls: Vec<Value>) -> String {
    let reason = if calls.is_empty() {
        "stop"
    } else {
        "tool_calls"
    };
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"content":text,"tool_calls":calls},"finish_reason":reason}]})
    )
}
fn call(index: usize, id: &str, name: &str, args: Value) -> Value {
    json!({"index":index,"id":id,"function":{"name":name,"arguments":args.to_string()}})
}
fn config(url: String, protocol: Protocol) -> Config {
    Config {
        provider: "fixture".into(),
        max_steps: 12,
        mcp: BTreeMap::new(),
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
    }
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for entry in walkdir::WalkDir::new(root).into_iter().filter_entry(|e| {
        e.depth() == 0
            || !matches!(
                e.file_name().to_str().unwrap_or(""),
                ".git" | "agent" | "target" | ".pm-agent" | "__pycache__" | ".venv"
            )
    }) {
        let e = entry.unwrap();
        let rel = e.path().strip_prefix(root).unwrap();
        let dest = dir.path().join(rel);
        if e.file_type().is_dir() {
            std::fs::create_dir_all(dest).unwrap();
        } else if e.file_type().is_file() {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
    let workspace = Workspace::open(dir.path()).unwrap();
    (dir, workspace)
}
fn events() -> (
    tokio::sync::mpsc::UnboundedSender<Event>,
    tokio::sync::mpsc::UnboundedReceiver<Event>,
) {
    tokio::sync::mpsc::unbounded_channel()
}

#[tokio::test]
async fn clarification_prevents_parallel_write_and_resumes_after_actual_answer() {
    let (_dir, w) = fixture();
    let (url,q,_,server)=server(vec![chat("",vec![call(0,"write","write_draft",json!({})),call(1,"ask","ask_clarification",json!({"question":"Which approval owner is responsible?","body":"The source does not name an owner. Provide the owner before generating the PRD."}))])]).await;
    let cfg = config(url, Protocol::ChatCompletions);
    let runtime = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut session = Session::new(&cfg, "prd").unwrap();
    runtime
        .input(&mut session, "Create a PRD from the project charter.")
        .unwrap();
    let (tx, mut rx) = events();
    runtime
        .run(&mut session, None, &tx, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(session.status, Status::WaitingClarification);
    assert_eq!(session.questions.len(), 1);
    assert!(session.pending.is_empty());
    let displayed = std::iter::from_fn(|| rx.try_recv().ok())
        .any(|e| matches!(e,Event::Notice(t) if t.contains("Which approval owner")));
    assert!(displayed);
    let question = session.questions[0].path.clone();
    let id = session.id.clone();
    drop(runtime);
    let runtime = Runtime::open(w.clone(), cfg).await.unwrap();
    let mut session: Session = runtime.store.load(&id).unwrap();
    runtime
        .input(
            &mut session,
            "The approval owner is the designated service manager; no individual has been named.",
        )
        .unwrap();
    q.lock().unwrap().push_back(chat("",vec![call(0,"resolve","resolve_clarification",json!({"path":question,"resolution":"The user identified the responsible role; the individual's name remains unspecified."}))]));
    q.lock().unwrap().push_back(chat(
        "The responsible role was recorded; no individual was invented.",
        vec![],
    ));
    runtime
        .run(&mut session, None, &tx, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(session.status, Status::Completed);
    assert!(session.questions[0].resolved);
    assert!(w.read(&question).unwrap().contains("answer_source"));
    server.abort();
}

#[tokio::test]
async fn tool_roundtrip_creates_a_sourced_draft_and_preserves_hashes() {
    let (_dir, w) = fixture();
    let source = w.read("00_Governance/Project.md").unwrap();
    let draft = note(
        json!({"id":"PRD-TEST","type":"prd","title":"Draft","doc_status":"draft","language":"en"}),
        "# Draft\n\nThe charter requires confirmed project inputs.",
    )
    .unwrap();
    let (url,_,requests,server)=server(vec![chat("",vec![call(0,"read","read_document",json!({"path":"00_Governance/Project.md"}))]),chat("",vec![call(0,"write","write_draft",json!({"path":"04_Requirements/PRD/PRD-TEST.md","content":draft,"expected_hash":null,"citations":[{"path":"00_Governance/Project.md","sha256":hash(source.as_bytes())}]}))]),chat("Draft saved; semantic review remains necessary.",vec![])]).await;
    let cfg = config(url, Protocol::ChatCompletions);
    let runtime = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    runtime
        .input(&mut s, "Create a draft grounded in the charter.")
        .unwrap();
    runtime
        .run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed);
    assert!(
        w.read("04_Requirements/PRD/PRD-TEST.md")
            .unwrap()
            .contains("evidence_paths")
    );
    let req = requests.lock().unwrap();
    assert!(
        req[1]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "tool" && m["tool_call_id"] == "read")
    );
    server.abort();
}

#[tokio::test]
async fn responses_adapter_replays_reasoning_and_tool_output() {
    let response = json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"reasoning","id":"r1","encrypted_content":"opaque"},{"type":"function_call","call_id":"call1","name":"list_documents","arguments":"{}"}]}});
    let final_response = json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}]}});
    let (url, _, requests, server) = server(vec![
        format!("data: {response}\n\n"),
        format!(
            "data: {}\n\ndata: {final_response}\n\n",
            json!({"type":"response.output_text.delta","delta":"Done"})
        ),
    ])
    .await;
    let provider =
        Provider::new(config(url, Protocol::Responses).active().unwrap().clone()).unwrap();
    let mut history = vec![json!({"role":"user","content":"List documents"})];
    let c = provider
        .complete(
            "Test",
            &history,
            &tools::specs(),
            &events().0,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    provider.append_completion(&mut history, &c);
    provider.append_tool(&mut history, &c.calls[0], "[]");
    let final_c = provider
        .complete(
            "Test",
            &history,
            &tools::specs(),
            &events().0,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(final_c.text, "Done");
    let req = requests.lock().unwrap();
    assert_eq!(req[1]["store"], false);
    assert_eq!(req[1]["input"][1]["encrypted_content"], "opaque");
    assert_eq!(req[1]["input"][3]["call_id"], "call1");
    server.abort();
}

#[tokio::test]
async fn interrupted_operation_is_not_replayed_on_resume() {
    let (_dir, w) = fixture();
    let (url, _, requests, server) = server(vec![]).await;
    let cfg = config(url, Protocol::ChatCompletions);
    let runtime = Runtime::open(w, cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "workflow").unwrap();
    s.pending.push(pm_agent::provider::Call {
        id: "uncertain".into(),
        name: "pm".into(),
        arguments: json!({"command":"status","args":[]}).to_string(),
    });
    runtime
        .store
        .begin_call(&s.id, "uncertain", "pm", "{}")
        .unwrap();
    runtime
        .run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::NeedsReconciliation);
    assert!(requests.lock().unwrap().is_empty());
    runtime
        .reconcile(
            &mut s,
            "The report was generated; inspected the file locally.",
        )
        .unwrap();
    assert!(s.pending.is_empty());
    assert_eq!(s.status, Status::Ready);
    server.abort();
}

#[tokio::test]
async fn approval_denial_does_not_execute_the_tool() {
    let (_dir, w) = fixture();
    let (url, q, _, server) = server(vec![chat(
        "",
        vec![call(
            0,
            "settings",
            "pm",
            json!({"command":"settings","args":["--document-language","zh-TW"]}),
        )],
    )])
    .await;
    let cfg = config(url, Protocol::ChatCompletions);
    let runtime = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "workflow").unwrap();
    runtime.input(&mut s, "Review language settings.").unwrap();
    runtime
        .run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::WaitingApproval);
    runtime.decision(&mut s, false).unwrap();
    q.lock()
        .unwrap()
        .push_back(chat("The requested change was denied.", vec![]));
    runtime
        .run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(w.language().unwrap(), "en");
    server.abort();
}

#[tokio::test]
async fn mcp_stdio_discovers_only_allowlisted_tools_and_calls_them() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mcp_fixture.py");
    let cfg = BTreeMap::from([(
        "fixture".into(),
        McpConfig::Stdio {
            command: std::env::var("PM_TEST_PYTHON").unwrap_or("python3".into()),
            args: vec![script.to_string_lossy().into()],
            env_names: vec![],
            allowed_tools: vec!["echo".into()],
        },
    )]);
    let client = Mcp::connect(&cfg).await.unwrap();
    assert_eq!(client.specs().len(), 1);
    let value = client
        .call("mcp_fixture_echo", json!({"text":"fixture"}))
        .await
        .unwrap();
    assert_eq!(value["content"][0]["text"], "fixture");
    assert!(client.call("mcp_fixture_hidden", json!({})).await.is_err());
}

#[tokio::test]
async fn mcp_streamable_http_initializes_lists_and_calls() {
    async fn handler(Json(message): Json<Value>) -> axum::response::Response {
        if message.get("id").is_none() {
            return StatusCode::ACCEPTED.into_response();
        }
        let result = match message["method"].as_str().unwrap() {
            "initialize" => {
                json!({"protocolVersion":message["params"]["protocolVersion"],"capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
            }
            "tools/list" => {
                json!({"tools":[{"name":"echo","inputSchema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}]})
            }
            "tools/call" => {
                json!({"content":[{"type":"text","text":message["params"]["arguments"]["text"]}]})
            }
            _ => json!({}),
        };
        Json(json!({"jsonrpc":"2.0","id":message["id"],"result":result})).into_response()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/mcp", post(handler)))
            .await
            .unwrap();
    });
    let cfg = BTreeMap::from([(
        "http".into(),
        McpConfig::Http {
            url,
            bearer_token_env: None,
            allowed_tools: vec!["echo".into()],
        },
    )]);
    let mcp = Mcp::connect(&cfg).await.unwrap();
    assert_eq!(
        mcp.call("mcp_http_echo", json!({"text":"ok"}))
            .await
            .unwrap()["content"][0]["text"],
        "ok"
    );
    task.abort();
}

#[test]
fn invalid_tool_arguments_and_remote_schema_references_are_rejected() {
    let spec = tools::specs()
        .into_iter()
        .find(|t| t.name == "read_document")
        .unwrap();
    assert!(tools::validate_arguments(&spec.parameters, &json!({"path":42})).is_err());
    assert!(
        tools::validate_arguments(&spec.parameters, &json!({"path":"x","extra":"injection"}))
            .is_err()
    );
    assert!(
        tools::validate_arguments(
            &json!({"$ref":"https://untrusted.example/schema"}),
            &json!({})
        )
        .is_err()
    );
}

#[tokio::test]
async fn bundled_pm_tools_preserve_unknown_planning_inputs() {
    let (_dir, w) = fixture();
    let result = tools::run_pm(&w, "plan", &[]).await.unwrap();
    assert_eq!(result["successful"], false);
    let report = w.read("06_Delivery/Plan-Report.md").unwrap();
    assert!(report.contains("blocked"));
    assert!(!report.contains("Scheduled remaining effort"));
    let status = tools::run_pm(&w, "status", &[]).await.unwrap();
    assert_eq!(status["successful"], true);
}

#[tokio::test]
async fn provider_http_errors_are_not_retried_or_logged_with_sensitive_bodies() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let calls = calls.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    "sensitive-gateway-debug-value",
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let provider = Provider::new(
        config(url, Protocol::ChatCompletions)
            .active()
            .unwrap()
            .clone(),
    )
    .unwrap();
    let error = provider
        .complete("Test", &[], &[], &events().0, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("429"));
    assert!(!error.to_string().contains("sensitive-gateway"));
    assert_eq!(observed.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn cancellation_interrupts_a_pending_network_request() {
    let started = Arc::new(tokio::sync::Notify::new());
    let notification = started.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let started = started.clone();
            async move {
                started.notify_one();
                std::future::pending::<()>().await;
                "unreachable"
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let provider = Provider::new(
        config(url, Protocol::ChatCompletions)
            .active()
            .unwrap()
            .clone(),
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        notification.notified().await;
        signal.cancel();
    });
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        provider.complete("Test", &[], &[], &events().0, &cancel),
    )
    .await
    .unwrap();
    assert!(result.unwrap_err().to_string().contains("Cancelled"));
    task.abort();
}

#[tokio::test]
async fn rust_tools_capture_repository_and_openwiki_evidence() {
    let (_dir, w) = fixture();
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join("src")).unwrap();
    std::fs::create_dir(repo.path().join("openwiki")).unwrap();
    std::fs::write(
        repo.path().join("src/handler.rs"),
        "pub fn process() -> bool { true }\n",
    )
    .unwrap();
    std::fs::write(
        repo.path().join("openwiki/index.md"),
        "# Fixture wiki\n\nThe handler returns true.\n",
    )
    .unwrap();
    let args = vec![
        "--id".into(),
        "fixture-service".into(),
        "--path".into(),
        repo.path().to_string_lossy().into(),
        "--wiki".into(),
        "openwiki".into(),
        "--code-root".into(),
        "src".into(),
    ];
    assert_eq!(
        tools::run_pm(&w, "repo-add", &args).await.unwrap()["successful"],
        true
    );
    assert_eq!(
        tools::run_pm(&w, "sync", &["--repo".into(), "fixture-service".into()])
            .await
            .unwrap()["successful"],
        true
    );
    let args = [
        "--id",
        "EVD-TEST",
        "--claim",
        "Static code returns true",
        "--repo",
        "fixture-service",
        "--path",
        "src/handler.rs",
        "--start",
        "1",
        "--end",
        "1",
    ]
    .map(String::from);
    let result = tools::run_pm(&w, "evidence", &args).await.unwrap();
    assert_eq!(result["successful"], true, "{result}");
    let evidence = w.read("01_Sources/Evidence/EVD-TEST.md").unwrap();
    assert!(evidence.contains("handler.rs"));
    assert!(evidence.contains("sha256"));
    assert_eq!(
        std::fs::read_to_string(repo.path().join("src/handler.rs")).unwrap(),
        "pub fn process() -> bool { true }\n"
    );
}
