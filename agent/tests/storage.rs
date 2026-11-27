use pm_agent::{
    backup, storage,
    store::Store,
    workspace::{Workspace, WorkspaceLock},
};
use rusqlite::Connection;
use serde_json::json;
use std::{fs, path::Path, process::Command};

// Independent copy of the SQL used before schema versioning was introduced.
const LEGACY: &str = "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, updated TEXT NOT NULL, payload TEXT NOT NULL); CREATE TABLE IF NOT EXISTS ledger (id INTEGER PRIMARY KEY, session TEXT NOT NULL, call_id TEXT NOT NULL, name TEXT NOT NULL, arguments TEXT NOT NULL, status TEXT NOT NULL, result TEXT, UNIQUE(session,call_id)); CREATE TABLE IF NOT EXISTS documents(path TEXT PRIMARY KEY,title TEXT,kind TEXT,body TEXT,hash TEXT);";
const PAYLOAD: &str = r#"{"id":"session","messages":[{"content":"來源原文"}],"usage":null,"pending":{"call_id":"pending"}}"#;

fn legacy(path: &Path) -> Connection {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(LEGACY).unwrap();
    conn.execute(
        "INSERT INTO sessions VALUES ('session','historical time',?1)",
        [PAYLOAD],
    )
    .unwrap();
    conn.execute_batch("INSERT INTO ledger VALUES (7,'session','pending','external','{}','started',NULL);
        INSERT INTO ledger VALUES (8,'session','done','external','{}','done','historical result');
        INSERT INTO documents VALUES ('REQ.md','原文','requirement','preserved content','old hash');").unwrap();
    conn
}

fn version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn preserved(path: &Path) {
    let conn = Connection::open(path).unwrap();
    let row: (String, String) = conn
        .query_row("SELECT updated,payload FROM sessions", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(row, ("historical time".into(), PAYLOAD.into()));
    let rows: Vec<(i64, String, Option<String>)> = conn
        .prepare("SELECT id,status,result FROM ledger ORDER BY id")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (7, "started".into(), None),
            (8, "done".into(), Some("historical result".into()))
        ]
    );
    let doc: (String, String) = conn
        .query_row("SELECT body,hash FROM documents", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(doc, ("preserved content".into(), "old hash".into()));
}

#[test]
fn new_store_is_v1_and_inspection_does_not_initialize() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("private/runtime.sqlite3");
    assert_eq!(
        storage::inspect(&path).unwrap().state,
        storage::State::Uninitialized
    );
    assert!(!path.parent().unwrap().exists());
    assert!(storage::migrate(&path).is_err());
    Store::open(&path)
        .unwrap()
        .save("new", &json!({"text":"hello"}))
        .unwrap();
    let report = storage::inspect(&path).unwrap();
    assert_eq!(report.state, storage::State::Current);
    assert_eq!(report.schema_version, 1);
    assert_eq!(report.application_id, storage::APPLICATION_ID);
    assert!(!report.migration_required);
    assert!(storage::migrate(&path).unwrap().safety_snapshot.is_none());
    assert!(!path.parent().unwrap().join("migrations").exists());
}

#[test]
fn legacy_wal_migration_preserves_payload_ledger_index_and_private_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite3");
    let writer = legacy(&path); // Remains open so committed rows are still in WAL.
    assert!(path.with_extension("sqlite3-wal").metadata().unwrap().len() > 0);
    assert_eq!(
        storage::inspect(&path).unwrap().state,
        storage::State::Legacy
    );
    assert_eq!(version(&path), 0);
    let snapshot = storage::migrate(&path).unwrap().safety_snapshot.unwrap();
    assert_eq!(version(&path), 1);
    assert_eq!(version(&snapshot), 0);
    preserved(&path);
    preserved(&snapshot);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.load::<serde_json::Value>("session").unwrap(),
        serde_json::from_str::<serde_json::Value>(PAYLOAD).unwrap()
    );
    assert_eq!(
        store.call_result("session", "pending").unwrap(),
        Some(("started".into(), None))
    );
    assert!(
        store
            .begin_call("session", "pending", "external", "{}")
            .is_err()
    );
    assert!(storage::migrate(&path).unwrap().safety_snapshot.is_none());
    assert_eq!(fs::read_dir(snapshot.parent().unwrap()).unwrap().count(), 1);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            snapshot.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            snapshot
                .parent()
                .unwrap()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    drop(writer);
}

