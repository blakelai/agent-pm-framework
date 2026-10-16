use pm_agent::{
    profile, records, tools,
    workspace::{Workspace, hash, note, parse_note},
};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(std::env::var("PM_TEST_PYTHON").unwrap_or_else(|_| "python3".into()))
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/source_fixtures.py"))
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
async fn run(w: &Workspace, command: &str, args: &[&str]) -> Value {
    tools::run_pm(
        w,
        command,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
    )
    .await
    .unwrap()
}
fn document(root: &Path, path: &str) -> (Value, String) {
    parse_note(&fs::read_to_string(root.join(path)).unwrap()).unwrap()
}
fn without_dates(mut meta: Value) -> Value {
    for key in ["imported_at", "captured_at", "created_at"] {
        meta.as_object_mut().unwrap().remove(key);
    }
    meta
}

#[tokio::test]
async fn imported_sources_preserve_bytes_locators_languages_and_legacy_metadata() {
    let dir = fixture();
    let w = Workspace::open(&dir.path().join("native")).unwrap();
    let legacy = dir.path().join("legacy");
    let expected: Value =
        serde_json::from_slice(&fs::read(dir.path().join("expected.json")).unwrap()).unwrap();
    for case in expected["ingest"].as_array().unwrap() {
        let file = case["file"].as_str().unwrap();
        let result = run(&w, "ingest", &[file]).await;
        assert_eq!(result["successful"], true, "{file}: {result}");
        assert_eq!(result["engine"], "rust");
        let path = case["path"].as_str().unwrap();
        let (meta, body) = document(&w.root, path);
        assert_eq!(
            without_dates(meta.clone()),
            without_dates(case["meta"].clone()),
            "{file}"
        );
        assert_eq!(
            fs::read(w.root.join(meta["asset"].as_str().unwrap())).unwrap(),
            fs::read(file).unwrap()
        );
        if meta["extraction_status"] != "extraction_failed" {
            assert_eq!(body.trim(), case["body"].as_str().unwrap().trim());
        }
        let extracted = format!("01_Sources/Extracted/{}.md", meta["id"].as_str().unwrap());
        let text = fs::read_to_string(w.root.join(&extracted)).unwrap();
        if file.ends_with("pages.pdf") {
            for marker in [
                "page-1",
                "page-2",
                "page-3",
                "Page one",
                "Page two",
                "(No text extracted;",
            ] {
                assert!(text.contains(marker), "{text}");
            }
        } else {
            assert_eq!(text, fs::read_to_string(legacy.join(extracted)).unwrap());
        }
        let amended = note(meta.clone(), &format!("{body}\nHuman annotation")).unwrap();
        fs::write(w.root.join(path), &amended).unwrap();
        assert_eq!(run(&w, "ingest", &[file]).await["successful"], true);
        assert_eq!(fs::read_to_string(w.root.join(path)).unwrap(), amended);
    }
    let source = expected["ingest"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["file"].as_str().unwrap().ends_with("原文.txt"))
        .unwrap();
    let result = run(
        &w,
        "evidence",
        &[
            "--id",
            "EVD-MATERIAL",
            "--source",
            source["meta"]["id"].as_str().unwrap(),
            "--locator",
            "text",
            "--claim",
            "Provided claim",
        ],
    )
    .await;
    assert_eq!(result["successful"], true, "{result}");
    let (meta, _) = document(&w.root, "01_Sources/Evidence/EVD-MATERIAL.md");
    assert_eq!(meta["assertion_type"], "reported_need");
    assert_eq!(meta["workflow_status"], "draft");
    assert_eq!(meta["source_sha256"], source["meta"]["sha256"]);
    fs::write(
        w.root.join(source["meta"]["asset"].as_str().unwrap()),
        "tampered",
    )
    .unwrap();
    assert_eq!(
        run(&w, "ingest", &[source["file"].as_str().unwrap()]).await["successful"],
        false
    );
    assert_eq!(
        run(
            &w,
            "evidence",
            &[
                "--id",
                "EVD-TAMPER",
                "--source",
                source["meta"]["id"].as_str().unwrap(),
                "--locator",
                "text",
                "--claim",
                "Claim"
            ]
        )
        .await["successful"],
        false
    );
}

