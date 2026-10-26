//! How services sharing a project database hear of each other's changes,
//! and stop once a newer app takes the database over. Each service has a
//! connection of its own, as a service in another process does.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use rusqlite::Connection;
use tandem_board_core::db::backup::backup_dir;
use tandem_board_core::db::{AppDb, Changes, DbError, app_db_path, project_db_path};
use tandem_board_core::model::TaskId;
use tandem_board_core::service::{Actor, Config, NewTask, Service, TaskFilter};
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

    /// A service that checks for other processes' changes every
    /// `watch_interval`.
    fn service(&self, watch_interval: Duration) -> Service {
        let clock = Arc::clone(&self.now);
        let config = Config {
            user_name: "sam".into(),
            clock: Arc::new(move || *clock.lock().unwrap()),
            time_zone: TimeZone::UTC,
            watch_interval,
            ..Config::default()
        };
        Service::open(self.data.path(), "test", config).unwrap()
    }

    fn advance(&self, hours: i64) {
        *self.now.lock().unwrap() += SignedDuration::from_hours(hours);
    }

    fn db(&self) -> Connection {
        Connection::open(project_db_path(self.data.path(), "test")).unwrap()
    }
}

const FAST: Duration = Duration::from_millis(10);
const NEVER: Duration = Duration::from_secs(3600);

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

fn next(events: &Receiver<Changes>) -> Changes {
    events.recv_timeout(Duration::from_secs(10)).unwrap()
}

fn changes(changed: &[TaskId], removed: &[TaskId]) -> Changes {
    Changes {
        changed: changed.iter().copied().collect(),
        removed: removed.iter().copied().collect(),
    }
}

#[test]
fn another_connections_changes_arrive_without_a_call() {
    let env = Env::new();
    let app = env.service(FAST);
    let other = env.service(FAST);
    let events = app.subscribe();

    let id = create(&other, "Elsewhere");
    assert_eq!(next(&events), changes(&[id], &[]));
    other.delete_task(id).unwrap();
    assert_eq!(next(&events), changes(&[], &[id]));
    // The service's own changes still arrive, once.
    other.restore_task(id).unwrap();
    assert_eq!(next(&events), changes(&[id], &[]));
    app.delete_task(id).unwrap();
    assert_eq!(next(&events), changes(&[], &[id]));
    assert_eq!(
        events.recv_timeout(Duration::from_millis(100)),
        Err(RecvTimeoutError::Timeout)
    );
}

#[test]
fn a_call_sends_earlier_changes_from_elsewhere_first() {
    let env = Env::new();
    let app = env.service(NEVER);
    let other = env.service(NEVER);
    let events = app.subscribe();

    let elsewhere = create(&other, "Elsewhere");
    other.delete_task(elsewhere).unwrap();
    let here = create(&app, "Here");
    // Each task as it is now, the other process's changes first.
    assert_eq!(next(&events), changes(&[], &[elsewhere]));
    assert_eq!(next(&events), changes(&[here], &[]));
}

