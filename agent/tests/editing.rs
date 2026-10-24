use pm_agent::{
    baselines,
    editing::{Edit, Target},
    planning,
    records::{self, Records},
    workspace::{self, Workspace},
};
use serde_json::{Value, json};

fn put(w: &Workspace, path: &str, meta: Value, body: &str) {
    workspace::atomic_write(
        &w.path(path).unwrap(),
        workspace::note(meta, body).unwrap().as_bytes(),
    )
    .unwrap();
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("00_Governance")).unwrap();
    std::fs::write(dir.path().join("00_Governance/Project.md"),workspace::note(json!({"id":"PROJECT-TEST","type":"project","title":"Synthetic project","language":"en"}),"# Synthetic fixture").unwrap()).unwrap();
    let w = Workspace::open(dir.path()).unwrap();
    put(
        &w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","title":"Settings","document_language":"en","agent_language":"en","language":"en"}),
        "# Settings",
    );
    workspace::atomic_write(
        &w.path("tools/locales/en.json").unwrap(),
        include_bytes!("../../tools/locales/en.json"),
    )
    .unwrap();
    put(
        &w,
        "06_Delivery/Planning.md",
        json!({"id":"PLAN-CONFIG","type":"planning_config","title":"Planning","language":"en","as_of":"2026-11-02"}),
        "# Planning",
    );
    put(
        &w,
        "06_Delivery/Iterations.md",
        json!({"type":"Reference","title":"Iterations","language":"en"}),
        "# Iterations\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n| 1 | 2026-11-02 | 2026-11-13 | Synthetic goal |\n\nKeep this paragraph.",
    );
    put(
        &w,
        "06_Delivery/People.md",
        json!({"type":"Reference","title":"People","language":"en"}),
        "# People\n\n| Person | Role | Sprint | Project | FTE | Workdays | Absence | Focus | Reserve |\n|---|---|---|---|---|---|---|---|---|\n| Fixture person | Backend | 1 | PROJECT-TEST | 1 | 10 | 0 | 0.8 | 0.1 |\n\nKeep this paragraph.",
    );
    put(
        &w,
        "02_Domain/test/CONTEXT.md",
        json!({"id":"LANG-test","type":"glossary","context":"test","title":"Test glossary","language":"en","version":1,"doc_status":"draft"}),
        "# Test glossary",
    );
    put(
        &w,
        "07_Decisions/QUESTION-1.md",
        json!({"id":"QUESTION-1","type":"question","title":"Synthetic question","language":"en","workflow_status":"open"}),
        "# Synthetic question",
    );
    put(
        &w,
        "05_Backlog/PBI-1.md",
        json!({"id":"PBI-1","type":"pbi","title":"Synthetic enabler","language":"en","owner":"Fixture person","contexts":["test"],"work_type":"enabler","delivery_status":"proposed","origins":["[[07_Decisions/QUESTION-1]]"],"sprint":null,"custom_annotation":"preserved"}),
        "# Synthetic enabler\n\n- AC-01: A measurable fixture outcome.\n\n| Role | O | M | P |\n|---|---|---|---|\n",
    );
    (dir, w)
}
fn set(e: &mut Edit, key: &str, value: &str) {
    e.apply(&Target::Property(key.into()), value).unwrap();
}
fn row(e: &mut Edit, table: usize, values: &[&str]) {
    e.apply(&Target::AddRow(table), "").unwrap();
    let r = e
        .entries()
        .iter()
        .filter_map(|e| {
            if let Target::Cell(t, r, _) = e.target {
                (t == table).then_some(r)
            } else {
                None
            }
        })
        .max()
        .unwrap();
    for (c, value) in values.iter().enumerate() {
        e.apply(&Target::Cell(table, r, c), value).unwrap();
    }
}
fn save(w: &Workspace, e: &Edit) {
    let preview = e.preview(w).unwrap();
    let saved = e.save(w, &preview).unwrap();
    assert!(saved.warning.is_none());
    assert!(
        w.read(saved.audit_path.as_deref().unwrap())
            .unwrap()
            .contains("Agent record language: en")
    );
}

