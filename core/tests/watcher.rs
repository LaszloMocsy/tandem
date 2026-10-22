use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tandem_board_core::index::Changes;
use tandem_board_core::service::{Actor, Config, NewTask, Service, TaskFilter, TaskSummary};
use tandem_board_core::store::{Status, TaskId};
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

    fn summary(&self, id: TaskId) -> TaskSummary {
        self.service
            .list_tasks(&Actor::User, &TaskFilter::default())
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    }

    fn title(&self, id: TaskId) -> String {
        self.summary(id).title
    }

    /// The repo: the folder `.tandem/` is in.
    fn repo(&self) -> &Path {
        self.service.task_dir().parent().unwrap()
    }
}

/// Every event that arrives until `SETTLE` passes without one. Gives up
/// after ten seconds, so an event loop fails the test instead of hanging it.
fn drain(events: &Receiver<Changes>) -> Vec<Changes> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut out = Vec::new();
    while Instant::now() < deadline
        && let Ok(changes) = events.recv_timeout(SETTLE)
    {
        out.push(changes);
    }
    out
}

fn changed(ids: &[TaskId]) -> Changes {
    Changes {
        changed: ids.iter().copied().collect(),
        ..Changes::default()
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
fn reading_the_folder_causes_no_events() {
    let env = Env::new();
    let id = env.create("Task");
    let _watcher = env.watch();
    let events = env.service.subscribe();

    // On Linux every open is an event, and a scan lists the folder and
    // reads every file in it, so reacting to reads would scan forever.
    env.service.rescan().unwrap();
    fs::read_to_string(env.path(id)).unwrap();
    env.service.name().unwrap();
    assert_eq!(drain(&events), Vec::<Changes>::new());
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

/// Runs git in `repo`, asserting it succeeds. Signing and hooks from your
/// own git config are off, through the repo's config.
fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Waits until `done` holds, or fails after ten seconds.
fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn switching_branches_updates_the_board_and_drops_leases() {
    let env = Env::new();
    let repo = env.repo();
    git(repo, &["init", "--quiet", "--initial-branch=main"]);
    let hooks = repo.join(".git").join("no-hooks");
    for (key, value) in [
        ("user.name", "Sam"),
        ("user.email", "sam@example.com"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", hooks.to_str().unwrap()),
    ] {
        git(repo, &["config", key, value]);
    }

    // On main an agent holds the task; on review it is finished.
    let id = env.create("Task");
    env.service
        .start_task(&Actor::agent("agent-a"), id)
        .unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "--quiet", "-m", "Start the task"]);
    git(repo, &["switch", "--quiet", "-c", "review"]);
    let path = env.path(id);
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("status: in_progress", "status: finished"),
    )
    .unwrap();
    git(repo, &["commit", "--quiet", "-am", "Finish the task"]);
    git(repo, &["switch", "--quiet", "main"]);
    env.service.rescan().unwrap();
    assert!(env.summary(id).lease.is_some());

    let _watcher = env.watch();
    let events = env.service.subscribe();
    git(repo, &["switch", "--quiet", "review"]);
    wait_for("the board shows the review branch", || {
        env.summary(id).status == Status::Finished
    });
    assert_eq!(env.summary(id).lease, None);
    let seen: BTreeSet<TaskId> = drain(&events)
        .into_iter()
        .flat_map(|changes| changes.changed)
        .collect();
    assert!(seen.contains(&id), "no tasks-changed for {id}");

    // Back on main the task is in progress again, with no lease to return.
    git(repo, &["switch", "--quiet", "main"]);
    wait_for("the board shows main again", || {
        env.summary(id).status == Status::InProgress
    });
    assert_eq!(env.summary(id).lease, None);
}

#[test]
fn an_edit_to_config_toml_is_reported_without_a_loop() {
    let env = Env::new();
    let _watcher = env.watch();
    let events = env.service.subscribe();
    let config_changed = Changes {
        config: true,
        ..Changes::default()
    };

    // A pull brings in a new name.
    let config = env.service.task_dir().join("config.toml");
    fs::write(&config, "format_version = 1\nname = \"Pulled\"\n").unwrap();
    let first = events.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(first, config_changed);
    assert_eq!(env.service.name().unwrap().as_deref(), Some("Pulled"));
    assert_eq!(drain(&events), Vec::<Changes>::new());

    // Renaming from the app is reported the same way, and only once.
    env.service.set_name("Renamed").unwrap();
    let events = drain(&events);
    assert!(!events.is_empty());
    assert!(events.iter().all(|changes| *changes == config_changed));
}
