//! Synthetic local fixtures only; no fixture actor or event represents a real project.
use pm_agent::{
    baselines,
    delivery::{self, PROFILE, Snapshot},
    editing::{Edit, Target},
    records::{self, Records},
    tools, traceability, validation,
    workspace::{self, Document, Workspace},
};
use serde_json::{Value, json};

fn put(w: &Workspace, path: &str, m: Value) {
    workspace::atomic_write(
        &w.path(path).unwrap(),
        workspace::note(
            m,
            "# Synthetic fixture\n\nThis is test data, not a project event.\n\nAC-01: The supplied synthetic outcome is observable.",
        )
        .unwrap()
        .as_bytes(),
    )
    .unwrap();
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("00_Governance")).unwrap();
    std::fs::write(dir.path().join("00_Governance/Project.md"), workspace::note(json!({"id":"PROJECT-TEST","type":"project","title":"Synthetic project","language":"en"}), "# Synthetic project").unwrap()).unwrap();
    let w = Workspace::open(dir.path()).unwrap();
    put(
        &w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","title":"Settings","language":"en","document_language":"en","agent_language":"en"}),
    );
    workspace::atomic_write(
        &w.path("tools/locales/en.json").unwrap(),
        include_bytes!("../../tools/locales/en.json"),
    )
    .unwrap();
    put(
        &w,
        "04_Requirements/REQ-1.md",
        json!({"id":"REQ-1","type":"requirement","title":"Synthetic requirement","language":"en","version":1,"doc_status":"draft"}),
    );
    put(
        &w,
        "05_Backlog/PBI-1.md",
        json!({"id":"PBI-1","type":"pbi","title":"Synthetic item","language":"en","owner":"Fixture owner","contexts":["fixture"],"parents":["[[04_Requirements/PRD-1]]"],"work_type":"feature","delivery_status":"done","baseline_id":"BASE-1","satisfies":["[[04_Requirements/REQ-1]]"],"ready_by":"Fixture owner","ready_at":"2026-10-01T09:00:00Z","readiness_evidence":["[[00_Governance/Project]]"],"acceptance_evidence":["[[08_Verification/TR-1]]"],"dod_evidence":["[[00_Governance/Project]]"],"ac_coverage":["AC-01=TC-1"]}),
    );
    put(
        &w,
        "08_Verification/TC-1.md",
        json!({"id":"TC-1","type":"testcase","title":"Synthetic case","language":"en","requirements":["[[04_Requirements/REQ-1]]"]}),
    );
    put(
        &w,
        "04_Requirements/PRD-1.md",
        json!({"id":"PRD-1","type":"prd","title":"Synthetic product scope","language":"en","version":1,"doc_status":"draft"}),
    );
    put(
        &w,
        "02_Domain/fixture/CONTEXT.md",
        json!({"id":"LANG-fixture","type":"glossary","title":"Synthetic glossary","context":"fixture","language":"en","version":1,"doc_status":"draft"}),
    );
    baselines::capture(
        &Records::load(&w).unwrap(),
        "BASE-1",
        &["REQ-1".into(), "PRD-1".into(), "LANG-fixture".into()],
    )
    .unwrap();
    let (_, hash) = baselines::load(&w, "BASE-1").unwrap();
    put(
        &w,
        "07_Decisions/DEC-BASE.md",
        json!({"id":"DEC-BASE","type":"decision","title":"Synthetic baseline approval","language":"en","decision_type":"baseline_approval","workflow_status":"accepted","baseline_id":"BASE-1","baseline_sha256":hash,"decided_by":"Fixture actor","decided_at":"2026-10-01T08:00:00Z","decision_evidence":["[[00_Governance/Project]]"]}),
    );
    let req = records::read(&w, "04_Requirements/REQ-1.md").unwrap();
    put(
        &w,
        "08_Verification/TR-1.md",
        json!({"id":"TR-1","type":"test_run","title":"Synthetic result","language":"en","result":"passed","cases":["[[08_Verification/TC-1]]"],"requirement_versions":[records::fingerprint(&req).unwrap()],"executed_by":"Fixture tester","executed_at":"2026-10-02T08:00:00Z","environment":"synthetic","build":"synthetic-sha","evidence":["[[00_Governance/Project]]"]}),
    );
    (dir, w)
}
fn uat() -> Value {
    json!({"id":"UAT-1","type":"uat","title":"Synthetic UAT","language":"en","workflow_schema":PROFILE,"workflow_status":"accepted","owner":"Fixture owner","baseline_id":"BASE-1","items":["[[05_Backlog/PBI-1]]"],"requirements":["[[04_Requirements/REQ-1]]"],"cases":["[[08_Verification/TC-1]]"],"test_runs":["[[08_Verification/TR-1]]"],"reviewed_by":"Fixture owner","reviewed_at":"2026-10-02T09:00:00Z","decision_evidence":["[[00_Governance/Project]]"]})
}
fn release() -> Value {
    json!({"id":"REL-1","type":"release","title":"Synthetic release","language":"en","workflow_schema":PROFILE,"workflow_status":"released","owner":"Fixture owner","baseline_id":"BASE-1","items":["[[05_Backlog/PBI-1]]"],"test_runs":["[[08_Verification/TR-1]]"],"uat_records":["[[08_Verification/UAT/UAT-1]]"],"go_no_go":"go","decided_by":"Fixture owner","decided_at":"2026-10-02T10:00:00Z","decision_evidence":["[[00_Governance/Project]]"],"cutover_plan":["[[00_Governance/Project]]"],"rollback_plan":["[[00_Governance/Project]]"],"handover":["[[00_Governance/Project]]"],"released_by":"Fixture operator","released_at":"2026-10-02T11:00:00Z","release_evidence":["[[00_Governance/Project]]"]})
}
fn document(m: Value) -> Document {
    Document {
        path: format!(
            "{}{}.md",
            delivery::folder(m["type"].as_str().unwrap()).unwrap(),
            m["id"].as_str().unwrap()
        ),
        title: m["title"].as_str().unwrap().into(),
        kind: m["type"].as_str().unwrap().into(),
        id: m["id"].as_str().map(String::from),
        hash: "synthetic".into(),
        metadata: m,
        body: "# Synthetic fixture".into(),
    }
}
fn check(w: &Workspace, m: Value) -> anyhow::Result<()> {
    delivery::check(&Records::load(w)?, &document(m))
}
fn set(e: &mut Edit, key: &str, value: &str) {
    e.apply(&Target::Property(key.into()), value).unwrap();
}
fn draft(w: &Workspace, kind: &str, id: &str) -> Edit {
    let mut e = Edit::create(w, kind).unwrap();
    set(&mut e, "id", id);
    set(&mut e, "title", "Synthetic fixture");
    e.apply(&Target::Body, "# Synthetic fixture\n\nOnly supplied facts.")
        .unwrap();
    e
}

