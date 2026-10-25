//! Single-flight, read-only polling of local knowledge and its review inputs.
use super::App;
use crate::{
    delivery, records, schedule,
    store::Store,
    workspace::{self, Document, Workspace},
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, time::Duration};
use walkdir::WalkDir;

pub const INTERVAL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Views {
    planning: bool,
    delivery: bool,
    inspecting: bool,
    questions: bool,
}
impl Views {
    pub fn of(app: &App) -> Self {
        Self {
            planning: app.planner.is_some(),
            delivery: app.delivery.is_some(),
            inspecting: app.inspector.is_some(),
            questions: app.questions.is_some(),
        }
    }
}

#[derive(Clone)]
pub struct Request {
    generation: u64,
    views: Views,
    previous: Option<String>,
}
pub struct Update {
    signature: String,
    docs: Vec<Document>,
    planning: Option<schedule::Snapshot>,
    delivery: Option<delivery::Snapshot>,
    questions: Option<crate::clarification::Snapshot>,
}

/// A manual refresh, save or view switch supersedes any outstanding read.
pub struct Monitor {
    generation: u64,
    running: bool,
    signature: Option<String>,
}
impl Monitor {
    pub fn new(w: &Workspace, docs: &[Document]) -> Self {
        Self {
            generation: 0,
            running: false,
            signature: fingerprint(w, docs).ok(),
        }
    }
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn request(&mut self, views: Views, retry: bool) -> Option<Request> {
        if self.running {
            return None;
        }
        self.running = true;
        Some(Request {
            generation: self.generation,
            views,
            previous: if retry { None } else { self.signature.clone() },
        })
    }
    pub fn finish(
        &mut self,
        request: &Request,
        result: Result<Option<Update>>,
        app: &mut App,
        store: &mut Store,
    ) -> bool {
        self.running = false;
        if request.generation != self.generation || request.views != Views::of(app) {
            return false;
        }
        match result {
            Ok(None) => false,
            Ok(Some(update)) => {
                // Do not acknowledge a failed index; the next poll must retry it.
                let signature = update.signature.clone();
                match apply(update, app, store) {
                    Ok(()) => {
                        self.signature = Some(signature);
                        true
                    }
                    Err(e) => {
                        self.signature = None;
                        unavailable(format!("{e:#}"), app, store)
                    }
                }
            }
            Err(e) => {
                self.signature = None;
                unavailable(format!("{e:#}"), app, store)
            }
        }
    }
}

fn fingerprint(w: &Workspace, docs: &[Document]) -> Result<String> {
    let mut hashes: BTreeMap<String, String> = docs
        .iter()
        .map(|d| (d.path.clone(), d.hash.clone()))
        .collect();
    for d in docs
        .iter()
        .filter(|d| d.kind == "question" && d.metadata.get("clarification_schema").is_some())
    {
        if let Some(path) = d.metadata["answer_path"].as_str() {
            let fingerprint = match w.read(path) {
                Ok(text) => workspace::hash(text.as_bytes()),
                Err(e) => format!("unavailable: {e}"),
            };
            hashes.insert(format!("answer:{path}"), fingerprint);
        }
    }
    // Baseline payloads affect acceptance even though they are not browse records.
    // Locale catalogs affect planning. Raw assets, repo checkouts, Git and runtime
    // state are deliberately outside this display refresh contract.
    for root in ["00_Governance/Baselines", "tools/locales"] {
        let directory = w.path(root)?;
        if !directory.exists() {
            continue;
        }
        for entry in WalkDir::new(directory).follow_links(false) {
            let entry = entry?;
            ensure!(
                !entry.file_type().is_symlink(),
                "Linked refresh input is not permitted: {}",
                entry.path().display()
            );
            if !entry.file_type().is_file()
                || !matches!(
                    entry.path().extension().and_then(|s| s.to_str()),
                    Some("md" | "json")
                )
            {
                continue;
            }
            let path = entry
                .path()
                .strip_prefix(&w.root)?
                .to_str()
                .context("Refresh input path must be UTF-8")?
                .to_string();
            let safe = w.path(&path)?;
            ensure!(
                entry.metadata()?.len() <= workspace::MAX_FILE,
                "Refresh input exceeds the 2 MiB limit: {path}"
            );
            hashes.insert(path, workspace::hash(&std::fs::read(safe)?));
        }
    }
    Ok(workspace::hash(serde_json::to_string(&hashes)?.as_bytes()))
}

