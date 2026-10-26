//! Services in separate processes sharing one project database, as the app,
//! an MCP server and a CLI do. The tests start this test binary again as
//! the other process, running `child` with a role in `ROLE_VAR`, and find
//! the database through `TANDEM_DATA_DIR`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use jiff::tz::TimeZone;
use rusqlite::Connection;
use tandem_board_core::db::{Changes, DATA_DIR_VAR, data_dir, project_db_path};
use tandem_board_core::model::{Status, TaskId};
use tandem_board_core::service::{Actor, Config, Finish, NewTask, Service, TaskEdit, TaskFilter};
use tempfile::TempDir;

/// Which part the child process plays; unset in the parent.
const ROLE_VAR: &str = "TANDEM_TEST_ROLE";

/// Rounds of the claim race, each on a new task.
const ROUNDS: usize = 10;

/// Agents racing for each task, split between the two processes.
const RACERS: usize = 100;

fn config() -> Config {
    Config {
        user_name: "sam".into(),
        time_zone: TimeZone::UTC,
        watch_interval: Duration::from_millis(10),
        ..Config::default()
    }
}

/// The project's service, in the data directory `TANDEM_DATA_DIR` names.
fn service() -> Service {
    Service::open(&data_dir().unwrap(), "test", config()).unwrap()
}

/// Starts this binary again as a child playing `role`, with `dir` as its
/// data directory.
fn spawn(dir: &Path, role: &str) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child", "--nocapture", "--test-threads=1"])
        .env(DATA_DIR_VAR, dir)
        .env(ROLE_VAR, role)
        .spawn()
        .unwrap()
}

fn wait(child: Child) {
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "the child process failed");
}

/// Waits until `path` exists.
fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} never appeared",
            path.display()
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn create(service: &Service, actor: &Actor, title: &str) -> TaskId {
    service
        .create_task(
            actor,
            NewTask {
                title: title.into(),
                acceptance_criteria: vec!["Done".into()],
                ..NewTask::default()
            },
        )
        .unwrap()
        .id
}

/// The other process. Does nothing when the test harness runs it in the
/// parent.
#[test]
fn child() {
    let Ok(role) = std::env::var(ROLE_VAR) else {
        return;
    };
    let dir = data_dir().unwrap();
    match role.as_str() {
        "racer-0" | "racer-1" => race(&dir, &role),
        "agent" => work(&dir),
        _ => panic!("unknown role `{role}`"),
    }
}