#[test]
fn four_draft_types_preserve_unknown_inputs_and_project_language() {
    let (_dir, w) = fixture();
    for (i, kind) in delivery::KINDS.iter().enumerate() {
        let e = draft(&w, kind, &format!("RECORD-{i}"));
        let preview = e.preview(&w).unwrap();
        let (m, _) = records::parse_project_note(&preview.content).unwrap();
        assert_eq!(m["workflow_schema"], PROFILE);
        assert_eq!(m["workflow_status"], "draft");
        assert!(m["owner"].is_null());
        assert!(m["released_at"].is_null());
        assert!(!w.path(&preview.path).unwrap().exists());
        let saved = e.save(&w, &preview).unwrap();
        assert!(saved.audit_path.is_some());
    }
    put(
        &w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","title":"Settings","language":"en","document_language":"zh-TW","agent_language":"en"}),
    );
    let mut e = draft(&w, "release", "REL-ZH");
    set(&mut e, "title", "合成發布草稿");
    e.apply(&Target::Body, "# 合成發布草稿\n\n測試用資料。")
        .unwrap();
    assert!(
        e.preview(&w)
            .unwrap()
            .content
            .contains("language: \"zh-TW\"")
    );
}
#[test]
fn change_decision_requires_impact_and_preserves_history() {
    let (_dir, w) = fixture();
    let mut e = draft(&w, "change", "CHANGE-1");
    set(&mut e, "workflow_status", "accepted");
    assert!(e.preview(&w).is_err());
    for (k, v) in [
        ("owner", "Fixture owner"),
        ("decided_by", "Fixture owner"),
        ("decided_at", "2026-10-03T09:00:00+08:00"),
        ("decision_evidence", "[[00_Governance/Project]]"),
        ("affected_records", "[[04_Requirements/REQ-1]]"),
    ] {
        set(&mut e, k, v);
    }
    assert!(
        e.preview(&w)
            .unwrap_err()
            .to_string()
            .contains("impact_evidence")
    );
    set(&mut e, "impact_evidence", "[[00_Governance/Project]]");
    let p = e.preview(&w).unwrap();
    e.save(&w, &p).unwrap();
    assert!(
        Edit::load(&w, &p.path)
            .unwrap_err()
            .to_string()
            .contains("historical")
    );
    let mut revision = draft(&w, "change", "CHANGE-2");
    set(
        &mut revision,
        "previous_records",
        "[[07_Decisions/Changes/CHANGE-1]]",
    );
    assert!(revision.preview(&w).is_ok());
}
#[test]
fn uat_acceptance_uses_pinned_versions_and_rejects_newer_failure_or_simulation() {
    let (_dir, w) = fixture();
    check(&w, uat()).unwrap();
    let mut current = records::read(&w, "04_Requirements/REQ-1.md")
        .unwrap()
        .metadata;
    current["version"] = json!(2);
    put(&w, "04_Requirements/REQ-1.md", current);
    check(&w, uat()).unwrap(); // Accepted baseline remains version 1.
    let mut run = records::read(&w, "08_Verification/TR-1.md")
        .unwrap()
        .metadata;
    run["simulation"] = json!(true);
    put(&w, "08_Verification/TR-1.md", run.clone());
    assert!(check(&w, uat()).is_err());
    run["simulation"] = json!(false);
    put(&w, "08_Verification/TR-1.md", run.clone());
    run["id"] = json!("TR-2");
    run["result"] = json!("failed");
    run["executed_at"] = json!("2026-10-02T08:30:00Z");
    put(&w, "08_Verification/TR-2.md", run);
    assert!(check(&w, uat()).unwrap_err().to_string().contains("Latest"));
}
#[test]
fn uat_rejects_missing_coverage_wrong_scope_version_and_event_order() {
    let (_dir, w) = fixture();
    for field in ["requirements", "items", "cases", "test_runs"] {
        let mut m = uat();
        m[field] = json!([]);
        assert!(check(&w, m).is_err(), "{field}");
    }
    let mut m = uat();
    m["reviewed_at"] = json!("2026-10-01T07:00:00Z");
    assert!(check(&w, m).unwrap_err().to_string().contains("precedes"));
    let mut run = records::read(&w, "08_Verification/TR-1.md")
        .unwrap()
        .metadata;
    run["requirement_versions"] = json!(["REQ-1@9:wrong"]);
    put(&w, "08_Verification/TR-1.md", run);
    assert!(check(&w, uat()).is_err());
}
#[test]
fn release_requires_go_uat_operations_and_actual_release_evidence() {
    let (_dir, w) = fixture();
    put(&w, "08_Verification/UAT/UAT-1.md", uat());
    check(&w, release()).unwrap();
    for field in [
        "uat_records",
        "cutover_plan",
        "rollback_plan",
        "handover",
        "release_evidence",
    ] {
        let mut m = release();
        m[field] = json!([]);
        assert!(check(&w, m).is_err(), "{field}");
    }
    for (key, value) in [
        ("go_no_go", "no_go"),
        ("released_at", "2026-10-01T08:00:00Z"),
        ("decided_at", "2026-10-01T08:00:00Z"),
    ] {
        let mut m = release();
        m[key] = json!(value);
        assert!(check(&w, m).is_err(), "{key}");
    }
    let mut m = release();
    m["workflow_status"] = json!("ready");
    m["released_at"] = Value::Null;
    m["released_by"] = Value::Null;
    m["release_evidence"] = json!([]);
    check(&w, m).unwrap();
    put(&w, "06_Delivery/Releases/REL-1.md", release());
    assert!(Edit::load(&w, "06_Delivery/Releases/REL-1.md").is_err());
    let req = records::read(&w, "04_Requirements/REQ-1.md").unwrap();
    assert_eq!(
        traceability::evidence_for(&Records::load(&w).unwrap(), &req)
            .unwrap()
            .releases
            .len(),
        1
    );
    let mut m = release();
    m["release_evidence"] = json!([]);
    put(&w, "06_Delivery/Releases/REL-1.md", m);
    assert!(
        traceability::evidence_for(&Records::load(&w).unwrap(), &req)
            .unwrap()
            .releases
            .is_empty()
    );
    assert!(
        validation::inspect(&w)
            .unwrap()
            .iter()
            .any(|f| f.code == "DELIVERY_WORKFLOW" && f.file.ends_with("REL-1.md"))
    );
}
#[test]
fn benefit_measurements_keep_zero_distinct_from_unknown_and_preserve_window() {
    let (_dir, w) = fixture();
    let mut m = json!({"id":"BEN-1","type":"benefit","title":"Synthetic metric","language":"en","workflow_schema":PROFILE,"workflow_status":"observing","owner":"Fixture owner","metric":"Synthetic count","unit":"count","measurement_definition":"Count fixture records per observation window","baseline_value":0,"target_value":10,"data_sources":["[[00_Governance/Project]]"],"observation_start":"2026-10-01","observation_end":"2026-10-05"});
    check(&w, m.clone()).unwrap();
    m["workflow_status"] = json!("measured");
    assert!(check(&w, m.clone()).is_err());
    m["actual_value"] = json!(0);
    m["measured_by"] = json!("Fixture analyst");
    m["measured_at"] = json!("2026-10-05T09:00:00+08:00");
    m["measurement_evidence"] = json!(["[[00_Governance/Project]]"]);
    check(&w, m.clone()).unwrap();
    m["measured_at"] = json!("2026-10-04T09:00:00+08:00");
    assert!(check(&w, m.clone()).is_err());
    m["measured_at"] = json!("2026-10-05T09:00:00+08:00");
    m["baseline_value"] = Value::Null;
    assert!(check(&w, m.clone()).is_err());
    m["baseline_value"] = json!(0);
    m["actual_value"] = Value::Null;
    assert!(check(&w, m).is_err());
}
#[test]
fn done_label_cannot_bypass_existing_pbi_completion_checks() {
    let (_dir, w) = fixture();
    let mut item = records::read(&w, "05_Backlog/PBI-1.md").unwrap().metadata;
    item["dod_evidence"] = json!([]);
    put(&w, "05_Backlog/PBI-1.md", item);
    assert!(
        check(&w, uat())
            .unwrap_err()
            .to_string()
            .contains("DONE_EVIDENCE")
    );
}
#[test]
fn snapshot_is_read_only_labels_legacy_and_review_rechecks_evidence() {
    let (_dir, w) = fixture();
    let mut legacy = release();
    legacy.as_object_mut().unwrap().remove("workflow_schema");
    put(&w, "06_Delivery/Releases/REL-1.md", legacy);
    let s = Snapshot::load(&w);
    assert!(s.entries[0].problem.as_ref().unwrap().contains("Legacy"));
    assert!(Edit::load(&w, "06_Delivery/Releases/REL-1.md").is_err());
    let e = draft(&w, "uat", "UAT-DRAFT");
    let p = e.preview(&w).unwrap();
    let source = w.read("00_Governance/Project.md").unwrap();
    workspace::atomic_write(
        &w.path("00_Governance/Project.md").unwrap(),
        format!("{source}\nExternal evidence changed.").as_bytes(),
    )
    .unwrap();
    assert!(e.save(&w, &p).is_err());
    assert!(!w.path(&p.path).unwrap().exists());
    let mut wrong = uat();
    wrong["workflow_schema"] = json!("unknown-v99");
    assert!(check(&w, wrong).is_err());
}
#[tokio::test]
async fn agent_can_read_delivery_gaps_without_authorizing_outcomes() {
    let (_dir, w) = fixture();
    let e = draft(&w, "benefit", "BEN-DRAFT");
    let p = e.preview(&w).unwrap();
    e.save(&w, &p).unwrap();
    let before = w
        .documents()
        .unwrap()
        .into_iter()
        .map(|d| (d.path, d.hash))
        .collect::<Vec<_>>();
    assert!(!tools::requires_approval(
        "pm",
        &json!({"command":"delivery","args":[]})
    ));
    let result = tools::run_pm(&w, "delivery", &[]).await.unwrap();
    assert_eq!(result["exit_code"], 0);
    assert!(
        result["records"][0]["next_gate"]
            .as_str()
            .unwrap()
            .contains("owner")
    );
    assert_eq!(
        before,
        w.documents()
            .unwrap()
            .into_iter()
            .map(|d| (d.path, d.hash))
            .collect::<Vec<_>>()
    );
}

