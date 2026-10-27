//! Operator-only recovery. Private runtime configuration is outside the archived scope.
use crate::workspace::{Workspace, WorkspaceLock, hash};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags, backup::StepResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions as FileOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;
use walkdir::WalkDir;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const FORMAT: &str = "pm-vault-backup-v1";
const SCOPE: &str = "project-knowledge-v1";
const MANIFEST: &str = "manifest.json";
const RUNTIME: &str = ".pm-agent/runtime.sqlite3";
const MAX_FILES: usize = 20_000;
const MAX_FILE: u64 = 50 * 1024 * 1024;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
const MAX_MANIFEST: u64 = 8 * 1024 * 1024;
const ROOTS: &[&str] = &[
    "00_Governance",
    "01_Sources",
    "02_Domain",
    "03_Systems",
    "04_Requirements",
    "05_Backlog",
    "06_Delivery",
    "07_Decisions",
    "08_Verification",
    "09_Operations",
    "80_Templates",
    "90_Agent",
    ".agents",
];
const REQUIRED: &[&str] = &["00_Governance/Project.md", "00_Governance/Settings.md"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub scope: String,
    pub app_version: String,
    pub created_at: String,
    pub includes_sessions: bool,
    pub files: Vec<Entry>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub path: PathBuf,
    pub files: usize,
    pub bytes: u64,
    pub includes_sessions: bool,
    pub format: String,
    pub manifest_sha256: String,
}
fn cancelled(cancel: &CancellationToken) -> Result<()> {
    ensure!(
        !cancel.is_cancelled(),
        "Recovery operation cancelled before publication"
    );
    Ok(())
}

fn private(path: &Path, directory: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
        )?;
    }
    Ok(())
}
fn safe_path(path: &str) -> Result<()> {
    ensure!(
        !path.is_empty() && path.len() <= 4096,
        "Invalid backup path length"
    );
    ensure!(
        !path.contains(['\\', ':'])
            && !path.chars().any(char::is_control)
            && path
                .split('/')
                .all(|c| !c.is_empty() && c != "." && c != ".."),
        "Unsafe backup path: {path:?}"
    );
    Ok(())
}
fn included(path: &str) -> bool {
    let components: Vec<_> = path.split('/').collect();
    ROOTS.contains(&components[0])
        || (components.len() == 1 && (path.ends_with(".md") || path.ends_with(".canvas")))
        || (components.len() == 2 && components[0] == "agent" && path.ends_with(".md"))
        || path == "tools/README.md"
        || path.starts_with("tools/locales/")
}
fn inventory(workspace: &Workspace) -> Result<Vec<String>> {
    let mut paths = vec![];
    for entry in WalkDir::new(&workspace.root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if e.depth() == 0 {
                return true;
            }
            let Ok(path) = e.path().strip_prefix(&workspace.root) else {
                return false;
            };
            let Some(path) = path.to_str() else {
                return true;
            };
            included(path) || matches!(path, "agent" | "tools" | "tools/locales")
        })
    {
        let entry = entry?;
        if entry.depth() == 0 {
            continue;
        }
        let path = entry
            .path()
            .strip_prefix(&workspace.root)?
            .to_str()
            .context("Backup requires UTF-8 paths")?;
        safe_path(path)?;
        ensure!(
            !entry.file_type().is_symlink(),
            "Backup cannot include symlinks: {path}"
        );
        if entry.file_type().is_dir() {
            continue;
        }
        ensure!(
            entry.file_type().is_file(),
            "Backup requires regular files: {path}"
        );
        if included(path) {
            paths.push(path.to_string());
        }
        ensure!(
            paths.len() <= MAX_FILES,
            "Backup exceeds the file-count limit"
        );
    }
    paths.sort();
    ensure!(
        REQUIRED.iter().all(|p| paths.iter().any(|v| v == p)),
        "Backup requires Project.md and Settings.md"
    );
    Ok(paths)
}
fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "Expected a regular file: {}",
        path.display()
    );
    ensure!(
        meta.len() <= limit,
        "Backup file exceeds its size limit: {}",
        path.display()
    );
    let mut bytes = vec![];
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Backup file grew beyond its size limit"
    );
    Ok(bytes)
}
fn report(path: PathBuf, manifest: &Manifest, sessions: bool) -> Report {
    Report {
        path,
        files: manifest
            .files
            .iter()
            .filter(|e| sessions || e.path != RUNTIME)
            .count(),
        bytes: manifest
            .files
            .iter()
            .filter(|e| sessions || e.path != RUNTIME)
            .map(|e| e.bytes)
            .sum(),
        includes_sessions: sessions,
        format: manifest.format.clone(),
        manifest_sha256: hash(
            &serde_json::to_vec(manifest).expect("Manifest contains serializable scalar fields"),
        ),
    }
}
fn check_database(path: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let result: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(result == "ok", "Runtime SQLite integrity check failed");
    // Pin the current recovery contract without executing stored SQL or model/tool work.
    conn.prepare("SELECT id, updated, payload FROM sessions LIMIT 0")?;
    conn.prepare("SELECT session, call_id, name, arguments, status, result FROM ledger LIMIT 0")?;
    conn.prepare("SELECT path, title, kind, body, hash FROM documents LIMIT 0")?;
    let foreign: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type IN ('trigger','view') OR (type='table' AND name NOT IN ('sessions','ledger','documents'))",
        [], |r| r.get(0)
    )?;
    ensure!(
        foreign == 0,
        "Unsupported runtime database objects; no automatic migration is available"
    );
    Ok(())
}
fn snapshot_database(source: &Path, target: &Path, cancel: &CancellationToken) -> Result<()> {
    let meta = fs::symlink_metadata(source)?;
    ensure!(
        meta.is_file() && meta.len() <= MAX_FILE,
        "Missing or oversized regular runtime database"
    );
    check_database(source)?;
    let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let page_size: u32 = source.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let mut destination = Connection::open(target)?;
    {
        let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cancelled(cancel)?;
            ensure!(
                Instant::now() < deadline,
                "Runtime backup timed out; close active database writers and retry"
            );
            ensure!(
                backup.progress().pagecount as u64 * u64::from(page_size) <= MAX_FILE,
                "Runtime snapshot exceeds the 50 MiB file limit"
            );
            match backup.step(128)? {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                _ => bail!("Unsupported SQLite backup result"),
            }
            ensure!(
                fs::metadata(target)?.len() <= MAX_FILE,
                "Runtime snapshot exceeds the 50 MiB file limit"
            );
        }
    }
    destination.execute_batch("PRAGMA journal_mode=DELETE;")?;
    drop(destination);
    private(target, false)?;
    check_database(target)
}

