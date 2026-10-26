use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use rusqlite::Connection;
use tandem_board_core::db::backup::{
    self, backup_dir, backups_owner, changed_since, keep_only, set_backups_owner,
};
use tandem_board_core::db::{AppDb, project_db_path};
use tandem_board_core::model::TaskId;
use tandem_board_core::service::{Actor, Config, NewTask, Service, TaskFilter};
use tempfile::TempDir;

/// A project with a service whose clock only moves when a test moves it.
struct Env {
    data: TempDir,
    now: Arc<Mutex<Timestamp>>,
    service: Service,
}

impl Env {
    fn new() -> Self {
        let data = TempDir::new().unwrap();
        let now = Arc::new(Mutex::new("2026-10-23T09:00:00Z".parse().unwrap()));
        let clock = Arc::clone(&now);
        let config = Config {
            lease_duration: SignedDuration::from_mins(15),
            user_name: "sam".into(),
            clock: Arc::new(move || *clock.lock().unwrap()),
            time_zone: TimeZone::UTC,
            ..Config::default()
        };
        let service = Service::open(data.path(), "test", config).unwrap();
        Self { data, now, service }
    }

    fn dir(&self) -> PathBuf {
        backup_dir(self.data.path(), "test")
    }

    fn advance(&self, secs: i64) {
        *self.now.lock().unwrap() += SignedDuration::from_secs(secs);
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

    fn titles(&self) -> Vec<String> {
        let mut titles: Vec<String> = self
            .service
            .list_tasks(&Actor::User, &TaskFilter::default())
            .unwrap()
            .into_iter()
            .map(|task| task.title)
            .collect();
        titles.sort();
        titles
    }
}

#[test]
fn a_backup_restores_the_board_and_the_restore_can_be_undone() {
    let env = Env::new();
    let kept = env.create("Kept");
    let saved = env.service.back_up(&env.dir()).unwrap();
    assert_eq!(saved.name, "20261023T090000.000Z");
    assert_eq!(saved.tasks, Some(1));
    assert!(saved.path.is_file());

    env.advance(60);
    env.service.delete_task(kept).unwrap();
    let added = env.create("Added later");
    assert_eq!(env.titles(), ["Added later"]);

    let changes = env.service.subscribe();
    env.advance(60);
    let undo = env.service.restore_backup(&env.dir(), &saved.name).unwrap();
    assert_eq!(env.titles(), ["Kept"]);
    let changed = changes.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(changed.changed, [kept].into());
    assert_eq!(changed.removed, [added].into());
    // History comes back with the task.
    let events: Vec<String> = env
        .service
        .get_task(&Actor::User, kept)
        .unwrap()
        .content
        .changelog
        .into_iter()
        .map(|e| e.event)
        .collect();
    assert_eq!(events, ["created"]);

    // The state before the restore was backed up first.
    assert_eq!(undo.tasks, Some(1));
    let listed: Vec<String> = backup::list(&env.dir())
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(listed, [undo.name.clone(), saved.name.clone()]);
    env.service.restore_backup(&env.dir(), &undo.name).unwrap();
    assert_eq!(env.titles(), ["Added later"]);

    // The service keeps working on the restored database.
    env.create("After");
    let agent = Actor::agent("claude-code-7f2a");
    let task = env.create("For the agent");
    env.service.start_task(&agent, task).unwrap();
    let who = env
        .service
        .get_task(&Actor::User, task)
        .unwrap()
        .content
        .changelog[0]
        .who
        .clone();
    assert_eq!(who, "sam");
}

#[test]
fn a_restore_is_refused_while_an_agent_works() {
    let env = Env::new();
    let task = env.create("Busy");
    let saved = env.service.back_up(&env.dir()).unwrap();
    let agent = Actor::agent("claude-code-7f2a");
    env.service.start_task(&agent, task).unwrap();

    let error = env
        .service
        .restore_backup(&env.dir(), &saved.name)
        .unwrap_err();
    assert_eq!(error.code(), "agents_working");
    assert!(error.to_string().contains(task.as_str()), "{error}");
    assert_eq!(backup::list(&env.dir()).unwrap().len(), 1);

    // A stale lease doesn't count, and the backup's leases are dropped.
    env.advance(16 * 60);
    env.service.restore_backup(&env.dir(), &saved.name).unwrap();
    let summary = env
        .service
        .list_tasks(&Actor::User, &TaskFilter::default())
        .unwrap()
        .remove(0);
    assert_eq!(summary.lease, None);
}

#[test]
fn only_sound_backups_are_restored() {
    let env = Env::new();
    env.create("Kept");
    let code = |name: &str| {
        env.service
            .restore_backup(&env.dir(), name)
            .unwrap_err()
            .code()
    };
    assert_eq!(code("20261023T090000.000Z"), "invalid_argument");
    assert_eq!(code("../db/projects/test"), "invalid_argument");

    fs::create_dir_all(env.dir()).unwrap();
    fs::write(env.dir().join("20261022T090000.000Z.db"), "not a database").unwrap();
    assert_eq!(code("20261022T090000.000Z"), "invalid_argument");

    let newer = env.service.back_up(&env.dir()).unwrap();
    Connection::open(&newer.path)
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    assert_eq!(code(&newer.name), "invalid_argument");
    assert_eq!(env.titles(), ["Kept"]);
}

#[test]
fn backups_made_at_the_same_time_get_their_own_names() {
    let env = Env::new();
    let first = env.service.back_up(&env.dir()).unwrap();
    let second = env.service.back_up(&env.dir()).unwrap();
    assert_eq!(first.name, "20261023T090000.000Z");
    assert_eq!(second.name, "20261023T090000.001Z");
    // Other files in the folder are not backups.
    fs::write(env.dir().join("notes.txt"), "").unwrap();
    set_backups_owner(&env.dir(), &PathBuf::from("/code/app/.git")).unwrap();
    assert_eq!(backup::list(&env.dir()).unwrap().len(), 2);

    keep_only(&env.dir(), &second.name).unwrap();
    let left: Vec<String> = backup::list(&env.dir())
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(left, [second.name]);
    assert_eq!(backups_owner(&env.dir()).as_deref(), Some("/code/app/.git"));
}

#[test]
fn pruning_deletes_what_the_plan_does_not_keep() {
    let env = Env::new();
    for _ in 0..3 {
        env.service.back_up(&env.dir()).unwrap();
        env.advance(20 * 3600);
    }
    // Made at 09:00 on the 23rd, 05:00 on the 24th and 01:00 on the 25th;
    // two days later only the newest of each day is left.
    let now: Timestamp = "2026-10-27T09:00:00Z".parse().unwrap();
    assert_eq!(backup::prune(&env.dir(), now, &TimeZone::UTC).unwrap(), 0);
    let later: Timestamp = "2026-12-01T00:00:00Z".parse().unwrap();
    assert_eq!(backup::prune(&env.dir(), later, &TimeZone::UTC).unwrap(), 3);
    assert!(backup::list(&env.dir()).unwrap().is_empty());
}

#[test]
fn changes_after_a_backup_are_noticed() {
    let env = Env::new();
    let db = project_db_path(env.data.path(), "test");
    let saved = env.service.back_up(&env.dir()).unwrap();
    let after = Timestamp::now() + SignedDuration::from_secs(1);
    assert!(!changed_since(&db, after));
    assert!(changed_since(&db, saved.at));
    assert!(!changed_since(
        &env.data.path().join("missing.db"),
        saved.at
    ));
}

#[test]
fn writes_within_a_backups_millisecond_do_not_count() {
    let env = Env::new();
    let db = project_db_path(env.data.path(), "test");
    let saved = env.service.back_up(&env.dir()).unwrap();
    let touch = |at: Timestamp| {
        for suffix in ["", "-wal"] {
            let mut path = db.as_os_str().to_owned();
            path.push(suffix);
            fs::File::options()
                .write(true)
                .open(PathBuf::from(path))
                .unwrap()
                .set_modified(at.into())
                .unwrap();
        }
    };
    // Backup times are kept to the millisecond, so the backup may have been
    // made late in it.
    touch(saved.at + SignedDuration::from_micros(999));
    assert!(!changed_since(&db, saved.at));
    touch(saved.at + SignedDuration::from_millis(1));
    assert!(changed_since(&db, saved.at));
}

#[test]
fn a_new_project_does_not_take_an_id_with_another_projects_backups() {
    let data = TempDir::new().unwrap();
    let parent = TempDir::new().unwrap();
    let (old, new) = (parent.path().join("a/app"), parent.path().join("b/app"));
    fs::create_dir_all(&old).unwrap();
    fs::create_dir_all(&new).unwrap();
    // The last backup of a removed project `app`.
    let dir = backup_dir(data.path(), "app");
    set_backups_owner(&dir, &old.canonicalize().unwrap()).unwrap();

    let mut app = AppDb::open(data.path()).unwrap();
    assert_eq!(app.add_project(&new, &new, "app").unwrap().id, "app-2");
    // The project the backups belong to gets its id back.
    assert_eq!(app.add_project(&old, &old, "app").unwrap().id, "app");
}

#[test]
fn an_export_holds_every_task_with_its_history() {
    let env = Env::new();
    let parent = env.create("Parent");
    let child = env
        .service
        .create_task(
            &Actor::User,
            NewTask {
                title: "Child".into(),
                description: "Do it.".into(),
                acceptance_criteria: vec!["Works".into()],
                parent: Some(parent),
                labels: vec!["ui".into()],
                depends_on: vec![parent],
                ..NewTask::default()
            },
        )
        .unwrap()
        .id;
    let gone = env.create("Gone");
    env.service.delete_task(gone).unwrap();

    let export = env.service.export_json().unwrap();
    assert_eq!(export["format"], "tandem-tasks");
    assert_eq!(export["version"], 1);
    assert_eq!(export["exported_at"], "2026-10-23T09:00:00.000Z");
    let tasks = export["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 3);
    let task = tasks.iter().find(|t| t["id"] == child.as_str()).unwrap();
    assert_eq!(task["parent"], parent.as_str());
    assert_eq!(task["depends_on"], serde_json::json!([parent.as_str()]));
    assert_eq!(task["labels"], serde_json::json!(["ui"]));
    assert_eq!(
        task["criteria"],
        serde_json::json!([{ "text": "Works", "met": false }])
    );
    assert_eq!(task["description"], "Do it.");
    assert_eq!(task["created_by"]["name"], "sam");
    assert_eq!(task["events"][0]["kind"], "created");
    let deleted = tasks.iter().find(|t| t["id"] == gone.as_str()).unwrap();
    assert_eq!(deleted["deleted_at"], "2026-10-23T09:00:00.000Z");

    let markdown = env.service.export_markdown().unwrap();
    let mut names: Vec<&str> = markdown.iter().map(|(name, _)| name.as_str()).collect();
    names.sort();
    let mut expected = vec![parent.file_name(), child.file_name()];
    expected.sort();
    assert_eq!(names, expected);
    let (_, text) = markdown
        .iter()
        .find(|(name, _)| *name == child.file_name())
        .unwrap();
    assert!(text.contains("title: Child"), "{text}");
}