pub fn scan(w: &Workspace, request: &Request) -> Result<Option<Update>> {
    let docs = w.documents()?;
    let signature = fingerprint(w, &docs)?;
    if request.previous.as_deref() == Some(&signature) {
        return Ok(None);
    }
    let planning = request.views.planning.then(|| schedule::Snapshot::load(w));
    let delivery = request.views.delivery.then(|| delivery::Snapshot::load(w));
    let questions = request
        .views
        .questions
        .then(|| crate::clarification::Snapshot::load(w));
    // A changing tree cannot be represented as one fresh display snapshot.
    // This is an optimistic check, not a filesystem transaction or an attestation.
    ensure!(
        fingerprint(w, &w.documents()?)? == signature,
        "Project files changed during refresh; retrying automatically"
    );
    Ok(Some(Update {
        signature,
        docs,
        planning,
        delivery,
        questions,
    }))
}

pub fn manual(w: &Workspace, app: &mut App, store: &mut Store) {
    let request = Request {
        generation: 0,
        views: Views::of(app),
        previous: None,
    };
    let result = scan(w, &request)
        .and_then(|update| apply(update.context("Missing refresh snapshot")?, app, store));
    if let Err(e) = result {
        unavailable(format!("{e:#}"), app, store);
    }
}

fn apply(update: Update, app: &mut App, store: &mut Store) -> Result<()> {
    store
        .reindex(&update.docs)
        .context("Document index refresh failed")?;
    app.replace_documents(update.docs);
    app.refresh_error = None;
    app.refresh_status = format!("Files checked at {}", records::timestamp());
    if let (Some(p), Some(snapshot)) = (&mut app.planner, update.planning) {
        p.replace(snapshot);
    }
    if let (Some(d), Some(snapshot)) = (&mut app.delivery, update.delivery) {
        d.replace(snapshot);
    }
    if let (Some(q), Some(snapshot)) = (&mut app.questions, update.questions) {
        q.replace(snapshot);
    }
    if let Some(form) = &mut app.form {
        form.inputs_changed();
    }
    if let Some(inspector) = &mut app.inspector {
        inspector.invalidate();
    }
    Ok(())
}

