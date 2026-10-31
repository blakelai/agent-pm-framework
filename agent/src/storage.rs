//! Runtime schema ownership and the one supported migration: the original unversioned store.
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, backup::StepResult};
use serde::Serialize;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use tokio_util::sync::CancellationToken;

pub const VERSION: i64 = 1;
pub const APPLICATION_ID: i64 = 0x504d4147; // PMAG
const SCHEMA: &str = "
CREATE TABLE sessions (id TEXT PRIMARY KEY, updated TEXT NOT NULL, payload TEXT NOT NULL);
CREATE TABLE ledger (id INTEGER PRIMARY KEY, session TEXT NOT NULL, call_id TEXT NOT NULL, name TEXT NOT NULL, arguments TEXT NOT NULL, status TEXT NOT NULL, result TEXT, UNIQUE(session,call_id));
CREATE TABLE documents(path TEXT PRIMARY KEY,title TEXT,kind TEXT,body TEXT,hash TEXT);";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Uninitialized,
    Legacy,
    Current,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub path: PathBuf,
    pub state: State,
    pub schema_version: i64,
    pub application_id: i64,
    pub supported_version: i64,
    pub migration_required: bool,
    /// Present only for the migration performed by this invocation.
    pub safety_snapshot: Option<PathBuf>,
}

fn report(path: &Path, state: State) -> Report {
    Report {
        path: path.into(),
        state,
        schema_version: if state == State::Current { VERSION } else { 0 },
        application_id: if state == State::Current {
            APPLICATION_ID
        } else {
            0
        },
        supported_version: VERSION,
        migration_required: state == State::Legacy,
        safety_snapshot: None,
    }
}

// Comparing the complete stored DDL also rejects hidden/generated columns, extra constraints,
// partial indexes, triggers and views. Only whitespace/case differences are tolerated.
type SchemaObject = (String, String, String, Option<Vec<String>>);

fn sql_tokens(sql: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    for c in sql.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c.to_ascii_lowercase());
        } else {
            if !word.is_empty() {
                tokens.push(std::mem::take(&mut word));
            }
            if !c.is_ascii_whitespace() {
                tokens.push(c.to_string());
            }
        }
    }
    if !word.is_empty() {
        tokens.push(word);
    }
    tokens
}

fn schema(conn: &Connection) -> Result<Vec<SchemaObject>> {
    Ok(conn
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")?
        .query_map([], |r| {
            let sql: Option<String> = r.get(3)?;
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, sql.map(|s| sql_tokens(&s))))
        })?
        .collect::<rusqlite::Result<_>>()?)
}

fn classify(conn: &Connection) -> Result<State> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let app: i64 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    ensure!(
        (version == 0 && app == 0) || (version == VERSION && app == APPLICATION_ID),
        "Unsupported runtime schema version {version} / application ID {app}; supported: original unversioned schema or PM Agent v{VERSION}. Use a compatible PM Agent or restore a supported backup into a new Vault"
    );
    let actual = schema(conn)?;
    if actual.is_empty() && version == 0 {
        return Ok(State::Uninitialized);
    }
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(SCHEMA)?;
    ensure!(
        actual == schema(&reference)?,
        "Unsupported runtime database schema; only the original three-table layout and version 1 are supported. Preserve this database and use a compatible PM Agent or restore a supported backup into a new Vault"
    );
    let integrity: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(integrity == "ok", "Runtime SQLite integrity check failed");
    Ok(if version == 0 {
        State::Legacy
    } else {
        State::Current
    })
}

fn safe_paths(path: &Path) -> Result<()> {
    let parent = path.parent().context("Missing store directory")?;
    if let Ok(meta) = fs::symlink_metadata(parent) {
        ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "Runtime directory must be a regular directory"
        );
    }
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        match fs::symlink_metadata(Path::new(&name)) {
            Ok(meta) => ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "Runtime database and sidecars must be regular files"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Inspect without initializing, migrating, configuring providers, or changing project content.
pub fn inspect(path: &Path) -> Result<Report> {
    safe_paths(path)?;
    if !path.exists() {
        return Ok(report(path, State::Uninitialized));
    }
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let tx = conn.transaction()?;
    Ok(report(path, classify(&tx)?))
}

pub(crate) fn check_database(path: &Path) -> Result<()> {
    ensure!(
        inspect(path)?.state != State::Uninitialized,
        "Runtime database has not been initialized"
    );
    Ok(())
}

#[cfg(unix)]
fn private(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        path,
        fs::Permissions::from_mode(if directory { 0o700 } else { 0o600 }),
    )?;
    Ok(())
}
#[cfg(not(unix))]
fn private(_: &Path, _: bool) -> Result<()> {
    Ok(())
}