#[test]
fn capacity_edits_preview_without_writing_and_preserve_prose() {
    let (_d, w) = fixture();
    let original = w.read("06_Delivery/People.md").unwrap();
    let mut e = Edit::load(&w, "06_Delivery/People.md").unwrap();
    e.apply(&Target::Cell(0, 0, 4), "0.5").unwrap();
    let preview = e.preview(&w).unwrap();
    assert_eq!(w.read("06_Delivery/People.md").unwrap(), original);
    assert!(preview.changes.iter().any(|s| s.contains("0.5")));
    assert!((planning::calculate(&w).unwrap().caps[0].capacity - 7.2).abs() < 1e-9);
    e.save(&w, &preview).unwrap();
    assert!((planning::calculate(&w).unwrap().caps[0].capacity - 3.6).abs() < 1e-9);
    assert!(
        w.read("06_Delivery/People.md")
            .unwrap()
            .contains("Keep this paragraph.")
    );
    let mut e = Edit::load(&w, "06_Delivery/People.md").unwrap();
    row(
        &mut e,
        0,
        &[
            "Fixture person",
            "QA",
            "1",
            "PROJECT-OTHER",
            "0.6",
            "10",
            "0",
            "0.8",
            "0.1",
        ],
    );
    assert!(
        e.preview(&w)
            .unwrap_err()
            .to_string()
            .contains("overallocated")
    );
    e.apply(&Target::Cell(0, 1, 4), "0.5").unwrap();
    e.apply(&Target::Cell(0, 1, 5), "").unwrap();
    assert!(e.preview(&w).is_err());
    e.apply(&Target::RemoveRow(0, 1), "").unwrap();
    e.apply(&Target::Cell(0, 0, 5), "99").unwrap();
    assert!(format!("{:#}", e.preview(&w).unwrap_err()).contains("calendar days"));
}

#[test]
fn iterations_and_forecast_date_are_checked_without_inventing_capacity() {
    let (_d, w) = fixture();
    let mut e = Edit::load(&w, "06_Delivery/Iterations.md").unwrap();
    row(&mut e, 0, &["2", "2026-11-12", "2026-11-20", ""]);
    assert!(e.preview(&w).unwrap_err().to_string().contains("overlap"));
    e.apply(&Target::Cell(0, 1, 1), "2026-11-16").unwrap();
    save(&w, &e);
    let mut e = Edit::load(&w, "06_Delivery/Planning.md").unwrap();
    assert!(
        e.apply(&Target::Property("as_of".into()), "2026-2-3")
            .is_err()
    );
    set(&mut e, "as_of", "");
    assert!(
        e.preview(&w)
            .unwrap()
            .warnings
            .iter()
            .any(|s| s.contains("Forecast unavailable"))
    );
    save(&w, &e);
    assert!(planning::calculate(&w).is_err());
    let mut e = Edit::load(&w, "06_Delivery/Iterations.md").unwrap();
    e.apply(&Target::RemoveRow(0, 0), "").unwrap();
    assert!(
        e.preview(&w)
            .unwrap_err()
            .to_string()
            .contains("missing Sprint 1")
    );
}

#[test]
fn pbi_status_and_remaining_estimates_use_actual_readiness_inputs() {
    let (_d, w) = fixture();
    let mut e = Edit::load(&w, "05_Backlog/PBI-1.md").unwrap();
    set(&mut e, "title", "Updated synthetic enabler");
    let preview = e.preview(&w).unwrap();
    assert!(preview.content.contains("sprint: null"));
    let mut cancelled = e.clone();
    set(&mut cancelled, "delivery_status", "cancelled");
    set(&mut cancelled, "sprint", "1");
    assert!(cancelled.preview(&w).is_ok());
    set(&mut e, "delivery_status", "ready");
    assert!(
        e.preview(&w)
            .unwrap_err()
            .to_string()
            .contains("READINESS_RECORD")
    );
    set(&mut e, "ready_by", "Synthetic team");
    set(&mut e, "ready_at", "2026-11-02");
    set(&mut e, "readiness_evidence", "[[00_Governance/Project]]");
    save(&w, &e);
    let mut e = Edit::load(&w, "05_Backlog/PBI-1.md").unwrap();
    set(&mut e, "sprint", "1");
    let mut retyped = e.clone();
    set(&mut retyped, "work_type", "bug");
    assert!(
        retyped
            .preview(&w)
            .unwrap_err()
            .to_string()
            .contains("Work type cannot change")
    );
    set(&mut e, "delivery_status", "in_progress");
    assert!(e.preview(&w).is_err());
    row(&mut e, 0, &["Backend", "1", "2", "3"]);
    assert!(e.preview(&w).unwrap_err().to_string().contains("remaining"));
    row(&mut e, 1, &["Backend", "1", "1", "2"]);
    save(&w, &e);
    assert!(
        w.read("05_Backlog/PBI-1.md")
            .unwrap()
            .contains("custom_annotation: \"preserved\"")
    );
    let forecast = planning::calculate(&w).unwrap();
    assert_eq!(forecast.items[0].original, Some(2.));
    assert!((forecast.items[0].remaining.unwrap() - 7. / 6.).abs() < 1e-9);
}

