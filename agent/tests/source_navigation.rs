use pm_agent::{
    evaluation,
    records::Records,
    tools,
    workspace::{Workspace, hash, note},
};
use serde_json::json;
use std::path::Path;

async fn fixture() -> (tempfile::TempDir, Workspace) {
    let source = Workspace::open(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    let parent = tempfile::tempdir().unwrap();
    let root = evaluation::prepare(&source, "wiki-code-conflict", parent.path(), "en")
        .await
        .unwrap();
    (parent, Workspace::open(&root.join("vault")).unwrap())
}

#[tokio::test]
async fn registered_sources_expose_actual_snapshot_paths_for_reading() {
    let (_parent, w) = fixture().await;
    // Discovery must continue from captured bytes when the live repository moves.
    std::fs::rename(
        w.root.parent().unwrap().join("repository"),
        w.root.parent().unwrap().join("offline-repository"),
    )
    .unwrap();
    let files = tools::execute(&w, "list_source_files", &json!({"source_id":"REPO-EVAL"}))
        .await
        .expect("Agent needs a discoverable snapshot file list");
    for suffix in ["/openwiki/Access.md", "/src/access.rs"] {
        let file = files["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["path"].as_str().is_some_and(|p| p.ends_with(suffix)))
            .unwrap();
        let read = tools::execute(&w, "read_document", &json!({"path":file["path"]}))
            .await
            .unwrap();
        assert_eq!(read["sha256"], file["sha256"]);
        assert!(!read["content"].as_str().unwrap().is_empty());
    }
}

#[tokio::test]
async fn paged_snapshot_discovery_pins_versions_and_checks_hidden_page_integrity() {
    let (_parent, w) = fixture().await;
    let first = tools::execute(
        &w,
        "list_source_files",
        &json!({"source_id":"REPO-EVAL","limit":1}),
    )
    .await
    .unwrap();
    assert_eq!(first["total"], 2);
    assert_eq!(first["next_offset"], 1);
    let second = tools::execute(
        &w,
        "list_source_files",
        &json!({"source_id":"REPO-EVAL","snapshot_id":first["snapshot_id"],"offset":1,"limit":1}),
    )
    .await
    .unwrap();
    assert!(second["next_offset"].is_null());
    assert_ne!(first["files"][0]["path"], second["files"][0]["path"]);
    let r = Records::load(&w).unwrap();
    let d = r.by_id("REPO-EVAL").unwrap();
    let mut meta = d.metadata.clone();
    meta["latest_snapshot"] = serde_json::Value::Null;
    std::fs::write(w.path(&d.path).unwrap(), note(meta, &d.body).unwrap()).unwrap();
    assert!(
        tools::execute(&w, "list_source_files", &json!({"source_id":"REPO-EVAL"}))
            .await
            .is_err()
    );
    let pinned =
        json!({"source_id":"REPO-EVAL","snapshot_id":first["snapshot_id"],"prefix":"src/"});
    let files = tools::execute(&w, "list_source_files", &pinned)
        .await
        .unwrap();
    assert_eq!(files["files"][0]["source_path"], "src/access.rs");
    let wiki = first["files"][0]["path"].as_str().unwrap();
    assert!(wiki.ends_with("/openwiki/Access.md"));
    std::fs::write(w.path(wiki).unwrap(), "changed outside the selected prefix").unwrap();
    assert!(
        tools::execute(&w, "list_source_files", &pinned)
            .await
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
}

#[tokio::test]
async fn material_discovery_separates_original_extraction_and_metadata_and_rejects_tampering() {
    let (_parent, w) = fixture().await;
    let catalog = tools::execute(&w, "list_source_files", &json!({}))
        .await
        .unwrap();
    let id = catalog["sources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["kind"] == "source")
        .unwrap()["id"]
        .clone();
    let files = tools::execute(&w, "list_source_files", &json!({"source_id":id}))
        .await
        .unwrap();
    assert_eq!(files["content_read"], false);
    assert_eq!(files["files"].as_array().unwrap().len(), 2);
    for f in files["files"].as_array().unwrap() {
        assert_ne!(f["path"], files["source_note"]);
        let read = tools::execute(&w, "read_document", &json!({"path":f["path"]}))
            .await
            .unwrap();
        assert_eq!(read["sha256"], f["sha256"]);
    }
    let original = files["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "original")
        .unwrap()["path"]
        .as_str()
        .unwrap();
    std::fs::write(w.path(original).unwrap(), "tampered").unwrap();
    assert!(
        tools::execute(&w, "list_source_files", &json!({"source_id":id}))
            .await
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );
}

#[tokio::test]
async fn unavailable_extractions_and_binary_sources_are_explicit() {
    let (_parent, w) = fixture().await;
    let bytes = [0xff, 0xfe, 0x00];
    let asset = "01_Sources/Assets/image.png";
    std::fs::write(w.path(asset).unwrap(), bytes).unwrap();
    let content=note(json!({"type":"source","id":"SRC-BINARY","title":"Synthetic image","language":"en","asset":asset,"sha256":hash(&bytes),"extraction_status":"extraction_failed"}),"# Synthetic image").unwrap();
    std::fs::write(w.path("01_Sources/Notes/SRC-BINARY.md").unwrap(), content).unwrap();
    let value = tools::execute(&w, "list_source_files", &json!({"source_id":"SRC-BINARY"}))
        .await
        .unwrap();
    assert_eq!(value["files"][0]["can_read_document"], false);
    assert!(!value["problems"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn discovery_rejects_linked_inputs_and_keeps_application_internals_private() {
    let (_parent, w) = fixture().await;
    let files = tools::execute(&w, "list_source_files", &json!({"source_id":"REPO-EVAL"}))
        .await
        .unwrap();
    let path = w.path(files["files"][0]["path"].as_str().unwrap()).unwrap();
    let original = std::fs::read(&path).unwrap();
    let outside = w.root.parent().unwrap().join("outside.md");
    std::fs::write(&outside, original).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(outside, &path).unwrap();
    assert!(
        tools::execute(&w, "list_source_files", &json!({"source_id":"REPO-EVAL"}))
            .await
            .is_err()
    );
    std::fs::write(w.root.join("agent/Private.md"), "private").unwrap();
    for p in [
        "agent/Private.md",
        "agent/config.example.toml",
        "agent/src/main.rs",
        ".pm-agent/config.toml",
        "tests/source_navigation.rs",
    ] {
        assert!(w.read(p).is_err(), "{p}");
    }
    assert!(
        w.documents()
            .unwrap()
            .iter()
            .all(|d| d.path != "agent/Private.md")
    );
    assert!(
        w.documents()
            .unwrap()
            .iter()
            .any(|d| d.path == "agent/Materials.md")
    );
    let spec = tools::specs()
        .into_iter()
        .find(|s| s.name == "list_source_files")
        .unwrap();
    assert!(tools::validate_arguments(&spec.parameters, &json!({})).is_ok());
    for args in [
        json!({"limit":0}),
        json!({"limit":101}),
        json!({"offset":-1}),
        json!({"path":"../private"}),
    ] {
        assert!(tools::validate_arguments(&spec.parameters, &args).is_err());
    }
    for args in [
        json!({"source_id":"../escape"}),
        json!({"source_id":"PROJECT-EVAL"}),
        json!({"source_id":"REPO-EVAL","snapshot_id":"../escape"}),
        json!({"prefix":"src/"}),
    ] {
        assert!(
            tools::execute(&w, "list_source_files", &args)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn evaluation_includes_source_guides_reached_by_the_agent() {
    let (_parent, w) = fixture().await;
    for path in [
        "03_Systems/OpenWiki-Integration.md",
        "03_Systems/Integration-Review.md",
        "02_Domain/Code-Mappings.md",
        "08_Verification/Acceptance-and-Release.md",
        "agent/Materials.md",
    ] {
        assert!(
            w.read(path).is_ok(),
            "Referenced framework guidance missing: {path}"
        );
    }
    // Every local Markdown guide linked from this seed and present in the
    // framework must also be reachable here. Do not follow links to copy data.
    let framework = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut missing = vec![];
    for document in w.documents().unwrap() {
        if document.path.starts_with("01_Sources/Notes/") {
            continue;
        }
        for event in pulldown_cmark::Parser::new(&document.body) {
            if let pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) = event
            {
                let target = dest_url.split('#').next().unwrap();
                if target.contains("://") || !target.ends_with(".md") {
                    continue;
                }
                let mut relative = Path::new(&document.path).parent().unwrap().to_path_buf();
                for part in Path::new(target).components() {
                    match part {
                        std::path::Component::Normal(p) => relative.push(p),
                        std::path::Component::ParentDir => {
                            relative.pop();
                        }
                        std::path::Component::CurDir => {}
                        _ => continue,
                    }
                }
                if framework.join(&relative).is_file()
                    && w.read(relative.to_str().unwrap()).is_err()
                {
                    missing.push(format!("{} -> {}", document.path, relative.display()));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "Unreachable local knowledge references: {missing:?}"
    );
}