/// Create a new archive outside the Vault. Close TUI/Chat first; other PM Agent writers are locked out.
pub fn create(workspace: &Workspace, output: &Path, include_sessions: bool) -> Result<Report> {
    let lock = WorkspaceLock::acquire(workspace)?;
    create_locked(
        workspace,
        output,
        include_sessions,
        &lock,
        &CancellationToken::new(),
    )
}
pub(crate) fn create_locked(
    workspace: &Workspace,
    output: &Path,
    include_sessions: bool,
    lock: &WorkspaceLock,
    cancel: &CancellationToken,
) -> Result<Report> {
    lock.verify(workspace)?;
    cancelled(cancel)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    ensure!(
        !parent.starts_with(&workspace.root),
        "Store backups outside the source Vault"
    );
    let output = parent.join(
        output
            .file_name()
            .context("Backup output needs a filename")?,
    );
    ensure!(
        !output.try_exists()? && fs::symlink_metadata(&output).is_err(),
        "Backup output already exists"
    );
    let paths = inventory(workspace)?;
    let temporary = NamedTempFile::new_in(&parent)?;
    private(temporary.path(), false)?;
    let mut zip = ZipWriter::new(temporary.reopen()?);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o600);
    let mut manifest = Manifest {
        format: FORMAT.into(),
        scope: SCOPE.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        includes_sessions: include_sessions,
        files: vec![],
    };
    let mut total = 0;
    let mut append = |path: &str, bytes: &[u8]| -> Result<()> {
        total += bytes.len() as u64;
        ensure!(
            bytes.len() as u64 <= MAX_FILE && total <= MAX_TOTAL,
            "Backup exceeds its byte limit"
        );
        ensure!(
            manifest.files.len() < MAX_FILES,
            "Backup exceeds the file-count limit"
        );
        zip.start_file(format!("vault/{path}"), options)?;
        zip.write_all(bytes)?;
        manifest.files.push(Entry {
            path: path.into(),
            bytes: bytes.len() as u64,
            sha256: hash(bytes),
        });
        Ok(())
    };
    for path in &paths {
        cancelled(cancel)?;
        append(path, &read_file(&workspace.path(path)?, MAX_FILE)?)?;
    }
    if include_sessions {
        for suffix in ["-wal", "-shm", "-journal"] {
            workspace.path(&format!("{RUNTIME}{suffix}"))?;
        }
        let snapshot = tempfile::tempdir_in(&parent)?;
        let db = snapshot.path().join("runtime.sqlite3");
        snapshot_database(&workspace.path(RUNTIME)?, &db, cancel)?;
        append(RUNTIME, &read_file(&db, MAX_FILE)?)?;
    }
    let data = serde_json::to_vec_pretty(&manifest)?;
    ensure!(
        data.len() as u64 <= MAX_MANIFEST,
        "Backup manifest exceeds its size limit"
    );
    zip.start_file(MANIFEST, options)?;
    zip.write_all(&data)?;
    zip.finish()?.sync_all()?;
    // External editors do not honor the process lock. Detect changed bytes and inventory before publishing.
    ensure!(
        paths == inventory(workspace)?,
        "Project files changed during backup; retry with editors paused"
    );
    for entry in manifest.files.iter().filter(|e| e.path != RUNTIME) {
        cancelled(cancel)?;
        ensure!(
            hash(&read_file(&workspace.path(&entry.path)?, MAX_FILE)?) == entry.sha256,
            "Project file changed during backup: {}",
            entry.path
        );
    }
    check_cancellable(temporary.path(), cancel)?;
    cancelled(cancel)?;
    temporary
        .persist_noclobber(&output)
        .context("Cannot publish backup without overwriting an existing file")?;
    Ok(report(output, &manifest, include_sessions))
}