/// Copy committed WAL content. A separate read connection permits use while the caller owns
/// an IMMEDIATE migration transaction, which excludes other writers until the stamp commits.
pub(crate) fn snapshot_database(
    source: &Path,
    target: &Path,
    limit: Option<u64>,
    cancel: &CancellationToken,
) -> Result<()> {
    if let Some(limit) = limit {
        ensure!(
            fs::symlink_metadata(source)?.len() <= limit,
            "Runtime snapshot exceeds the 50 MiB file limit"
        );
    }
    check_database(source)?;
    let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let page_size: u32 = source.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let mut destination = Connection::open(target)?;
    private(target, false)?;
    {
        let backup = rusqlite::backup::Backup::new(&source, &mut destination)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            ensure!(!cancel.is_cancelled(), "Recovery cancelled");
            ensure!(
                Instant::now() < deadline,
                "Runtime backup timed out; close active database writers and retry"
            );
            let step = backup.step(128)?;
            if let Some(limit) = limit {
                ensure!(
                    backup.progress().pagecount as u64 * u64::from(page_size) <= limit
                        && fs::metadata(target)?.len() <= limit,
                    "Runtime snapshot exceeds the 50 MiB file limit"
                );
            }
            match step {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy | StepResult::Locked => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                _ => bail!("Unsupported SQLite backup result"),
            }
        }
    }
    destination.execute_batch("PRAGMA journal_mode=DELETE;")?;
    drop(destination);
    check_database(target)?;
    File::open(target)?.sync_all()?;
    Ok(())
}

fn safety_snapshot(path: &Path) -> Result<PathBuf> {
    let dir = path
        .parent()
        .context("Missing store directory")?
        .join("migrations");
    if let Ok(meta) = fs::symlink_metadata(&dir) {
        ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "Migration snapshot directory must be a regular directory"
        );
    }
    fs::create_dir_all(&dir)?;
    private(&dir, true)?;
    File::open(path.parent().context("Missing store directory")?)?.sync_all()?;
    let temp = NamedTempFile::new_in(&dir)?;
    snapshot_database(path, temp.path(), None, &CancellationToken::new())?;
    let target = dir.join(format!("v0-to-v1-{}.sqlite3", uuid::Uuid::new_v4()));
    temp.persist_noclobber(&target)?;
    File::open(&dir)?.sync_all()?;
    Ok(target)
}

/// Operator migration of an existing store, without loading or interpreting saved sessions.
pub fn migrate(path: &Path) -> Result<Report> {
    ensure!(
        path.is_file(),
        "No runtime database; open sessions or the workbench to initialize one"
    );
    Ok(open(path)?.1)
}

pub(crate) fn open(path: &Path) -> Result<(Connection, Report)> {
    inspect(path)?; // Fail closed before opening an unsupported database for writing.
    let parent = path.parent().context("Missing store directory")?;
    fs::create_dir_all(parent)?;
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let state = classify(&tx)?; // Recheck after acquiring the database writer lock.
    let snapshot = if state == State::Legacy {
        Some(
            safety_snapshot(path)
                .context("Migration stopped before changing the schema: safety snapshot failed")?,
        )
    } else {
        None
    };
    if state == State::Uninitialized {
        tx.execute_batch(SCHEMA)?;
    }
    if state != State::Current {
        tx.pragma_update(None, "application_id", APPLICATION_ID)?;
        tx.pragma_update(None, "user_version", VERSION)?;
    }
    tx.commit().with_context(|| {
        format!("Runtime schema transaction failed; safety snapshot: {snapshot:?}")
    })?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    private(path, false)?;
    let mut report = report(path, State::Current);
    report.safety_snapshot = snapshot;
    Ok((conn, report))
}
