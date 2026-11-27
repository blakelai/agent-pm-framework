use pm_agent::{
    editing::{Edit, Target},
    materials,
    records::{self, Records},
    reviews, sources, validation,
    workspace::{self, Workspace},
};
use serde_json::{Value, json};
fn put(w: &Workspace, path: &str, m: Value, body: &str) {
    workspace::atomic_write(
        &w.path(path).unwrap(),
        workspace::note(m, body).unwrap().as_bytes(),
    )
    .unwrap();
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let dir = tempfile::tempdir().unwrap();
    let w = Workspace {
        root: dir.path().into(),
    };
    put(
        &w,
        "00_Governance/Project.md",
        json!({"id":"PROJECT-TEST","type":"project","language":"en","title":"Synthetic project"}),
        "# Synthetic project",
    );
    put(
        &w,
        "00_Governance/Settings.md",
        json!({"type":"project_settings","language":"en","title":"Settings","document_language":"en","agent_language":"en"}),
        "# Settings",
    );
    workspace::atomic_write(
        &w.path("tools/locales/en.json").unwrap(),
        include_bytes!("../../tools/locales/en.json"),
    )
    .unwrap();
    workspace::atomic_write(
        &w.path("input.txt").unwrap(),
        "Synthetic source. 原文保留".as_bytes(),
    )
    .unwrap();
    sources::ingest(&w, "input.txt").unwrap();
    let id = materials::Catalog::load(&w).entries[0].id.clone();
    sources::evidence(
        &Records::load(&w).unwrap(),
        sources::Evidence {
            id: "EVD-ONE",
            claim: "Synthetic statement",
            source: Some(&id),
            locator: Some("text line 1"),
            repo: None,
            path: None,
            start: None,
            end: None,
        },
    )
    .unwrap();
    (dir, w, id)
}
fn set(e: &mut Edit, k: &str, v: &str) {
    e.apply(&Target::Property(k.into()), v).unwrap();
}
fn edit(w: &Workspace, target: &str, id: &str) -> Edit {
    let r = Records::load(w).unwrap();
    let d = r.by_id(target).unwrap();
    let mut e = Edit::material_review(w, target, &d.hash).unwrap();
    for (k, v) in [
        ("id", id),
        ("title", "Synthetic material review"),
        ("coverage", "full"),
        ("locator", "text line 1"),
        (
            "review_scope",
            "All supplied text; no charts in this synthetic fixture",
        ),
        ("reviewed_by", "Synthetic reviewer"),
        ("reviewed_at", "2026-01-01T08:00:00Z"),
        (
            "review_method",
            "Compared original text against the supplied statement",
        ),
        ("review_basis", "original"),
        ("outcome", "verified"),
        ("findings", "Synthetic content matched the recorded scope"),
        ("workflow_status", "recorded"),
    ] {
        set(&mut e, k, v);
    }
    e.apply(
        &Target::Body,
        "# Synthetic review\n\nThis fixture does not represent business approval.",
    )
    .unwrap();
    e
}
fn save(w: &Workspace, e: &Edit) -> String {
    let p = e.preview(w).unwrap();
    e.save(w, &p).unwrap().path
}
fn summary(w: &Workspace, id: &str) -> reviews::Summary {
    let r = Records::load(w).unwrap();
    reviews::summary(&r, r.by_id(id).unwrap())
}
fn update(w: &Workspace, path: &str, key: &str, value: Value) {
    let d = records::read(w, path).unwrap();
    let mut m = d.metadata;
    m[key] = value;
    put(w, path, m, &d.body);
}