#[test]
fn feature_readiness_and_done_require_the_pinned_baseline_and_passing_run() {
    let (_d, w) = fixture();
    put(
        &w,
        "04_Requirements/PRD/PRD-1.md",
        json!({"id":"PRD-1","type":"prd","title":"Synthetic PRD","language":"en","version":1,"doc_status":"draft"}),
        "# Synthetic PRD",
    );
    put(
        &w,
        "04_Requirements/REQ-1.md",
        json!({"id":"REQ-1","type":"requirement","title":"Synthetic requirement","language":"en","version":1,"doc_status":"draft"}),
        "# Synthetic requirement",
    );
    put(
        &w,
        "08_Verification/Cases/TC-1.md",
        json!({"id":"TC-1","type":"testcase","title":"Synthetic case","language":"en","requirements":["[[04_Requirements/REQ-1]]"]}),
        "# Synthetic case",
    );
    let mut e = Edit::load(&w, "05_Backlog/PBI-1.md").unwrap();
    set(&mut e, "work_type", "feature");
    set(&mut e, "parents", "[[04_Requirements/PRD/PRD-1]]");
    set(&mut e, "satisfies", "[[04_Requirements/REQ-1]]");
    set(&mut e, "delivery_status", "ready");
    set(&mut e, "ready_by", "Synthetic team");
    set(&mut e, "ready_at", "2026-11-02");
    set(&mut e, "readiness_evidence", "[[00_Governance/Project]]");
    assert!(
        e.preview(&w)
            .unwrap_err()
            .to_string()
            .contains("EXECUTION_BASELINE")
    );
    baselines::capture(
        &Records::load(&w).unwrap(),
        "BASE-TEST",
        &["PRD-1".into(), "REQ-1".into(), "LANG-test".into()],
    )
    .unwrap();
    let (_, sha) = baselines::load(&w, "BASE-TEST").unwrap();
    put(
        &w,
        "07_Decisions/DEC-BASE.md",
        json!({"id":"DEC-BASE","type":"decision","title":"Synthetic approval fixture","language":"en","decision_type":"baseline_approval","baseline_id":"BASE-TEST","baseline_sha256":sha,"workflow_status":"accepted","decided_by":"Synthetic fixture actor","decided_at":"2026-11-02T08:00:00Z","decision_evidence":["[[00_Governance/Project]]"]}),
        "# Synthetic approval fixture",
    );
    set(&mut e, "baseline_id", "BASE-TEST");
    save(&w, &e);
    let mut e = Edit::load(&w, "05_Backlog/PBI-1.md").unwrap();
    set(&mut e, "delivery_status", "done");
    assert!(e.preview(&w).is_err());
    let req = records::read(&w, "04_Requirements/REQ-1.md").unwrap();
    let mut run = json!({"id":"TR-1","type":"test_run","title":"Synthetic test run","language":"en","result":"passed","cases":["[[08_Verification/Cases/TC-1]]"],"requirement_versions":[records::fingerprint(&req).unwrap()],"executed_at":"2026-11-02T09:00:00Z","executed_by":"Synthetic test actor","environment":"fixture","build":"fixture-sha","evidence":["[[00_Governance/Project]]"],"simulation":true});
    put(
        &w,
        "08_Verification/TR-1.md",
        run.clone(),
        "# Synthetic test run",
    );
    set(&mut e, "acceptance_evidence", "[[08_Verification/TR-1]]");
    set(&mut e, "dod_evidence", "[[00_Governance/Project]]");
    set(&mut e, "ac_coverage", "AC-01=TC-1");
    assert!(
        e.preview(&w)
            .unwrap_err()
            .to_string()
            .contains("DONE_TEST_RESULTS")
    );
    run["simulation"] = json!(false);
    put(&w, "08_Verification/TR-1.md", run, "# Synthetic test run");
    save(&w, &e);
}

