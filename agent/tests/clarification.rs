//! Synthetic fixtures only; no inferred business answers or events.
use pm_agent::{
    baselines, clarification as c,
    records::{self, Records},
    tools,
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
fn doc(w: &Workspace, id: &str, body: &str) -> String {
    let path = format!("04_Requirements/{id}.md");
    put(
        w,
        &path,
        json!({"type":"requirement","id":id,"title":id,"language":"en","doc_status":"draft","version":1}),
        body,
    );
    path
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    workspace::atomic_write(
        &dir.path().join("00_Governance/Project.md"),
        workspace::note(
            json!({"type":"project","id":"PROJECT-TEST","title":"Synthetic","language":"en"}),
            "# Synthetic",
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
    doc(&w, "REQ-ROOT", "# Missing rule");
    doc(&w, "REQ-CHILD", "# Dependent\n[[REQ-ROOT]]");
    doc(&w, "REQ-LEAF", "# Downstream\n[[REQ-CHILD]]");
    doc(&w, "REQ-FREE", "# Independent supplied text");
    (dir, w)
}
fn ask(w: &Workspace, args: Value) -> String {
    let path = "07_Decisions/QUESTION-TEST.md";
    let mut args = args;
    args["question"] = json!("What is the rule?");
    let meta = c::ask_metadata(w, &args, "QUESTION-TEST", "session-test").unwrap();
    put(w, path, meta, "# Missing rule\n\nSupply the source.");
    path.into()
}
fn scoped(w: &Workspace) -> String {
    ask(
        w,
        json!({"blocking_scope":"records","affected_ids":["REQ-ROOT"]}),
    )
}
fn draft(id: &str, body: &str) -> String {
    workspace::note(json!({"type":"requirement","id":id,"title":"Synthetic","language":"en","doc_status":"draft","version":1}),body).unwrap()
}
fn citations(w: &Workspace, path: &str) -> Vec<Value> {
    vec![json!({"path":path,"sha256":workspace::hash(w.read(path).unwrap().as_bytes())})]
}
#[test]
fn scoped_drafts_follow_transitive_dependencies_and_cannot_remove_links_or_rename_ids() {
    let (_dir, w) = fixture();
    scoped(&w);
    for id in ["REQ-ROOT", "REQ-CHILD", "REQ-LEAF"] {
        let path = format!("04_Requirements/{id}.md");
        let old = w.read(&path).unwrap();
        assert!(
            w.save_draft(
                &path,
                &draft(id, "# Link removed"),
                Some(&workspace::hash(old.as_bytes())),
                &citations(&w, "04_Requirements/REQ-FREE.md")
            )
            .is_err()
        );
    }
    let renamed = draft("REQ-ROOT", "# Renamed file");
    assert!(
        !c::blockers(&w, "04_Requirements/New.md", Some(&renamed), &[])
            .unwrap()
            .is_empty()
    );
    let new = draft("REQ-NEW", "# New dependency\n[[REQ-LEAF]]");
    assert!(
        !c::blockers(&w, "04_Requirements/New.md", Some(&new), &[])
            .unwrap()
            .is_empty()
    );
    assert!(
        !c::blockers(
            &w,
            "04_Requirements/New.md",
            Some(&draft("REQ-NEW", "# Draft")),
            &citations(&w, "04_Requirements/REQ-ROOT.md")
        )
        .unwrap()
        .is_empty()
    );
    w.save_draft(
        "04_Requirements/REQ-NEW.md",
        &draft("REQ-NEW", "# Supported independent text"),
        None,
        &citations(&w, "04_Requirements/REQ-FREE.md"),
    )
    .unwrap();
    assert!(c::has_open(&w).unwrap());
}
#[test]
fn project_default_invalid_contracts_and_unknown_relationships_fail_closed() {
    let (_dir, w) = fixture();
    let q = ask(&w, json!({}));
    assert_eq!(
        c::blockers(&w, "04_Requirements/New.md", None, &[])
            .unwrap()
            .len(),
        1
    );
    let mut d = records::read(&w, &q).unwrap();
    d.metadata["blocking_scope"] = json!("records");
    put(&w, &q, d.metadata.clone(), &d.body);
    assert!(
        c::blockers(&w, "04_Requirements/New.md", None, &[]).unwrap()[0]
            .reason
            .contains("contract unavailable")
    );
    d.metadata["affected_ids"] = json!(["REQ-ROOT"]);
    put(&w, &q, d.metadata, &d.body);
    let unknown = draft("REQ-NEW", "# Missing dependency\n[[UNKNOWN]]");
    assert!(
        c::blockers(&w, "04_Requirements/New.md", Some(&unknown), &[]).unwrap()[0]
            .reason
            .contains("independence")
    );
    std::fs::remove_file(w.path("04_Requirements/REQ-ROOT.md").unwrap()).unwrap();
    assert!(
        c::blockers(&w, "04_Requirements/New.md", None, &[]).unwrap()[0]
            .reason
            .contains("Missing scoped record")
    );
    let mut d = records::read(&w, &q).unwrap();
    d.metadata.as_object_mut().unwrap().remove("id");
    put(&w, &q, d.metadata, &d.body);
    assert!(c::blockers(&w, "04_Requirements/New.md", None, &[]).is_err());
    assert!(c::Snapshot::load(&w).problem.is_some());
}
#[test]
fn path_scopes_validate_managed_paths_and_block_planned_documents() {
    let (_dir, w) = fixture();
    let r = Records::load(&w).unwrap();
    for path in [
        "../outside.md",
        "04_Requirements/../outside.md",
        "00_Governance/Baselines/X.md",
        "01_Sources/Assets/X.md",
        "README.md",
    ] {
        assert!(c::scope(&r,&json!({"clarification_schema":c::PROFILE,"blocking_scope":"records","affected_ids":[],"affected_paths":[path]})).is_err(),"{path}");
    }
    ask(
        &w,
        json!({"blocking_scope":"records","affected_paths":["04_Requirements/Planned.md"]}),
    );
    assert_eq!(
        c::blockers(&w, "04_Requirements/Planned.md", None, &[])
            .unwrap()
            .len(),
        1
    );
    assert!(
        c::blockers(&w, "04_Requirements/Elsewhere.md", None, &[])
            .unwrap()
            .is_empty()
    );
}
#[test]
fn actual_response_receipt_is_not_resolution_and_changed_question_or_source_revokes_it() {
    let (_dir, w) = fixture();
    let path = scoped(&w);
    assert!(c::resolve(&w, &path, "session-test", "Supposed answer").is_err());
    let source = "01_Sources/Assets/USER-test.txt";
    workspace::atomic_write(&w.path(source).unwrap(), b"Actual synthetic response").unwrap();
    assert!(c::record_answer(&w, &path, "different-session", source).is_err());
    c::record_answer(&w, &path, "session-test", source).unwrap();
    assert!(c::has_open(&w).unwrap());
    c::resolve(
        &w,
        &path,
        "session-test",
        "The supplied response addresses this synthetic gap.",
    )
    .unwrap();
    assert!(!c::has_open(&w).unwrap());
    let d = records::read(&w, &path).unwrap();
    assert_eq!(c::answer(&w, &d).unwrap(), "Actual synthetic response");
    let mut meta = d.metadata;
    meta["affected_ids"] = json!(["REQ-FREE"]);
    put(&w, &path, meta, &d.body);
    assert!(
        !c::blockers(&w, "04_Requirements/New.md", None, &[])
            .unwrap()
            .is_empty()
    );
    let mut d = records::read(&w, &path).unwrap();
    d.metadata["workflow_status"] = json!("open");
    put(&w, &path, d.metadata, &d.body);
    c::record_answer(&w, &path, "session-test", source).unwrap();
    workspace::atomic_write(&w.path(source).unwrap(), b"Changed response").unwrap();
    assert!(c::resolve(&w, &path, "session-test", "Cannot use the modified asset").is_err());
    assert!(c::Snapshot::load(&w).entries[0].problem.is_some());
}
#[test]
fn generic_draft_writes_cannot_create_or_replace_questions() {
    let (_dir, w) = fixture();
    let path = scoped(&w);
    let d = records::read(&w, &path).unwrap();
    let mut meta = d.metadata;
    meta["doc_status"] = json!("draft");
    put(&w, &path, meta, &d.body);
    let text = w.read(&path).unwrap();
    let err = w
        .save_draft(
            &path,
            &draft("QUESTION-TEST", "# Changed type"),
            Some(&workspace::hash(text.as_bytes())),
            &citations(&w, "04_Requirements/REQ-FREE.md"),
        )
        .unwrap_err();
    assert!(err.to_string().contains("Question scope"));
    let mut meta = records::read(&w, &path).unwrap().metadata;
    meta["id"] = json!("QUESTION-OTHER");
    assert!(
        w.save_draft(
            "07_Decisions/Other.md",
            &workspace::note(meta, "# Hidden question").unwrap(),
            None,
            &citations(&w, "04_Requirements/REQ-FREE.md")
        )
        .is_err()
    );
}
#[tokio::test]
async fn questions_report_is_read_only_and_baselines_respect_scopes() {
    let (_dir, w) = fixture();
    let q = scoped(&w);
    let before = w.read(&q).unwrap();
    let report = tools::run_pm(&w, "questions", &[]).await.unwrap();
    assert_eq!(report["successful"], true);
    assert_eq!(report["entries"][0]["blocked"].as_array().unwrap().len(), 3);
    assert_eq!(w.read(&q).unwrap(), before);
    assert!(!w.root.join("90_Agent/Runs").exists());
    let records = Records::load(&w).unwrap();
    assert!(baselines::capture(&records, "BASE-BLOCKED", &["REQ-LEAF".into()]).is_err());
    assert!(!w.root.join("00_Governance/Baselines/BASE-BLOCKED").exists());
    baselines::capture(&records, "BASE-FREE", &["REQ-FREE".into()]).unwrap();
}
#[test]
fn citation_edges_contexts_and_legacy_records_have_explicit_behavior() {
    let (_dir, w) = fixture();
    put(
        &w,
        "04_Requirements/REQ-CITED.md",
        json!({"id":"REQ-CITED","type":"requirement","evidence_paths":["04_Requirements/REQ-ROOT.md"]}),
        "# Citations",
    );
    scoped(&w);
    assert_eq!(
        c::blockers(&w, "04_Requirements/REQ-CITED.md", None, &[])
            .unwrap()
            .len(),
        1
    );
    put(
        &w,
        "07_Decisions/QUESTION-TEST.md",
        json!({"id":"QUESTION-TEST","type":"question","workflow_status":"open"}),
        "# Legacy",
    );
    assert!(
        c::blockers(&w, "04_Requirements/New.md", None, &[])
            .unwrap()
            .is_empty()
    );
    assert!(c::Snapshot::load(&w).entries[0].legacy);
    put(
        &w,
        "02_Domain/demo/CONTEXT.md",
        json!({"id":"LANG-demo","type":"glossary","context":"demo"}),
        "# Context",
    );
    put(
        &w,
        "04_Requirements/REQ-CONTEXT.md",
        json!({"id":"REQ-CONTEXT","type":"requirement","contexts":["demo"]}),
        "# Meaning",
    );
    ask(
        &w,
        json!({"blocking_scope":"records","affected_ids":["LANG-demo"]}),
    );
    assert_eq!(
        c::blockers(&w, "04_Requirements/REQ-CONTEXT.md", None, &[])
            .unwrap()
            .len(),
        1
    );
}
