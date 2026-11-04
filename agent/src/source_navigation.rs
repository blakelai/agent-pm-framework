//! Discover registered evidence without walking live repositories or changing inputs.
use crate::{
    records::{self, Records},
    sources,
    workspace::{MAX_FILE, Workspace, hash},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

fn page(mut rows: Vec<Value>, offset: usize, limit: usize, key: &str) -> Value {
    rows.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let total = rows.len();
    let next = offset.saturating_add(limit);
    let mut value = json!({"total":total,"offset":offset,"next_offset":if next < total {Some(next)}else{None},"content_read":false,"trust":"source_data"});
    value[key] = json!(
        rows.into_iter()
            .skip(offset)
            .take(limit)
            .collect::<Vec<_>>()
    );
    value
}

fn file(w: &Workspace, path: &str, kind: &str, expected: Option<&str>) -> Result<Value> {
    let bytes = sources::bytes(&w.path(path)?, 50_000_000)?;
    let sha = hash(&bytes);
    if let Some(expected) = expected {
        ensure!(sha == expected, "Source hash mismatch: {path}");
    }
    Ok(
        json!({"path":path,"kind":kind,"sha256":sha,"bytes":bytes.len(),
        "can_read_document":bytes.len() as u64 <= MAX_FILE && std::str::from_utf8(&bytes).is_ok()}),
    )
}

pub fn list(w: &Workspace, args: &Value) -> Result<Value> {
    let offset = args
        .get("offset")
        .map(|v| v.as_u64().context("offset must be a nonnegative integer"))
        .transpose()?
        .unwrap_or(0);
    let limit = args
        .get("limit")
        .map(|v| v.as_u64().context("limit must be an integer"))
        .transpose()?
        .unwrap_or(50);
    ensure!(
        offset <= 100_000 && (1..=100).contains(&limit),
        "Use offset <= 100000 and limit 1 through 100"
    );
    let source = args
        .get("source_id")
        .filter(|v| !v.is_null())
        .map(|v| v.as_str().context("source_id must be a string"))
        .transpose()?;
    let snapshot = args
        .get("snapshot_id")
        .filter(|v| !v.is_null())
        .map(|v| v.as_str().context("snapshot_id must be a string"))
        .transpose()?;
    let prefix = args
        .get("prefix")
        .map(|v| v.as_str().context("prefix must be a string"))
        .transpose()?
        .unwrap_or("");
    let r = Records::load(w)?;
    let Some(id) = source else {
        ensure!(
            snapshot.is_none() && prefix.is_empty(),
            "Choose source_id before snapshot_id or prefix"
        );
        return Ok(page(r.active.iter().filter(|d|matches!(d.kind.as_str(),"source"|"repository"))
            .map(|d|json!({"id":d.id,"kind":d.kind,"path":d.path,"title":d.title,"latest_snapshot":d.metadata.get("latest_snapshot"),"content_read":false})).collect(),offset as usize,limit as usize,"sources"));
    };
    records::identifier(id)?;
    let d = r.by_id(id)?;
    let mut files = vec![];
    let mut problems = vec![];
    let mut selected_snapshot = None;
    let mut repo_id = None;
    let mut capture = Value::Null;
    match d.kind.as_str() {
        "source" => {
            ensure!(
                snapshot.is_none(),
                "snapshot_id applies only to repository records"
            );
            let asset = d.metadata["asset"]
                .as_str()
                .context("Source has no asset path")?;
            ensure!(
                asset.starts_with("01_Sources/Assets/"),
                "Original must be under 01_Sources/Assets"
            );
            let expected = d.metadata["sha256"]
                .as_str()
                .context("Source has no pinned hash")?;
            files.push(file(w, asset, "original", Some(expected))?);
            let extracted = format!("01_Sources/Extracted/{id}.md");
            // A missing extraction is an explicit gap, not permission to hide
            // a linked path or publish a preview of an unverified original.
            let safe = w.path(&extracted)?;
            if safe.exists() {
                files.push(file(w, &extracted, "extraction", None)?);
            } else {
                problems.push(format!("Extraction unavailable: {extracted}"));
            }
            if d.metadata["extraction_status"] != "text_extracted" {
                problems.push("Review the original for missing visual/layout content and the recorded extraction status.".into());
            }
        }
        "repository" => {
            let rid = d.metadata["repo_id"]
                .as_str()
                .context("Repository has no repo_id")?;
            ensure!(
                id == format!("REPO-{rid}"),
                "Repository record identity mismatch"
            );
            let sid = snapshot
                .or_else(|| d.metadata["latest_snapshot"].as_str())
                .context("No captured snapshot; run pm sync with the registered repo_id first")?;
            let m = sources::manifest(w, rid, sid)?;
            selected_snapshot = Some(sid);
            repo_id = Some(rid);
            capture = json!({"manifest_path":format!("03_Systems/Snapshots/{sid}/manifest.json"),
                "revision":m["revision"],"dirty":m["dirty"],"captured_at":m["captured_at"],
                "skipped_count":m["skipped"].as_array().map(Vec::len),
                "missing_root_count":m["missing_roots"].as_array().map(Vec::len),
                "openwiki_active_run":m["openwiki_active_run"],"claims_validation":"not_performed"});
            if m["skipped"].as_array().is_some_and(|a| !a.is_empty())
                || m["missing_roots"].as_array().is_some_and(|a| !a.is_empty())
                || m["openwiki_active_run"] == true
            {
                problems.push("Captured scope has skipped/missing files or an active OpenWiki run; inspect the returned manifest_path before claiming completeness.".into());
            }
            for (path, sha) in m["files"]
                .as_object()
                .context("Snapshot file mapping missing")?
            {
                // Manifest verification covers all captured files, including
                // paths outside the selected page or prefix.
                files.push(json!({"path":format!("03_Systems/Snapshots/{sid}/files/{path}"),"source_path":path,"kind":"snapshot_file","sha256":sha}));
            }
        }
        _ => anyhow::bail!("Choose a source or repository ID from list_source_files"),
    }
    files.retain(|f| {
        f["source_path"]
            .as_str()
            .or_else(|| f["path"].as_str())
            .is_some_and(|p| p.starts_with(prefix))
    });
    let mut result = page(files, offset as usize, limit as usize, "files");
    result["source_id"] = json!(id);
    result["source_note"] = json!(d.path);
    result["repo_id"] = json!(repo_id);
    result["snapshot_id"] = json!(selected_snapshot);
    result["snapshot"] = capture;
    result["extraction_status"] = d
        .metadata
        .get("extraction_status")
        .cloned()
        .unwrap_or(Value::Null);
    result["problems"] = json!(problems);
    result["next_step"] = json!(
        "Read relevant returned paths with read_document; listing or reading a source note does not inspect source content. Request missing material when it is unavailable."
    );
    Ok(result)
}
