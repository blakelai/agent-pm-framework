//! Maintainer packaging helpers, deliberately absent from the Agent tool registry.
use crate::workspace::{Workspace, hash, parse_note};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::NamedTempFile;
use zip::{ZipWriter, write::SimpleFileOptions};

const INSTALLER: &[u8] = include_bytes!("../distribution/install.sh");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const EMPTY: &[&str] = &[
    "01_Sources/Assets/",
    "01_Sources/Extracted/",
    "01_Sources/Notes/",
    "01_Sources/Evidence/",
    "01_Sources/Reviews/",
    "03_Systems/Repositories/",
    "03_Systems/Analysis/",
    "03_Systems/Snapshots/",
    "04_Requirements/",
    "05_Backlog/",
    "07_Decisions/",
    "08_Verification/Cases/",
    "90_Agent/Runs/",
    "90_Agent/Reports/",
    "00_Governance/Baselines/",
];

#[derive(Debug, Serialize)]
pub struct Report {
    pub archive: PathBuf,
    pub checksum: PathBuf,
    pub sha256: String,
    pub target: String,
    pub version: String,
    pub framework_revision: String,
    pub files: usize,
}

pub fn target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        _ => anyhow::bail!("Prebuilt packages support macOS Apple Silicon and Linux x86_64 only"),
    }
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()?;
    ensure!(out.status.success(), "Git source inspection failed");
    Ok(String::from_utf8(out.stdout)?)
}

fn included(path: &str) -> bool {
    let first = path.split('/').next().unwrap_or("");
    matches!(
        first,
        "00_Governance"
            | "01_Sources"
            | "02_Domain"
            | "03_Systems"
            | "04_Requirements"
            | "05_Backlog"
            | "06_Delivery"
            | "07_Decisions"
            | "08_Verification"
            | "09_Operations"
            | "80_Templates"
            | "90_Agent"
            | ".agents"
    ) || (!path.contains('/')
        && (path.ends_with(".md") || path.ends_with(".canvas") || path == ".gitignore"))
        || (path.starts_with("agent/") && path.matches('/').count() == 1 && path.ends_with(".md"))
        || matches!(
            path,
            "agent/config.example.toml"
                | "tools/README.md"
                | "tests/README.md"
                | ".obsidian/app.json"
                | ".obsidian/appearance.json"
                | ".obsidian/core-plugins.json"
                | ".obsidian/templates.json"
        )
        || path.starts_with("tools/locales/")
}

fn inventory(root: &Path) -> Result<(String, Vec<String>)> {
    ensure!(
        fs::canonicalize(git(root, &["rev-parse", "--show-toplevel"])?.trim())? == root,
        "Package from the framework repository root"
    );
    ensure!(
        git(root, &["status", "--porcelain", "--untracked-files=normal"])?
            .trim()
            .is_empty(),
        "Packaging requires a clean committed checkout"
    );
    let revision = git(root, &["rev-parse", "HEAD"])?.trim().to_owned();
    ensure!(
        revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()),
        "Unsupported source revision"
    );
    let files: Vec<_> = git(root, &["ls-files", "-z"])?
        .split('\0')
        .filter(|p| !p.is_empty() && included(p))
        .map(str::to_owned)
        .collect();
    let (meta, _) = parse_note(&fs::read_to_string(root.join("00_Governance/Project.md"))?)?;
    ensure!(
        meta["id"].as_str() == Some("PROJECT-NEW")
            && meta["workflow_status"].as_str() == Some("unconfigured"),
        "Package the unconfigured framework, not a populated project"
    );
    for path in &files {
        ensure!(
            path.bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-._/".contains(&c))
                && !path
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == ".."),
            "Unsupported package path: {path}"
        );
        ensure!(
            !EMPTY.iter().any(|prefix| path.starts_with(prefix)) || path.ends_with("/.gitkeep"),
            "Project-specific records cannot be packaged: {path}"
        );
    }
    for required in [
        "AGENTS.md",
        "00_Governance/Settings.md",
        ".agents/skills/pm-workflow/SKILL.md",
        "tools/locales/en.json",
        "agent/config.example.toml",
    ] {
        ensure!(
            files.iter().any(|p| p == required),
            "Missing starter file: {required}"
        );
    }
    Ok((revision, files))
}

