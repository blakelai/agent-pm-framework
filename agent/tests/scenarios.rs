//! Synthetic planning inputs and the offline scenario CLI.
use pm_agent::{
    scenarios,
    workspace::{self, Workspace},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

const A: &str = "05_Backlog/PBI-A.md";
const B: &str = "05_Backlog/PBI-B.md";
const QUESTION: &str = "07_Decisions/QUESTION-1.md";
const REPORT: &str = "06_Delivery/Plan-Report.md";

fn put(w: &Workspace, path: &str, meta: Value, body: &str) {
    write(w, path, &workspace::note(meta, body).unwrap());
}

fn write(w: &Workspace, path: &str, text: &str) {
    workspace::atomic_write(&w.path(path).unwrap(), text.as_bytes()).unwrap();
}

fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace {
        root: dir.path().to_owned(),
    };
    put(
        &w,
        "00_Governance/Project.md",
        json!({"id":"PROJECT-TEST","type":"project","title":"Synthetic","language":"en"}),
        "# Project",
    );
    put(
        &w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","title":"Settings","document_language":"en","agent_language":"en","language":"en"}),
        "# Settings",
    );
    write(
        &w,
        "tools/locales/en.json",
        include_str!("../../tools/locales/en.json"),
    );
    put(
        &w,
        "06_Delivery/Planning.md",
        json!({"type":"planning_config","title":"Planning","language":"en","as_of":"2026-11-02"}),
        "# Planning",
    );
    put(
        &w,
        "06_Delivery/Iterations.md",
        json!({"type":"Reference","title":"Iterations","language":"en"}),
        "# Iterations\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n| 1 | 2026-11-02 | 2026-11-13 | First |\n| 2 | 2026-11-16 | 2026-11-27 | Second |",
    );
    put(
        &w,
        "06_Delivery/People.md",
        json!({"type":"Reference","title":"People","language":"en"}),
        "# People\n\n| Person | Role | Sprint | Project | FTE | Workdays | Absence | Focus | Reserve |\n|---|---|---|---|---|---|---|---|---|\n| Test | Backend | 1 | PROJECT-TEST | 0.6 | 10 | 0 | 1 | 0 |\n| Test | Backend | 2 | PROJECT-TEST | 0.2 | 10 | 0 | 1 | 0 |",
    );
    put(
        &w,
        "02_Domain/test/CONTEXT.md",
        json!({"id":"LANG-test","type":"glossary","context":"test","title":"Test glossary","language":"en","version":1,"doc_status":"draft"}),
        "# Test glossary",
    );
    put(
        &w,
        QUESTION,
        json!({"id":"QUESTION-1","type":"question","title":"Synthetic question","language":"en","workflow_status":"open"}),
        "# Synthetic question",
    );
    for (path, id, sprint, effort, dependencies) in [
        (A, "PBI-A", Some(1), Some(3), vec![]),
        (B, "PBI-B", Some(2), Some(2), vec!["[[05_Backlog/PBI-A]]"]),
        ("05_Backlog/PBI-C.md", "PBI-C", None, None, vec![]),
    ] {
        let estimate = effort
            .map(|n| {
                format!(
                    "\n\n| Role | O | M | P |\n|---|---|---|---|\n|  Backend  | {n} | {n} | {n} |"
                )
            })
            .unwrap_or_default();
        let meta = json!({"id":id,"type":"pbi","title":id,"language":"en","owner":"Synthetic owner","contexts":["test"],
            "work_type":"enabler","delivery_status":"proposed","origins":["[[07_Decisions/QUESTION-1]]"],"sprint":sprint,
            "depends_on":dependencies,"custom_property":"preserved"});
        let text = workspace::note(
            meta,
            &format!("# {id}\n\n- AC-01: Fixture outcome.{estimate}"),
        )
        .unwrap();
        write(&w, path, &text.replace('\n', "\r\n"));
    }
    put(
        &w,
        REPORT,
        json!({"type":"report","title":"Historical forecast","language":"en"}),
        "# Historical forecast",
    );
    (dir, w)
}

fn cli(w: &Workspace, args: &[&str]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_pm-agent"));
    cmd.args([
        "--vault",
        w.root.to_str().unwrap(),
        "--config",
        "/missing-provider.toml",
        "scenario",
    ]);
    cmd.args(args).env("PATH", "").output().unwrap()
}

fn value(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn rejected(output: Output, message: &str) {
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains(message), "{diagnostic}");
}

fn specification() -> tempfile::NamedTempFile {
    let input = tempfile::NamedTempFile::new().unwrap();
    let data = json!({"scenarios":[
        {"id":"Swap","moves":[{"pbi":"PBI-A","sprint":2},{"pbi":"PBI-B","sprint":1}]},
        {"id":"Together","moves":[{"pbi":"PBI-A","sprint":2}]},
        {"id":"MissingEstimate","moves":[{"pbi":"PBI-C","sprint":2}]},
    ]});
    fs::write(input.path(), serde_json::to_vec(&data).unwrap()).unwrap();
    input
}

fn save_comparison(w: &Workspace, input: &Path) -> Value {
    value(cli(
        w,
        &["compare", input.to_str().unwrap(), "--id", "MODEL-1"],
    ))
}