/// Races `RACERS / 2` agents for each task in `tasks`, once both
/// processes are ready, and writes how many won each round.
fn race(dir: &Path, role: &str) {
    let service = Arc::new(service());
    let tasks: Vec<TaskId> = fs::read_to_string(dir.join("tasks"))
        .unwrap()
        .lines()
        .map(|id| id.parse().unwrap())
        .collect();
    fs::write(dir.join(format!("ready-{role}")), "").unwrap();
    wait_for(&dir.join("go"));

    let mut report = String::new();
    for id in tasks {
        let barrier = Arc::new(Barrier::new(RACERS / 2));
        let handles: Vec<_> = (0..RACERS / 2)
            .map(|n| {
                let service = Arc::clone(&service);
                let barrier = Arc::clone(&barrier);
                let agent = Actor::agent(format!("{role}-{n}"));
                thread::spawn(move || {
                    barrier.wait();
                    service.start_task(&agent, id).map_err(|e| e.code())
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let won = results.iter().filter(|r| r.is_ok()).count();
        let refused = results
            .iter()
            .filter_map(|r| r.as_ref().err().copied())
            .collect::<BTreeSet<&str>>();
        let refused: Vec<&str> = refused.into_iter().collect();
        report += &format!("{id} {won} {}\n", refused.join(","));
    }
    fs::write(dir.join(format!("result-{role}")), report).unwrap();
}

/// Creates a task, then claims, edits and finishes it, as an agent would
/// through its own MCP server.
fn work(dir: &Path) {
    let service = service();
    let id = create(&service, &Actor::User, "From the other process");
    service
        .update_task(
            &Actor::User,
            id,
            TaskEdit {
                title: Some("Edited in the other process".into()),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    let agent = Actor::agent("agent-other");
    service.start_task(&agent, id).unwrap();
    service
        .update_task(
            &agent,
            id,
            TaskEdit {
                relevant_files: Some(vec!["src/other.rs".into()]),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    service.log_progress(&agent, id, "Working.").unwrap();
    service
        .finish_task(
            &agent,
            id,
            Finish {
                summary: "Done.".into(),
                met: vec![1],
                ..Finish::default()
            },
        )
        .unwrap();
    fs::write(dir.join("task"), id.as_str()).unwrap();
}

#[test]
fn concurrent_starts_from_two_processes_have_one_winner_each_time() {
    let data = TempDir::new().unwrap();
    let dir = data.path();
    let tasks: Vec<TaskId> = {
        let service = Service::open(dir, "test", config()).unwrap();
        (0..ROUNDS)
            .map(|n| create(&service, &Actor::User, &format!("Contended {n}")))
            .collect()
    };
    let list: Vec<&str> = tasks.iter().map(TaskId::as_str).collect();
    fs::write(dir.join("tasks"), list.join("\n")).unwrap();

    let children = [spawn(dir, "racer-0"), spawn(dir, "racer-1")];
    wait_for(&dir.join("ready-racer-0"));
    wait_for(&dir.join("ready-racer-1"));
    fs::write(dir.join("go"), "").unwrap();
    for child in children {
        wait(child);
    }

    let mut winners = vec![0; ROUNDS];
    for role in ["racer-0", "racer-1"] {
        let report = fs::read_to_string(dir.join(format!("result-{role}"))).unwrap();
        for (round, line) in report.lines().enumerate() {
            let parts: Vec<&str> = line.split(' ').collect();
            assert_eq!(parts[0], tasks[round].as_str());
            winners[round] += parts[1].parse::<usize>().unwrap();
            // Every loser was told who holds the task.
            assert!(
                parts
                    .get(2)
                    .is_none_or(|codes| *codes == "claimed_by_other"),
                "{line}"
            );
        }
    }
    assert_eq!(winners, vec![1; ROUNDS]);

    let db = Connection::open(project_db_path(dir, "test")).unwrap();
    let started: i64 = db
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'started'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(started, ROUNDS as i64);
}

/// Collects the changes `events` receives until `done` holds, or fails
/// after a while.
fn collect_until(events: &Receiver<Changes>, done: impl Fn(&Changes) -> bool) -> Changes {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut all = Changes::default();
    while !done(&all) {
        let left = deadline.saturating_duration_since(Instant::now());
        let changes = events
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("only received {all:?}"));
        all.merge(changes);
    }
    all
}

#[test]
fn the_board_hears_of_work_done_in_another_process() {
    let data = TempDir::new().unwrap();
    let dir = data.path();
    let app = Service::open(dir, "test", config()).unwrap();
    let events = app.subscribe();

    wait(spawn(dir, "agent"));
    let id: TaskId = fs::read_to_string(dir.join("task"))
        .unwrap()
        .parse()
        .unwrap();
    let all = collect_until(&events, |all| all.changed.contains(&id));
    assert!(all.removed.is_empty());

    let tasks = app
        .list_tasks(&Actor::User, &TaskFilter::default())
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].title, "Edited in the other process");
    assert_eq!(tasks[0].status, Status::Finished);
    assert!(tasks[0].lease.is_none());
    let detail = app.get_task(&Actor::User, id).unwrap();
    assert_eq!(detail.content.relevant_files, ["src/other.rs"]);

    // You review it in the app, and the other process's work stays.
    app.mark_reviewed(id).unwrap();
    assert_eq!(
        app.get_task(&Actor::User, id).unwrap().summary.status,
        Status::Done
    );
}
