//! Synthetic protocol fixtures exercise the actual runtime; no semantic certification.
use axum::{Json, Router, extract::State, routing::post};
use pm_agent::{
    config::{Config, McpConfig, Protocol, ProviderConfig},
    evaluation::{self, Outcome},
    runtime::Status,
    workspace::{Workspace, hash},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

fn source() -> Workspace {
    Workspace::open(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap()
}
async fn prepare(case: &str, language: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let run = evaluation::prepare(&source(), case, dir.path(), language)
        .await
        .unwrap();
    (dir, run)
}
fn call(id: &str, name: &str, args: Value) -> Value {
    json!({"index":0,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}})
}
fn message(calls: Vec<Value>) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"index":0,"delta":{"content":if calls.is_empty(){"Synthetic completed response"}else{""},"tool_calls":calls},"finish_reason":if calls.is_empty(){"stop"}else{"tool_calls"}}]})
    )
}
type Script = Arc<Mutex<(VecDeque<String>, Vec<Value>)>>;
async fn server(script: Vec<String>) -> (Config, Script, tokio::task::JoinHandle<()>) {
    async fn handler(
        State(state): State<Script>,
        Json(request): Json<Value>,
    ) -> ([(&'static str, &'static str); 1], String) {
        let response = {
            let mut s = state.lock().unwrap();
            s.1.push(request);
            s.0.pop_front().unwrap_or_else(|| message(vec![]))
        };
        if response == "DELAY" {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        ([("content-type", "text/event-stream")], response)
    }
    let state = Arc::new(Mutex::new((VecDeque::from(script), vec![])));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/v1/chat/completions", post(handler))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let cfg = Config {
        provider: "fixture".into(),
        max_steps: 32,
        session_limits: Default::default(),
        providers: BTreeMap::from([(
            "fixture".into(),
            ProviderConfig {
                protocol: Protocol::ChatCompletions,
                base_url: url,
                model: "synthetic-fixture".into(),
                api_key_env: None,
                tools: true,
                request_usage: true,
                timeout_seconds: 5,
            },
        )]),
        mcp: BTreeMap::from([(
            "must-not-start".into(),
            McpConfig::Stdio {
                command: "nonexistent-evaluation-mcp".into(),
                args: vec![],
                env_names: vec![],
                allowed_tools: vec![],
            },
        )]),
    };
    (cfg, state, task)
}

#[tokio::test]
async fn chain_checks_actual_linked_files_citations_and_unperformed_semantic_review() {
    let (_dir, path) = prepare("draft-chain", "en").await;
    let w = Workspace::open(&path.join("vault")).unwrap();
    let citation = json!({"path":"01_Sources/Assets/interview.txt","sha256":hash(w.read("01_Sources/Assets/interview.txt").unwrap().as_bytes())});
    let mut script = vec![read_interview()];
    for (id, kind, folder, parents) in [
        ("LANG-EVAL", "glossary", "02_Domain", json!([])),
        ("BRD-EVAL", "brd", "04_Requirements", json!([])),
        ("PRD-EVAL", "prd", "04_Requirements", json!(["BRD-EVAL"])),
        (
            "REQ-EVAL",
            "requirement",
            "04_Requirements",
            json!(["PRD-EVAL"]),
        ),
        ("PBI-EVAL", "pbi", "05_Backlog", json!(["PRD-EVAL"])),
    ] {
        let mut meta = json!({"id":id,"type":kind,"title":"Synthetic draft","language":"en","doc_status":"draft","version":1,"parents":parents,"contexts":["Access"],"owner":null});
        if kind == "glossary" {
            meta["context"] = json!("Access");
        }
        if kind == "pbi" {
            meta["satisfies"] = json!(["REQ-EVAL"]);
            meta["sprint"] = Value::Null;
            meta["delivery_status"] = json!("proposed");
            meta["work_type"] = json!("feature");
        }
        let body = if kind == "pbi" {
            "# Synthetic draft\n\n- AC-01: Reject a duration outside 1 through 7 days.\n\n| Role | O | M | P |\n|---|---:|---:|---:|\n"
        } else {
            "# Synthetic draft\n\n**Access request**: A request for temporary application access. ^term-access\n"
        };
        let content = pm_agent::workspace::note(meta, body).unwrap();
        script.push(message(vec![call(id,"write_draft",json!({"path":format!("{folder}/{id}.md"),"content":content,"expected_hash":null,"citations":[citation.clone()]}))]));
        script.push(supported_review(
            &content,
            &w.read("01_Sources/Assets/interview.txt").unwrap(),
        ));
    }
    for name in ["validate", "coverage"] {
        script.push(message(vec![call(
            name,
            "pm",
            json!({"command":name,"args":[]}),
        )]));
    }
    script.push(message(vec![]));
    let (cfg, _, task) = server(script).await;
    let r = run(&path, cfg, 20).await;
    for id in [
        "created_glossary",
        "created_brd",
        "created_prd",
        "created_requirement",
        "created_pbi",
        "connected_requirement_chain",
        "versioned_draft_citations",
        "unscheduled_unknown_effort",
        "coverage_executed",
    ] {
        assert_eq!(
            r.report.checks.iter().find(|c| c.id == id).unwrap().outcome,
            Outcome::Pass,
            "{id}: {:?}",
            r.report.checks
        );
    }
    assert_eq!(
        r.report.mechanical_outcome,
        Outcome::Partial,
        "Missing owner and evidence-review inputs must remain visible"
    );
    assert_eq!(r.report.semantic_review, Outcome::NotRun);
    task.abort();
}

