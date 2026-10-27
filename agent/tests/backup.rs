use pm_agent::{backup, store::Store, workspace::Workspace};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
    process::Command,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

fn put(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    put(
        dir.path(),
        "00_Governance/Project.md",
        b"---\ntype: project\nlanguage: zh-TW\ntitle: Synthetic\n---\n\n# Synthetic project\n",
    );
    put(
        dir.path(),
        "00_Governance/Settings.md",
        b"---\ntype: project_settings\ndocument_language: zh-TW\nagent_language: en\n---\n",
    );
    for path in [
        "01_Sources/Assets/SOURCE/original.bin",
        "01_Sources/Assets/SOURCE/.env",
        "01_Sources/Extracted/source.txt",
        "01_Sources/Reviews/REVIEW-TEST.md",
        "03_Systems/Repositories/service/Snapshots/revision/openwiki/index.md",
        "00_Governance/Baselines/B-TEST/REQ-TEST.md",
        ".agents/skills/custom/SKILL.md",
        "90_Agent/Runs/RUN-TEST.md",
        "tools/locales/zh-TW.json",
        "tools/README.md",
        "agent/README.md",
        "Framework.canvas",
        "README.md",
    ] {
        put(
            dir.path(),
            path,
            "Exact synthetic bytes.\r\n原文\u{0000}\u{00ff}".as_bytes(),
        );
    }
    for path in [
        ".pm-agent/config.toml",
        ".pm-agent/probe.json",
        ".obsidian/plugins/plugin/main.js",
        ".git/config",
        ".env",
        "agent/src/main.rs",
        "agent/target/app",
        "arbitrary.txt",
        "node_modules/private.js",
        ".env.local",
    ] {
        put(dir.path(), path, b"excluded fixture");
    }
    let workspace = Workspace::open(dir.path()).unwrap();
    (dir, workspace)
}
fn contents(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let name = f.name().unwrap().to_string();
            let mut bytes = vec![];
            f.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn archive(path: &Path, entries: &[(String, Vec<u8>)]) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(
            name,
            SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o600),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}
fn change_manifest(entries: &mut [(String, Vec<u8>)], change: impl FnOnce(&mut Value)) {
    let bytes = &mut entries
        .iter_mut()
        .find(|(n, _)| n == "manifest.json")
        .unwrap()
        .1;
    let mut manifest: Value = serde_json::from_slice(bytes).unwrap();
    change(&mut manifest);
    *bytes = serde_json::to_vec(&manifest).unwrap();
}