#[test]
fn a_service_too_far_behind_hears_of_every_task() {
    let env = Env::new();
    let other = env.service(NEVER);
    // Known to the app from the start, and cleaned up while it lags.
    let cleaned = create(&other, "Cleaned up");
    let app = env.service(NEVER);
    let events = app.subscribe();

    let kept = create(&other, "Kept");
    let gone = create(&other, "Gone");
    other.delete_task(gone).unwrap();
    other.delete_task(cleaned).unwrap();
    other.purge_deleted().unwrap();
    // Created and cleaned up unseen, so the app has nothing to tell.
    let unseen = create(&other, "Unseen");
    other.delete_task(unseen).unwrap();
    other.purge_deleted().unwrap();
    let gone_too = create(&other, "Gone too");
    other.delete_task(gone_too).unwrap();
    env.advance(25);
    let late = create(&other, "Late");
    // Backing up prunes the feed, so the app never sees the first rows.
    other.back_up(&backup_dir(env.data.path(), "test")).unwrap();
    let rows: i64 = env
        .db()
        .query_row("SELECT count(*) FROM changes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 1);

    app.list_tasks(&Actor::User, &TaskFilter::default())
        .unwrap();
    assert_eq!(next(&events), changes(&[kept, late], &[cleaned, gone_too]));

    // A task cleaned up once is not sent again; a deleted one is, as it
    // goes for good.
    other.delete_task(kept).unwrap();
    other.purge_deleted().unwrap();
    env.advance(25);
    other.back_up(&backup_dir(env.data.path(), "test")).unwrap();
    let later = create(&other, "Later still");
    app.list_tasks(&Actor::User, &TaskFilter::default())
        .unwrap();
    assert_eq!(next(&events), changes(&[late, later], &[kept, gone_too]));
}

#[test]
fn a_restore_elsewhere_reaches_every_service() {
    let env = Env::new();
    let app = env.service(FAST);
    let other = env.service(FAST);
    let dir = backup_dir(env.data.path(), "test");

    let kept = create(&app, "Kept");
    let saved = other.back_up(&dir).unwrap();
    let added = create(&app, "Added later");
    let before: i64 = env
        .db()
        .query_row("SELECT max(seq) FROM changes", [], |row| row.get(0))
        .unwrap();

    let events = app.subscribe();
    env.advance(1);
    other.restore_backup(&dir, &saved.name).unwrap();
    assert_eq!(next(&events), changes(&[kept], &[added]));
    // The feed carries on from where it was, not from the backup's.
    let after: i64 = env
        .db()
        .query_row(
            "SELECT min(seq) FROM changes WHERE seq > ?1",
            [before],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(after, before + 1);

    // Both services carry on, and you are still one person.
    create(&app, "After");
    create(&other, "After too");
    let users: i64 = env
        .db()
        .query_row(
            "SELECT count(*) FROM actors WHERE kind = 'user'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(users, 1);
}

#[test]
fn a_newer_app_taking_the_database_over_stops_the_service() {
    let env = Env::new();
    let app = env.service(FAST);
    let id = create(&app, "Task");
    let events = app.subscribe();

    env.db().pragma_update(None, "user_version", 99).unwrap();

    let code = |result: Result<(), tandem_board_core::service::ServiceError>| {
        result.map_err(|e| e.code()).unwrap_err()
    };
    assert_eq!(code(app.check_schema()), "newer_schema");
    // Reads and writes alike.
    assert_eq!(
        code(app.get_task(&Actor::User, id).map(drop)),
        "newer_schema"
    );
    assert_eq!(code(app.delete_task(id)), "newer_schema");
    assert_eq!(
        code(app.list_agents(SignedDuration::from_hours(1)).map(drop)),
        "newer_schema"
    );
    assert_eq!(
        code(app.start_task(&Actor::agent("agent-a"), id).map(drop)),
        "newer_schema"
    );
    let message = app.check_schema().unwrap_err().to_string();
    assert!(message.contains("update Tandem"), "{message}");

    // Subscribers' channels close.
    assert_eq!(
        events.recv_timeout(Duration::from_secs(10)),
        Err(RecvTimeoutError::Disconnected)
    );
    // Nothing was written after the takeover.
    let leases: i64 = env
        .db()
        .query_row("SELECT count(*) FROM leases", [], |row| row.get(0))
        .unwrap();
    assert_eq!(leases, 0);
}

#[test]
fn app_db_refuses_a_newer_schema_while_open() {
    let data = TempDir::new().unwrap();
    let app = AppDb::open(data.path()).unwrap();
    app.projects().unwrap();
    let conn = Connection::open(app_db_path(data.path())).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();

    assert!(matches!(
        app.projects(),
        Err(DbError::NewerSchema { version: 99, .. })
    ));
    assert!(matches!(
        app.set_name("tandem", "Tandem"),
        Err(DbError::NewerSchema { .. })
    ));
    drop(app);
    assert!(matches!(
        AppDb::open(data.path()),
        Err(DbError::NewerSchema { .. })
    ));
}