fn staged_script(w: &Workspace) -> Vec<String> {
    let citation = json!({"path":"01_Sources/Assets/interview.txt","sha256":hash(w.read("01_Sources/Assets/interview.txt").unwrap().as_bytes())});
    let mut script = vec![];
    let mut previous: Option<String> = None;
    for (id, kind, folder, parents) in [
        ("LANG-EVAL", "glossary", "02_Domain", json!([])),
        ("BRD-EVAL", "brd", "04_Requirements", json!([])),
        ("PRD-EVAL", "prd", "04_Requirements", json!(["BRD-EVAL"])),
        (
            "REQ-EVAL",
            "requirement",
            "04_Requirements",
            json!(["PRD-EVAL"]),
        ),
        ("PBI-EVAL", "pbi", "05_Backlog", json!(["PRD-EVAL"])),
    ] {
        script.push(read_interview());
        if let Some(path) = &previous {
            script.push(message(vec![call(
                "read-upstream",
                "read_document",
                json!({"path":path}),
            )]));
        }
        let mut meta = json!({"id":id,"type":kind,"title":"Synthetic draft","language":"en","doc_status":"draft","version":1,"parents":parents,"contexts":["Access"],"owner":null});
        if kind == "glossary" {
            meta["context"] = json!("Access");
        }
        if kind == "pbi" {
            meta["satisfies"] = json!(["REQ-EVAL"]);
            meta["delivery_status"] = json!("proposed");
            meta["work_type"] = json!("feature");
        }
        let body = if kind == "pbi" {
            "# Synthetic draft\n\n- AC-01: Reject a duration outside 1 through 7 days.\n\n| Role | O | M | P |\n|---|---:|---:|---:|\n"
        } else {
            "# Synthetic draft\n\n**Access request**: A request for temporary application access. ^term-access\n"
        };
        let content = pm_agent::workspace::note(meta, body).unwrap();
        let document = format!("{folder}/{id}.md");
        script.push(message(vec![call(id,"write_draft",json!({"path":document,"content":content,"expected_hash":null,"citations":[citation.clone()]}))]));
        script.push(supported_review(
            &content,
            &w.read("01_Sources/Assets/interview.txt").unwrap(),
        ));
        script.push(message(vec![call(
            "validate",
            "pm",
            json!({"command":"validate","args":[]}),
        )]));
        script.push(message(vec![]));
        previous = Some(document);
    }
    script.push(read_interview());
    for command in ["validate", "coverage"] {
        script.push(message(vec![call(
            command,
            "pm",
            json!({"command":command,"args":[]}),
        )]));
    }
    script.push(message(vec![]));
    script
}