#[tokio::test]
async fn snapshots_and_evidence_match_legacy_and_reject_changed_pinned_bytes() {
    let dir = fixture();
    let w = Workspace::open(&dir.path().join("native")).unwrap();
    let legacy = dir.path().join("legacy");
    let args = [
        "--id",
        "service",
        "--path",
        "01_Sources/Assets/repository",
        "--wiki",
        "openwiki",
        "--code-root",
        "src",
        "--code-root",
        "missing",
    ];
    assert_eq!(run(&w, "repo-add", &args).await["successful"], true);
    let (mut registered, body) = document(&legacy, "03_Systems/Repositories/REPO-service.md");
    registered["latest_snapshot"] = Value::Null;
    assert_eq!(
        document(&w.root, "03_Systems/Repositories/REPO-service.md"),
        (registered, body)
    );
    let result = run(&w, "sync", &["--repo", "service"]).await;
    assert_eq!(result["successful"], true, "{result}");
    let expected: Value =
        serde_json::from_slice(&fs::read(dir.path().join("expected.json")).unwrap()).unwrap();
    assert_eq!(result["reports"][0]["path"], expected["snapshot"]);
    let base = Path::new(expected["snapshot"].as_str().unwrap())
        .parent()
        .unwrap();
    let read_manifest = |root: &Path| {
        serde_json::from_slice::<Value>(&fs::read(root.join(base).join("manifest.json")).unwrap())
            .unwrap()
    };
    let m = read_manifest(&w.root);
    assert_eq!(
        without_dates(m.clone()),
        without_dates(read_manifest(&legacy))
    );
    for path in m["files"].as_object().unwrap().keys() {
        assert_eq!(
            fs::read(w.root.join(base).join("files").join(path)).unwrap(),
            fs::read(legacy.join(base).join("files").join(path)).unwrap()
        );
    }
    let manifest_bytes = fs::read(w.root.join(base).join("manifest.json")).unwrap();
    assert_eq!(
        run(&w, "sync", &["--repo", "service"]).await["successful"],
        true
    );
    assert_eq!(
        fs::read(w.root.join(base).join("manifest.json")).unwrap(),
        manifest_bytes
    );
    for (id, path, claim) in [
        ("EVD-CODE", "src/handler.rs", "Static observation"),
        ("EVD-WIKI", "openwiki/overview.md", "Documented behavior"),
    ] {
        let args = [
            "--id", id, "--repo", "service", "--path", path, "--start", "1", "--end", "3",
            "--claim", claim,
        ];
        assert_eq!(run(&w, "evidence", &args).await["successful"], true);
        let (m, b) = document(&w.root, &format!("01_Sources/Evidence/{id}.md"));
        let (old, old_body) = document(&legacy, &format!("01_Sources/Evidence/{id}.md"));
        assert_eq!(without_dates(m), without_dates(old));
        assert_eq!(b.trim(), old_body.trim());
        assert_eq!(run(&w, "evidence", &args).await["successful"], false);
    }
    fs::write(w.root.join(base).join("files/src/handler.rs"), "tampered\n").unwrap();
    assert_eq!(
        run(&w, "sync", &["--repo", "service"]).await["successful"],
        false
    );
    assert_eq!(
        run(
            &w,
            "evidence",
            &[
                "--id",
                "EVD-TAMPER",
                "--repo",
                "service",
                "--path",
                "src/handler.rs",
                "--start",
                "1",
                "--end",
                "1",
                "--claim",
                "Claim"
            ]
        )
        .await["successful"],
        false
    );
    assert!(!w.root.join("01_Sources/Evidence/EVD-TAMPER.md").exists());
}

