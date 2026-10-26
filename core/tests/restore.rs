//! Restoring a backup while other services share the project's database:
//! the restore marks the database as restoring, which keeps every other
//! call out until the copy is in, so no change is made only to be lost.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use rusqlite::Connection;
use tandem_board_core::db::backup::backup_dir;
use tandem_board_core::db::project_db_path;
use tandem_board_core::model::TaskId;
use tandem_board_core::service::{
    Actor, Config, NewTask, RESTORE_TIMEOUT, Service, ServiceError, TaskFilter,
};
use tempfile::TempDir;

struct Env {
    data: TempDir,
    now: Arc<Mutex<Timestamp>>,
}

impl Env {
    fn new() -> Self {
        Self {
            data: TempDir::new().unwrap(),
            now: Arc::new(Mutex::new("2026-10-23T09:00:00Z".parse().unwrap())),
        }
    }

    /// A service with a connection of its own, as one in another process
    /// has.
    fn service(&self) -> Service {
        let clock = Arc::clone(&self.now);
        let config = Config {
            user_name: "sam".into(),
            clock: Arc::new(move || *clock.lock().unwrap()),
            time_zone: TimeZone::UTC,
            ..Config::default()
        };
        Service::open(self.data.path(), "test", config).unwrap()
    }

    fn dir(&self) -> PathBuf {
        backup_dir(self.data.path(), "test")
    }

    fn advance(&self, duration: SignedDuration) {
        *self.now.lock().unwrap() += duration;
    }

    fn db(&self) -> Connection {
        Connection::open(project_db_path(self.data.path(), "test")).unwrap()
    }

    /// Whether the database is marked as restoring, expired or not.
    fn marked(&self) -> bool {
        self.db()
            .query_row("SELECT count(*) FROM restoring", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap()
            > 0
    }
}

fn create(service: &Service, title: &str) -> TaskId {
    service
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

fn code<T>(result: Result<T, ServiceError>) -> &'static str {
    result.err().map(|e| e.code()).unwrap_or("ok")
}

#[test]
fn a_restore_in_progress_keeps_every_other_call_out() {
    let env = Env::new();
    let app = env.service();
    let id = create(&app, "Task");
    let saved = app.back_up(&env.dir()).unwrap();
    // As a restore in another process leaves it until its copy is in.
    let until = env.now.lock().unwrap().as_millisecond() + RESTORE_TIMEOUT.as_millis() as i64;
    env.db()
        .execute(
            "INSERT INTO restoring (id, expires_at) VALUES (1, ?1)",
            [until],
        )
        .unwrap();

    let agent = Actor::agent("agent-a");
    assert_eq!(code(app.start_task(&agent, id)), "restoring");
    let new = NewTask {
        title: "New".into(),
        ..NewTask::default()
    };
    assert_eq!(code(app.create_task(&Actor::User, new)), "restoring");
    assert_eq!(code(app.delete_task(id)), "restoring");
    assert_eq!(
        code(app.list_tasks(&agent, &TaskFilter::default())),
        "restoring"
    );
    assert_eq!(code(app.register_agent(Some("claude-code"))), "restoring");
    assert_eq!(code(app.back_up(&env.dir())), "restoring");
    assert_eq!(
        code(app.restore_backup(&env.dir(), &saved.name)),
        "restoring"
    );
    let message = app.start_task(&agent, id).unwrap_err().to_string();
    assert!(message.contains("try again"), "{message}");
    // The refused agent call extended nothing and recorded nothing.
    let agents: i64 = env
        .db()
        .query_row(
            "SELECT count(*) FROM actors WHERE kind = 'agent'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(agents, 0);
    // Reads that change nothing still work.
    assert!(app.deleted_tasks().unwrap().is_empty());

    // A restore that never ends lets calls back in once its mark expires.
    env.advance(RESTORE_TIMEOUT);
    app.start_task(&agent, id).unwrap();
}

#[test]
fn a_restore_clears_its_mark_even_from_a_backup_made_during_one() {
    let env = Env::new();
    let app = env.service();
    let other = env.service();
    create(&app, "Kept");
    let saved = app.back_up(&env.dir()).unwrap();
    create(&app, "Added later");

    env.advance(SignedDuration::from_secs(1));
    // The undo backup is made while the database is marked.
    let undo = other.restore_backup(&env.dir(), &saved.name).unwrap();
    assert!(!env.marked());
    create(&app, "After");

    env.advance(SignedDuration::from_secs(1));
    app.restore_backup(&env.dir(), &undo.name).unwrap();
    assert!(!env.marked());
    let titles: Vec<String> = other
        .list_tasks(&Actor::User, &TaskFilter::default())
        .unwrap()
        .into_iter()
        .map(|task| task.title)
        .collect();
    assert_eq!(titles.len(), 2, "{titles:?}");
}

#[cfg(unix)]
#[test]
fn a_failed_restore_lets_calls_back_in_at_once() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let env = Env::new();
    let app = env.service();
    let id = create(&app, "Task");
    let saved = app.back_up(&env.dir()).unwrap();
    // The undo backup can't be written.
    fs::set_permissions(env.dir(), fs::Permissions::from_mode(0o555)).unwrap();
    let restored = app.restore_backup(&env.dir(), &saved.name);
    fs::set_permissions(env.dir(), fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(code(restored), "internal");
    assert!(!env.marked());
    app.start_task(&Actor::agent("agent-a"), id).unwrap();
}

/// An agent in another process tries to claim a task as soon as a restore
/// marks the database, and keeps trying until a claim is granted. The
/// claim must hold: it can only be granted once the restore is in.
#[test]
fn a_claim_during_a_restore_waits_for_it_and_holds() {
    for _ in 0..10 {
        let env = Env::new();
        let app = env.service();
        let agent_side = Arc::new(env.service());
        let id = create(&app, "Contended");
        let saved = app.back_up(&env.dir()).unwrap();
        env.advance(SignedDuration::from_secs(1));

        let done = Arc::new(AtomicBool::new(false));
        let claimer = {
            let done = Arc::clone(&done);
            let agent_side = Arc::clone(&agent_side);
            let db = env.db();
            thread::spawn(move || {
                let marked = || {
                    db.query_row("SELECT count(*) FROM restoring", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .is_ok_and(|n| n > 0)
                };
                while !marked() && !done.load(Ordering::SeqCst) {
                    thread::yield_now();
                }
                loop {
                    match agent_side.start_task(&Actor::agent("agent-a"), id) {
                        Ok(_) => return,
                        Err(e) if e.code() == "restoring" => thread::yield_now(),
                        Err(e) => panic!("{e}"),
                    }
                }
            })
        };
        app.restore_backup(&env.dir(), &saved.name).unwrap();
        done.store(true, Ordering::SeqCst);
        claimer.join().unwrap();

        let lease = app
            .get_task(&Actor::User, id)
            .unwrap()
            .summary
            .lease
            .map(|lease| lease.agent_id);
        assert_eq!(lease.as_deref(), Some("agent-a"));
    }
}
