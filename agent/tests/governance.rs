use pm_agent::{
    baselines,
    records::{self, Records},
    tools,
    workspace::{Workspace, note, parse_note},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn fixtures() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    python(dir.path(), None);
    dir
}
fn python(path: &Path, mode: Option<&str>) {
    let mut command =
        Command::new(std::env::var("PM_TEST_PYTHON").unwrap_or_else(|_| "python3".into()));
    command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/governance_fixtures.py"))
        .arg(path);
    if let Some(mode) = mode {
        command.arg(mode);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
async fn run(w: &Workspace, command: &str, args: &[&str]) -> Value {
    let args: Vec<_> = args.iter().map(|s| s.to_string()).collect();
    let result = tools::run_pm(w, "/no-python-for-native-tools", command, &args)
        .await
        .unwrap();
    assert_eq!(result["engine"], "rust");
    result
}
#[tokio::test]
async fn governance_matches_legacy_reports_and_baseline_decisions_without_python() {
    let dir = fixtures();
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(dir.path().join("cases.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 34);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let w = Workspace::open(&dir.path().join(name)).unwrap();
        for (key, command, args) in [
            ("coverage", "coverage", vec![]),
            ("status", "status", vec![]),
            ("impact", "impact", vec!["PRD-001"]),
            ("context_impact", "impact", vec!["LANG-alpha"]),
        ] {
            let result = run(&w, command, &args).await;
            assert_eq!(
                result["successful"],
                case.get(format!("{key}_error")).is_none(),
                "{name}/{key}: {result}"
            );
            if result["successful"] == true {
                let (_, body) =
                    parse_note(result["reports"][0]["content"].as_str().unwrap()).unwrap();
                assert_eq!(body.trim(), case[key].as_str().unwrap(), "{name}/{key}");
            }
        }
        let result = run(&w, "fingerprint", &["REQ-001"]).await;
        assert_eq!(
            result["fingerprint"], case["fingerprint"],
            "{name}: {result}"
        );
        if case.get("pinned").is_some() || case.get("pinned_error").is_some() {
            let result = run(&w, "fingerprint", &["REQ-001", "--baseline", "BASE-TEST"]).await;
            assert_eq!(
                result["successful"],
                case.get("pinned_error").is_none(),
                "{name}: {result}"
            );
            if result["successful"] == true {
                assert_eq!(result["fingerprint"], case["pinned"], "{name}");
            }
        }
    }
}
#[test]
fn semantic_hashes_preserve_existing_unicode_numeric_and_annotation_contract() {
    let dir = fixtures();
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(dir.path().join("hashes.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 262);
    for case in cases {
        assert_eq!(
            records::content_hash(&case["meta"], case["body"].as_str().unwrap()).unwrap(),
            case["hash"],
            "{}",
            case["meta"]
        );
        let mut meta = case["meta"].clone();
        meta["doc_status"] = json!("approved");
        meta["baseline_id"] = json!("BASE-TEST");
        assert_eq!(
            records::content_hash(&meta, case["body"].as_str().unwrap()).unwrap(),
            case["hash"]
        );
    }
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(dir.path().join("notes.json")).unwrap()).unwrap();
    for case in cases {
        let (meta, body) = records::parse_project_note(case["text"].as_str().unwrap()).unwrap();
        assert_eq!(meta, case["meta"]);
        assert_eq!(body.trim(), case["body"].as_str().unwrap());
        assert_eq!(records::content_hash(&meta, &body).unwrap(), case["hash"]);
    }
}
#[tokio::test]
async fn native_candidates_are_compatible_immutable_and_never_self_approve() {
    let dir = fixtures();
    let w = Workspace::open(&dir.path().join("base")).unwrap();
    let before = w.read("04_Requirements/Items/REQ-001.md").unwrap();
    let args = [
        "--id",
        "BASE-NATIVE",
        "--include",
        "PRD-001",
        "--include",
        "REQ-001",
        "--include",
        "LANG-alpha",
    ];
    let captured = run(&w, "baseline", &args).await;
    assert_eq!(captured["successful"], true, "{captured}");
    assert_eq!(captured["approved"], false);
    python(&w.root, Some("verify-native"));
    assert_eq!(w.read("04_Requirements/Items/REQ-001.md").unwrap(), before);
    assert!(baselines::approval(&Records::load(&w).unwrap(), "BASE-NATIVE").is_err());
    let manifest_path = w
        .path("00_Governance/Baselines/BASE-NATIVE/manifest.json")
        .unwrap();
    let original = fs::read(&manifest_path).unwrap();
    assert_eq!(run(&w, "baseline", &args).await["successful"], false);
    assert_eq!(fs::read(&manifest_path).unwrap(), original);
    let mut manifest: Value = serde_json::from_slice(&original).unwrap();
    manifest["documents"]["REQ-001"]["snapshot"] =
        json!("../../../04_Requirements/Items/REQ-001.md");
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(baselines::load(&w, "BASE-NATIVE").is_err());
    assert_eq!(
        run(
            &w,
            "baseline",
            &["--id", "../outside", "--include", "REQ-001"]
        )
        .await["successful"],
        false
    );
}
#[tokio::test]
async fn settings_packets_and_invalid_inputs_preserve_project_intent() {
    let dir = fixtures();
    let w = Workspace::open(&dir.path().join("base")).unwrap();
    let before = w.read("04_Requirements/Items/REQ-001.md").unwrap();
    let changed = run(&w, "settings", &["--document-language", "jA-jp"]).await;
    assert_eq!(changed["settings"]["document_language"], "ja-JP");
    assert_eq!(w.read("04_Requirements/Items/REQ-001.md").unwrap(), before);
    assert_eq!(
        run(&w, "settings", &["--document-language", "../en"]).await["successful"],
        false
    );
    let packet = run(&w, "prepare", &["review", "--focus", "REQ-001"]).await;
    assert_eq!(packet["successful"], true, "{packet}");
    let (meta, body) = parse_note(packet["reports"][0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(meta["language"], "en");
    assert_eq!(meta["workflow_status"], "prepared");
    assert!(body.contains("Project document language: ja-JP"));
    assert!(body.contains("REQ-001"));
    assert!(!body.contains("REQ-002"));
    assert_eq!(
        run(&w, "prepare", &["unknown-stage"]).await["successful"],
        false
    );
    assert_eq!(run(&w, "coverage", &["--help"]).await["successful"], true);
    assert_eq!(
        run(&w, "coverage", &["--invented"]).await["successful"],
        false
    );
    let duplicate = w.path("05_Backlog/duplicate.md").unwrap();
    fs::write(
        &duplicate,
        note(json!({"id":"PBI-001","type":"pbi"}), "# Duplicate fixture").unwrap(),
    )
    .unwrap();
    assert_eq!(run(&w, "coverage", &[]).await["successful"], false);
    fs::remove_file(&duplicate).unwrap();
    #[cfg(unix)]
    {
        let path = w.path("05_Backlog/PBI-001.md").unwrap();
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(dir.path().join("passed/05_Backlog/PBI-001.md"), &path).unwrap();
        assert_eq!(run(&w, "coverage", &[]).await["successful"], false);
    }
}
