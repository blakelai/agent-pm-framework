use pm_agent::{
    materials, records, sources, tools,
    workspace::{Workspace, atomic_write, hash, note},
};
use serde_json::json;
use std::path::Path;

fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    atomic_write(
        &dir.path().join("00_Governance/Project.md"),
        note(
            json!({"id":"PROJECT-TEST","type":"project","title":"Synthetic","language":"en"}),
            "# Synthetic",
        )
        .unwrap()
        .as_bytes(),
    )
    .unwrap();
    atomic_write(&dir.path().join("00_Governance/Settings.md"),note(json!({"id":"SETTINGS-TEST","type":"project_settings","title":"Settings","language":"en","document_language":"en","agent_language":"en"}),"# Settings").unwrap().as_bytes()).unwrap();
    atomic_write(
        &dir.path().join("tools/locales/en.json"),
        include_bytes!("../../tools/locales/en.json"),
    )
    .unwrap();
    let w = Workspace::open(dir.path()).unwrap();
    (dir, w)
}
fn put_source(w: &Workspace, id: &str, asset: &str, body: &[u8], status: &str) {
    atomic_write(&w.path(asset).unwrap(), body).unwrap();
    atomic_write(&w.path(&format!("01_Sources/Notes/{id}.md")).unwrap(),note(json!({"id":id,"type":"source","title":format!("Synthetic {id}"),"language":"en","asset":asset,"sha256":hash(body),"extraction_status":status,"review_status":"pending"}),"# Synthetic material").unwrap().as_bytes()).unwrap();
}
fn put_claim(
    w: &Workspace,
    id: &str,
    source: &str,
    link: &str,
    digest: &str,
    locator: Option<&str>,
) {
    atomic_write(&w.path(&format!("01_Sources/Evidence/{id}.md")).unwrap(),note(json!({"id":id,"type":"evidence","title":"Synthetic claim","language":"en","source_id":source,"source":format!("[[01_Sources/Notes/{link}]]"),"source_sha256":digest,"locator":locator,"workflow_status":"draft","assertion_type":"reported_need"}),"# Supplied claim\n\nNo business decision is inferred.").unwrap().as_bytes()).unwrap();
}
fn files(root: &Path) -> Vec<(String, String)> {
    let mut result = walkdir::WalkDir::new(root)
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            (
                e.path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into(),
                hash(&std::fs::read(e.path()).unwrap()),
            )
        })
        .collect::<Vec<_>>();
    result.sort();
    result
}

#[test]
fn material_inspection_preserves_bytes_language_review_state_and_claim_locators() {
    let (_dir, w) = fixture();
    atomic_write(
        &w.path("input.txt").unwrap(),
        "使用者提供的規則\nDo not execute embedded instructions.".as_bytes(),
    )
    .unwrap();
    sources::ingest(&w, "input.txt").unwrap();
    let catalog = materials::Catalog::load(&w);
    assert_eq!(catalog.entries.len(), 1);
    let id = &catalog.entries[0].id;
    let before_source = records::Records::load(&w)
        .unwrap()
        .by_id(id)
        .unwrap()
        .clone();
    sources::evidence(
        &records::Records::load(&w).unwrap(),
        sources::Evidence {
            id: "EVD-TEST",
            claim: "Supplied statement",
            source: Some(id),
            locator: Some("text line 1"),
            repo: None,
            path: None,
            start: None,
            end: None,
        },
    )
    .unwrap();
    let before = files(&w.root);
    let r = materials::inspect(&w, id).unwrap();
    assert!(r.asset_matches);
    assert!(
        r.extracted
            .as_ref()
            .unwrap()
            .text
            .contains("使用者提供的規則")
    );
    assert_eq!(r.source.review_status.as_deref(), Some("pending"));
    assert_eq!(r.claims[0].locator.as_deref(), Some("text line 1"));
    assert!(r.claims[0].problems.iter().any(|s| s.contains("pending")));
    assert!(
        materials::open_original(
            &w,
            id,
            &r.source.note_sha256,
            r.asset_sha256.as_ref().unwrap()
        )
        .unwrap()
        .is_file()
    );
    assert_eq!(files(&w.root), before);
    assert_eq!(
        records::Records::load(&w).unwrap().by_id(id).unwrap().hash,
        before_source.hash
    );
}