#[test]
fn failed_snapshot_leaves_legacy_data_and_version_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite3");
    drop(legacy(&path));
    let before = fs::read(&path).unwrap();
    fs::write(dir.path().join("migrations"), b"occupied").unwrap();
    let error = storage::migrate(&path).unwrap_err().to_string();
    assert!(error.contains("safety snapshot failed"), "{error}");
    assert_eq!(version(&path), 0);
    assert_eq!(fs::read(&path).unwrap(), before);
    preserved(&path);
}

#[test]
fn unsupported_versions_and_schema_changes_fail_without_mutation() {
    for sql in [
        "PRAGMA user_version=2",
        "PRAGMA application_id=42",
        "PRAGMA user_version=1", // A matching version without our application ID is not ours.
        "PRAGMA user_version=1; PRAGMA application_id=1347240263; CREATE INDEX extra ON sessions(updated)",
        "CREATE TABLE unknown (value TEXT)",
        "DROP TABLE documents",
        "DROP TABLE sessions; CREATE TABLE sessions (id TEXT PRIMARY KEY, updated TEXTNOTNULL, payload TEXT NOT NULL)",
        "CREATE INDEX extra ON sessions(updated)",
        "CREATE TRIGGER unexpected AFTER INSERT ON sessions BEGIN DELETE FROM ledger; END",
        "ALTER TABLE ledger ADD COLUMN hidden TEXT GENERATED ALWAYS AS (name) VIRTUAL",
        "DROP TABLE ledger; CREATE TABLE ledger (id INTEGER PRIMARY KEY, session TEXT NOT NULL, call_id TEXT NOT NULL, name TEXT NOT NULL, arguments TEXT NOT NULL, status TEXT NOT NULL, result TEXT)",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime.sqlite3");
        let conn = legacy(&path);
        conn.execute_batch(sql).unwrap();
        drop(conn);
        let before = fs::read(&path).unwrap();
        assert!(storage::inspect(&path).is_err(), "{sql}");
        assert!(storage::migrate(&path).is_err(), "{sql}");
        assert!(Store::open(&path).is_err(), "{sql}");
        assert_eq!(fs::read(&path).unwrap(), before, "{sql}");
        assert!(!dir.path().join("migrations").exists());
    }
}

#[test]
fn simultaneous_openers_create_only_one_migration_snapshot() {
    // Bound the actual multi-connection close/open regression in a child process so
    // a bundled SQLite lock-order regression fails CI instead of hanging forever.
    if std::env::var_os("PM_STORAGE_STRESS_CHILD").is_none() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "simultaneous_openers_create_only_one_migration_snapshot",
                "--nocapture",
            ])
            .env("PM_STORAGE_STRESS_CHILD", "1")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "Concurrent database open/close failed");
                return;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("Concurrent database open/close timed out; possible SQLite mutex deadlock");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite3");
    drop(legacy(&path));
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                for _ in 0..32 {
                    barrier.wait();
                    Store::open(&path).unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(
        fs::read_dir(dir.path().join("migrations")).unwrap().count(),
        1
    );
    preserved(&path);
}