#[test]
fn ready_release_preserves_go_scope_until_actual_release_is_recorded() {
    let (_dir, w) = fixture();
    put(&w, "08_Verification/UAT/UAT-1.md", uat());
    let mut ready = release();
    ready["workflow_status"] = json!("ready");
    ready["released_by"] = Value::Null;
    ready["released_at"] = Value::Null;
    ready["release_evidence"] = json!([]);
    put(&w, "06_Delivery/Releases/REL-1.md", ready);
    let mut e = Edit::load(&w, "06_Delivery/Releases/REL-1.md").unwrap();
    set(&mut e, "title", "Changed go scope");
    assert!(
        e.preview(&w)
            .unwrap_err()
            .to_string()
            .contains("reviewed scope")
    );
    let mut e = Edit::load(&w, "06_Delivery/Releases/REL-1.md").unwrap();
    set(&mut e, "workflow_status", "released");
    assert!(e.preview(&w).is_err());
    set(&mut e, "released_by", "Fixture operator");
    set(&mut e, "released_at", "2026-10-02T11:00:00Z");
    set(&mut e, "release_evidence", "[[00_Governance/Project]]");
    let p = e.preview(&w).unwrap();
    e.save(&w, &p).unwrap();
    assert!(Edit::load(&w, &p.path).is_err());
}