fn unavailable(problem: String, app: &mut App, store: &mut Store) -> bool {
    if app.refresh_error.as_deref() == Some(&problem) {
        return false;
    }
    app.replace_documents(vec![]);
    if let Some(inspector) = &mut app.inspector {
        inspector.invalidate();
    }
    let index_error = store.reindex(&[]).err();
    app.refresh_error = Some(match index_error {
        Some(e) => format!("{problem}; index unavailable: {e}"),
        None => problem.clone(),
    });
    if let Some(planner) = &mut app.planner {
        planner.replace(schedule::Snapshot {
            loaded_at: records::timestamp(),
            periods: vec![],
            items: vec![],
            people: vec![],
            forecast: None,
            problems: vec![problem.clone()],
        });
    }
    if let Some(q) = &mut app.questions {
        q.replace(crate::clarification::Snapshot {
            loaded_at: records::timestamp(),
            entries: vec![],
            problem: Some(problem.clone()),
        });
    }
    if let Some(delivery) = &mut app.delivery {
        delivery.replace(delivery::Snapshot {
            loaded_at: records::timestamp(),
            entries: vec![],
            problem: Some(problem),
        });
    }
    if let Some(form) = &mut app.form {
        form.inputs_changed();
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::{delivery::Delivery, planner::Planner};
    use serde_json::json;

    fn put(w: &Workspace, path: &str, body: &str) {
        workspace::atomic_write(
            &w.path(path).unwrap(),
            workspace::note(
                json!({"type":"Reference","title":path,"language":"en"}),
                body,
            )
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
    }
    fn fixture() -> (tempfile::TempDir, Workspace, App, Store, Monitor) {
        let dir = tempfile::tempdir().unwrap();
        workspace::atomic_write(&dir.path().join("00_Governance/Project.md"),
            workspace::note(json!({"id":"PROJECT-TEST","type":"project","title":"Synthetic project","language":"en"}), "# Fixture").unwrap().as_bytes()).unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        put(&w, "04_Requirements/TEST.md", "# Original");
        let app = App::new(w.documents().unwrap());
        let mut store = Store::open(&dir.path().join(".pm-agent/runtime.sqlite3")).unwrap();
        store.reindex(&app.docs).unwrap();
        let monitor = Monitor::new(&w, &app.docs);
        (dir, w, app, store, monitor)
    }
    fn poll(w: &Workspace, app: &mut App, store: &mut Store, monitor: &mut Monitor) -> bool {
        let request = monitor
            .request(Views::of(app), app.refresh_error.is_some())
            .unwrap();
        let result = scan(w, &request);
        monitor.finish(&request, result, app, store)
    }
    #[test]
    fn content_hashes_detect_same_size_time_atomic_replacement_rename_and_delete() {
        let (_dir, w, mut app, mut store, mut monitor) = fixture();
        assert!(!poll(&w, &mut app, &mut store, &mut monitor));
        let path = w.path("04_Requirements/TEST.md").unwrap();
        let before = std::fs::metadata(&path).unwrap();
        put(&w, "04_Requirements/TEST.md", "# Modified");
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(before.modified().unwrap()))
            .unwrap();
        assert_eq!(before.len(), std::fs::metadata(&path).unwrap().len());
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert_eq!(
            store.search("Modified").unwrap(),
            vec!["04_Requirements/TEST.md"]
        );
        std::fs::rename(&path, w.path("04_Requirements/MOVED.md").unwrap()).unwrap();
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert_eq!(
            store.search("Modified").unwrap(),
            vec!["04_Requirements/MOVED.md"]
        );
        std::fs::remove_file(w.path("04_Requirements/MOVED.md").unwrap()).unwrap();
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert!(store.search("Modified").unwrap().is_empty());
        put(&w, "04_Requirements/NEW.md", "# Created");
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert_eq!(
            store.search("Created").unwrap(),
            vec!["04_Requirements/NEW.md"]
        );
    }
    #[test]
    fn runtime_and_raw_assets_do_not_trigger_refresh_but_baselines_and_locales_do() {
        let (_dir, w, mut app, mut store, mut monitor) = fixture();
        for path in [
            ".git/HEAD",
            ".pm-agent/config.toml",
            "01_Sources/Assets/image.png",
            "03_Systems/Snapshots/source/main.rs",
        ] {
            workspace::atomic_write(&w.path(path).unwrap(), b"Synthetic ignored data").unwrap();
        }
        store
            .save("fixture", &json!({"input":"Unsent synthetic data"}))
            .unwrap();
        assert!(!poll(&w, &mut app, &mut store, &mut monitor));
        for path in [
            "00_Governance/Baselines/BASE-TEST/manifest.json",
            "00_Governance/Baselines/BASE-TEST/files/REQ-TEST.md",
            "tools/locales/en.json",
        ] {
            workspace::atomic_write(&w.path(path).unwrap(), b"Synthetic changed input").unwrap();
            assert!(poll(&w, &mut app, &mut store, &mut monitor));
            assert!(!app.docs.iter().any(|d| d.path == path));
        }
        assert!(!poll(&w, &mut app, &mut store, &mut monitor));
    }
    #[test]
    fn unreadable_documents_clear_display_and_index_then_recover_without_restarting() {
        let (_dir, w, mut app, mut store, mut monitor) = fixture();
        app.planner = Some(Planner::new(schedule::Snapshot::load(&w)));
        app.delivery = Some(Delivery::new(delivery::Snapshot::load(&w)));
        app.input.insert("Unsaved 中文 draft");
        let transcript = app.output.clone();
        workspace::atomic_write(
            &w.path("04_Requirements/TEST.md").unwrap(),
            b"---\ntype: [unfinished",
        )
        .unwrap();
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert!(app.docs.is_empty());
        assert!(
            app.refresh_error
                .as_ref()
                .unwrap()
                .contains("04_Requirements/TEST.md")
        );
        assert!(store.search("Original").unwrap().is_empty());
        assert!(!poll(&w, &mut app, &mut store, &mut monitor));
        assert_eq!(app.input.text, "Unsaved 中文 draft");
        assert_eq!(app.output, transcript);
        for (width, height) in [(1, 1), (20, 6), (100, 24)] {
            let mut t =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            t.draw(|f| app.draw(f)).unwrap();
        }
        put(&w, "04_Requirements/TEST.md", "# Original");
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert!(app.refresh_error.is_none());
        assert_eq!(
            store.search("Original").unwrap(),
            vec!["04_Requirements/TEST.md"]
        );
    }
    #[test]
    fn single_flight_rejects_superseded_reads_and_manual_failure_can_recover_same_bytes() {
        let (_dir, w, mut app, mut store, mut monitor) = fixture();
        let request = monitor.request(Views::of(&app), false).unwrap();
        assert!(monitor.request(Views::of(&app), false).is_none());
        put(&w, "04_Requirements/TEST.md", "# Old result");
        let old = scan(&w, &request);
        put(&w, "04_Requirements/TEST.md", "# Current result");
        manual(&w, &mut app, &mut store);
        monitor.invalidate();
        assert!(!monitor.finish(&request, old, &mut app, &mut store));
        assert!(app.docs.iter().any(|d| d.body.contains("Current result")));
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        workspace::atomic_write(&w.path("04_Requirements/TEST.md").unwrap(), b"unfinished")
            .unwrap();
        manual(&w, &mut app, &mut store);
        put(&w, "04_Requirements/TEST.md", "# Current result");
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert!(app.refresh_error.is_none());
    }
    #[test]
    fn auxiliary_symlinks_and_oversized_inputs_are_not_read() {
        let (_dir, w, mut app, mut store, mut monitor) = fixture();
        let large = w
            .path("00_Governance/Baselines/BASE-TEST/manifest.json")
            .unwrap();
        workspace::atomic_write(&large, b"").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&large)
            .unwrap()
            .set_len(workspace::MAX_FILE + 1)
            .unwrap();
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert!(app.refresh_error.as_ref().unwrap().contains("2 MiB"));
        std::fs::remove_file(large).unwrap();
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(w.root.join("tools")).unwrap();
            std::os::unix::fs::symlink(outside.path(), w.root.join("tools/locales")).unwrap();
            assert!(poll(&w, &mut app, &mut store, &mut monitor));
            assert!(app.refresh_error.as_ref().unwrap().contains("Symlinks"));
            std::fs::remove_file(w.root.join("tools/locales")).unwrap();
        }
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert!(app.refresh_error.is_none());
    }
    #[test]
    fn changed_inputs_recalculate_open_planning_and_delivery_without_writing_reports() {
        let (_dir, w, mut app, mut store, mut monitor) = fixture();
        for (path, metadata, body) in [
            (
                "00_Governance/Settings.md",
                json!({"type":"project_settings","title":"Settings","language":"en","document_language":"en","agent_language":"en"}),
                "# Settings",
            ),
            (
                "06_Delivery/Planning.md",
                json!({"type":"planning_config","title":"Planning","language":"en","as_of":"2026-11-02"}),
                "# Planning",
            ),
            (
                "07_Decisions/Changes/CHANGE-TEST.md",
                json!({"id":"CHANGE-TEST","type":"change","workflow_schema":"delivery-v1","title":"Synthetic change","language":"en","workflow_status":"draft","owner":null}),
                "# Synthetic change",
            ),
        ] {
            workspace::atomic_write(
                &w.path(path).unwrap(),
                workspace::note(metadata, body).unwrap().as_bytes(),
            )
            .unwrap();
        }
        put(
            &w,
            "06_Delivery/Iterations.md",
            "# Iterations\n\n| Sprint | Start | End | Goal |\n|---|---|---|---|\n| 1 | 2026-11-02 | 2026-11-13 | Synthetic goal |",
        );
        let people = "# People\n\n| Person | Role | Sprint | Project | FTE | Workdays | Absence | Focus | Reserve |\n|---|---|---|---|---|---|---|---|---|\n| Fixture | Backend | 1 | PROJECT-TEST | 1 | 10 | 0 | 1 | 0 |";
        put(&w, "06_Delivery/People.md", people);
        workspace::atomic_write(
            &w.path("tools/locales/en.json").unwrap(),
            include_bytes!("../../../tools/locales/en.json"),
        )
        .unwrap();
        app.planner = Some(Planner::new(schedule::Snapshot::load(&w)));
        app.delivery = Some(Delivery::new(delivery::Snapshot::load(&w)));
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        put(
            &w,
            "06_Delivery/People.md",
            &people.replace("| 1 | 10 |", "| 0.5 | 10 |"),
        );
        let request = monitor.request(Views::of(&app), false).unwrap();
        let update = scan(&w, &request).unwrap().unwrap();
        assert_eq!(
            update
                .planning
                .as_ref()
                .unwrap()
                .forecast
                .as_ref()
                .unwrap()
                .caps[0]
                .capacity,
            5.0
        );
        assert!(
            update.delivery.as_ref().unwrap().entries[0]
                .next_gate
                .contains("owner")
        );
        assert!(monitor.finish(&request, Ok(Some(update)), &mut app, &mut store));
        assert!(!w.root.join("06_Delivery/Plan-Report.md").exists());
        assert!(!w.root.join("90_Agent/Runs").exists());
    }
    #[test]
    fn file_change_invalidates_inspection_and_its_outstanding_worker() {
        let (_dir, w, mut app, mut store, mut monitor) = fixture();
        let request = crate::inspection::Request {
            id: "PROJECT-TEST".into(),
            target: crate::inspection::Target::Impact,
        };
        let mut inspector = super::super::inspection::Inspector::new(request.clone());
        inspector.start(request.clone(), 1);
        app.inspector = Some(inspector);
        put(&w, "04_Requirements/TEST.md", "# Changed input");
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        let inspector = app.inspector.as_mut().unwrap();
        assert!(inspector.ticket.is_none());
        assert!(!inspector.finish(1, crate::inspection::load(&w, &request)));
        assert!(inspector.message.contains("Results cleared"));
    }
    #[test]
    fn cited_answer_changes_refresh_questions() {
        let (_dir, w, mut app, mut store, mut monitor) = fixture();
        workspace::atomic_write(&w.path("07_Decisions/QUESTION-TEST.md").unwrap(),workspace::note(serde_json::json!({"type":"question","id":"QUESTION-TEST","title":"Synthetic question","clarification_schema":"clarification-v1","workflow_status":"open","blocking_scope":"project","affected_ids":[],"affected_paths":[],"answer_path":"01_Sources/Assets/USER-test.txt"}),"# Missing source").unwrap().as_bytes()).unwrap();
        workspace::atomic_write(
            &w.path("01_Sources/Assets/USER-test.txt").unwrap(),
            b"First actual fixture input",
        )
        .unwrap();
        app.questions = Some(super::super::questions::Questions::new(
            crate::clarification::Snapshot::load(&w),
        ));
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        workspace::atomic_write(
            &w.path("01_Sources/Assets/USER-test.txt").unwrap(),
            b"Changed fixture input",
        )
        .unwrap();
        assert!(poll(&w, &mut app, &mut store, &mut monitor));
        assert!(!poll(&w, &mut app, &mut store, &mut monitor));
    }
}