#[test]
fn failed_commit_rolls_back_both_markers_and_keeps_a_recoverable_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("runtime.sqlite3");
    let reader = legacy(&path);
    reader
        .execute_batch("PRAGMA journal_mode=DELETE; BEGIN; SELECT * FROM sessions;")
        .unwrap();
    // A rollback-journal reader permits the reserved migration lock and snapshot, but
    // prevents the final exclusive commit. Exercise SQLite's real rollback path.
    let error = storage::migrate(&path).unwrap_err().to_string();
    assert!(
        error.contains("Runtime schema transaction failed"),
        "{error}"
    );
    reader.execute_batch("ROLLBACK").unwrap();
    drop(reader);
    assert_eq!(
        storage::inspect(&path).unwrap().state,
        storage::State::Legacy
    );
    preserved(&path);
    let snapshots: Vec<_> = fs::read_dir(dir.path().join("migrations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(
        storage::inspect(&snapshots[0]).unwrap().state,
        storage::State::Legacy
    );
    preserved(&snapshots[0]);
}

#[test]
fn cli_storage_needs_no_config_and_migration_obeys_workspace_lock() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("00_Governance")).unwrap();
    fs::write(dir.path().join("00_Governance/Project.md"), "# Fixture").unwrap();
    let run = |action| {
        Command::new(env!("CARGO_BIN_EXE_pm-agent"))
            .arg("--vault")
            .arg(dir.path())
            .arg("--config")
            .arg("missing-config.toml")
            .args(["storage", action])
            .output()
            .unwrap()
    };
    let missing = run("inspect");
    assert!(missing.status.success());
    assert!(!dir.path().join(".pm-agent").exists());
    let path = dir.path().join(".pm-agent/runtime.sqlite3");
    drop(legacy(&path));
    let workspace = Workspace::open(dir.path()).unwrap();
    let lock = WorkspaceLock::acquire(&workspace).unwrap();
    assert!(run("inspect").status.success());
    assert!(!run("migrate").status.success());
    assert_eq!(version(&path), 0);
    drop(lock);
    let migrated = run("migrate");
    assert!(
        migrated.status.success(),
        "{}",
        String::from_utf8_lossy(&migrated.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&migrated.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert!(Path::new(value["safety_snapshot"].as_str().unwrap()).is_file());
    assert!(!dir.path().join(".pm-agent/config.toml").exists());
}

#[test]
fn legacy_backup_restores_before_migration_and_future_runtime_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("00_Governance")).unwrap();
    fs::write(dir.path().join("00_Governance/Project.md"), "# Fixture").unwrap();
    fs::write(
        dir.path().join("00_Governance/Settings.md"),
        "# Fixture settings",
    )
    .unwrap();
    let path = dir.path().join(".pm-agent/runtime.sqlite3");
    drop(legacy(&path));
    let workspace = Workspace::open(dir.path()).unwrap();
    let out = tempfile::tempdir().unwrap();
    let archive = out.path().join("legacy.pmbak");
    backup::create(&workspace, &archive, true).unwrap();
    assert_eq!(version(&path), 0);
    let restored = backup::restore(&archive, out.path(), true).unwrap();
    let restored_db = restored.path.join(".pm-agent/runtime.sqlite3");
    assert_eq!(version(&restored_db), 0);
    Store::open(&restored_db).unwrap();
    assert_eq!(version(&restored_db), 1);
    preserved(&restored_db);
    Connection::open(&path)
        .unwrap()
        .execute_batch("PRAGMA user_version=99")
        .unwrap();
    let destination = out.path().join("unsupported.pmbak");
    assert!(backup::create(&workspace, &destination, true).is_err());
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn migration_rejects_linked_database_sidecars_and_snapshot_directory() {
    use std::os::unix::fs::symlink;
    for suffix in ["", "-wal", "-shm", "-journal", "migrations"] {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime.sqlite3");
        drop(legacy(&path));
        let target = outside.path().join("untouched");
        fs::write(&target, b"preserve").unwrap();
        let link = if suffix == "migrations" {
            dir.path().join(suffix)
        } else {
            dir.path().join(format!("runtime.sqlite3{suffix}"))
        };
        if suffix.is_empty() {
            fs::remove_file(&path).unwrap();
        }
        symlink(&target, link).unwrap();
        assert!(storage::migrate(&path).is_err(), "{suffix}");
        assert_eq!(fs::read(&target).unwrap(), b"preserve");
    }
}