#[tokio::test]
async fn staged_workflow_uses_actual_upstream_files_and_fresh_sessions_with_per_stage_caps() {
    let (_dir, path) = prepare("draft-stages", "en").await;
    let w = Workspace::open(&path.join("vault")).unwrap();
    let script = staged_script(&w);
    let (mut cfg, requests, task) = server(script).await;
    cfg.max_steps = 1;
    cfg.session_limits.max_requests = Some(0);
    let r = run(&path, cfg, 6).await;
    assert_eq!(r.report.stages.len(), 6, "{:?}", r.report.checks);
    assert_eq!(
        r.report
            .stages
            .iter()
            .map(|s| &s.session_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        6
    );
    assert!(
        r.report
            .stages
            .iter()
            .all(|s| s.status == Status::Completed && s.usage.recorded_attempts <= 6)
    );
    assert_eq!(r.report.usage.as_ref().unwrap().recorded_attempts, 33);
    assert_eq!(requests.lock().unwrap().1.len(), 33);
    assert_eq!(
        r.report
            .checks
            .iter()
            .find(|c| c.id == "stage_sequence")
            .unwrap()
            .outcome,
        Outcome::Pass
    );
    assert_eq!(
        r.report
            .checks
            .iter()
            .find(|c| c.id == "connected_requirement_chain")
            .unwrap()
            .outcome,
        Outcome::Pass
    );
    assert_eq!(
        r.report
            .checks
            .iter()
            .find(|c| c.id == "versioned_draft_citations")
            .unwrap()
            .outcome,
        Outcome::Pass
    );
    assert_eq!(
        r.report.mechanical_outcome,
        Outcome::Partial,
        "Unknown ownership remains a reported gap"
    );
    assert_eq!(r.report.semantic_review, Outcome::NotRun);
    let session: Value =
        serde_json::from_slice(&std::fs::read(path.join("stages/brd/Session.json")).unwrap())
            .unwrap();
    let history = session["history"].to_string();
    assert!(
        history.contains("LANG-EVAL"),
        "BRD must receive actual glossary bytes through a tool read"
    );
    let docs: Value = serde_json::from_slice(
        &std::fs::read(path.join("stages/glossary/Documents.json")).unwrap(),
    )
    .unwrap();
    assert!(docs.get("02_Domain/LANG-EVAL.md").is_some());
    assert!(docs.get("04_Requirements/BRD-EVAL.md").is_none());
    assert!(evaluation::inspect(&path).unwrap().current);
    std::fs::write(path.join("stages/glossary/Documents.json"), "{}").unwrap();
    assert!(!evaluation::inspect(&path).unwrap().current);
    task.abort();
}

#[tokio::test]
async fn stages_require_their_own_post_write_checks_and_preserve_earlier_evidence() {
    let (_dir, path) = prepare("draft-stages", "en").await;
    let w = Workspace::open(&path.join("vault")).unwrap();
    let mut script = staged_script(&w);
    script.remove(9); // BRD stage omits validation; glossary validation must not substitute.
    let (cfg, _, task) = server(script).await;
    let r = run(&path, cfg, 6).await;
    assert_eq!(r.report.stages.len(), 2);
    assert_eq!(
        r.report.stages[1]
            .checks
            .iter()
            .find(|c| c.id == "stage_validate")
            .unwrap()
            .outcome,
        Outcome::Fail
    );
    assert!(!path.join("stages/prd").exists());
    assert!(evaluation::inspect(&path).unwrap().current);
    task.abort();
}

#[tokio::test]
async fn staged_cancel_before_dispatch_sends_no_request_and_keeps_later_stages_unperformed() {
    let (_dir, path) = prepare("draft-stages", "en").await;
    let (cfg, requests, task) = server(vec![]).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    let r = evaluation::run(&path, cfg, Some(6), 20, &tx, &cancel)
        .await
        .unwrap();
    assert_eq!(r.report.session_status, Some(Status::Cancelled));
    assert_eq!(r.report.stages.len(), 1);
    assert_eq!(r.report.mechanical_outcome, Outcome::Partial);
    assert!(requests.lock().unwrap().1.is_empty());
    assert!(evaluation::inspect(&path).unwrap().current);
    task.abort();
}

#[tokio::test]
async fn evaluation_without_explicit_request_cap_can_complete_after_thirty_two_requests() {
    let (_dir, path) = prepare("missing-inputs", "en").await;
    let mut script = vec![read_interview(); 40];
    script.push(ask());
    let (mut cfg, requests, task) = server(script).await;
    cfg.max_steps = 1;
    cfg.session_limits.max_requests = Some(0);
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    let r = evaluation::run(&path, cfg, None, 20, &tx, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(r.report.mechanical_outcome, Outcome::Pass);
    assert_eq!(r.report.max_requests, None);
    assert_eq!(r.report.usage.unwrap().recorded_attempts, 41);
    assert_eq!(requests.lock().unwrap().1.len(), 41);
    task.abort();
}

#[tokio::test]
async fn staged_workflow_stops_at_limit_or_clarification_without_starting_later_stages() {
    for (cap, script, status) in [
        (1, vec![read_interview(), ask()], Status::LimitReached),
        (
            6,
            vec![read_interview(), ask()],
            Status::WaitingClarification,
        ),
    ] {
        let (_dir, path) = prepare("draft-stages", "en").await;
        let (cfg, requests, task) = server(script).await;
        let r = run(&path, cfg, cap).await;
        assert_eq!(r.report.stages.len(), 1);
        assert_eq!(r.report.session_status, Some(status));
        assert_eq!(r.report.mechanical_outcome, Outcome::Partial);
        assert!(!path.join("stages/brd").exists());
        assert!(requests.lock().unwrap().1.len() <= 2);
        assert!(evaluation::inspect(&path).unwrap().current);
        task.abort();
    }
}

#[tokio::test]
async fn failed_or_timed_out_provider_keeps_evidence_and_does_not_certify_unobserved_steps() {
    for script in ["data: {broken json}\n\n", "DELAY"] {
        let (_dir, path) = prepare("draft-chain", "en").await;
        let (cfg, requests, task) = server(vec![script.into()]).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let r = evaluation::run(&path, cfg, Some(8), 1, &tx, &CancellationToken::new())
            .await
            .unwrap();
        drop(tx);
        drain.await.unwrap();
        assert_eq!(r.report.state, "finished");
        assert_eq!(requests.lock().unwrap().1.len(), 1);
        assert_ne!(r.report.mechanical_outcome, Outcome::Pass);
        assert_eq!(
            r.report
                .checks
                .iter()
                .find(|c| c.id == "created_pbi")
                .unwrap()
                .outcome,
            Outcome::NotRun
        );
        assert_eq!(r.report.usage.unwrap().complete_total_tokens, None);
        assert!(path.join("Session.json").is_file());
        assert!(evaluation::inspect(&path).unwrap().current);
        task.abort();
    }
}
fn read_interview() -> String {
    message(vec![call(
        "read-interview",
        "read_document",
        json!({"path":"01_Sources/Assets/interview.txt"}),
    )])
}
fn ask() -> String {
    message(vec![call(
        "ask",
        "ask_clarification",
        json!({"question":"Which approval rule is required?","body":"# Synthetic missing information\n\nThe supplied material does not decide the future rule. Supply the responsible business decision and staffing before dependent work.","blocking_scope":"project"}),
    )])
}
async fn run(path: &Path, config: Config, requests: u64) -> evaluation::Inspection {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let result = evaluation::run(
        path,
        config,
        Some(requests),
        20,
        &tx,
        &CancellationToken::new(),
    )
    .await
    .unwrap();
    drop(tx);
    drain.await.unwrap();
    result
}

#[tokio::test]
async fn preparations_are_isolated_versioned_localized_and_unexecuted() {
    let original = std::fs::read(source().root.join("00_Governance/Project.md")).unwrap();
    for case in evaluation::cases() {
        for language in ["en", "zh-TW"] {
            let (_dir, path) = prepare(&case.id, language).await;
            let value = evaluation::inspect(&path).unwrap();
            assert!(value.current);
            assert_eq!(value.report.state, "prepared");
            assert_eq!(value.report.mechanical_outcome, Outcome::NotRun);
            assert_eq!(value.report.semantic_review, Outcome::NotRun);
            assert!(!path.join("vault/.pm-agent").exists());
            assert!(!path.join("vault/agent/src").exists());
            let manifest: Value =
                serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(manifest["fixture_version"], 4);
            assert!(
                manifest["inputs"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .any(|k| k.ends_with("/src/access.rs"))
            );
            let project =
                std::fs::read_to_string(path.join("vault/00_Governance/Project.md")).unwrap();
            assert!(project.contains(if language == "zh-TW" {
                "# 合成評估專案"
            } else {
                "# Synthetic evaluation project"
            }));
            if case.id == "capacity-forecast" {
                let plan =
                    pm_agent::planning::calculate(&Workspace::open(&path.join("vault")).unwrap())
                        .unwrap();
                assert_eq!(
                    plan.items
                        .iter()
                        .find(|i| i.id == "PBI-EVAL-UNKNOWN")
                        .unwrap()
                        .remaining,
                    None
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
        }
    }
    assert_eq!(
        std::fs::read(source().root.join("00_Governance/Project.md")).unwrap(),
        original
    );
}

#[tokio::test]
async fn actual_question_pause_passes_only_mechanical_checks_and_never_answers_or_replays() {
    let (_dir, path) = prepare("missing-inputs", "en").await;
    let (config, requests, task) = server(vec![read_interview(), ask()]).await;
    let result = run(&path, config.clone(), 8).await;
    assert_eq!(
        result.report.mechanical_outcome,
        Outcome::Pass,
        "{:?}",
        result.report.checks
    );
    assert_eq!(result.report.semantic_review, Outcome::NotRun);
    assert_eq!(
        result.report.session_status,
        Some(Status::WaitingClarification)
    );
    assert_eq!(
        result.report.usage.as_ref().unwrap().complete_total_tokens,
        None
    );
    assert_eq!(requests.lock().unwrap().1.len(), 2);
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        evaluation::run(&path, config, Some(8), 20, &tx, &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(requests.lock().unwrap().1.len(), 2);
    let session: Value =
        serde_json::from_slice(&std::fs::read(path.join("Session.json")).unwrap()).unwrap();
    assert_eq!(session["questions"][0]["answer"], Value::Null);
    assert!(path.join("Actions.json").is_file());
    task.abort();
}

#[tokio::test]
async fn inspection_accepts_pinned_imported_content_but_not_source_metadata() {
    for (kind, expected) in [
        ("asset", Outcome::Pass),
        ("extraction", Outcome::Pass),
        ("note", Outcome::Fail),
    ] {
        let (_dir, path) = prepare("missing-inputs", "en").await;
        let raw = std::fs::read(path.join("vault/01_Sources/Assets/interview.txt")).unwrap();
        let sha = hash(&raw);
        let input = match kind {
            "asset" => format!("01_Sources/Assets/{sha}.txt"),
            "extraction" => format!("01_Sources/Extracted/SRC-{}.md", &sha[..16]),
            _ => format!("01_Sources/Notes/SRC-{}.md", &sha[..16]),
        };
        let (cfg, _, task) = server(vec![
            message(vec![call("read", "read_document", json!({"path":input}))]),
            ask(),
        ])
        .await;
        let result = run(&path, cfg, 8).await;
        assert_eq!(
            result
                .report
                .checks
                .iter()
                .find(|c| c.id == "source_inspection")
                .unwrap()
                .outcome,
            expected,
            "{kind}: {:?}",
            result.report.checks
        );
        task.abort();
    }
}

#[tokio::test]
async fn wiki_conflict_requires_both_actual_snapshot_reads() {
    let (_dir, path) = prepare("wiki-code-conflict", "en").await;
    let m: Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    let mut script = vec![
        message(vec![call("catalog", "list_source_files", json!({}))]),
        message(vec![call(
            "files",
            "list_source_files",
            json!({"source_id":"REPO-EVAL"}),
        )]),
        read_interview(),
    ];
    for (id, suffix) in [("wiki", "/openwiki/Access.md"), ("code", "/src/access.rs")] {
        let p = m["inputs"]
            .as_object()
            .unwrap()
            .keys()
            .find(|p| p.starts_with("vault/03_Systems/Snapshots/") && p.ends_with(suffix))
            .unwrap();
        script.push(message(vec![call(
            id,
            "read_document",
            json!({"path":p.strip_prefix("vault/").unwrap()}),
        )]));
    }
    script.push(ask());
    let (cfg, requests, task) = server(script).await;
    let r = run(&path, cfg, 12).await;
    assert!(
        requests.lock().unwrap().1[0]
            .to_string()
            .contains("list_source_files")
    );
    assert_eq!(
        r.report.mechanical_outcome,
        Outcome::Pass,
        "{:?}",
        r.report.checks
    );
    task.abort();
    let (_dir, path) = prepare("wiki-code-conflict", "en").await;
    let (cfg, _, task) = server(vec![read_interview(), ask()]).await;
    let r = run(&path, cfg, 12).await;
    assert_eq!(
        r.report
            .checks
            .iter()
            .find(|c| c.id == "both_pinned_sources_inspected")
            .unwrap()
            .outcome,
        Outcome::Fail
    );
    task.abort();
}

#[tokio::test]
async fn native_forecast_records_shortage_and_unknowns_without_mutating_inputs() {
    let (_dir, path) = prepare("capacity-forecast", "zh-TW").await;
    let (cfg, requests, task) = server(vec![
        read_interview(),
        message(vec![call(
            "plan",
            "pm",
            json!({"command":"plan","args":[]}),
        )]),
        message(vec![]),
    ])
    .await;
    let r = run(&path, cfg, 8).await;
    assert_eq!(
        r.report.mechanical_outcome,
        Outcome::Pass,
        "{:?}",
        r.report.checks
    );
    let req = requests.lock().unwrap();
    assert!(
        !req.1[0].to_string().contains("3.6"),
        "Do not give the expected numerical result to the provider"
    );
    assert!(!req.1[0].to_string().contains("native_capacity_and_demand"));
    assert_eq!(r.report.semantic_review, Outcome::NotRun);
    task.abort();
}

#[tokio::test]
async fn promises_without_documents_fail_and_changed_evidence_is_historical() {
    let (_dir, path) = prepare("draft-chain", "en").await;
    let (cfg, _, task) = server(vec![read_interview(), message(vec![])]).await;
    let r = run(&path, cfg, 8).await;
    assert_eq!(r.report.mechanical_outcome, Outcome::Fail);
    assert_eq!(
        r.report
            .checks
            .iter()
            .find(|c| c.id == "created_pbi")
            .unwrap()
            .outcome,
        Outcome::Fail
    );
    let before = std::fs::read(path.join("report.json")).unwrap();
    std::fs::write(
        path.join("vault/01_Sources/Assets/interview.txt"),
        "changed source",
    )
    .unwrap();
    std::fs::write(path.join("Actions.json"), "[]").unwrap();
    let old = evaluation::inspect(&path).unwrap();
    assert!(!old.current);
    assert!(old.changes.iter().any(|c| c.contains("Actions.json")));
    assert_eq!(std::fs::read(path.join("report.json")).unwrap(), before);
    std::fs::write(path.join("Task.md"), "changed task").unwrap();
    std::fs::write(path.join("Report.md"), "changed report").unwrap();
    let edited = evaluation::inspect(&path).unwrap();
    for file in ["Task.md", "Report.md"] {
        assert!(edited.changes.iter().any(|c| c.contains(file)));
    }
    task.abort();
}

#[tokio::test]
async fn stale_preparation_and_request_limit_cannot_make_an_extra_call() {
    let (_dir, path) = prepare("missing-inputs", "en").await;
    std::fs::write(
        path.join("vault/01_Sources/Assets/interview.txt"),
        "changed",
    )
    .unwrap();
    let (cfg, requests, task) = server(vec![]).await;
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        evaluation::run(&path, cfg, Some(8), 20, &tx, &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(requests.lock().unwrap().1.is_empty());
    task.abort();
    let (_dir, path) = prepare("missing-inputs", "en").await;
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    manifest["runner_sha256"] = json!("0".repeat(64));
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(path.join("manifest.json"), &bytes).unwrap();
    let mut report: Value =
        serde_json::from_slice(&std::fs::read(path.join("report.json")).unwrap()).unwrap();
    report["manifest_sha256"] = json!(hash(&bytes));
    std::fs::write(
        path.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    let (cfg, requests, task) = server(vec![]).await;
    let err = evaluation::run(&path, cfg, Some(8), 20, &tx, &CancellationToken::new())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("executable changed"));
    assert!(requests.lock().unwrap().1.is_empty());
    task.abort();
    let (_dir, path) = prepare("missing-inputs", "en").await;
    let (cfg, requests, task) = server(vec![read_interview(), ask()]).await;
    let r = run(&path, cfg, 1).await;
    assert_eq!(requests.lock().unwrap().1.len(), 1);
    assert_eq!(r.report.session_status, Some(Status::LimitReached));
    assert_ne!(r.report.mechanical_outcome, Outcome::Pass);
    let saved: Value =
        serde_json::from_slice(&std::fs::read(path.join("Session.json")).unwrap()).unwrap();
    assert_eq!(saved["pending"].as_array().unwrap().len(), 1);
    task.abort();
}

#[tokio::test]
async fn cli_list_and_inspection_need_no_provider_and_path_boundaries_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("must-not-be-created");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .args(["--vault", missing.to_str().unwrap(), "eval", "list"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!missing.exists());
    assert_eq!(
        serde_json::from_slice::<Vec<Value>>(&out.stdout)
            .unwrap()
            .len(),
        6
    );
    assert!(
        evaluation::prepare(&source(), "missing-inputs", &source().root, "en")
            .await
            .is_err()
    );
    assert!(
        evaluation::prepare(&source(), "unknown-case", dir.path(), "en")
            .await
            .is_err()
    );
    let (_dir, path) = prepare("missing-inputs", "en").await;
    let digest = hash(&std::fs::read(path.join("manifest.json")).unwrap());
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .args([
            "--vault",
            missing.to_str().unwrap(),
            "eval",
            "inspect",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        hash(&std::fs::read(path.join("manifest.json")).unwrap()),
        digest
    );
    assert!(!path.join("vault/.pm-agent").exists());
    #[cfg(unix)]
    {
        std::fs::remove_file(path.join("vault/01_Sources/Assets/interview.txt")).unwrap();
        std::os::unix::fs::symlink(
            path.join("Task.md"),
            path.join("vault/01_Sources/Assets/interview.txt"),
        )
        .unwrap();
        assert!(evaluation::inspect(&path).is_err());
    }
}

fn supported_review(content: &str, source: &str) -> String {
    let assessments: Vec<_> = pm_agent::grounding::review_units(content).unwrap().iter().map(|unit| {
        if unit.text.starts_with('#') || unit.text.starts_with('|') {
            json!({"unit_id":unit.id,"classification":"structural","rationale":"Synthetic fixture markup.","evidence":[]})
        } else {
            json!({"unit_id":unit.id,"classification":"supported","rationale":"Scripted judgment for protocol testing only.","evidence":[{"path":"01_Sources/Assets/interview.txt","quote":source}]})
        }
    }).collect();
    message(vec![call(
        "review",
        "submit_grounding_review",
        json!({"decision":"supported","checked_sources":["01_Sources/Assets/interview.txt"],"assessments":assessments,"findings":[],"question":"","body":""}),
    )])
}

#[tokio::test]
async fn unsupported_proposal_requires_source_inspection_and_an_actual_unanswered_question() {
    let (_dir, path) = prepare("unsupported-claims", "en").await;
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).unwrap()).unwrap();
    let mut script = vec![
        read_interview(),
        message(vec![call(
            "proposal",
            "read_document",
            json!({"path":"01_Sources/Assets/proposal.txt"}),
        )]),
    ];
    for suffix in ["/openwiki/Access.md", "/src/access.rs"] {
        let path = manifest["inputs"]
            .as_object()
            .unwrap()
            .keys()
            .find(|p| p.starts_with("vault/03_Systems/Snapshots/") && p.ends_with(suffix))
            .unwrap();
        script.push(message(vec![call(
            suffix,
            "read_document",
            json!({"path":path.strip_prefix("vault/").unwrap()}),
        )]));
    }
    script.push(ask());
    let (cfg, _, task) = server(script).await;
    let r = run(&path, cfg, 12).await;
    assert_eq!(
        r.report.mechanical_outcome,
        Outcome::Pass,
        "{:?}",
        r.report.checks
    );
    assert_eq!(r.report.session_status, Some(Status::WaitingClarification));
    assert_eq!(r.report.semantic_review, Outcome::NotRun);
    assert!(r.current);
    task.abort();
}