#[tokio::test]
async fn source_scope_path_and_argument_errors_do_not_publish_records() {
    let dir = fixture();
    let w = Workspace::open(&dir.path().join("native")).unwrap();
    for args in [
        vec![
            "--id",
            "bad",
            "--path",
            "01_Sources/Assets/repository",
            "--wiki",
            "../../outside",
        ],
        vec!["--id", "../bad", "--path", "01_Sources/Assets/repository"],
        vec![
            "--id",
            "bad",
            "--path",
            "01_Sources/Assets/repository",
            "--code-root",
            "../outside",
        ],
    ] {
        assert_eq!(run(&w, "repo-add", &args).await["successful"], false);
    }
    assert!(!w.root.join("03_Systems/Repositories/REPO-bad.md").exists());
    assert_eq!(
        run(
            &w,
            "repo-add",
            &["--id", "service", "--path", "01_Sources/Assets/repository"]
        )
        .await["successful"],
        true
    );
    fs::write(
        w.root.join("01_Sources/Assets/repository/.openwikiignore"),
        "!src/handler.rs",
    )
    .unwrap();
    assert_eq!(
        run(&w, "sync", &["--repo", "service"]).await["successful"],
        false
    );
    fs::write(
        w.root.join("01_Sources/Assets/repository/.openwikiignore"),
        "**/*.rs\nopenwiki/\n",
    )
    .unwrap();
    assert_eq!(
        run(&w, "sync", &["--repo", "service"]).await["successful"],
        false
    );
    assert_eq!(
        run(&w, "evidence", &["--id", "EVD-MISSING", "--claim", "Claim"]).await["exit_code"],
        2
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            dir.path().join("inputs/原文.txt"),
            w.root.join("01_Sources/Assets/linked.txt"),
        )
        .unwrap();
        assert_eq!(
            run(&w, "ingest", &["01_Sources/Assets/linked.txt"]).await["successful"],
            false
        );
        std::os::unix::fs::symlink(
            dir.path().join("inputs"),
            w.root.join("01_Sources/Assets/repository/linked"),
        )
        .unwrap();
        assert_eq!(
            run(
                &w,
                "repo-add",
                &[
                    "--id",
                    "linked",
                    "--path",
                    "01_Sources/Assets/repository",
                    "--wiki",
                    "linked"
                ]
            )
            .await["successful"],
            false
        );
    }
}

#[tokio::test]
async fn portable_bundles_preserve_attachments_and_use_distinct_core_validation() {
    let dir = fixture();
    let w = Workspace::open(&dir.path().join("legacy")).unwrap();
    let checks: Value =
        serde_json::from_slice(&fs::read(dir.path().join("checks.json")).unwrap()).unwrap();
    for (name, expected) in checks.as_object().unwrap() {
        let path = dir.path().join("checks").join(name);
        let result = run(&w, "okf-check", &[path.to_str().unwrap()]).await;
        let actual = &result["validation"];
        for key in [
            "conformant",
            "documents_checked",
            "scope",
            "okf_version",
            "warnings",
        ] {
            assert_eq!(actual[key], expected[key], "{name}: {result}");
        }
        assert_eq!(
            actual["errors"].as_array().unwrap().len(),
            expected["errors"].as_array().unwrap().len()
        );
        assert_eq!(result["successful"], expected["conformant"]);
    }
    let source = w.root.join(".agents/skills/pm-prd/SKILL.md");
    let before = fs::read(&source).unwrap();
    let raw = "01_Sources/Assets/repository/openwiki/overview.md";
    let output = dir.path().join("bundle");
    let result = run(&w, "okf-export", &["--output", output.to_str().unwrap()]).await;
    assert_eq!(result["successful"], true, "{result}");
    assert!(profile::check_bundle(&output).unwrap().conformant);
    assert_eq!(fs::read(&source).unwrap(), before);
    assert_eq!(
        fs::read(output.join(format!("{raw}.txt"))).unwrap(),
        fs::read(w.root.join(raw)).unwrap()
    );
    let (meta, body) = document(&output, "90_Agent/Skills/pm-prd.md");
    assert_eq!(meta["type"], "Agent Skill");
    assert!(!body.contains("[["));
    assert!(body.contains("../../AGENTS.md"));
    let map: Value =
        serde_json::from_slice(&fs::read(output.join("export-map.json")).unwrap()).unwrap();
    let oracle = dir.path().join("legacy-bundle");
    let legacy_map: Value =
        serde_json::from_slice(&fs::read(oracle.join("export-map.json")).unwrap()).unwrap();
    assert_eq!(map["paths"], legacy_map["paths"]);
    for dest in map["paths"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_str)
    {
        if dest.ends_with(".md")
            && !matches!(
                Path::new(dest).file_name().unwrap().to_str(),
                Some("index.md" | "log.md")
            )
        {
            let (meta, body) = document(&output, dest);
            let (expected, expected_body) = document(&oracle, dest);
            assert_eq!(meta, expected, "{dest}");
            assert_eq!(body.trim(), expected_body.trim(), "{dest}");
        }
    }
    for (from, to) in map["paths"].as_object().unwrap() {
        if to.as_str().unwrap().ends_with(".md.txt") {
            assert_eq!(
                hash(&fs::read(w.root.join(from)).unwrap()),
                hash(&fs::read(output.join(to.as_str().unwrap())).unwrap())
            );
        }
    }
    for path in [output, w.root.join("exports/nested"), dir.path().to_owned()] {
        assert_eq!(
            run(&w, "okf-export", &["--output", path.to_str().unwrap()]).await["successful"],
            false
        );
    }
    let broken = w.root.join("90_Agent/invalid.md");
    fs::write(&broken, "# Missing profile metadata").unwrap();
    let failed = dir.path().join("failed");
    assert_eq!(
        run(&w, "okf-export", &["--output", failed.to_str().unwrap()]).await["successful"],
        false
    );
    assert!(!failed.exists());
}