#[test]
fn reviewed_save_is_explicit_preserves_material_and_uses_actual_fields() {
    let (_dir, w, id) = fixture();
    let r = Records::load(&w).unwrap();
    let source = r.by_id(&id).unwrap();
    let source_text = w.read(&source.path).unwrap();
    let evidence = w.read("01_Sources/Evidence/EVD-ONE.md").unwrap();
    let asset = std::fs::read(w.path(source.metadata["asset"].as_str().unwrap()).unwrap()).unwrap();
    let e = edit(&w, &id, "REVIEW-ONE");
    let p = e.preview(&w).unwrap();
    assert!(!w.path(&p.path).unwrap().exists());
    assert!(!summary(&w, &id).verified);
    let saved = e.save(&w, &p).unwrap();
    assert!(saved.audit_path.is_some());
    assert!(summary(&w, &id).verified);
    assert_eq!(summary(&w, &id).state, "verified");
    assert_eq!(w.read(&source.path).unwrap(), source_text);
    assert_eq!(w.read("01_Sources/Evidence/EVD-ONE.md").unwrap(), evidence);
    assert_eq!(
        std::fs::read(w.path(source.metadata["asset"].as_str().unwrap()).unwrap()).unwrap(),
        asset
    );
    assert!(Edit::load(&w, &saved.path).is_err());
    assert!(
        !validation::inspect(&w)
            .unwrap()
            .iter()
            .any(|f| f.code == "SOURCE_REVIEW")
    );
    let claim = edit(&w, "EVD-ONE", "REVIEW-CLAIM");
    save(&w, &claim);
    assert!(summary(&w, "EVD-ONE").verified);
    assert!(!validation::inspect(&w).unwrap().iter().any(|f| matches!(
        f.code,
        "SOURCE_REVIEW" | "EVIDENCE_REVIEW" | "VISUAL_REVIEW_REQUIRED"
    )));
    let inspected = materials::inspect(&w, &id).unwrap();
    assert_eq!(inspected.total_reviews, 2);
    assert!(inspected.claims[0].review.verified);
    assert!(
        w.read(saved.audit_path.as_deref().unwrap())
            .unwrap()
            .contains("language: \"en\"")
    );
}
#[test]
fn draft_partial_and_negative_reviews_preserve_unknowns_and_do_not_verify() {
    let (_dir, w, id) = fixture();
    let d = Records::load(&w).unwrap().by_id(&id).unwrap().clone();
    let mut blank = Edit::material_review(&w, &id, &d.hash).unwrap();
    assert!(
        blank
            .entries()
            .iter()
            .filter(|e| matches!(
                e.label.as_str(),
                "reviewed_by" | "reviewed_at" | "outcome" | "coverage"
            ))
            .all(|e| e.value.is_empty())
    );
    set(&mut blank, "id", "REVIEW-DRAFT");
    set(&mut blank, "title", "Pending review");
    blank
        .apply(&Target::Body, "# Pending review\n\nAwait actual review.")
        .unwrap();
    let path = save(&w, &blank);
    assert!(Edit::load(&w, &path).is_ok());
    assert!(!summary(&w, &id).verified);
    for field in [
        "reviewed_by",
        "reviewed_at",
        "review_scope",
        "review_method",
        "findings",
        "locator",
    ] {
        let mut e = edit(&w, &id, "REVIEW-FAIL");
        set(&mut e, field, "");
        assert!(e.preview(&w).is_err(), "{field}");
    }
    let mut partial = edit(&w, &id, "REVIEW-PARTIAL");
    set(&mut partial, "coverage", "partial");
    assert!(partial.preview(&w).is_err());
    set(&mut partial, "follow_up", "Review the remaining scope");
    save(&w, &partial);
    assert_eq!(summary(&w, &id).state, "partial");
    let mut negative = edit(&w, &id, "REVIEW-NEGATIVE");
    set(&mut negative, "outcome", "unavailable");
    set(&mut negative, "follow_up", "Supply legible content");
    set(
        &mut negative,
        "previous_records",
        "[[01_Sources/Reviews/REVIEW-PARTIAL]]",
    );
    set(&mut negative, "reviewed_at", "2026-01-02T08:00:00Z");
    save(&w, &negative);
    assert!(!summary(&w, &id).verified);
}
#[test]
fn corrections_are_explicit_history_is_retained_and_conflicts_block() {
    let (_dir, w, id) = fixture();
    let first = save(&w, &edit(&w, &id, "REVIEW-A"));
    let original = w.read(&first).unwrap();
    save(&w, &edit(&w, &id, "REVIEW-B"));
    assert!(!summary(&w, &id).verified);
    assert!(summary(&w, &id).problem.unwrap().contains("Conflicting"));
    let mut e = edit(&w, &id, "REVIEW-C");
    set(
        &mut e,
        "previous_records",
        "[[01_Sources/Reviews/REVIEW-A]]\n[[01_Sources/Reviews/REVIEW-B]]",
    );
    assert!(e.preview(&w).is_err());
    set(&mut e, "reviewed_at", "2026-01-03T08:00:00Z");
    set(&mut e, "outcome", "follow_up");
    set(&mut e, "follow_up", "Resolve a newly observed discrepancy");
    save(&w, &e);
    assert_eq!(summary(&w, &id).state, "follow_up");
    assert_eq!(w.read(&first).unwrap(), original);
    let mut wrong = edit(&w, "EVD-ONE", "REVIEW-WRONG");
    set(
        &mut wrong,
        "previous_records",
        "[[01_Sources/Reviews/REVIEW-A]]",
    );
    set(&mut wrong, "reviewed_at", "2026-01-04T08:00:00Z");
    assert!(wrong.preview(&w).is_err());
}
#[test]
fn changed_original_target_or_extraction_blocks_save_and_invalidates_recorded_review() {
    let (_dir, w, id) = fixture();
    let r = Records::load(&w).unwrap();
    let source = r.by_id(&id).unwrap();
    let path = source.metadata["asset"].as_str().unwrap();
    let raw = std::fs::read(w.path(path).unwrap()).unwrap();
    let e = edit(&w, &id, "REVIEW-A");
    let p = e.preview(&w).unwrap();
    workspace::atomic_write(&w.path(path).unwrap(), b"Changed").unwrap();
    assert!(e.save(&w, &p).is_err());
    workspace::atomic_write(&w.path(path).unwrap(), &raw).unwrap();
    save(&w, &e);
    let mut text_review = edit(&w, &id, "REVIEW-B");
    set(&mut text_review, "review_basis", "original_and_extraction");
    set(&mut text_review, "reviewed_at", "2026-01-02T08:00:00Z");
    set(
        &mut text_review,
        "previous_records",
        "[[01_Sources/Reviews/REVIEW-A]]",
    );
    save(&w, &text_review);
    assert!(summary(&w, &id).verified);
    let extracted = format!("01_Sources/Extracted/{id}.md");
    workspace::atomic_write(&w.path(&extracted).unwrap(), b"Changed extraction").unwrap();
    assert!(!summary(&w, &id).verified);
    assert!(
        validation::inspect(&w)
            .unwrap()
            .iter()
            .any(|f| f.code == "MATERIAL_REVIEW_STALE")
    );
    update(&w, &source.path, "title", json!("Changed source note"));
    assert!(e.preview(&w).is_err());
    assert!(Edit::material_review(&w, &id, &source.hash).is_err());
}
#[test]
fn review_profile_rejects_forged_fields_times_simulation_and_invalid_associations() {
    let (_dir, w, id) = fixture();
    let e = edit(&w, &id, "REVIEW-A");
    let path = save(&w, &e);
    let original = w.read(&path).unwrap();
    for (key, value) in [
        ("workflow_schema", json!("unknown")),
        ("simulation", json!(true)),
        ("reviewed_at", json!("2999-01-01T00:00:00Z")),
        ("target_sha256", json!("bad")),
        ("source_id", json!("SRC-MISSING")),
        ("target", json!("[[01_Sources/Evidence/EVD-ONE]]")),
    ] {
        update(&w, &path, key, value);
        assert!(!summary(&w, &id).verified, "{key}");
        workspace::atomic_write(&w.path(&path).unwrap(), original.as_bytes()).unwrap();
    }
    let mut claim = edit(&w, "EVD-ONE", "REVIEW-C");
    set(&mut claim, "locator", "invented page");
    assert!(claim.preview(&w).is_err());
    let mut e = edit(&w, &id, "REVIEW-B");
    assert!(
        e.apply(&Target::Property("target_sha256".into()), "replacement")
            .is_err()
    );
    update(
        &w,
        "01_Sources/Evidence/EVD-ONE.md",
        "source",
        json!("[[00_Governance/Project]]"),
    );
    let claim = Records::load(&w).unwrap().by_id("EVD-ONE").unwrap().clone();
    assert!(Edit::material_review(&w, "EVD-ONE", &claim.hash).is_err());
}
#[test]
fn review_language_drafts_and_legacy_states_keep_existing_boundaries() {
    let (_dir, w, id) = fixture();
    let d = Records::load(&w).unwrap().by_id(&id).unwrap().clone();
    update(&w, &d.path, "review_status", json!("verified"));
    assert_eq!(summary(&w, &id).state, "legacy_verified");
    update(
        &w,
        "00_Governance/Settings.md",
        "document_language",
        json!("zh-TW"),
    );
    let mut e = edit(&w, &id, "REVIEW-ZH");
    set(&mut e, "title", "素材審查");
    set(&mut e, "findings", "已核對提供的原文");
    e.apply(&Target::Body, "# 素材審查\n\n僅限合成測試資料。")
        .unwrap();
    let p = e.preview(&w).unwrap();
    assert!(p.content.contains("language: \"zh-TW\""));
    assert!(w.save_draft(&p.path, &p.content, None, &[]).is_err());
    e.save(&w, &p).unwrap();
    assert_eq!(summary(&w, &id).state, "verified");
    update(&w, &p.path, "workflow_schema", json!("invalid"));
    assert!(
        !summary(&w, &id).verified,
        "Invalid new history must not fall back to a legacy flag"
    );
}

#[test]
fn review_preview_is_bounded_and_changed_original_or_malformed_history_fails_closed() {
    let (_dir, w, id) = fixture();
    let path = save(&w, &edit(&w, &id, "REVIEW-LONG"));
    let original = w.read(&path).unwrap();
    update(&w, &path, "findings", json!("完整的觀察結果".repeat(5000)));
    let r = Records::load(&w).unwrap();
    let list = reviews::entries(&r, &id);
    assert!(list[0].content.len() < 17000);
    assert!(list[0].content.contains("Review preview truncated: true"));
    assert!(w.read(&path).unwrap().len() > 16000);
    let source = r.by_id(&id).unwrap();
    let asset = w.path(source.metadata["asset"].as_str().unwrap()).unwrap();
    let raw = std::fs::read(&asset).unwrap();
    workspace::atomic_write(&asset, b"Changed after recording").unwrap();
    assert!(!summary(&w, &id).verified);
    workspace::atomic_write(&asset, &raw).unwrap();
    workspace::atomic_write(&w.path(&path).unwrap(), original.as_bytes()).unwrap();
    update(&w, &path, "id", Value::Null);
    assert!(Records::load(&w).is_err());
    assert!(materials::Catalog::load(&w).problem.is_some());
}