#[test]
fn compares_pinned_capacity_dependencies_and_applies_only_the_reviewed_assignments() {
    let (_dir, w) = fixture();
    let before_a = w.read(A).unwrap();
    let before_b = w.read(B).unwrap();
    let historical_report = w.read(REPORT).unwrap();
    let spec = specification();
    let comparison = save_comparison(&w, spec.path());
    assert_eq!(comparison["simulation"], true);
    assert_eq!(
        comparison["inputs"][A],
        workspace::hash(before_a.as_bytes())
    );
    assert_eq!(
        comparison["inputs"]["tools/locales/en.json"],
        workspace::hash(include_bytes!("../../tools/locales/en.json"))
    );
    let swap = &comparison["scenarios"]["Swap"]["forecast"];
    let together = &comparison["scenarios"]["Together"]["forecast"];
    assert_eq!(swap["caps"][1]["capacity"], 2.0);
    assert_eq!(swap["caps"][1]["effort"], 3.0);
    assert_eq!(together["caps"][1]["effort"], 5.0);
    assert_ne!(swap["messages"], together["messages"]);
    assert!(swap["messages"].to_string().contains("PBI-A"));
    assert_eq!(
        comparison["scenarios"]["MissingEstimate"]["status"],
        "blocked"
    );
    assert!(
        comparison["scenarios"]["MissingEstimate"]["error"]
            .as_str()
            .unwrap()
            .contains("PBI-C")
    );
    assert_eq!(w.read(A).unwrap(), before_a);
    assert_eq!(w.read(B).unwrap(), before_b);
    assert_eq!(w.read(REPORT).unwrap(), historical_report);
    assert_eq!(value(cli(&w, &["show", "MODEL-1"])), comparison);
    let saved = scenarios::load(&w, "MODEL-1").unwrap();
    assert!(saved.describe("Swap").unwrap().contains("Over capacity"));
    rejected(cli(&w, &["review", "MODEL-1", "MissingEstimate"]), "PBI-C");

    let review = value(cli(&w, &["review", "MODEL-1", "Swap"]));
    assert_eq!(review["simulation"], true);
    assert_eq!(review["proposal"]["moves"].as_array().unwrap().len(), 2);
    let token = review["review_sha256"].as_str().unwrap();
    rejected(
        cli(
            &w,
            &["apply", "MODEL-1", "Swap", "--expect-review", "wrong"],
        ),
        "Scenario review changed",
    );
    let people = w.read("06_Delivery/People.md").unwrap();
    write(&w, "06_Delivery/People.md", &people.replace("0.2", "0.3"));
    rejected(
        cli(&w, &["apply", "MODEL-1", "Swap", "--expect-review", token]),
        "Planning inputs changed since simulation",
    );
    assert_eq!(value(cli(&w, &["show", "MODEL-1"])), comparison);
    write(&w, "06_Delivery/People.md", &people);
    let question = w.read(QUESTION).unwrap();
    write(&w, QUESTION, &(question.clone() + "\nOther source text.\n"));
    rejected(
        cli(&w, &["apply", "MODEL-1", "Swap", "--expect-review", token]),
        "Scenario review changed",
    );
    write(&w, QUESTION, &question);
    assert_eq!(w.read(A).unwrap(), before_a);
    assert_eq!(w.read(B).unwrap(), before_b);

    let applied = value(cli(
        &w,
        &["apply", "MODEL-1", "Swap", "--expect-review", token],
    ));
    assert_eq!(applied["status"], "applied");
    assert_eq!(
        w.read(A).unwrap(),
        before_a.replace("sprint: 1\r\n", "sprint: 2\r\n")
    );
    assert_eq!(
        w.read(B).unwrap(),
        before_b.replace("sprint: 2\r\n", "sprint: 1\r\n")
    );
    let audit = w.read(applied["audit_path"].as_str().unwrap()).unwrap();
    assert!(audit.contains("Simulated impact") && audit.contains(token));
    assert_eq!(w.read(REPORT).unwrap(), historical_report);
    rejected(
        cli(&w, &["apply", "MODEL-1", "Swap", "--expect-review", token]),
        "Planning inputs changed since simulation",
    );
}

#[test]
fn failed_batch_audit_rolls_back_all_assignments_and_missing_destinations_are_rejected() {
    let (_dir, w) = fixture();
    let spec = specification();
    let before = [w.read(A).unwrap(), w.read(B).unwrap()];
    save_comparison(&w, spec.path());
    let review = value(cli(&w, &["review", "MODEL-1", "Swap"]));
    // An ordinary file in the intended audit directory's place makes publication fail.
    write(&w, "90_Agent/Runs", "Synthetic writer obstruction");
    let token = review["review_sha256"].as_str().unwrap();
    rejected(
        cli(&w, &["apply", "MODEL-1", "Swap", "--expect-review", token]),
        "rolled back",
    );
    assert_eq!([w.read(A).unwrap(), w.read(B).unwrap()], before);
    assert!(
        !w.path(".pm-agent/planning-scenario-apply.json")
            .unwrap()
            .exists()
    );
    fs::remove_file(w.path("90_Agent/Runs").unwrap()).unwrap();
    let invalid = r#"{"scenarios":[{"id":"A","moves":[{"pbi":"PBI-A"}]},{"id":"B","moves":[{"pbi":"PBI-B","sprint":null}]}]}"#;
    fs::write(spec.path(), invalid).unwrap();
    rejected(
        cli(
            &w,
            &["compare", spec.path().to_str().unwrap(), "--id", "INVALID"],
        ),
        "missing field",
    );
}