#[test]
fn every_pm_entry_runs_as_a_binary_with_no_python_or_model_configuration() {
    let dir = fixture();
    let vault = dir.path().join("native");
    for command in [
        "plan",
        "validate",
        "coverage",
        "status",
        "impact",
        "baseline",
        "fingerprint",
        "settings",
        "prepare",
        "ingest",
        "repo-add",
        "sync",
        "evidence",
        "okf-export",
        "okf-check",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_pm-agent"))
            .env("PATH", dir.path().join("empty-path"))
            .arg("--vault")
            .arg(&vault)
            .args(["pm", command, "--help"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["engine"], "rust");
    }
    let output = Command::new(env!("CARGO_BIN_EXE_pm-agent"))
        .env("PATH", dir.path().join("empty-path"))
        .arg("--vault")
        .arg(&vault)
        .args(["pm", "ingest"])
        .arg(dir.path().join("inputs/pages.pdf"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["engine"], "rust");
    let d = records::read(
        &Workspace::open(&vault).unwrap(),
        result["reports"][0]["path"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(
        d.metadata["extraction_status"],
        json!("visual_review_required")
    );
}

#[tokio::test]
async fn office_limits_and_external_entities_preserve_source_with_failure_status() {
    let dir = fixture();
    let w = Workspace::open(&dir.path().join("native")).unwrap();
    for name in ["entities.docx", "oversize.docx"] {
        let input = dir.path().join("unsafe").join(name);
        let result = run(&w, "ingest", &[input.to_str().unwrap()]).await;
        assert_eq!(result["successful"], true, "{result}");
        let (meta, body) = document(&w.root, result["reports"][0]["path"].as_str().unwrap());
        assert_eq!(meta["extraction_status"], "extraction_failed");
        assert_eq!(meta["review_status"], "pending");
        assert!(!body.contains("NEVER-EXTRACT-THIS-PRIVATE-FIXTURE"));
        assert_eq!(
            fs::read(w.root.join(meta["asset"].as_str().unwrap())).unwrap(),
            fs::read(input).unwrap()
        );
    }
}

#[tokio::test]
async fn git_revision_and_dirty_state_are_read_without_running_repository_hooks() {
    let dir = fixture();
    let w = Workspace::open(&dir.path().join("native")).unwrap();
    let repo = w.root.join("01_Sources/Assets/repository");
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init"]);
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "-m",
        "Synthetic test fixture",
    ]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook = repo.join(".git/hooks/fixture-monitor");
        fs::write(&hook, "#!/bin/sh\nprintf unsafe > .git/fixture-hook-ran\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
        git(&["config", "core.fsmonitor", hook.to_str().unwrap()]);
    }
    // Maximum-length repository IDs still produce valid, reusable snapshot IDs.
    let id = "r".repeat(80);
    assert_eq!(
        run(
            &w,
            "repo-add",
            &["--id", &id, "--path", repo.to_str().unwrap()]
        )
        .await["successful"],
        true
    );
    let first = run(&w, "sync", &["--repo", &id]).await;
    assert_eq!(first["successful"], true, "{first}");
    let path = first["reports"][0]["path"].as_str().unwrap();
    let (old, _) = document(&w.root, path);
    assert_eq!(old["dirty"], false);
    assert_ne!(old["revision"], "unversioned");
    assert_eq!(run(&w, "sync", &["--repo", &id]).await["successful"], true);
    fs::write(repo.join("src/handler.rs"), "// changed\n").unwrap();
    let second = run(&w, "sync", &["--repo", &id]).await;
    assert_eq!(second["successful"], true, "{second}");
    let (new, _) = document(&w.root, second["reports"][0]["path"].as_str().unwrap());
    assert_eq!(new["dirty"], true);
    assert_eq!(new["revision"], old["revision"]);
    assert_ne!(new["id"], old["id"]);
    assert!(!repo.join(".git/fixture-hook-ran").exists());
}
