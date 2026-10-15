use std::collections::BTreeSet;
use std::fs;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tandem_board_core::index::Changes;
use tandem_board_core::service::{Actor, Config, NewTask, Service, TaskFilter};
use tandem_board_core::store::TaskId;
use tandem_board_core::watcher::{Watcher, WatcherConfig};
use tempfile::TempDir;

/// Long enough for the platform to deliver events and the debouncer to
/// settle, so silence after it means nothing more is coming.
const SETTLE: Duration = Duration::from_millis(1500);

struct Env {
    _data: TempDir,
    _repo: TempDir,
    service: Arc<Service>,
    /// Watcher errors; every test checks there were none.
    errors: Arc<Mutex<Vec<String>>>,
}

impl Drop for Env {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(*self.errors.lock().unwrap(), Vec::<String>::new());
        }
    }
}

impl Env {
    fn new() -> Self {
        let data = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let service =
            Arc::new(Service::open(data.path(), "test", repo.path(), Config::default()).unwrap());
        Self {
            _data: data,
            _repo: repo,
            service,
            errors: Arc::default(),
        }
    }

    fn create(&self, title: &str) -> TaskId {
        self.service
            .create_task(
                &Actor::User,
                NewTask {
                    title: title.into(),
                    ..NewTask::default()
                },
            )
            .unwrap()
            .id
    }

    fn watch(&self) -> Watcher {
        let errors = Arc::clone(&self.errors);
        let watcher = Watcher::start(
            Arc::clone(&self.service),
            WatcherConfig::default(),
            move |e| errors.lock().unwrap().push(e.to_string()),
        )
        .unwrap();
        // Let the platform watch start, and any events from before it drain.
        std::thread::sleep(Duration::from_millis(300));
        watcher
    }

    fn path(&self, id: TaskId) -> std::path::PathBuf {
        self.service.task_dir().join(id.file_name())
    }

    fn title(&self, id: TaskId) -> String {
        self.service
            .list_tasks(&Actor::User, &TaskFilter::default())
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
            .title
    }
}

/// Every event that arrives until `SETTLE` passes without one.
fn drain(events: &Receiver<Changes>) -> Vec<Changes> {
    let mut out = Vec::new();
    while let Ok(changes) = events.recv_timeout(SETTLE) {
        out.push(changes);
    }
    out
}

fn changed(ids: &[TaskId]) -> Changes {
    Changes {
        changed: ids.iter().copied().collect(),
        removed: BTreeSet::new(),
    }
}

#[test]
fn an_external_edit_updates_the_index_once() {
    let env = Env::new();
    let id = env.create("Before");
    let _watcher = env.watch();
    let events = env.service.subscribe();

    // Another editor saves the file.
    let path = env.path(id);
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("Before", "After");
    fs::write(&path, text).unwrap();

    let first = events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(first, changed(&[id]));
    assert_eq!(env.title(id), "After");
    assert_eq!(drain(&events), Vec::<Changes>::new());
}

#[test]
fn the_apps_own_write_causes_no_loop() {
    let env = Env::new();
    let id = env.create("Task");
    let _watcher = env.watch();
    let events = env.service.subscribe();

    env.service
        .start_task(&Actor::agent("agent-a"), id)
        .unwrap();
    let written = fs::read_to_string(env.path(id)).unwrap();
    let mtime = fs::metadata(env.path(id)).unwrap().modified().unwrap();

    // One event from the write itself, sent before the call returns, and
    // none from the watcher seeing the file change.
    assert_eq!(events.try_recv().unwrap(), changed(&[id]));
    assert_eq!(drain(&events), Vec::<Changes>::new());
    assert_eq!(fs::read_to_string(env.path(id)).unwrap(), written);
    assert_eq!(
        fs::metadata(env.path(id)).unwrap().modified().unwrap(),
        mtime
    );
}

#[test]
fn a_burst_of_edits_is_picked_up_in_full() {
    let env = Env::new();
    let ids: Vec<TaskId> = (0..25).map(|n| env.create(&format!("Task {n}"))).collect();
    let _watcher = env.watch();
    let events = env.service.subscribe();

    // A branch switch rewrites every file at once.
    for &id in &ids {
        let path = env.path(id);
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("Task", "Switched");
        fs::write(&path, text).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut seen = BTreeSet::new();
    while seen.len() < ids.len() && Instant::now() < deadline {
        if let Ok(changes) = events.recv_timeout(Duration::from_millis(200)) {
            seen.extend(changes.changed);
        }
    }
    assert_eq!(seen, ids.iter().copied().collect());
    assert!(ids.iter().all(|&id| env.title(id).starts_with("Switched")));
    assert_eq!(drain(&events), Vec::<Changes>::new());
}

#[test]
fn a_deleted_file_is_removed() {
    let env = Env::new();
    let id = env.create("Doomed");
    let _watcher = env.watch();
    let events = env.service.subscribe();

    fs::remove_file(env.path(id)).unwrap();
    let first = events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(first.removed, [id].into());
    assert!(
        env.service
            .list_tasks(&Actor::User, &TaskFilter::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(drain(&events), Vec::<Changes>::new());
}