#[test]
fn decision_and_risk_creation_preserve_language_and_require_outcome_evidence() {
    let (_d, w) = fixture();
    let mut decision = Edit::create(&w, "decision").unwrap();
    set(&mut decision, "id", "DEC-NEW-TEST");
    set(&mut decision, "title", "Synthetic decision");
    decision
        .apply(&Target::Body, "# Synthetic decision\n\nSupplied outcome.")
        .unwrap();
    set(&mut decision, "workflow_status", "accepted");
    assert!(decision.preview(&w).is_err());
    set(&mut decision, "decided_by", "Synthetic actor");
    set(&mut decision, "decided_at", "2026-11-02");
    set(
        &mut decision,
        "decision_evidence",
        "[[00_Governance/Project]]",
    );
    save(&w, &decision);
    assert!(Edit::load(&w, "07_Decisions/DEC-NEW-TEST.md").is_err());
    put(
        &w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","title":"Settings","document_language":"zh-TW","agent_language":"en","language":"en"}),
        "# Settings",
    );
    let mut risk = Edit::create(&w, "risk").unwrap();
    set(&mut risk, "id", "RISK-TEST");
    set(&mut risk, "title", "合成測試風險");
    risk.apply(
        &Target::Body,
        "# 合成測試風險\n\n測試資料，不代表實際事件。",
    )
    .unwrap();
    set(&mut risk, "workflow_status", "closed");
    assert!(risk.preview(&w).is_err());
    set(&mut risk, "evidence", "[[00_Governance/Project]]");
    save(&w, &risk);
    assert!(
        w.read("07_Decisions/RISK-TEST.md")
            .unwrap()
            .contains("language: \"zh-TW\"")
    );
    assert!(Edit::load(&w, "80_Templates/Risk.md").is_err());
}

#[test]
fn stale_document_or_evidence_changes_invalidate_review_and_cannot_overwrite() {
    let (_d, w) = fixture();
    let mut e = Edit::load(&w, "05_Backlog/PBI-1.md").unwrap();
    set(&mut e, "title", "Changed title");
    let reviewed = e.preview(&w).unwrap();
    let question = w.read("07_Decisions/QUESTION-1.md").unwrap();
    workspace::atomic_write(
        &w.path("07_Decisions/QUESTION-1.md").unwrap(),
        format!("{question}\nNew information.\n").as_bytes(),
    )
    .unwrap();
    assert!(e.save(&w, &reviewed).is_err());
    assert!(
        w.read("05_Backlog/PBI-1.md")
            .unwrap()
            .contains("Synthetic enabler")
    );
    let refreshed = e.preview(&w).unwrap();
    let original = w.read("05_Backlog/PBI-1.md").unwrap();
    workspace::atomic_write(
        &w.path("05_Backlog/PBI-1.md").unwrap(),
        format!("{original}\nExternal edit.\n").as_bytes(),
    )
    .unwrap();
    assert!(e.save(&w, &refreshed).is_err());
    assert!(
        w.read("05_Backlog/PBI-1.md")
            .unwrap()
            .contains("External edit.")
    );
    assert!(Edit::load(&w, "../outside.md").is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            w.path("05_Backlog/PBI-1.md").unwrap(),
            w.path("05_Backlog/LINK.md").unwrap(),
        )
        .unwrap();
        assert!(Edit::load(&w, "05_Backlog/LINK.md").is_err());
    }
}