fn native_binary(bytes: &[u8], target: &str) -> bool {
    match target {
        "aarch64-apple-darwin" => bytes.starts_with(&[0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 1]),
        "x86_64-unknown-linux-gnu" => {
            bytes.starts_with(b"\x7fELF\x02\x01") && bytes.get(18..20) == Some(&[62, 0])
        }
        _ => false,
    }
}

fn assemble(
    root: &Path,
    binary: &[u8],
    revision: &str,
    files: &[String],
    target: &str,
    output: &Path,
    recheck: impl FnOnce() -> Result<()>,
) -> Result<Report> {
    let name = format!("pm-agent-{VERSION}-{}-{target}", &revision[..12]);
    let archive = output.join(format!("{name}.zip"));
    let checksum = output.join(format!("{name}.zip.sha256"));
    ensure!(
        !archive.try_exists()? && !checksum.try_exists()?,
        "Package destination exists; choose a new output directory"
    );
    let workspace = Workspace::open(root)?;
    let mut content = vec![
        ("bin/pm-agent".into(), binary.to_vec(), 0o755),
        ("install.sh".into(), INSTALLER.to_vec(), 0o755),
        ("VERSION".into(), format!("{VERSION}\n").into_bytes(), 0o644),
        ("TARGET".into(), format!("{target}\n").into_bytes(), 0o644),
        (
            "REVISION".into(),
            format!("{revision}\n").into_bytes(),
            0o644,
        ),
    ];
    for relative in files {
        let path = workspace.path(relative)?;
        ensure!(
            fs::symlink_metadata(&path)?.is_file()
                && fs::metadata(&path)?.len() <= 50 * 1024 * 1024,
            "Not a bounded regular starter file: {relative}"
        );
        content.push((format!("framework/{relative}"), fs::read(path)?, 0o644));
    }
    let metadata = serde_json::json!({"format":"pm-agent-package-v1", "version":VERSION, "target":target,
        "framework_revision":revision, "binary_sha256":hash(binary), "scope":"binary-and-clean-starter-v1"});
    content.push((
        "package.json".into(),
        serde_json::to_vec_pretty(&metadata)?,
        0o644,
    ));
    content.sort_by(|a, b| a.0.cmp(&b.0));
    let sums: String = content
        .iter()
        .map(|(p, b, _)| format!("{}  {p}\n", hash(b)))
        .collect();
    content.push(("SHA256SUMS".into(), sums.into_bytes(), 0o644));
    let file = NamedTempFile::new_in(output)?;
    let mut zip = ZipWriter::new(file);
    for (path, bytes, mode) in &content {
        zip.start_file(
            format!("{name}/{path}"),
            SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(*mode),
        )?;
        zip.write_all(bytes)?;
    }
    let mut temp = zip.finish()?;
    temp.as_file().sync_all()?;
    let mut bytes = Vec::new();
    use std::io::{Seek, SeekFrom};
    temp.seek(SeekFrom::Start(0))?;
    temp.read_to_end(&mut bytes)?;
    let sha256 = hash(&bytes);
    let mut sum = NamedTempFile::new_in(output)?;
    writeln!(sum, "{sha256}  {name}.zip")?;
    sum.as_file().sync_all()?;
    recheck()?;
    temp.persist_noclobber(&archive)?;
    sum.persist_noclobber(&checksum).context("Archive was created but checksum publication failed; do not distribute this incomplete pair")?;
    Ok(Report {
        archive,
        checksum,
        sha256,
        target: target.into(),
        version: VERSION.into(),
        framework_revision: revision.into(),
        files: content.len(),
    })
}

