//! Disposable synthetic records; no test actor or approval is a project event.
use pm_agent::{
    baselines,
    inspection::{self, Request, Target},
    records::{self, Records},
    sources, tools,
    workspace::{self, Workspace},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn put(w: &Workspace, path: &str, meta: Value, body: &str) {
    workspace::atomic_write(
        &w.path(path).unwrap(),
        workspace::note(meta, body).unwrap().as_bytes(),
    )
    .unwrap();
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    workspace::atomic_write(
        &dir.path().join("00_Governance/Project.md"),
        workspace::note(
            json!({"type":"project","id":"PROJECT-TEST","title":"Synthetic","language":"en"}),
            "# Fixture",
        )
        .unwrap()
        .as_bytes(),
    )
    .unwrap();
    let w = Workspace::open(dir.path()).unwrap();
    put(
        &w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","title":"Settings","language":"en","document_language":"en","agent_language":"en"}),
        "# Settings",
    );
    workspace::atomic_write(
        &w.path("tools/locales/en.json").unwrap(),
        include_bytes!("../../tools/locales/en.json"),
    )
    .unwrap();
    put(
        &w,
        "04_Requirements/REQ-1.md",
        json!({"type":"requirement","id":"REQ-1","title":"Synthetic","language":"en","version":1,"doc_status":"draft"}),
        "# Old requirement\n\nOriginal text.",
    );
    (dir, w)
}
fn inspect(w: &Workspace, id: &str, target: Target) -> inspection::Snapshot {
    inspection::load(
        w,
        &Request {
            id: id.into(),
            target,
        },
    )
    .unwrap()
}
fn tree(w: &Workspace) -> BTreeMap<String, String> {
    walkdir::WalkDir::new(&w.root)
        .into_iter()
        .map(Result::unwrap)
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            (
                e.path()
                    .strip_prefix(&w.root)
                    .unwrap()
                    .to_string_lossy()
                    .into(),
                workspace::hash(&std::fs::read(e.path()).unwrap()),
            )
        })
        .collect()
}
#[test]
fn candidates_explain_cycles_contexts_and_source_provenance_without_writes() {
    let (_dir, w) = fixture();
    for (path, meta, body) in [
        (
            "03_Systems/Repositories/REPO-test.md",
            json!({"id":"REPO-test","type":"repository"}),
            "# Synthetic repo",
        ),
        (
            "01_Sources/Evidence/EVD-1.md",
            json!({"id":"EVD-1","type":"evidence","repo_id":"test"}),
            "# Synthetic evidence\n[[REQ-1]]",
        ),
        (
            "02_Domain/test/CONTEXT.md",
            json!({"id":"LANG-test","type":"glossary","context":"test"}),
            "# Fixture context",
        ),
        (
            "05_Backlog/PBI-1.md",
            json!({"id":"PBI-1","type":"pbi","contexts":["test"]}),
            "[[04_Requirements/REQ-1#anchor|Alias]] [[Missing]]",
        ),
        (
            "08_Verification/TC-1.md",
            json!({"id":"TC-1","type":"testcase"}),
            "[[PBI-1]]",
        ),
        (
            "01_Sources/Notes/SOURCE-1.md",
            json!({"id":"SOURCE-1","type":"source"}),
            "# Fixture material",
        ),
        (
            "01_Sources/Evidence/EVD-M.md",
            json!({"id":"EVD-M","type":"evidence","source_id":"SOURCE-1"}),
            "# Material claim",
        ),
    ] {
        put(&w, path, meta, body);
    }
    let req = records::read(&w, "04_Requirements/REQ-1.md").unwrap();
    put(
        &w,
        &req.path,
        req.metadata,
        "# Synthetic\n[[EVD-1]] [[TC-1]]",
    );
    let before = tree(&w);
    let s = inspect(&w, "REPO-test", Target::Impact);
    let tc = s.candidates.iter().find(|c| c.id == "TC-1").unwrap();
    assert_eq!(
        tc.chain.iter().map(|e| e.to.as_str()).collect::<Vec<_>>(),
        ["EVD-1", "REQ-1", "PBI-1", "TC-1"]
    );
    assert_eq!(tc.chain[0].basis, "evidence repo_id");
    assert!(!s.candidates.iter().any(|c| c.id == "REPO-test"));
    assert!(s.unresolved.iter().any(|s| s.contains("Missing")));
    let s = inspect(&w, "LANG-test", Target::Impact);
    assert_eq!(
        s.candidates.iter().find(|c| c.id == "PBI-1").unwrap().chain[0].basis,
        "contexts: test"
    );
    assert_eq!(
        inspect(&w, "SOURCE-1", Target::Impact).candidates[0].id,
        "EVD-M"
    );
    assert_eq!(tree(&w), before);
}
#[test]
fn ambiguous_contexts_are_unknown_instead_of_selecting_a_definition() {
    let (_dir, w) = fixture();
    for id in ["LANG-A", "LANG-B"] {
        put(
            &w,
            &format!("02_Domain/{id}.md"),
            json!({"id":id,"type":"glossary","context":"duplicate"}),
            "# Synthetic context",
        );
    }
    put(
        &w,
        "05_Backlog/PBI-1.md",
        json!({"id":"PBI-1","type":"pbi","contexts":["duplicate"]}),
        "# Synthetic item",
    );
    let s = inspect(&w, "LANG-A", Target::Impact);
    assert!(s.candidates.is_empty());
    assert!(s.unresolved[0].contains("2 definitions"));
}
#[test]
fn baseline_diff_preserves_bytes_and_distinguishes_approval_and_rescission() {
    let (_dir, w) = fixture();
    baselines::capture(&Records::load(&w).unwrap(), "BASE-1", &["REQ-1".into()]).unwrap();
    let pinned =
        std::fs::read(w.root.join("00_Governance/Baselines/BASE-1/files/REQ-1.md")).unwrap();
    let d = records::read(&w, "04_Requirements/REQ-1.md").unwrap();
    put(
        &w,
        &d.path,
        d.metadata,
        "# New requirement\n\nUpdated 中文.",
    );
    let before = tree(&w);
    let c = inspect(&w, "REQ-1", Target::Baseline("BASE-1".into()))
        .comparison
        .unwrap();
    assert!(c.status.starts_with("changed"));
    assert_eq!(c.basis["approval_verified"], false);
    let diff = c.diff.unwrap();
    assert!(
        diff.lines
            .iter()
            .any(|l| l.kind == '+' && l.text.contains("中文"))
    );
    assert!(
        diff.lines
            .iter()
            .any(|l| l.kind == '-' && l.text.contains("Original"))
    );
    assert_eq!(tree(&w), before);
    let (_, sha) = baselines::load(&w, "BASE-1").unwrap();
    let mut approval = json!({"id":"DEC-1","type":"decision","decision_type":"baseline_approval","baseline_id":"BASE-1","baseline_sha256":sha,"workflow_status":"accepted","decided_by":"Synthetic actor","decided_at":"2026-10-01","decision_evidence":["[[00_Governance/Project]]"]});
    put(
        &w,
        "07_Decisions/DEC-1.md",
        approval.clone(),
        "# Synthetic approval fixture",
    );
    assert_eq!(
        inspect(&w, "REQ-1", Target::Baseline("BASE-1".into()))
            .comparison
            .unwrap()
            .basis["approval_verified"],
        true
    );
    approval["workflow_status"] = json!("rescinded");
    put(
        &w,
        "07_Decisions/DEC-1.md",
        approval,
        "# Synthetic rescission fixture",
    );
    let c = inspect(&w, "REQ-1", Target::Baseline("BASE-1".into()))
        .comparison
        .unwrap();
    assert_eq!(c.basis["approval_verified"], false);
    assert!(
        c.basis["approval_note"]
            .as_str()
            .unwrap()
            .contains("rescinded")
    );
    assert_eq!(
        std::fs::read(w.root.join("00_Governance/Baselines/BASE-1/files/REQ-1.md")).unwrap(),
        pinned
    );
    assert!(
        inspect(&w, "PROJECT-TEST", Target::Baseline("BASE-1".into()))
            .comparison_error
            .unwrap()
            .contains("not in")
    );
    std::fs::write(
        w.root.join("00_Governance/Baselines/BASE-1/files/REQ-1.md"),
        "tampered",
    )
    .unwrap();
    assert!(
        inspect(&w, "REQ-1", Target::Baseline("BASE-1".into()))
            .comparison
            .is_none()
    );
}
fn repo_fixture(w: &Workspace) {
    for (path, text) in [
        ("src/main.rs", "fn old() {}\n"),
        ("openwiki/overview.md", "# Old documented behavior\n"),
    ] {
        workspace::atomic_write(
            &w.path(&format!("01_Sources/Assets/repository/{path}"))
                .unwrap(),
            text.as_bytes(),
        )
        .unwrap();
    }
    sources::add_repo(
        w,
        "test",
        "01_Sources/Assets/repository",
        "openwiki",
        &["src".into()],
    )
    .unwrap();
    sources::sync(&Records::load(w).unwrap(), "test").unwrap();
    for (id, path) in [
        ("EVD-CODE", "src/main.rs"),
        ("EVD-WIKI", "openwiki/overview.md"),
    ] {
        sources::evidence(
            &Records::load(w).unwrap(),
            sources::Evidence {
                id,
                claim: "Synthetic recorded statement",
                source: None,
                locator: None,
                repo: Some("test"),
                path: Some(path),
                start: Some(1),
                end: Some(1),
            },
        )
        .unwrap();
    }
}
#[test]
fn source_diff_uses_registered_snapshots_and_never_reads_live_repository() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    let (_dir, w) = fixture();
    repo_fixture(&w);
    assert_eq!(
        inspect(&w, "EVD-CODE", Target::Source)
            .comparison
            .unwrap()
            .status,
        "unchanged"
    );
    std::fs::write(
        w.root.join("01_Sources/Assets/repository/src/main.rs"),
        "fn updated() {}\n",
    )
    .unwrap();
    std::fs::write(
        w.root
            .join("01_Sources/Assets/repository/openwiki/overview.md"),
        "# New documented behavior\n",
    )
    .unwrap();
    assert_eq!(
        inspect(&w, "EVD-CODE", Target::Source)
            .comparison
            .unwrap()
            .status,
        "unchanged"
    );
    sources::sync(&Records::load(&w).unwrap(), "test").unwrap();
    // Removing the live repository proves inspection uses only captured files.
    std::fs::remove_dir_all(w.root.join("01_Sources/Assets/repository")).unwrap();
    let before = tree(&w);
    for (id, kind) in [
        ("EVD-CODE", "observed_implementation"),
        ("EVD-WIKI", "documented_behavior"),
    ] {
        let c = inspect(&w, id, Target::Source).comparison.unwrap();
        assert!(c.status.starts_with("changed"));
        assert_eq!(c.basis["assertion_type"], kind);
        assert_ne!(c.basis["old_snapshot"], c.basis["new_snapshot"]);
        assert_ne!(
            c.diff.as_ref().unwrap().old_sha256,
            c.diff.as_ref().unwrap().new_sha256
        );
    }
    assert_eq!(tree(&w), before);
}
#[test]
fn missing_snapshot_files_are_not_misreported_as_deleted_and_tampering_fails() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _guard = runtime.enter();
    let (_dir, w) = fixture();
    repo_fixture(&w);
    std::fs::remove_file(w.root.join("01_Sources/Assets/repository/src/main.rs")).unwrap();
    sources::sync(&Records::load(&w).unwrap(), "test").unwrap();
    let c = inspect(&w, "EVD-CODE", Target::Source).comparison.unwrap();
    assert!(c.status.starts_with("not_captured"));
    assert!(c.diff.is_none());
    let e = records::read(&w, "01_Sources/Evidence/EVD-CODE.md").unwrap();
    let snapshot = e.metadata["snapshot_id"].as_str().unwrap();
    let pinned = w
        .root
        .join(format!("03_Systems/Snapshots/{snapshot}/files/src/main.rs"));
    std::fs::write(&pinned, "corrupted").unwrap();
    assert!(
        inspect(&w, "EVD-CODE", Target::Source)
            .comparison_error
            .unwrap()
            .contains("hash mismatch")
    );
    assert!(
        inspect(&w, "REQ-1", Target::Source)
            .comparison_error
            .unwrap()
            .contains("evidence")
    );
}
#[test]
fn diffs_reconstruct_both_inputs_and_bound_expensive_cases() {
    let mut texts = vec![String::new()];
    for first in ["a\n", "b\n", "中文\r\n", "x\t\u{1b}\n"] {
        for second in ["", "a", "b\n", "中文\r\n"] {
            texts.push(format!("{first}{second}"));
        }
    }
    for a in &texts {
        for b in &texts {
            let d = inspection::diff(a, b).unwrap();
            for (excluded, final_newline, expected) in
                [('+', d.old_final_newline, a), ('-', d.new_final_newline, b)]
            {
                let selected = d
                    .lines
                    .iter()
                    .filter(|l| l.kind != excluded)
                    .collect::<Vec<_>>();
                let mut restored = String::new();
                for (i, l) in selected.iter().enumerate() {
                    assert_eq!(if excluded == '+' { l.old } else { l.new }, Some(i + 1));
                    restored.push_str(&l.text);
                    if i + 1 < selected.len() || final_newline {
                        restored.push('\n');
                    }
                }
                assert_eq!(&restored, expected);
            }
        }
    }
    let a = "old\n".repeat(1100);
    let b = "new\n".repeat(1100);
    let d = inspection::diff(&a, &b).unwrap();
    assert!(d.algorithm.contains("coarse"));
    assert_eq!(d.lines.len(), 2200);
    assert!(inspection::diff(&"x\n".repeat(20_001), "").is_err());
}
#[tokio::test]
async fn agent_tool_and_cli_report_explicit_comparison_failures() {
    let (_dir, w) = fixture();
    let before = tree(&w);
    let args = json!({"command":"inspect","args":["REQ-1"]});
    assert!(!tools::requires_approval("pm", &args));
    let result = tools::execute(&w, "pm", &args).await.unwrap();
    assert_eq!(result["successful"], true);
    assert_eq!(result["id"], "REQ-1");
    let result = tools::run_pm(
        &w,
        "inspect",
        &["REQ-1".into(), "--baseline".into(), "MISSING".into()],
    )
    .await
    .unwrap();
    assert_eq!(result["successful"], false);
    assert!(
        result["stderr"]
            .as_str()
            .unwrap()
            .contains("Comparison unavailable")
    );
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .args([
            "--vault",
            w.root.to_str().unwrap(),
            "pm",
            "inspect",
            "REQ-1",
        ])
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut after = tree(&w);
    assert_eq!(
        after.remove(".pm-agent/workspace.lock"),
        Some(workspace::hash(b""))
    );
    assert_eq!(after, before);
}
