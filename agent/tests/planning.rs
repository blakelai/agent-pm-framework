use pm_agent::{
    planning, tools,
    workspace::{Workspace, note, parse_note},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn cases() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let python = std::env::var("PM_TEST_PYTHON").unwrap_or_else(|_| "python3".into());
    let output = Command::new(python)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/planning_fixtures.py"))
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    dir
}
fn near(actual: f64, expected: f64, case: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{case}: {actual} != {expected}"
    );
}

#[tokio::test]
async fn native_forecasts_match_existing_python_contract_and_run_without_python() {
    let dir = cases();
    let cases: Vec<Value> =
        serde_json::from_slice(&fs::read(dir.path().join("cases.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 41);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let workspace = Workspace::open(&dir.path().join(name)).unwrap();
        // A nonexistent interpreter proves that the production tool dispatch is native.
        let result = tools::run_pm(&workspace, "/no-python-for-this-operation", "plan", &[])
            .await
            .unwrap();
        assert_eq!(result["engine"], "rust", "{name}");
        let report = workspace.read("06_Delivery/Plan-Report.md").unwrap();
        let (meta, body) = parse_note(&report).unwrap();
        if case.get("error").is_some() {
            assert_eq!(
                result["successful"], false,
                "{name}: Python rejected but Rust accepted"
            );
            assert_eq!(meta["workflow_status"], "blocked", "{name}");
            assert!(!result["stderr"].as_str().unwrap().is_empty(), "{name}");
            assert!(planning::calculate(&workspace).is_err(), "{name}");
            continue;
        }
        assert_eq!(result["successful"], true, "{name}: {result}");
        assert_eq!(meta["workflow_status"], "forecast", "{name}");
        let expected = &case["expected"];
        let forecast = &result["forecast"];
        assert_eq!(
            body.trim(),
            expected["body"].as_str().unwrap(),
            "Report parity: {name}"
        );
        assert_eq!(forecast["as_of"], expected["as_of"], "{name}");
        assert_eq!(forecast["messages"], expected["messages"], "{name}");
        let actual_caps = forecast["caps"].as_array().unwrap();
        let expected_caps = expected["caps"].as_array().unwrap();
        assert_eq!(actual_caps.len(), expected_caps.len(), "{name}");
        for (actual, expected) in actual_caps.iter().zip(expected_caps) {
            for key in ["sprint", "role", "items"] {
                assert_eq!(actual[key], expected[key], "{name}");
            }
            for key in ["capacity", "effort"] {
                near(
                    actual[key].as_f64().unwrap(),
                    expected[key].as_f64().unwrap(),
                    name,
                );
            }
        }
        let items = forecast["items"].as_array().unwrap();
        assert_eq!(items.len(), expected["totals"].as_object().unwrap().len());
        for item in items {
            let id = item["id"].as_str().unwrap();
            for (key, source) in [("remaining", "totals"), ("original", "original")] {
                if let Some(value) = expected[source][id].as_f64() {
                    near(item[key].as_f64().unwrap(), value, name);
                } else {
                    assert!(item[key].is_null(), "{name}: {id} must remain unknown");
                }
            }
        }
    }
}

#[test]
fn invalid_locales_preserve_report_and_inputs_cannot_escape_workspace() {
    let dir = cases();
    let workspace = Workspace::open(&dir.path().join("english")).unwrap();
    let path = workspace.path("06_Delivery/Plan-Report.md").unwrap();
    let previous = fs::read(&path).unwrap();
    let locale = workspace.path("tools/locales/en.json").unwrap();
    let mut data: Value = serde_json::from_slice(&fs::read(&locale).unwrap()).unwrap();
    data["plan_intro"] = json!("{invented}");
    fs::write(&locale, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(
        planning::run(&workspace, &[])
            .unwrap_err()
            .to_string()
            .contains("placeholders")
    );
    assert_eq!(fs::read(&path).unwrap(), previous);
    let settings = workspace.path("00_Governance/Settings.md").unwrap();
    fs::write(
        &settings,
        note(
            json!({"type":"settings","agent_language":"en","document_language":"../../en"}),
            "# Settings",
        )
        .unwrap(),
    )
    .unwrap();
    assert!(planning::run(&workspace, &[]).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    fs::write(
        &settings,
        note(
            json!({"type":"settings","agent_language":"en","document_language":"de-DE"}),
            "# Settings",
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        planning::run(&workspace, &[])
            .unwrap_err()
            .to_string()
            .contains("No project-document renderer")
    );
    assert_eq!(fs::read(&path).unwrap(), previous);
    // Even helper usage and invalid argument paths do not need an interpreter or mutate the report.
    assert!(
        planning::run(&workspace, &["--help".into()]).unwrap()["successful"]
            .as_bool()
            .unwrap()
    );
    assert!(planning::run(&workspace, &["--output".into(), "outside.md".into()]).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    #[cfg(unix)]
    {
        let workspace = Workspace::open(&dir.path().join("zh_tw")).unwrap();
        let pbi = workspace.path("05_Backlog/PBI-001.md").unwrap();
        fs::remove_file(&pbi).unwrap();
        std::os::unix::fs::symlink(dir.path().join("base/05_Backlog/PBI-001.md"), &pbi).unwrap();
        let result = planning::run(&workspace, &[]).unwrap();
        assert_eq!(result["successful"], false);
        assert!(result["stderr"].as_str().unwrap().contains("symlinks"));
    }
}