/// Build on each supported native host from a clean framework checkout and its trusted binary.
pub fn create(root: &Path, binary: &Path, output: &Path) -> Result<Report> {
    let root = fs::canonicalize(root)?;
    let output = fs::canonicalize(output)
        .context("Output must be an existing directory outside the checkout")?;
    ensure!(
        output.is_dir() && !output.starts_with(&root),
        "Output must be outside the checkout"
    );
    let (revision, files) = inventory(&root)?;
    let target = target()?;
    let meta = fs::symlink_metadata(binary)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 256 * 1024 * 1024,
        "Binary must be a regular file within 256 MiB"
    );
    let bytes = fs::read(binary)?;
    ensure!(
        native_binary(&bytes, target),
        "Binary architecture does not match this native packaging host"
    );
    let version = Command::new(binary).arg("--version").output()?;
    ensure!(
        version.status.success()
            && String::from_utf8(version.stdout)?.trim() == format!("pm-agent {VERSION}"),
        "Binary version does not match Cargo package version"
    );
    assemble(&root, &bytes, &revision, &files, target, &output, || {
        ensure!(
            inventory(&root)? == (revision.clone(), files.clone()),
            "Checkout changed during packaging"
        );
        ensure!(
            fs::read(binary)? == bytes,
            "Binary changed during packaging"
        );
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn put(root: &Path, path: &str, bytes: &[u8]) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    fn starter() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        put(
            dir.path(),
            "00_Governance/Project.md",
            b"---\nid: PROJECT-NEW\nworkflow_status: unconfigured\n---\n# Fixture\n",
        );
        for file in [
            "AGENTS.md",
            "00_Governance/Settings.md",
            ".agents/skills/pm-workflow/SKILL.md",
            "tools/locales/en.json",
            "agent/config.example.toml",
            ".obsidian/app.json",
        ] {
            put(dir.path(), file, b"synthetic starter\n");
        }
        dir
    }
    fn commit(root: &Path) {
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Fixture"],
            vec!["config", "user.email", "fixture@example.invalid"],
            vec!["add", "."],
            vec!["commit", "-qm", "fixture"],
        ] {
            git(root, &args).unwrap();
        }
    }
    fn bundle(parent: &Path, revision: char, binary: &[u8], platform: &str) -> PathBuf {
        let source = starter();
        let out = tempfile::tempdir().unwrap();
        let files = vec![
            "00_Governance/Project.md".into(),
            "00_Governance/Settings.md".into(),
            ".agents/skills/pm-workflow/SKILL.md".into(),
        ];
        let report = assemble(
            source.path(),
            binary,
            &revision.to_string().repeat(40),
            &files,
            platform,
            out.path(),
            || Ok(()),
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(&report.archive).unwrap()).unwrap();
        archive.extract(parent).unwrap();
        parent.join(report.archive.file_stem().unwrap())
    }
    fn install(package: &Path, prefix: &Path) -> std::process::Output {
        // macOS's temp directory may be reported through /var -> /private/var.
        let prefix = fs::canonicalize(prefix.parent().unwrap())
            .unwrap()
            .join(prefix.file_name().unwrap());
        Command::new("bash")
            .arg(package.join("install.sh"))
            .arg("--prefix")
            .arg(&prefix)
            .output()
            .unwrap()
    }
    fn succeeded(output: std::process::Output) {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn distribution_scope_excludes_runtime_builds_and_code_and_rejects_populated_or_dirty_starters()
    {
        let source = starter();
        for path in [
            ".pm-agent/config.toml",
            ".pm-agent/runtime.sqlite3",
            ".env",
            ".obsidian/workspace.json",
            "agent/target/private",
            "agent/src/main.rs",
            "tests/legacy/private.py",
        ] {
            put(source.path(), path, b"must not ship");
        }
        commit(source.path());
        let root = fs::canonicalize(source.path()).unwrap();
        let (_, files) = inventory(&root).unwrap();
        assert_eq!(files.len(), 7);
        assert!(
            files
                .iter()
                .all(|p| !p.contains("private") && !p.starts_with(".pm-agent"))
        );
        put(source.path(), "untracked.txt", b"draft");
        assert!(
            inventory(&root)
                .unwrap_err()
                .to_string()
                .contains("clean committed")
        );
        fs::remove_file(source.path().join("untracked.txt")).unwrap();
        put(
            source.path(),
            "05_Backlog/PBI-PRIVATE.md",
            b"project content",
        );
        commit(source.path());
        assert!(
            inventory(&root)
                .unwrap_err()
                .to_string()
                .contains("Project-specific")
        );
        fs::remove_file(source.path().join("05_Backlog/PBI-PRIVATE.md")).unwrap();
        put(
            source.path(),
            "00_Governance/Project.md",
            b"---\nid: PROJECT-REAL\nworkflow_status: draft\n---\n# Real project\n",
        );
        commit(source.path());
        assert!(
            inventory(&root)
                .unwrap_err()
                .to_string()
                .contains("unconfigured framework")
        );
    }

    #[test]
    fn distribution_checksums_preserve_bytes_modes_and_refuse_overwrites_or_linked_sources() {
        let source = starter();
        let out = tempfile::tempdir().unwrap();
        let files = vec![
            "00_Governance/Project.md".into(),
            ".agents/skills/pm-workflow/SKILL.md".into(),
        ];
        let revision = "a".repeat(40);
        let result = assemble(
            source.path(),
            b"binary fixture",
            &revision,
            &files,
            target().unwrap(),
            out.path(),
            || Ok(()),
        )
        .unwrap();
        let original = fs::read(&result.archive).unwrap();
        assert_eq!(result.sha256, hash(&original));
        assert!(
            fs::read_to_string(&result.checksum)
                .unwrap()
                .starts_with(&result.sha256)
        );
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&original)).unwrap();
        let top = result.archive.file_stem().unwrap().to_str().unwrap();
        let mut sums = String::new();
        zip.by_name(&format!("{top}/SHA256SUMS"))
            .unwrap()
            .read_to_string(&mut sums)
            .unwrap();
        for line in sums.lines() {
            let (expected, path) = line.split_once("  ").unwrap();
            let mut entry = zip.by_name(&format!("{top}/{path}")).unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            assert_eq!(hash(&bytes), expected);
            if path == "bin/pm-agent" {
                assert_eq!(entry.unix_mode().unwrap() & 0o777, 0o755);
            }
            if path == "framework/00_Governance/Project.md" {
                assert_eq!(
                    bytes,
                    fs::read(source.path().join("00_Governance/Project.md")).unwrap()
                );
            }
        }
        assert!(
            assemble(
                source.path(),
                b"replacement",
                &revision,
                &files,
                target().unwrap(),
                out.path(),
                || Ok(()),
            )
            .is_err()
        );
        assert_eq!(fs::read(&result.archive).unwrap(), original);
        assert!(
            assemble(
                source.path(),
                b"binary",
                &"b".repeat(40),
                &files,
                target().unwrap(),
                out.path(),
                || anyhow::bail!("fixture checkout changed")
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 2);
        let linked = source.path().join(".agents/skills/pm-workflow/SKILL.md");
        fs::remove_file(&linked).unwrap();
        symlink(source.path().join("AGENTS.md"), linked).unwrap();
        assert!(
            assemble(
                source.path(),
                b"binary",
                &"b".repeat(40),
                &files,
                target().unwrap(),
                out.path(),
                || Ok(()),
            )
            .is_err()
        );
    }

    #[test]
    fn installer_supports_spaces_reinstall_upgrade_and_explicit_binary_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("prefix with spaces");
        let first = bundle(dir.path(), 'a', b"old binary", target().unwrap());
        let second = bundle(dir.path(), 'b', b"new binary", target().unwrap());
        succeeded(install(&first, &prefix));
        let active = prefix.join("bin/pm-agent");
        let old_link = fs::read_link(&active).unwrap();
        succeeded(install(&first, &prefix));
        assert_eq!(fs::read_link(&active).unwrap(), old_link);
        succeeded(install(&second, &prefix));
        assert_eq!(fs::read(&active).unwrap(), b"new binary");
        let old_release = prefix
            .join("bin")
            .join(old_link)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_owned();
        assert_eq!(
            fs::read(old_release.join("bin/pm-agent")).unwrap(),
            b"old binary"
        );
        succeeded(install(&old_release, &prefix));
        assert_eq!(fs::read(&active).unwrap(), b"old binary");
        assert!(!prefix.join(".pm-agent").exists());
        assert!(!prefix.join("lib/pm-agent/.install-lock").exists());
    }

    #[test]
    fn installer_rejects_corruption_and_wrong_architecture_before_creating_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("install");
        let bad = bundle(dir.path(), 'a', b"binary", target().unwrap());
        fs::write(bad.join("bin/pm-agent"), b"tampered").unwrap();
        assert!(!install(&bad, &prefix).status.success());
        assert!(!prefix.exists());
        fs::write(bad.join("bin/pm-agent"), b"binary").unwrap();
        let sums = fs::read_to_string(bad.join("SHA256SUMS")).unwrap();
        fs::write(
            bad.join("SHA256SUMS"),
            sums.lines()
                .filter(|line| !line.ends_with("  bin/pm-agent"))
                .map(|line| format!("{line}\n"))
                .collect::<String>(),
        )
        .unwrap();
        assert!(!install(&bad, &prefix).status.success());
        assert!(!prefix.exists());
        let other = if target().unwrap() == "aarch64-apple-darwin" {
            "x86_64-unknown-linux-gnu"
        } else {
            "aarch64-apple-darwin"
        };
        let wrong = bundle(dir.path(), 'b', b"binary", other);
        assert!(!install(&wrong, &prefix).status.success());
        assert!(!prefix.exists());
    }

    #[test]
    fn installer_preserves_unmanaged_commands_and_rejects_linked_paths_and_busy_lock() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("install");
        let package = bundle(dir.path(), 'a', b"binary", target().unwrap());
        put(&prefix, "bin/pm-agent", b"unmanaged");
        assert!(!install(&package, &prefix).status.success());
        assert_eq!(fs::read(prefix.join("bin/pm-agent")).unwrap(), b"unmanaged");
        fs::remove_file(prefix.join("bin/pm-agent")).unwrap();
        symlink(dir.path(), prefix.join("bin/pm-agent")).unwrap();
        assert!(!install(&package, &prefix).status.success());
        fs::remove_file(prefix.join("bin/pm-agent")).unwrap();
        fs::create_dir(prefix.join("lib/pm-agent/.install-lock")).unwrap();
        assert!(!install(&package, &prefix).status.success());
        assert!(!prefix.join("bin/pm-agent").exists());
        let link = dir.path().join("linked-prefix");
        symlink(&prefix, &link).unwrap();
        assert!(!install(&package, &link).status.success());
    }

    #[test]
    fn installer_never_replaces_a_release_id_with_different_or_modified_content() {
        let dir = tempfile::tempdir().unwrap();
        let another = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("install");
        let first = bundle(dir.path(), 'a', b"original", target().unwrap());
        succeeded(install(&first, &prefix));
        let conflict = bundle(another.path(), 'a', b"different", target().unwrap());
        assert!(!install(&conflict, &prefix).status.success());
        assert_eq!(fs::read(prefix.join("bin/pm-agent")).unwrap(), b"original");
        fs::write(prefix.join("bin/pm-agent"), b"modified installation").unwrap();
        assert!(!install(&first, &prefix).status.success());
        assert_eq!(
            fs::read(prefix.join("bin/pm-agent")).unwrap(),
            b"modified installation"
        );
    }
}