#[test]
fn exact_project_roundtrip_has_explicit_scope_and_private_permissions() {
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("project.pmbak");
    let created = backup::create(&w, &path, false).unwrap();
    assert!(!created.includes_sessions);
    assert_eq!(created.files, 15);
    let entries = contents(&path);
    for excluded in [
        "config.toml",
        "probe.json",
        ".git/",
        ".obsidian/",
        "vault/.env",
        "main.rs",
        "target/",
        "arbitrary.txt",
        "node_modules",
    ] {
        assert!(
            !entries.iter().any(|(name, _)| name.contains(excluded)),
            "{excluded}"
        );
    }
    assert_eq!(backup::check(&path).unwrap().bytes, created.bytes);
    let restored = backup::restore(&path, out.path(), false).unwrap();
    assert_eq!(restored.files, created.files);
    assert!(!restored.path.join(".pm-agent").exists());
    for (name, bytes) in entries.iter().filter(|(n, _)| n.starts_with("vault/")) {
        let relative = name.strip_prefix("vault/").unwrap();
        assert_eq!(fs::read(w.root.join(relative)).unwrap(), *bytes);
        assert_eq!(fs::read(restored.path.join(relative)).unwrap(), *bytes);
    }
    // A second restore is a separate project, never a replacement.
    assert_ne!(
        backup::restore(&path, out.path(), false).unwrap().path,
        restored.path
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [&path, &restored.path.join("README.md")] {
            assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(
            fs::metadata(&restored.path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}

#[test]
fn live_wal_snapshot_preserves_sessions_and_uncertain_tool_outcomes() {
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
    let session = json!({"id":"synthetic-session","status":"Paused","request_history":[{"status":"interrupted"}]});
    store.save("synthetic-session", &session).unwrap();
    store
        .begin_call("synthetic-session", "uncertain", "external", "{}")
        .unwrap();
    store
        .begin_call("synthetic-session", "done", "local", "{}")
        .unwrap();
    store
        .finish_call("synthetic-session", "done", "completed", "fixture result")
        .unwrap();
    assert!(w.root.join(".pm-agent/runtime.sqlite3-wal").is_file());
    let path = out.path().join("private.pmbak");
    assert!(backup::create(&w, &path, true).unwrap().includes_sessions);
    let project_only = backup::restore(&path, out.path(), false).unwrap();
    assert!(!project_only.path.join(".pm-agent").exists());
    let restored = backup::restore(&path, out.path(), true).unwrap();
    assert!(!restored.path.join(".pm-agent/config.toml").exists());
    assert!(!restored.path.join(".pm-agent/runtime.sqlite3-wal").exists());
    let recovered = Store::open(&restored.path.join(".pm-agent/runtime.sqlite3")).unwrap();
    assert_eq!(
        recovered.load::<Value>("synthetic-session").unwrap(),
        session
    );
    assert_eq!(
        recovered
            .call_result("synthetic-session", "uncertain")
            .unwrap(),
        Some(("started".into(), None))
    );
    assert_eq!(
        recovered.call_result("synthetic-session", "done").unwrap(),
        Some(("completed".into(), Some("fixture result".into())))
    );
    assert!(
        recovered
            .begin_call("synthetic-session", "uncertain", "external", "{}")
            .is_err()
    );
}

#[test]
fn creation_refuses_overwrites_active_writers_nested_backups_and_missing_sessions() {
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("project.pmbak");
    fs::write(&path, b"existing file").unwrap();
    assert!(backup::create(&w, &path, false).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"existing file");
    assert!(backup::create(&w, &w.root.join("nested.pmbak"), false).is_err());
    let missing = out.path().join("missing.pmbak");
    assert!(backup::create(&w, &missing, true).is_err());
    assert!(!missing.exists());
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(w.root.join(".pm-agent/workspace.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(
        backup::create(&w, &missing, false)
            .unwrap_err()
            .to_string()
            .contains("Close other")
    );
    drop(lock);
    backup::create(&w, &missing, false).unwrap();
    assert!(backup::restore(&missing, &w.root, false).is_err());
    assert!(backup::restore(&missing, out.path(), true).is_err());
}

#[test]
fn damaged_missing_extra_and_traversing_entries_never_publish_a_restored_vault() {
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let good = out.path().join("good.pmbak");
    backup::create(&w, &good, false).unwrap();
    let original = contents(&good);
    let destination = out.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let bad = out.path().join("bad.pmbak");
    for variant in 0..8 {
        let mut entries = original.clone();
        match variant {
            0 => {
                entries
                    .iter_mut()
                    .find(|(n, _)| n == "vault/README.md")
                    .unwrap()
                    .1[0] ^= 1
            }
            1 => entries.retain(|(n, _)| n != "vault/README.md"),
            2 => entries.push(("vault/00_Governance/extra.md".into(), b"extra".to_vec())),
            3 => entries.push(("vault/../escaped.md".into(), b"escape".to_vec())),
            4 => entries.push(("vault/00_Governance\\escape.md".into(), vec![])),
            5 => entries.push(("/absolute.md".into(), vec![])),
            6 => entries.push(("vault/readme.md".into(), vec![])),
            _ => entries.push(("vault/00_Governance/\u{001b}[31m.md".into(), vec![])),
        }
        archive(&bad, &entries);
        if variant == 7 {
            assert!(
                !backup::check(&bad)
                    .unwrap_err()
                    .to_string()
                    .contains('\u{001b}')
            );
        }
        assert!(backup::check(&bad).is_err(), "variant {variant}");
        assert!(
            backup::restore(&bad, &destination, false).is_err(),
            "variant {variant}"
        );
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    }
    assert!(!out.path().join("escaped.md").exists());
}

#[test]
fn unknown_formats_inconsistent_manifests_and_size_limits_fail_closed() {
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let good = out.path().join("good.pmbak");
    backup::create(&w, &good, false).unwrap();
    let original = contents(&good);
    let bad = out.path().join("bad.pmbak");
    for variant in 0..8 {
        let mut entries = original.clone();
        change_manifest(&mut entries, |m| match variant {
            0 => m["format"] = json!("pm-vault-backup-v999"),
            1 => m["scope"] = json!("entire-filesystem"),
            2 => m["includes_sessions"] = json!(true),
            3 => m["files"][0]["bytes"] = json!(50 * 1024 * 1024 + 1),
            4 => m["files"][0]["sha256"] = json!("bad"),
            5 => {
                let duplicate = m["files"][0].clone();
                m["files"].as_array_mut().unwrap().push(duplicate);
            }
            6 => m["files"][0]["path"] = json!("../outside.md"),
            _ => m["created_at"] = json!("not a date"),
        });
        archive(&bad, &entries);
        assert!(backup::check(&bad).is_err(), "variant {variant}");
    }
    let oversized = w.root.join("01_Sources/Assets/oversized.bin");
    File::create(&oversized)
        .unwrap()
        .set_len(50 * 1024 * 1024 + 1)
        .unwrap();
    let target = out.path().join("oversized.pmbak");
    assert!(backup::create(&w, &target, false).is_err());
    assert!(!target.exists());
}

#[test]
fn duplicate_zip_names_and_oversized_directory_count_are_rejected_before_extraction() {
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let good = out.path().join("good.pmbak");
    backup::create(&w, &good, false).unwrap();
    let mut entries = contents(&good);
    entries.push(("vault/README.mx".into(), b"duplicate after patch".to_vec()));
    let bad = out.path().join("duplicate.pmbak");
    archive(&bad, &entries);
    // Patch both local and central names without changing offsets; the ZIP reader normally deduplicates them.
    let mut data = fs::read(&bad).unwrap();
    let needle = b"vault/README.mx";
    for i in 0..data.len() - needle.len() {
        if &data[i..i + needle.len()] == needle {
            data[i + needle.len() - 1] = b'd';
        }
    }
    fs::write(&bad, &data).unwrap();
    assert!(
        backup::check(&bad)
            .unwrap_err()
            .to_string()
            .contains("Duplicate")
    );
    let mut data = fs::read(&good).unwrap();
    let end = data.len() - 22;
    data[end + 8..end + 12].copy_from_slice(&[255; 4]);
    fs::write(&bad, &data).unwrap();
    assert!(
        backup::check(&bad)
            .unwrap_err()
            .to_string()
            .contains("entry count")
    );
}

#[cfg(unix)]
#[test]
fn symlinks_and_runtime_sidecar_links_are_not_followed() {
    use std::os::unix::fs::symlink;
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("project.pmbak");
    let link = w.root.join("01_Sources/linked.md");
    symlink(out.path().join("missing"), &link).unwrap();
    assert!(backup::create(&w, &target, false).is_err());
    assert!(!target.exists());
    fs::remove_file(link).unwrap();
    let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
    store.save("s", &json!({})).unwrap();
    drop(store);
    let sidecar = w.root.join(".pm-agent/runtime.sqlite3-wal");
    symlink(out.path().join("missing"), &sidecar).unwrap();
    assert!(backup::create(&w, &target, true).is_err());
    fs::remove_file(sidecar).unwrap();
    backup::create(&w, &target, false).unwrap();
    let archive_link = out.path().join("linked.pmbak");
    symlink(&target, &archive_link).unwrap();
    assert!(backup::check(&archive_link).is_err());
    assert!(backup::restore(&archive_link, out.path(), false).is_err());
}

#[test]
fn restore_validates_session_database_without_publishing_invalid_runtime() {
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("runtime.pmbak");
    let store = Store::open(&w.path(".pm-agent/runtime.sqlite3").unwrap()).unwrap();
    store.save("s", &json!({})).unwrap();
    backup::create(&w, &path, true).unwrap();
    let mut entries = contents(&path);
    let bytes = b"Not a SQLite database";
    entries
        .iter_mut()
        .find(|(n, _)| n == "vault/.pm-agent/runtime.sqlite3")
        .unwrap()
        .1 = bytes.to_vec();
    change_manifest(&mut entries, |m| {
        let e = m["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|e| e["path"] == ".pm-agent/runtime.sqlite3")
            .unwrap();
        e["bytes"] = json!(bytes.len());
        e["sha256"] = json!(pm_agent::workspace::hash(bytes));
    });
    archive(&path, &entries);
    // A checksum verifies bytes, not the semantic validity or authenticity of a supplied file.
    backup::check(&path).unwrap();
    let destination = out.path().join("destination");
    fs::create_dir(&destination).unwrap();
    assert!(backup::restore(&path, &destination, true).is_err());
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    backup::restore(&path, &destination, false).unwrap();
}

#[test]
fn standalone_cli_recovers_without_original_vault_configuration_or_external_programs() {
    let (_source, w) = fixture();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("project.pmbak");
    let bin = env!("CARGO_BIN_EXE_pm-agent");
    let created = Command::new(bin)
        .env("PATH", "")
        .args([
            "--vault",
            w.root.to_str().unwrap(),
            "backup",
            "create",
            "--output",
            path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let missing = out.path().join("lost-vault");
    let args = [
        "--vault",
        missing.to_str().unwrap(),
        "--config",
        "/missing/config.toml",
        "backup",
    ];
    let checked = Command::new(bin)
        .env("PATH", "")
        .args(args)
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let restored = Command::new(bin)
        .env("PATH", "")
        .args(args)
        .args([
            "restore",
            path.to_str().unwrap(),
            "--parent",
            out.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let report: Value = serde_json::from_slice(&restored.stdout).unwrap();
    assert!(
        Path::new(report["path"].as_str().unwrap())
            .join("00_Governance/Project.md")
            .is_file()
    );
    assert!(!missing.exists());
}