fn archive(path: &Path) -> Result<ZipArchive<File>> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "Backup must be a regular archive file"
    );
    ensure!(
        meta.len() <= MAX_TOTAL + MAX_MANIFEST + 32 * 1024 * 1024,
        "Backup archive exceeds its size limit"
    );
    // Our bounded format needs neither ZIP64 nor multipart archives/comments. Inspect the
    // fixed trailer before the ZIP reader allocates its directory (which deduplicates names).
    let mut file = File::open(path)?;
    ensure!(meta.len() >= 22, "Truncated backup archive");
    file.seek(SeekFrom::End(-22))?;
    let mut trailer = [0u8; 22];
    file.read_exact(&mut trailer)?;
    ensure!(
        &trailer[..4] == b"PK\x05\x06" && trailer[4..8] == [0; 4] && trailer[20..22] == [0; 2],
        "Unsupported backup ZIP trailer"
    );
    let count = u16::from_le_bytes(trailer[10..12].try_into()?) as usize;
    ensure!(
        trailer[8..10] == trailer[10..12] && count <= MAX_FILES + 1,
        "Invalid backup entry count"
    );
    let size = u32::from_le_bytes(trailer[12..16].try_into()?) as u64;
    let start = u32::from_le_bytes(trailer[16..20].try_into()?) as u64;
    ensure!(
        size <= 16 * 1024 * 1024 && start + size == meta.len() - 22,
        "Invalid or oversized backup directory"
    );
    file.rewind()?;
    let zip = ZipArchive::new(file)?;
    ensure!(
        zip.len() == count && zip.offset() == 0,
        "Duplicate entries or unsupported archive prefix"
    );
    Ok(zip)
}
fn read_entry(zip: &mut ZipArchive<File>, name: &str, limit: u64) -> Result<Vec<u8>> {
    let file = zip.by_name(name)?;
    ensure!(
        file.is_file() && !file.encrypted(),
        "Unsupported backup entry: {name}"
    );
    if let Some(mode) = file.unix_mode() {
        ensure!(
            mode & 0o170000 == 0 || mode & 0o170000 == 0o100000,
            "Backup entry is not a regular file: {name}"
        );
    }
    ensure!(
        file.size() <= limit,
        "Backup entry exceeds its declared limit: {name}"
    );
    let mut bytes = vec![];
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Backup entry expands beyond its limit: {name}"
    );
    Ok(bytes)
}
fn manifest(zip: &mut ZipArchive<File>) -> Result<Manifest> {
    let mut names = BTreeSet::new();
    let mut folded = BTreeSet::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        let name = std::str::from_utf8(entry.name_raw()).context("Backup paths must be UTF-8")?;
        safe_path(name)?;
        ensure!(
            names.insert(name.to_string()) && folded.insert(name.to_lowercase()),
            "Duplicate or case-colliding backup path: {name}"
        );
    }
    let manifest: Manifest = serde_json::from_slice(&read_entry(zip, MANIFEST, MAX_MANIFEST)?)?;
    ensure!(
        manifest.format == FORMAT && manifest.scope == SCOPE,
        "Unsupported backup format or scope; no automatic migration is available"
    );
    ensure!(
        !manifest.app_version.is_empty() && manifest.app_version.len() < 100,
        "Missing or invalid backup application version"
    );
    chrono::DateTime::parse_from_rfc3339(&manifest.created_at)
        .context("Invalid backup creation time")?;
    let mut expected = BTreeSet::from([MANIFEST.to_string()]);
    let mut total = 0u64;
    for entry in &manifest.files {
        safe_path(&entry.path)?;
        ensure!(
            included(&entry.path) || (manifest.includes_sessions && entry.path == RUNTIME),
            "Path is outside the backup scope: {}",
            entry.path
        );
        ensure!(
            expected.insert(format!("vault/{}", entry.path)),
            "Duplicate manifest entry"
        );
        ensure!(
            entry.bytes <= MAX_FILE,
            "Backup file exceeds the 50 MiB limit"
        );
        total = total
            .checked_add(entry.bytes)
            .context("Backup size overflow")?;
        ensure!(total <= MAX_TOTAL, "Backup exceeds the 1 GiB total limit");
        ensure!(
            entry.sha256.len() == 64
                && entry
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid backup SHA-256"
        );
    }
    ensure!(
        names == expected,
        "Backup contains missing or unexpected entries"
    );
    ensure!(
        REQUIRED
            .iter()
            .all(|p| expected.contains(&format!("vault/{p}"))),
        "Backup is missing Project.md or Settings.md"
    );
    ensure!(
        manifest.includes_sessions == expected.contains(&format!("vault/{RUNTIME}")),
        "Runtime inclusion does not match the manifest"
    );
    Ok(manifest)
}
fn verify(
    zip: &mut ZipArchive<File>,
    manifest: &Manifest,
    destination: Option<(&Path, bool)>,
    cancel: &CancellationToken,
) -> Result<()> {
    for entry in &manifest.files {
        cancelled(cancel)?;
        let bytes = read_entry(zip, &format!("vault/{}", entry.path), entry.bytes)?;
        ensure!(
            bytes.len() as u64 == entry.bytes && hash(&bytes) == entry.sha256,
            "Backup integrity mismatch: {}",
            entry.path
        );
        if let Some((root, sessions)) = destination {
            if entry.path == RUNTIME && !sessions {
                continue;
            }
            let path = root.join(&entry.path);
            let parent = path.parent().context("Missing restore parent")?;
            fs::create_dir_all(parent)?;
            for ancestor in parent.ancestors().take_while(|p| p.starts_with(root)) {
                private(ancestor, true)?;
            }
            let mut file = FileOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            private(&path, false)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
    }
    Ok(())
}
/// Read-only archive integrity verification; no Vault, config, model, or database writer is opened.
pub fn check(path: &Path) -> Result<Report> {
    check_cancellable(path, &CancellationToken::new())
}
pub(crate) fn check_cancellable(path: &Path, cancel: &CancellationToken) -> Result<Report> {
    cancelled(cancel)?;
    let mut zip = archive(path)?;
    let manifest = manifest(&mut zip)?;
    verify(&mut zip, &manifest, None, cancel)?;
    cancelled(cancel)?;
    Ok(report(
        path.to_path_buf(),
        &manifest,
        manifest.includes_sessions,
    ))
}
/// Restore into a unique, private child of an existing parent. Failed staging is discarded.
pub fn restore(path: &Path, parent: &Path, include_sessions: bool) -> Result<Report> {
    restore_cancellable(
        path,
        parent,
        include_sessions,
        None,
        &CancellationToken::new(),
    )
}
pub(crate) fn restore_cancellable(
    path: &Path,
    parent: &Path,
    include_sessions: bool,
    expected_manifest: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Report> {
    cancelled(cancel)?;
    let parent = parent
        .canonicalize()
        .context("Restore parent must already exist")?;
    ensure!(parent.is_dir(), "Restore parent must be a directory");
    ensure!(
        !parent
            .ancestors()
            .any(|p| p.join("00_Governance/Project.md").is_file()),
        "Restore outside existing project Vaults"
    );
    let mut zip = archive(path)?;
    let manifest = manifest(&mut zip)?;
    if let Some(expected) = expected_manifest {
        ensure!(
            report(path.into(), &manifest, manifest.includes_sessions).manifest_sha256 == expected,
            "Backup changed after review; check it again before restoring"
        );
    }
    ensure!(
        !include_sessions || manifest.includes_sessions,
        "This backup does not contain sessions"
    );
    let staging = tempfile::Builder::new()
        .prefix("pm-restored-")
        .tempdir_in(&parent)?;
    private(staging.path(), true)?;
    verify(
        &mut zip,
        &manifest,
        Some((staging.path(), include_sessions)),
        cancel,
    )?;
    Workspace::open(staging.path())?;
    if include_sessions {
        check_database(&staging.path().join(RUNTIME))?;
    }
    cancelled(cancel)?;
    let path = staging.keep();
    Ok(report(path, &manifest, include_sessions))
}