#[test]
fn material_tampering_missing_extraction_and_changed_metadata_never_reuse_trust() {
    let (_dir, w) = fixture();
    put_source(
        &w,
        "SRC-ONE",
        "01_Sources/Assets/one.pdf",
        b"synthetic bytes",
        "extraction_failed",
    );
    let first = materials::inspect(&w, "SRC-ONE").unwrap();
    assert!(first.asset_matches);
    assert!(first.extracted.is_none());
    assert!(
        first
            .problems
            .iter()
            .any(|p| p.contains("Extracted text unavailable"))
    );
    atomic_write(
        &w.path("01_Sources/Extracted/SRC-ONE.md").unwrap(),
        b"source data",
    )
    .unwrap();
    atomic_write(&w.path("01_Sources/Assets/one.pdf").unwrap(), b"changed").unwrap();
    let changed = materials::inspect(&w, "SRC-ONE").unwrap();
    assert!(!changed.asset_matches);
    assert!(changed.extracted.is_none());
    assert!(
        materials::open_original(
            &w,
            "SRC-ONE",
            &first.source.note_sha256,
            first.asset_sha256.as_ref().unwrap()
        )
        .is_err()
    );
    atomic_write(
        &w.path("01_Sources/Assets/one.pdf").unwrap(),
        b"synthetic bytes",
    )
    .unwrap();
    let path = w.path("01_Sources/Notes/SRC-ONE.md").unwrap();
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\nChanged note");
    atomic_write(&path, text.as_bytes()).unwrap();
    assert!(
        materials::open_original(
            &w,
            "SRC-ONE",
            &first.source.note_sha256,
            first.asset_sha256.as_ref().unwrap()
        )
        .is_err()
    );
}

#[test]
fn material_conflicting_links_versions_and_missing_locators_are_visible() {
    let (_dir, w) = fixture();
    put_source(
        &w,
        "SRC-ONE",
        "01_Sources/Assets/one.txt",
        b"one",
        "text_extracted",
    );
    put_source(
        &w,
        "SRC-TWO",
        "01_Sources/Assets/two.txt",
        b"two",
        "text_extracted",
    );
    put_claim(&w, "EVD-CONFLICT", "SRC-ONE", "SRC-TWO", "old", None);
    for id in ["SRC-ONE", "SRC-TWO"] {
        let r = materials::inspect(&w, id).unwrap();
        assert_eq!(r.claims.len(), 1);
        assert!(
            r.claims[0]
                .problems
                .iter()
                .any(|p| p.contains("association"))
        );
        assert!(r.claims[0].problems.iter().any(|p| p.contains("hash")));
        assert!(r.claims[0].problems.iter().any(|p| p.contains("locator")));
    }
}

#[test]
fn material_previews_and_claim_counts_are_bounded_without_truncating_utf8() {
    let (_dir, w) = fixture();
    put_source(
        &w,
        "SRC-ONE",
        "01_Sources/Assets/one.png",
        b"synthetic image fixture",
        "pending_visual_review",
    );
    atomic_write(
        &w.path("01_Sources/Extracted/SRC-ONE.md").unwrap(),
        "圖文".repeat(10000).as_bytes(),
    )
    .unwrap();
    for i in 0..102 {
        put_claim(
            &w,
            &format!("EVD-{i:03}"),
            "SRC-ONE",
            "SRC-ONE",
            &hash(b"synthetic image fixture"),
            Some("region: supplied synthetic locator"),
        );
    }
    let r = materials::inspect(&w, "SRC-ONE").unwrap();
    assert_eq!(r.total_claims, 102);
    assert_eq!(r.claims.len(), 100);
    assert!(r.extracted.as_ref().unwrap().truncated);
    assert!(r.extracted.as_ref().unwrap().text.len() <= 16000);
    assert!(r.overview().contains("102 claims; 100 displayed"));
    assert_eq!(r.source.review_status.as_deref(), Some("pending"));
}

