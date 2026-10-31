use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use pm_agent::{
    config::{Config, McpConfig, Protocol, ProviderConfig},
    mcp::Mcp,
    provider::{Event, Provider},
    runtime::{Runtime, Session, Status},
    tools,
    workspace::{Workspace, atomic_write, hash, note},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

type Queue = Arc<Mutex<VecDeque<String>>>;

fn budget_chat(text: &str, calls: Vec<Value>, usage: Value) -> String {
    chat(text, calls).replace(
        "data: [DONE]",
        &format!(
            "data: {}\n\ndata: [DONE]",
            json!({"choices":[],"usage":usage})
        ),
    )
}

#[tokio::test]
async fn budget_attempt_is_durable_before_dispatch_and_cancellation_keeps_unknown_usage() {
    let (_dir, w) = fixture();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut cfg = config(
        format!("http://{}/v1", listener.local_addr().unwrap()),
        Protocol::ChatCompletions,
    );
    cfg.session_limits.max_total_tokens = Some(100);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Cancellation fixture").unwrap();
    let seen = Arc::new(tokio::sync::Notify::new());
    let state = (
        w.path(".pm-agent/runtime.sqlite3").unwrap(),
        s.id.clone(),
        seen.clone(),
    );
    async fn handler(
        State((path, id, seen)): State<(std::path::PathBuf, String, Arc<tokio::sync::Notify>)>,
    ) -> impl IntoResponse {
        let store = pm_agent::store::Store::open(&path).unwrap();
        let s: Session = store.load(&id).unwrap();
        let attempts = &s.request_ledger.unwrap().requests;
        assert_eq!(attempts.len(), 1);
        assert!(matches!(
            attempts[0].state,
            pm_agent::budget::State::Started
        ));
        assert!(attempts[0].finished_at.is_none());
        seen.notify_one();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        (
            [("content-type", "text/event-stream")],
            chat("late", vec![]),
        )
    }
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .with_state(state),
        )
        .await
        .unwrap()
    });
    let cancel = CancellationToken::new();
    let (tx, _) = events();
    let (result, _) = tokio::join!(rt.run(&mut s, None, &tx, &cancel), async {
        tokio::time::timeout(std::time::Duration::from_secs(5), seen.notified())
            .await
            .unwrap();
        cancel.cancel();
    });
    result.unwrap();
    assert_eq!(s.status, Status::Cancelled);
    let mut s: Session = rt.store.load(&s.id).unwrap();
    assert!(matches!(
        s.request_ledger.as_ref().unwrap().requests[0].state,
        pm_agent::budget::State::Cancelled
    ));
    assert_eq!(
        pm_agent::budget::summary(s.request_ledger.as_ref()).complete_total_tokens,
        None
    );
    s.status = Status::Ready;
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::LimitReached);
    assert_eq!(s.request_ledger.unwrap().requests.len(), 1);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn budget_cli_ignores_legacy_request_cap_and_query_remains_local() {
    let (_dir, w) = fixture();
    let (url, _, requests, server) = server(vec![chat("Done.", vec![])]).await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    cfg.session_limits.max_requests = Some(0);
    let path = w.path(".pm-agent/config.toml").unwrap();
    atomic_write(&path, toml::to_string(&cfg).unwrap().as_bytes()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .args([
            "--vault",
            w.root.to_str().unwrap(),
            "chat",
            "Synthetic zero limit",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(requests.lock().unwrap().len(), 1);
    let store =
        pm_agent::store::Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
    let id = store.sessions().unwrap()[0].0.clone();
    let s: Session = store.load(&id).unwrap();
    assert_eq!(s.status, Status::Completed);
    assert_eq!(s.request_ledger.unwrap().requests.len(), 1);
    cfg.providers.get_mut("fixture").unwrap().model.clear();
    cfg.mcp.insert(
        "not-started".into(),
        McpConfig::Stdio {
            command: "this-command-must-not-run".into(),
            args: vec![],
            env_names: vec![],
            allowed_tools: vec![],
        },
    );
    atomic_write(&path, toml::to_string(&cfg).unwrap().as_bytes()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .args([
            "--vault",
            w.root.to_str().unwrap(),
            "budget",
            "--session",
            &id,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["limits"]["max_requests"].is_null());
    assert_eq!(report["observations"]["recorded_attempts"], 1);
    assert!(report["observations"]["complete_total_tokens"].is_null());
    assert!(report["pause_reason"].is_null());
    assert_eq!(requests.lock().unwrap().len(), 1);
    server.abort();
}

#[tokio::test]
async fn interactive_execution_ignores_legacy_caps_beyond_one_hundred_requests() {
    let (_dir, w) = fixture();
    let mut script = (0..105)
        .map(|i| {
            chat(
                "",
                vec![call(
                    0,
                    &format!("read-{i}"),
                    "read_document",
                    json!({"path":"00_Governance/Settings.md"}),
                )],
            )
        })
        .collect::<Vec<_>>();
    script.push(chat("Completed after 105 tool rounds.", vec![]));
    script.push(chat("Continued.", vec![]));
    let (url, _, requests, server) = server(script).await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    cfg.max_steps = 1;
    cfg.session_limits.max_requests = Some(0);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Exercise continued work").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed, "{:?}", s.error);
    assert_eq!(requests.lock().unwrap().len(), 106);
    assert!(s.pending.is_empty());
    assert_eq!(rt.store.calls(&s.id).unwrap().len(), 105);
    let id = s.id.clone();
    drop(rt);
    let rt = Runtime::open(w, cfg).await.unwrap();
    let mut s: Session = rt.store.load(&id).unwrap();
    rt.input(&mut s, "Continue this saved session").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed);
    assert_eq!(requests.lock().unwrap().len(), 107);
    assert_eq!(s.request_ledger.unwrap().requests.len(), 107);
    server.abort();
}

#[tokio::test]
async fn manual_cancellation_stops_long_running_chat_and_preserves_completed_tools() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (_dir, w) = fixture();
    let count = Arc::new(AtomicUsize::new(0));
    let reached = Arc::new(tokio::sync::Notify::new());
    async fn handler(
        State((count, reached)): State<(Arc<AtomicUsize>, Arc<tokio::sync::Notify>)>,
    ) -> impl IntoResponse {
        let n = count.fetch_add(1, Ordering::SeqCst) + 1;
        if n == 41 {
            reached.notify_one();
            std::future::pending::<()>().await;
        }
        (
            [("content-type", "text/event-stream")],
            chat(
                "",
                vec![call(
                    0,
                    &format!("read-{n}"),
                    "read_document",
                    json!({"path":"00_Governance/Settings.md"}),
                )],
            ),
        )
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let cfg = config(
        format!("http://{}/v1", listener.local_addr().unwrap()),
        Protocol::ChatCompletions,
    );
    let state = (count.clone(), reached.clone());
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .with_state(state),
        )
        .await
        .unwrap()
    });
    let rt = Runtime::open(w, cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Continue until cancelled").unwrap();
    let cancel = CancellationToken::new();
    let (tx, _) = events();
    let (result, ()) = tokio::join!(rt.run(&mut s, None, &tx, &cancel), async {
        tokio::time::timeout(std::time::Duration::from_secs(10), reached.notified())
            .await
            .unwrap();
        cancel.cancel();
    });
    result.unwrap();
    assert_eq!(s.status, Status::Cancelled);
    assert_eq!(count.load(Ordering::SeqCst), 41);
    let saved: Session = rt.store.load(&s.id).unwrap();
    assert_eq!(saved.status, Status::Cancelled);
    assert_eq!(rt.store.calls(&s.id).unwrap().len(), 40);
    let ledger = saved.request_ledger.unwrap();
    assert_eq!(ledger.requests.len(), 41);
    assert!(matches!(
        ledger.requests.last().unwrap().state,
        pm_agent::budget::State::Cancelled
    ));
    server.abort();
}

#[tokio::test]
async fn budget_reported_threshold_uses_exact_totals_and_allows_completed_final_text() {
    for protocol in [Protocol::ChatCompletions, Protocol::Responses] {
        let (_dir, w) = fixture();
        let usage = json!({"input_tokens":10,"output_tokens":2,"prompt_tokens":10,"completion_tokens":2,"total_tokens":150});
        let response = if protocol == Protocol::Responses {
            format!(
                "data: {}\n\ndata: {}\n\n",
                json!({"type":"response.output_text.delta","delta":"Done."}),
                json!({"type":"response.completed","response":{"status":"completed","output":[],"usage":usage}})
            )
        } else {
            budget_chat("Done.", vec![], usage)
        };
        let (url, _, requests, server) = server(vec![response]).await;
        let mut cfg = config(url, protocol);
        cfg.session_limits.max_total_tokens = Some(100);
        let rt = Runtime::open(w, cfg.clone()).await.unwrap();
        let mut s = Session::new(&cfg, "prd").unwrap();
        rt.input(&mut s, "Synthetic task").unwrap();
        rt.run(&mut s, None, &events().0, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(s.status, Status::Completed);
        assert_eq!(s.last_output, "Done.");
        assert_eq!(
            pm_agent::budget::summary(s.request_ledger.as_ref()).complete_total_tokens,
            Some(150)
        );
        assert_eq!(s.usage.as_ref().unwrap().total_tokens, Some(150));
        rt.input(&mut s, "Next turn").unwrap();
        rt.run(&mut s, None, &events().0, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(s.status, Status::LimitReached);
        assert_eq!(requests.lock().unwrap().len(), 1);
        server.abort();
    }
}

#[tokio::test]
async fn budget_missing_totals_block_pending_tools_until_operator_disables_guard() {
    let (_dir, w) = fixture();
    let (url, _, requests, server) = server(vec![
        budget_chat(
            "",
            vec![call(0, "list", "list_documents", json!({}))],
            json!({"prompt_tokens":44,"completion_tokens":5}),
        ),
        budget_chat("Read.", vec![], json!({"total_tokens":0})),
    ])
    .await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    cfg.session_limits.max_total_tokens = Some(100);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Inspect").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::LimitReached);
    assert!(rt.store.call_result(&s.id, "list").unwrap().is_none());
    assert_eq!(s.usage.as_ref().unwrap().total_tokens, None);
    drop(rt);
    cfg.session_limits.max_total_tokens = Some(100000);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    s.status = Status::Ready;
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::LimitReached);
    assert_eq!(requests.lock().unwrap().len(), 1);
    drop(rt);
    cfg.session_limits.max_total_tokens = None;
    let rt = Runtime::open(w.clone(), cfg).await.unwrap();
    s.status = Status::Ready;
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed);
    let summary = pm_agent::budget::summary(s.request_ledger.as_ref());
    assert_eq!(summary.complete_total_tokens, None);
    assert_eq!(summary.reported_total_sum, Some(0));
    assert_eq!(requests.lock().unwrap().len(), 2);
    server.abort();
}

#[tokio::test]
async fn budget_failed_requests_remain_counted_without_automatic_retry() {
    let (_dir, w) = fixture();
    let (url, _, requests, server) = server(vec![
        "data: invalid-json\n\n".into(),
        chat("Explicitly resumed.", vec![]),
    ])
    .await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    cfg.session_limits.max_requests = Some(1);
    let rt = Runtime::open(w, cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Inspect").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Failed);
    let mut s: Session = rt.store.load(&s.id).unwrap();
    let attempt = &s.request_ledger.as_ref().unwrap().requests[0];
    assert!(matches!(attempt.state, pm_agent::budget::State::Failed));
    assert!(attempt.finished_at.is_some());
    assert_eq!(attempt.usage, None);
    assert_eq!(requests.lock().unwrap().len(), 1);
    s.status = Status::Ready;
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed);
    assert_eq!(requests.lock().unwrap().len(), 2);
    server.abort();
}

#[tokio::test]
async fn budget_legacy_sessions_keep_unknown_history_and_disabled_limits_work() {
    let (_dir, w) = fixture();
    let (url, _, requests, server) = server(vec![budget_chat(
        "Done.",
        vec![],
        json!({"total_tokens":7}),
    )])
    .await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    cfg.session_limits.max_requests = Some(10);
    let mut value = serde_json::to_value(Session::new(&cfg, "prd").unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("request_ledger");
    let mut s: Session = serde_json::from_value(value).unwrap();
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    rt.input(&mut s, "Legacy fixture").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed);
    let summary = pm_agent::budget::summary(s.request_ledger.as_ref());
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert!(summary.prior_requests_unknown);
    assert_eq!(summary.reported_total_sum, Some(7));
    assert_eq!(summary.complete_total_tokens, None);
    server.abort();
}

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
        session_limits: Default::default(),
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
    let (url,_,requests,server)=server(vec![chat("",vec![call(0,"read","read_document",json!({"path":"00_Governance/Project.md"}))]),chat("",vec![call(0,"write","write_draft",json!({"path":"04_Requirements/PRD/PRD-TEST.md","content":draft,"expected_hash":null,"citations":[{"path":"00_Governance/Project.md","sha256":hash(source.as_bytes())}]}))]),supported_review("00_Governance/Project.md"),chat("Draft saved; semantic review remains necessary.",vec![])]).await;
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
async fn scoped_question_allows_explicit_independent_work_but_blocks_other_sessions() {
    let (_dir, w) = fixture();
    let root = "04_Requirements/REQ-SCOPED.md";
    atomic_write(&w.path(root).unwrap(),note(json!({"id":"REQ-SCOPED","type":"requirement","title":"Synthetic","language":"en","doc_status":"draft"}),"# Synthetic gap").unwrap().as_bytes()).unwrap();
    let (url,queue,_,server)=server(vec![chat("",vec![call(0,"ask-scope","ask_clarification",json!({"question":"Which rule applies?","body":"Supply the missing rule for REQ-SCOPED.","blocking_scope":"records","affected_ids":["REQ-SCOPED"]}))])]).await;
    let cfg = config(url, Protocol::ChatCompletions);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Inspect the synthetic source.").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::WaitingClarification);
    let question = s.questions[0].path.clone();
    let before = w.read(&question).unwrap();
    rt.continue_independent(&mut s).unwrap();
    assert!(s.questions[0].answer.is_none());
    assert_eq!(w.read(&question).unwrap(), before);
    let content=note(json!({"type":"prd","id":"PRD-FREE","title":"Independent draft","language":"en","doc_status":"draft"}),"# Supported independent draft").unwrap();
    let args = json!({"path":"04_Requirements/PRD-FREE.md","content":content,"expected_hash":null,"citations":[{"path":"00_Governance/Project.md","sha256":hash(w.read("00_Governance/Project.md").unwrap().as_bytes())}]});
    queue.lock().unwrap().extend([
        chat("", vec![call(0, "independent", "write_draft", args)]),
        supported_review("00_Governance/Project.md"),
        chat("Independent work saved; the rule remains unknown.", vec![]),
    ]);
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::WaitingClarification);
    assert!(w.path("04_Requirements/PRD-FREE.md").unwrap().exists());
    assert_eq!(w.read(&question).unwrap(), before);
    let mut other = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut other, "Review the scoped requirement.")
        .unwrap();
    let blocked = json!({"path":root,"content":note(json!({"id":"REQ-SCOPED","type":"requirement","title":"Synthetic","language":"en","doc_status":"draft"}),"# Unsupported update").unwrap(),"expected_hash":hash(w.read(root).unwrap().as_bytes()),"citations":[{"path":"00_Governance/Project.md","sha256":hash(w.read("00_Governance/Project.md").unwrap().as_bytes())}]});
    queue.lock().unwrap().extend([
        chat("", vec![call(0, "blocked", "write_draft", blocked)]),
        chat("Blocked by the existing scoped question.", vec![]),
    ]);
    rt.run(&mut other, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(other.status, Status::WaitingClarification);
    assert_eq!(other.blocked_questions, vec![question.clone()]);
    assert!(!w.read(root).unwrap().contains("Unsupported update"));
    assert!(rt.answer(&mut other, &question, "Wrong session").is_err());
    let mut old = serde_json::to_value(&s).unwrap();
    old.as_object_mut().unwrap().remove("blocked_questions");
    let restored: Session = serde_json::from_value(old).unwrap();
    assert!(restored.blocked_questions.is_empty());
    server.abort();
}

#[tokio::test]
async fn multiple_questions_require_explicit_answers_and_stale_answers_cannot_resolve() {
    let (_dir, w) = fixture();
    let (url, queue, _, server) = server(vec![chat(
        "",
        vec![call(
            0,
            "ask-one",
            "ask_clarification",
            json!({"question":"First missing fact?","body":"Supply the first fact."}),
        )],
    )])
    .await;
    let cfg = config(url, Protocol::ChatCompletions);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Inspect gaps.").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    rt.continue_independent(&mut s).unwrap();
    queue.lock().unwrap().push_back(chat(
        "",
        vec![call(
            0,
            "ask-two",
            "ask_clarification",
            json!({"question":"Second missing fact?","body":"Supply the second fact."}),
        )],
    ));
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    let first = s.questions[0].path.clone();
    let second = s.questions[1].path.clone();
    assert!(rt.input(&mut s, "Ambiguous reply").is_err());
    rt.answer(&mut s, &first, "Actual synthetic first answer.")
        .unwrap();
    assert!(s.questions[0].answer.is_some());
    assert!(s.questions[1].answer.is_none());
    assert!(!w.read(&second).unwrap().contains("answer_received_at"));
    let text = w.read(&first).unwrap();
    let (meta, body) = pm_agent::workspace::parse_note(&text).unwrap();
    atomic_write(
        &w.path(&first).unwrap(),
        note(meta, &format!("{body}\nNew material question."))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    queue.lock().unwrap().extend([
        chat(
            "",
            vec![call(
                0,
                "stale-resolve",
                "resolve_clarification",
                json!({"path":first,"resolution":"The old answer is enough."}),
            )],
        ),
        chat("A fresh answer is required.", vec![]),
    ]);
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::WaitingClarification);
    assert!(!s.questions[0].resolved);
    assert!(s.history.iter().any(|h| {
        h.to_string()
            .contains("does not match the current question")
    }));
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
async fn explicit_answer_adopts_manual_question_and_unresolved_questions_gate_mcp() {
    let (_dir, w) = fixture();
    let path = "07_Decisions/Manual/QUESTION-MANUAL.md";
    atomic_write(&w.path(path).unwrap(),note(json!({"id":"QUESTION-MANUAL","type":"question","title":"Synthetic manual question","language":"en","workflow_status":"open","clarification_schema":"clarification-v1","blocking_scope":"project","affected_ids":[],"affected_paths":[]}),"# Supply the missing source").unwrap().as_bytes()).unwrap();
    let (url, queue, _, server) = server(vec![
        chat(
            "",
            vec![call(
                0,
                "blocked-mcp",
                "mcp_fixture_echo",
                json!({"text":"Do not execute"}),
            )],
        ),
        chat("The project question remains open.", vec![]),
    ])
    .await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    cfg.mcp.insert(
        "fixture".into(),
        McpConfig::Stdio {
            command: std::env::var("PM_TEST_PYTHON").unwrap_or("python3".into()),
            args: vec![
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/mcp_fixture.py")
                    .to_string_lossy()
                    .into(),
            ],
            env_names: vec![],
            allowed_tools: vec!["echo".into()],
        },
    );
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Inspect the synthetic project.").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::WaitingClarification);
    assert!(
        rt.store
            .call_result(&s.id, "blocked-mcp")
            .unwrap()
            .is_none()
    );
    rt.answer(
        &mut s,
        "QUESTION-MANUAL",
        "Actual source supplied by the fixture user.",
    )
    .unwrap();
    assert_eq!(s.questions[0].path, path);
    assert_eq!(
        pm_agent::records::read(&w, path).unwrap().metadata["session_id"],
        s.id
    );
    queue.lock().unwrap().extend([
        chat(
            "",
            vec![call(
                0,
                "manual-resolve",
                "resolve_clarification",
                json!({"path":path,"resolution":"The user supplied the requested fixture source."}),
            )],
        ),
        chat("Recorded response resolves this question.", vec![]),
    ]);
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed);
    assert!(s.blocked_questions.is_empty());
    server.abort();
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

#[tokio::test]
async fn captured_user_answer_enters_grounding_packet_after_actual_resolution() {
    use pm_agent::grounding::Packet;
    let (_dir, w) = fixture();
    let mut args = grounding_args(&w, "The measured status baseline is 25%.");
    let (url, queue, _, task) = server(vec![
        chat("", vec![call(0, "draft", "write_draft", args.clone())]),
        rejected_review("The measured status baseline is 25%."),
    ])
    .await;
    let cfg = config(url, Protocol::ChatCompletions);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Create the draft.").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::WaitingClarification);
    let question = s.questions[0].path.clone();
    rt.input(
        &mut s,
        "Synthetic answer: a supplied measurement records the baseline as 25%.",
    )
    .unwrap();
    queue.lock().unwrap().extend([
        chat("",vec![call(0,"resolve","resolve_clarification",json!({"path":question,"resolution":"The actual fixture answer provides a measured baseline."}))]),
        chat("The answer was recorded.",vec![]),
    ]);
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert!(s.questions[0].resolved);
    args["citations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"path":question,"sha256":hash(w.read(&question).unwrap().as_bytes())}));
    let packet = Packet::prepare(&w, &args).unwrap();
    assert_eq!(packet.sources.len(), 3);
    let answer = packet
        .sources
        .iter()
        .find(|x| x.path.starts_with("01_Sources/Assets/USER-"))
        .unwrap();
    assert!(
        answer
            .content
            .contains("supplied measurement records the baseline as 25%")
    );
    atomic_write(&w.path(&answer.path).unwrap(), b"Tampered answer").unwrap();
    assert!(Packet::prepare(&w, &args).is_err());
    task.abort();
}
#[tokio::test]
async fn responses_protocol_executes_the_same_grounding_gate() {
    let (_dir, w) = fixture();
    let args = grounding_args(&w, "Integer durations from 1 through 7 are valid.");
    let response = |output: Value| {
        format!(
            "data: {}\n\n",
            json!({"type":"response.completed","response":{"status":"completed","output":output}})
        )
    };
    let (url,_,requests,task)=server(vec![
        response(json!([{"type":"function_call","call_id":"draft","name":"write_draft","arguments":args.to_string()}])),
        response(json!([{"type":"function_call","call_id":"review","name":"submit_grounding_review","arguments":json!({"decision":"supported","checked_sources":["01_Sources/Assets/grounding.txt"],"findings":[],"question":"","body":""}).to_string()}])),
        response(json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Saved draft."}]}])),
    ]).await;
    let cfg = config(url, Protocol::Responses);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Create draft.").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed, "{:?}", s.error);
    assert!(w.path("04_Requirements/PRD-GROUND.md").unwrap().exists());
    let req = requests.lock().unwrap();
    assert_eq!(req[1]["tools"][0]["name"], "submit_grounding_review");
    assert_eq!(req[1]["input"].as_array().unwrap().len(), 1);
    assert_eq!(s.draft_reviews.len(), 1);
    task.abort();
}

fn supported_review(path: &str) -> String {
    chat(
        "",
        vec![call(
            0,
            "review",
            "submit_grounding_review",
            json!({"decision":"supported","checked_sources":[path],"findings":[],"question":"","body":""}),
        )],
    )
}

#[test]
fn grounding_requires_all_source_paths_even_when_content_is_identical() {
    use pm_agent::{
        grounding,
        provider::{Call, Completion},
    };
    let (_dir, w) = fixture();
    let mut args = grounding_args(&w, "Integer durations from 1 through 7 are valid.");
    let source = "01_Sources/Assets/grounding.txt";
    let alias = "01_Sources/Assets/copy.txt";
    atomic_write(&w.path(alias).unwrap(), w.read(source).unwrap().as_bytes()).unwrap();
    args["citations"]
        .as_array_mut()
        .unwrap()
        .push(json!({"path":alias,"sha256":hash(w.read(alias).unwrap().as_bytes())}));
    let packet = grounding::Packet::prepare(&w, &args).unwrap();
    assert_eq!(packet.sources.len(), 2);
    for (paths, valid) in [(json!([source]), false), (json!([source, alias]), true)] {
        let completion=Completion{text:String::new(),calls:vec![Call{id:"review".into(),name:"submit_grounding_review".into(),arguments:json!({"decision":"supported","checked_sources":paths,"findings":[],"question":"","body":""}).to_string()}],items:vec![],usage:None};
        assert_eq!(grounding::parse(&packet, &completion).is_ok(), valid);
    }
}

fn grounding_args(w: &Workspace, claim: &str) -> Value {
    let source = "01_Sources/Assets/grounding.txt";
    atomic_write(&w.path(source).unwrap(), b"Numeric baselines, integrations and timeout behavior are unknown. Integer durations from 1 through 7 are valid.").unwrap();
    let content = note(json!({"id":"PRD-GROUND","type":"prd","title":"Grounding fixture","doc_status":"draft","language":w.language().unwrap()}), &format!("# Grounding fixture\n\n{claim}")).unwrap();
    json!({"path":"04_Requirements/PRD-GROUND.md","content":content,"expected_hash":null,"citations":[{"path":source,"sha256":hash(w.read(source).unwrap().as_bytes())}]})
}
fn rejected_review(claim: &str) -> String {
    chat(
        "PRIVATE_REVIEW",
        vec![call(
            0,
            "review",
            "submit_grounding_review",
            json!({
                "decision":"needs_clarification","checked_sources":["01_Sources/Assets/grounding.txt"],
                "findings":[{"claim":claim,"reason":"The cited interview leaves this input unknown."}],
                "question":"請提供此敘述的依據或確認它仍是未知資訊。",
                "body":"訪談未提供此數值或行為。請補充相關資料；PRD-GROUND 與其相依工作暫停，負責人及日期尚未提供。"
            }),
        )],
    )
}
#[tokio::test]
async fn grounding_blocks_unsupported_baselines_dependencies_and_behaviors_with_scoped_questions() {
    for claim in [
        "Observable status baseline is 0%.",
        "There are no external dependencies.",
        "Validation and creation must be atomic on timeout.",
    ] {
        let (_dir, w) = fixture();
        let args = grounding_args(&w, claim);
        let (url, _, requests, task) = server(vec![
            chat(
                "",
                vec![
                    call(0, "draft", "write_draft", args),
                    call(1, "later", "pm", json!({"command":"coverage","args":[]})),
                ],
            ),
            rejected_review(claim),
        ])
        .await;
        let cfg = config(url, Protocol::ChatCompletions);
        let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
        let mut s = Session::new(&cfg, "prd").unwrap();
        rt.input(
            &mut s,
            "Author conversation must not enter the reviewer context.",
        )
        .unwrap();
        let (tx, mut rx) = events();
        rt.run(&mut s, None, &tx, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(s.status, Status::WaitingClarification, "{:?}", s.error);
        assert!(!w.path("04_Requirements/PRD-GROUND.md").unwrap().exists());
        assert!(s.pending.is_empty());
        let question = w.read(&s.questions[0].path).unwrap();
        assert!(question.contains("04_Requirements/PRD-GROUND.md"));
        assert!(s.questions[0].answer.is_none());
        assert_eq!(s.request_ledger.as_ref().unwrap().requests.len(), 2);
        assert_eq!(s.draft_reviews.len(), 1);
        let saved: Session = rt.store.load(&s.id).unwrap();
        assert!(saved.draft_reviews[0].review.is_some());
        let calls = rt.store.calls(&s.id).unwrap();
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0]
                .result
                .as_ref()
                .unwrap()
                .contains("blocked_by_grounding")
        );
        let observed: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert!(
            observed
                .iter()
                .any(|e| matches!(e,Event::Notice(t) if t.contains("請提供")))
        );
        assert!(
            !observed
                .iter()
                .any(|e| matches!(e,Event::Text(t) if t.contains("PRIVATE_REVIEW")))
        );
        let req = requests.lock().unwrap();
        assert_eq!(req[1]["tools"].as_array().unwrap().len(), 1);
        assert_eq!(
            req[1]["tools"][0]["function"]["name"],
            "submit_grounding_review"
        );
        assert_eq!(req[1]["messages"].as_array().unwrap().len(), 2);
        assert!(
            !req[1]
                .to_string()
                .contains("Author conversation must not enter")
        );
        task.abort();
    }
}
#[tokio::test]
async fn malformed_or_operational_reviewer_calls_never_write_or_execute_tools() {
    for result in [
        chat("I approve.", vec![]),
        chat(
            "",
            vec![call(
                0,
                "bad",
                "pm",
                json!({"command":"coverage","args":[]}),
            )],
        ),
        rejected_review("This sentence is absent from the candidate."),
    ] {
        let (_dir, w) = fixture();
        let args = grounding_args(&w, "Integer durations from 1 through 7 are valid.");
        let (url, _, requests, task) = server(vec![
            chat("", vec![call(0, "draft", "write_draft", args)]),
            result,
        ])
        .await;
        let cfg = config(url, Protocol::ChatCompletions);
        let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
        let mut s = Session::new(&cfg, "prd").unwrap();
        rt.input(&mut s, "Review fixture.").unwrap();
        rt.run(&mut s, None, &events().0, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(s.status, Status::Failed);
        assert!(!w.path("04_Requirements/PRD-GROUND.md").unwrap().exists());
        assert_eq!(requests.lock().unwrap().len(), 2);
        assert!(rt.store.calls(&s.id).unwrap().is_empty());
        assert_eq!(s.draft_reviews[0].state, "failed");
        task.abort();
    }
}
#[tokio::test]
async fn grounding_checks_transitive_evidence_and_rejects_stale_or_private_sources() {
    use pm_agent::grounding::Packet;
    let (_dir, w) = fixture();
    let args = grounding_args(&w, "Integer durations from 1 through 7 are valid.");
    let source = args["citations"][0].clone();
    let upstream = "04_Requirements/BRD-UPSTREAM.md";
    let metadata = json!({"id":"BRD-UPSTREAM","type":"brd","title":"Upstream","language":"en","doc_status":"draft","sources":[{"resource":source["path"],"sha256":source["sha256"]}]});
    atomic_write(
        &w.path(upstream).unwrap(),
        note(
            metadata,
            "# Upstream\n\nA generated summary cannot authorize new business facts.",
        )
        .unwrap()
        .as_bytes(),
    )
    .unwrap();
    let mut args = args;
    args["citations"] =
        json!([{"path":upstream,"sha256":hash(w.read(upstream).unwrap().as_bytes())}]);
    let packet = Packet::prepare(&w, &args).unwrap();
    assert_eq!(packet.sources.len(), 2);
    atomic_write(
        &w.path(source["path"].as_str().unwrap()).unwrap(),
        b"Changed source.",
    )
    .unwrap();
    assert!(packet.ensure_current(&w, &args).is_err());
    assert!(!w.path("04_Requirements/PRD-GROUND.md").unwrap().exists());
    args["citations"] = json!([{"path":".pm-agent/config.toml","sha256":"ignored"}]);
    assert!(Packet::prepare(&w, &args).is_err());
}
#[tokio::test]
async fn reported_token_pause_after_review_reuses_receipt_without_a_second_review_request() {
    let (_dir, w) = fixture();
    let args = grounding_args(&w, "Integer durations from 1 through 7 are valid.");
    let usage = json!({"total_tokens":10});
    let review = supported_review("01_Sources/Assets/grounding.txt").replace(
        "data: [DONE]",
        &format!(
            "data: {}\n\ndata: [DONE]",
            json!({"choices":[],"usage":usage})
        ),
    );
    let (url, _, requests, task) = server(vec![
        budget_chat("", vec![call(0, "draft", "write_draft", args)], usage),
        review,
        budget_chat("Saved.", vec![], json!({"total_tokens":2})),
    ])
    .await;
    let mut cfg = config(url, Protocol::ChatCompletions);
    cfg.session_limits.max_total_tokens = Some(15);
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Create the supported draft.").unwrap();
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::LimitReached);
    assert!(!w.path("04_Requirements/PRD-GROUND.md").unwrap().exists());
    assert_eq!(s.draft_reviews.len(), 1);
    let id = s.id.clone();
    drop(rt);
    cfg.session_limits.max_total_tokens = Some(50);
    let rt = Runtime::open(w.clone(), cfg).await.unwrap();
    let mut s: Session = rt.store.load(&id).unwrap();
    s.status = Status::Ready;
    rt.run(&mut s, None, &events().0, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(s.status, Status::Completed, "{:?}", s.error);
    assert!(w.path("04_Requirements/PRD-GROUND.md").unwrap().exists());
    assert_eq!(requests.lock().unwrap().len(), 3);
    assert_eq!(s.draft_reviews.len(), 1);
    assert_eq!(
        pm_agent::budget::summary(s.request_ledger.as_ref()).complete_total_tokens,
        Some(22)
    );
    task.abort();
}
#[tokio::test]
async fn cancelling_read_only_grounding_preserves_pending_draft_without_uncertain_write() {
    let (_dir, w) = fixture();
    let args = grounding_args(&w, "Integer durations from 1 through 7 are valid.");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let seen = Arc::new(tokio::sync::Notify::new());
    let state = (args, seen.clone());
    async fn handler(
        State((args, seen)): State<(Value, Arc<tokio::sync::Notify>)>,
        Json(req): Json<Value>,
    ) -> impl IntoResponse {
        if req["tools"][0]["function"]["name"] == "submit_grounding_review" {
            seen.notify_one();
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            (
                [("content-type", "text/event-stream")],
                supported_review("01_Sources/Assets/grounding.txt"),
            )
        } else {
            (
                [("content-type", "text/event-stream")],
                chat("", vec![call(0, "draft", "write_draft", args)]),
            )
        }
    }
    let cfg = config(
        format!("http://{}/v1", listener.local_addr().unwrap()),
        Protocol::ChatCompletions,
    );
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(handler))
                .with_state(state),
        )
        .await
        .unwrap()
    });
    let rt = Runtime::open(w.clone(), cfg.clone()).await.unwrap();
    let mut s = Session::new(&cfg, "prd").unwrap();
    rt.input(&mut s, "Create draft.").unwrap();
    let cancel = CancellationToken::new();
    let (tx, _) = events();
    let (result, _) = tokio::join!(rt.run(&mut s, None, &tx, &cancel), async {
        tokio::time::timeout(std::time::Duration::from_secs(5), seen.notified())
            .await
            .unwrap();
        cancel.cancel();
    });
    result.unwrap();
    assert_eq!(s.status, Status::Cancelled);
    assert_eq!(s.draft_reviews[0].state, "cancelled");
    assert!(!w.path("04_Requirements/PRD-GROUND.md").unwrap().exists());
    assert!(rt.store.calls(&s.id).unwrap().is_empty());
    assert_eq!(s.pending.len(), 1);
    assert!(matches!(
        s.request_ledger.as_ref().unwrap().requests[1].state,
        pm_agent::budget::State::Cancelled
    ));
    task.abort();
}
