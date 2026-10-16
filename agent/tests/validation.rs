use pm_agent::{
    profile, tools, validation,
    workspace::{Workspace, note, parse_note},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn fixtures(base_only: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut command =
        Command::new(std::env::var("PM_TEST_PYTHON").unwrap_or_else(|_| "python3".into()));
    command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/validation_fixtures.py"))
        .arg(dir.path());
    if base_only {
        command.arg("base-only");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    dir
}
async fn run(workspace: &Workspace, strict: bool) -> Value {
    let args = if strict {
        vec!["--strict".into()]
    } else {
        vec![]
    };
    let result = tools::run_pm(workspace, "validate", &args).await.unwrap();
    assert_eq!(result["engine"], "rust");
    result
}
fn signatures(value: &Value) -> Vec<String> {
    let mut values: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|f| format!("{}|{}|{}", f["file"], f["level"], f["code"]))
        .collect();
    values.sort();
    values
}
#[tokio::test]
async fn native_validation_preserves_legacy_findings_and_strict_exit_contract() {
    let dir = fixtures(false);
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(dir.path().join("validation.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 79);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let workspace = Workspace::open(&dir.path().join(name)).unwrap();
        let expected = &case["findings"];
        let result = run(&workspace, false).await;
        assert!(result["findings"].is_array(), "{name}: {result}");
        assert_eq!(
            signatures(&result["findings"]),
            signatures(expected),
            "{name}\nnative: {}\nlegacy: {expected}",
            result["findings"]
        );
        let errors = expected
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["level"] == "ERROR")
            .count();
        let warnings = expected
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["level"] == "WARN")
            .count();
        assert_eq!(result["errors"], errors, "{name}");
        assert_eq!(result["warnings"], warnings, "{name}");
        assert_eq!(result["successful"], errors == 0, "{name}");
        assert_eq!(
            result["exit_code"],
            if errors == 0 { 0 } else { 1 },
            "{name}"
        );
        let (meta, body) = parse_note(result["reports"][0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(meta["language"], "en");
        assert!(body.contains(&format!("Errors: {errors}; warnings: {warnings}")));
        // These checks have deliberately stable human-facing messages as well as codes.
        for f in expected.as_array().unwrap().iter().filter(|f| {
            [
                "TERMINOLOGY",
                "OWNERSHIP",
                "DRAFT",
                "NEWER_DRAFT",
                "READINESS_RECORD",
                "VISUAL_REVIEW_REQUIRED",
                "STALE_EVIDENCE",
            ]
            .contains(&f["code"].as_str().unwrap())
        }) {
            assert!(
                result["findings"].as_array().unwrap().contains(f),
                "{name}: missing exact diagnostic {f}"
            );
        }
        if ["clean", "drafts", "missing_readiness", "done"].contains(&name) {
            let strict = run(&workspace, true).await;
            assert_eq!(
                strict["successful"],
                errors + warnings == 0,
                "strict {name}"
            );
        }
    }
}
#[tokio::test]
async fn malformed_records_and_symlinks_are_reported_without_hiding_other_findings() {
    let dir = fixtures(true);
    let workspace = Workspace::open(&dir.path().join("base")).unwrap();
    let original = workspace.read("04_Requirements/Items/REQ-001.md").unwrap();
    fs::write(
        workspace.path("04_Requirements/broken.md").unwrap(),
        "---\nid: broken\nid: repeated\n---\n# Broken",
    )
    .unwrap();
    fs::write(
        workspace.path("04_Requirements/nonutf8.md").unwrap(),
        [0xff, 0xfe],
    )
    .unwrap();
    fs::write(
        workspace.path("04_Requirements/bad-id.md").unwrap(),
        note(json!({"id":true,"type":"prd","title":"Bad ID"}), "# Bad ID").unwrap(),
    )
    .unwrap();
    let path = workspace.path("05_Backlog/PBI-001.md").unwrap();
    let (mut meta, body) = parse_note(&fs::read_to_string(&path).unwrap()).unwrap();
    meta["contexts"] = json!([{"invalid":"context"}]);
    meta["delivery_status"] = json!("ready");
    fs::write(&path, note(meta, &body).unwrap()).unwrap();
    let evidence_path = workspace
        .path("01_Sources/Evidence/EVD-CODE-001.md")
        .unwrap();
    let (mut evidence, evidence_body) =
        parse_note(&fs::read_to_string(&evidence_path).unwrap()).unwrap();
    evidence["source_path"] = json!("../outside-source-root");
    fs::write(&evidence_path, note(evidence, &evidence_body).unwrap()).unwrap();
    let result = run(&workspace, false).await;
    assert_eq!(result["exit_code"], 1, "{result}");
    for code in [
        "PROPERTIES",
        "PROPERTY_TYPE",
        "READINESS_RECORD",
        "EXECUTION_BASELINE",
        "EVIDENCE_SOURCE",
        "OKF_PROFILE",
    ] {
        assert!(
            result["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["code"] == code),
            "Missing {code}: {result}"
        );
    }
    assert_eq!(
        workspace.read("04_Requirements/Items/REQ-001.md").unwrap(),
        original
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            dir.path().join("private"),
            workspace.root.join("04_Requirements/linked.md"),
        )
        .unwrap();
        fs::write(&path, fs::read_to_string(&path).unwrap() + "\n[[linked]]\n").unwrap();
        let result = run(&workspace, false).await;
        assert!(
            result["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["code"] == "PROPERTIES" && f["file"] == "04_Requirements/linked.md")
        );
        assert!(
            result["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["code"] == "LINK" && f["file"] == "05_Backlog/PBI-001.md")
        );
    }
}
#[test]
fn cli_validation_reports_invalid_settings_without_starting_python() {
    let dir = fixtures(true);
    let workspace = Workspace::open(&dir.path().join("base")).unwrap();
    let path = workspace.path("00_Governance/Settings.md").unwrap();
    let (mut meta, body) = parse_note(&fs::read_to_string(&path).unwrap()).unwrap();
    meta["agent_language"] = json!("zh-TW");
    fs::write(&path, note(meta, &body).unwrap()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .arg("--vault")
        .arg(&workspace.root)
        .args(["pm", "validate"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["engine"], "rust");
    assert!(
        result["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["code"] == "LANGUAGE_SETTINGS")
    );
}
#[test]
fn authoring_profile_includes_guides_but_excludes_raw_private_and_skill_adapters() {
    let dir = fixtures(true);
    let workspace = Workspace::open(&dir.path().join("base")).unwrap();
    let before = profile::inspect_authored(&workspace).unwrap();
    assert!(before.errors.is_empty(), "{before:?}");
    for path in [
        "agent/new-guide.md",
        "01_Sources/Assets/raw.md",
        ".pm-agent/secret.md",
        ".agents/new/SKILL.md",
    ] {
        let path = workspace.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "# No metadata").unwrap();
    }
    let after = profile::inspect_authored(&workspace).unwrap();
    assert_eq!(after.documents_checked, before.documents_checked + 1);
    assert_eq!(after.errors.len(), 1);
    assert!(after.errors[0].starts_with("agent/new-guide.md:"));
    let findings = validation::inspect(&workspace).unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.code == "OKF_PROFILE" && f.message.starts_with("agent/new-guide.md:"))
    );
}