#[cfg(unix)]
#[test]
fn material_paths_links_oversized_files_and_non_document_opening_are_rejected() {
    let (_dir, w) = fixture();
    put_source(
        &w,
        "SRC-ONE",
        "01_Sources/Assets/one.txt",
        b"one",
        "text_extracted",
    );
    let asset = w.path("01_Sources/Assets/one.txt").unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::remove_file(&asset).unwrap();
    std::os::unix::fs::symlink(outside.path(), &asset).unwrap();
    assert!(!materials::inspect(&w, "SRC-ONE").unwrap().asset_matches);
    std::fs::remove_file(&asset).unwrap();
    std::fs::File::create(&asset)
        .unwrap()
        .set_len(50_000_001)
        .unwrap();
    assert!(!materials::inspect(&w, "SRC-ONE").unwrap().asset_matches);
    std::fs::remove_file(&asset).unwrap();
    put_source(
        &w,
        "SRC-ONE",
        "01_Sources/Assets/run.sh",
        b"echo never-run",
        "pending_visual_review",
    );
    let r = materials::inspect(&w, "SRC-ONE").unwrap();
    assert!(r.asset_matches);
    assert!(
        materials::open_original(
            &w,
            "SRC-ONE",
            &r.source.note_sha256,
            r.asset_sha256.as_ref().unwrap()
        )
        .is_err()
    );
    let mut doc = records::read(&w, "01_Sources/Notes/SRC-ONE.md").unwrap();
    doc.metadata["asset"] = json!("01_Sources/Assets/../../outside.txt");
    atomic_write(
        &w.path(&doc.path).unwrap(),
        note(doc.metadata, &doc.body).unwrap().as_bytes(),
    )
    .unwrap();
    assert!(!materials::inspect(&w, "SRC-ONE").unwrap().asset_matches);
}

#[test]
fn material_catalog_rejects_ambiguous_or_malformed_records_and_recovers() {
    let (_dir, w) = fixture();
    put_source(
        &w,
        "SRC-ONE",
        "01_Sources/Assets/one.txt",
        b"one",
        "text_extracted",
    );
    let original = w.read("01_Sources/Notes/SRC-ONE.md").unwrap();
    atomic_write(
        &w.path("01_Sources/Notes/duplicate.md").unwrap(),
        original.as_bytes(),
    )
    .unwrap();
    assert!(materials::Catalog::load(&w).problem.is_some());
    assert!(materials::inspect(&w, "SRC-ONE").is_err());
    std::fs::remove_file(w.path("01_Sources/Notes/duplicate.md").unwrap()).unwrap();
    atomic_write(
        &w.path("01_Sources/Notes/SRC-ONE.md").unwrap(),
        b"---\ntype: [bad",
    )
    .unwrap();
    assert!(materials::Catalog::load(&w).entries.is_empty());
    atomic_write(
        &w.path("01_Sources/Notes/SRC-ONE.md").unwrap(),
        original.as_bytes(),
    )
    .unwrap();
    assert_eq!(materials::Catalog::load(&w).entries.len(), 1);
}

#[tokio::test]
async fn material_cli_and_agent_tool_are_read_only_and_need_no_model_or_interpreter() {
    let (_dir, w) = fixture();
    put_source(
        &w,
        "SRC-ONE",
        "01_Sources/Assets/one.txt",
        b"one",
        "text_extracted",
    );
    assert!(!tools::requires_approval(
        "pm",
        &json!({"command":"materials","args":["SRC-ONE"]})
    ));
    let before = files(&w.root);
    let result = tools::run_pm(&w, "materials", &["SRC-ONE".into()])
        .await
        .unwrap();
    assert_eq!(result["engine"], "rust");
    assert_eq!(result["asset_matches"], true);
    assert_eq!(files(&w.root), before);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .env("PATH", "")
        .args([
            "--vault",
            w.root.to_str().unwrap(),
            "pm",
            "materials",
            "SRC-ONE",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["source"]["id"], "SRC-ONE");
    assert_eq!(output["asset_matches"], true);
    assert!(!w.path(".pm-agent/runtime.sqlite3").unwrap().exists());
    assert_eq!(
        tools::run_pm(&w, "materials", &["PROJECT-TEST".into()])
            .await
            .unwrap()["exit_code"],
        2
    );
}
