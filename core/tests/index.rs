use std::cell::Cell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jiff::Timestamp;
use rusqlite::Connection;
use tandem_board_core::index::{
    AppDb, Changes, IndexError, IndexedTask, Problem, ProjectIndex, project_db_path,
};
use tandem_board_core::store::{Access, Date, Priority, ReadOnly, Status, TaskId, read_task};
use tempfile::TempDir;

const A: &str = "a7rq4ne";
const B: &str = "k3f9x2m";
const C: &str = "p2xmq7d";
const D: &str = "m4vt8ha";

fn id(s: &str) -> TaskId {
    s.parse().unwrap()
}

fn ids(list: &[&str]) -> BTreeSet<TaskId> {
    list.iter().copied().map(id).collect()
}

fn task(id: &str, title: &str, status: &str, extra: &str) -> String {
    format!(
        "---\nid: {id}\ntitle: {title}\nstatus: {status}\n{extra}created: 2026-10-13\n---\n\n\
         ## Description\n{title}.\n"
    )
}

/// A repo and an app data directory, both temporary.
struct Env {
    data: TempDir,
    repo: TempDir,
    /// Every write gets a later mtime than the one before, so tests never
    /// depend on the clock moving between two writes.
    clock: Cell<u64>,
}

impl Env {
    fn new() -> Self {
        let env = Self {
            data: TempDir::new().unwrap(),
            repo: TempDir::new().unwrap(),
            clock: Cell::new(1_790_000_000),
        };
        fs::create_dir(env.dir()).unwrap();
        env
    }

    fn dir(&self) -> PathBuf {
        self.repo.path().join(".tandem")
    }

    fn path(&self, id: &str) -> PathBuf {
        self.dir().join(format!("{id}.md"))
    }

    fn db(&self) -> PathBuf {
        project_db_path(self.data.path(), "test")
    }

    fn open(&self) -> ProjectIndex {
        ProjectIndex::open(self.data.path(), "test", self.repo.path()).unwrap()
    }

    fn write(&self, id: &str, text: &str) {
        let path = self.path(id);
        fs::write(&path, text).unwrap();
        self.clock.set(self.clock.get() + 1);
        set_mtime(&path, UNIX_EPOCH + Duration::from_secs(self.clock.get()));
    }

    fn remove(&self, id: &str) {
        fs::remove_file(self.path(id)).unwrap();
    }
}

fn set_mtime(path: &Path, time: SystemTime) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn titles(index: &ProjectIndex) -> Vec<(TaskId, String)> {
    index
        .tasks()
        .unwrap()
        .into_iter()
        .map(|t| (t.id, t.title))
        .collect()
}

fn problem(index: &ProjectIndex, task: &str) -> Option<Problem> {
    index.task(id(task)).unwrap().unwrap().problem
}

fn changes(changed: &[&str], removed: &[&str]) -> Changes {
    Changes {
        changed: ids(changed),
        removed: ids(removed),
    }
}

fn names(conn: &Connection, kind: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .unwrap();
    stmt.query_map([kind], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn journal_mode(conn: &Connection) -> String {
    conn.pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap()
}

fn add_lease(index: &ProjectIndex, task: &str) {
    index
        .connection()
        .execute(
            "INSERT INTO leases (task_id, agent_id, claimed_at, expires_at)
             VALUES (?1, 'claude-code-7f2a', '2026-10-15T09:00:00.000Z', '2026-10-15T09:15:00.000Z')",
            [task],
        )
        .unwrap();
}

fn leases(index: &ProjectIndex) -> Vec<String> {
    let mut stmt = index
        .connection()
        .prepare("SELECT task_id FROM leases ORDER BY task_id")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn creates_exactly_the_spec_tables_in_wal_mode() {
    let env = Env::new();
    let index = env.open();
    assert_eq!(
        names(index.connection(), "table"),
        ["agents", "leases", "task_flags", "tasks", "work_log"]
    );
    assert_eq!(
        names(index.connection(), "index"),
        ["tasks_by_parent", "work_log_by_task"]
    );
    assert_eq!(journal_mode(index.connection()), "wal");
    assert!(env.db().ends_with("projects/test.db"));
    assert!(env.db().is_file());

    let mut app = AppDb::open(env.data.path()).unwrap();
    app.add_project(env.repo.path()).unwrap();
    let conn = Connection::open(env.data.path().join("app.db")).unwrap();
    assert_eq!(names(&conn, "table"), ["projects"]);
    assert_eq!(journal_mode(&conn), "wal");
}

#[test]
fn indexes_every_field() {
    let env = Env::new();
    env.write(
        A,
        "---\nid: a7rq4ne\ntitle: Fix token refresh race\nstatus: finished\npriority: high\n\
         parent: k3f9x2m\nlabels: [auth, needs review]\ndepends_on: [p2xmq7d]\n\
         created: 2026-10-13\nreviewed_at: 2026-10-14\n---\n\n## Description\nText.\n",
    );
    env.write(B, &task(B, "Parent", "todo", ""));
    env.write(C, &task(C, "Dependency", "done", ""));
    let index = env.open();
    assert_eq!(
        index.task(id(A)).unwrap().unwrap(),
        IndexedTask {
            id: id(A),
            path: ".tandem/a7rq4ne.md".into(),
            title: "Fix token refresh race".into(),
            status: Status::Finished,
            priority: Priority::High,
            parent: Some(id(B)),
            labels: vec!["auth".into(), "needs review".into()],
            depends_on: vec![id(C)],
            created: Some(Date::constant(2026, 10, 13)),
            reviewed_at: Some(Date::constant(2026, 10, 14)),
            body: "\n## Description\nText.\n".into(),
            problem: None,
        }
    );
    // A task without a priority sorts as medium.
    let b = index.task(id(B)).unwrap().unwrap();
    assert_eq!(
        (b.priority, b.labels, b.parent),
        (Priority::Medium, vec![], None)
    );
    assert_eq!(index.task(id(D)).unwrap(), None);
}

#[test]
fn rescan_after_edits() {
    let env = Env::new();
    env.write(A, &task(A, "One", "todo", ""));
    env.write(B, &task(B, "Two", "todo", ""));
    env.write(C, &task(C, "Three", "todo", ""));
    let mut index = env.open();
    assert_eq!(index.rescan().unwrap(), Changes::default());

    env.write(A, &task(A, "One, edited", "in_progress", ""));
    env.remove(B);
    env.write(D, &task(D, "Four", "backlog", ""));
    // Files that aren't `<id>.md` are ignored.
    fs::write(env.dir().join("notes.md"), "not a task").unwrap();
    fs::write(env.dir().join("a7rq4ne.md.orig"), "not a task").unwrap();

    assert_eq!(index.rescan().unwrap(), changes(&[A, D], &[B]));
    assert_eq!(
        titles(&index),
        [
            (id(A), "One, edited".into()),
            (id(D), "Four".into()),
            (id(C), "Three".into())
        ]
    );
    assert_eq!(
        index.task(id(A)).unwrap().unwrap().status,
        Status::InProgress
    );
    assert_eq!(index.rescan().unwrap(), Changes::default());
}

#[test]
fn rescan_skips_files_whose_mtime_and_size_are_unchanged() {
    let env = Env::new();
    env.write(A, &task(A, "Same", "todo", ""));
    let mut index = env.open();
    let mtime = fs::metadata(env.path(A)).unwrap().modified().unwrap();

    // Same size and mtime: not even read, by design.
    fs::write(env.path(A), task(A, "Diff", "todo", "")).unwrap();
    set_mtime(&env.path(A), mtime);
    assert_eq!(index.rescan().unwrap(), Changes::default());
    assert_eq!(titles(&index), [(id(A), "Same".into())]);

    // A new mtime with the same size is read and picked up.
    env.write(A, &task(A, "Diff", "todo", ""));
    assert_eq!(index.rescan().unwrap(), changes(&[A], &[]));
    assert_eq!(titles(&index), [(id(A), "Diff".into())]);
}

#[test]
fn rescan_only_updates_the_stat_when_the_hash_is_unchanged() {
    let env = Env::new();
    env.write(A, &task(A, "Same", "todo", ""));
    let mut index = env.open();
    let stored = |index: &ProjectIndex| -> (i64, String) {
        index
            .connection()
            .query_row("SELECT file_mtime, file_hash FROM tasks", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
    };
    let (mtime, hash) = stored(&index);

    // A touch, or a checkout that writes the same bytes.
    env.write(A, &task(A, "Same", "todo", ""));
    assert_eq!(index.rescan().unwrap(), Changes::default());
    let (new_mtime, new_hash) = stored(&index);
    assert_eq!(new_mtime, mtime + 1000);
    assert_eq!(new_hash, hash);
}

#[test]
fn rescan_task_reindexes_one_file() {
    let env = Env::new();
    env.write(A, &task(A, "One", "todo", ""));
    env.write(B, &task(B, "Two", "todo", ""));
    let mut index = env.open();

    env.write(A, &task(A, "One, edited", "todo", ""));
    env.write(B, &task(B, "Two, edited", "todo", ""));
    assert_eq!(index.rescan_task(id(A)).unwrap(), changes(&[A], &[]));
    // A second event for the same write changes nothing.
    assert_eq!(index.rescan_task(id(A)).unwrap(), Changes::default());
    assert_eq!(
        titles(&index),
        [(id(A), "One, edited".into()), (id(B), "Two".into())]
    );

    env.remove(A);
    assert_eq!(index.rescan_task(id(A)).unwrap(), changes(&[], &[A]));
    assert_eq!(index.rescan_task(id(C)).unwrap(), Changes::default());
    env.write(C, &task(C, "New", "todo", ""));
    assert_eq!(index.rescan_task(id(C)).unwrap(), changes(&[C], &[]));
}

#[test]
fn a_deleted_database_rebuilds_from_files() {
    let env = Env::new();
    env.write(A, &task(A, "Parent", "todo", ""));
    env.write(B, &task(B, "Child", "in_progress", "parent: a7rq4ne\n"));
    env.write(C, &task(C, "Broken", "todo", "nope: [\n"));
    let index = env.open();
    add_lease(&index, B);
    let before = index.tasks().unwrap();
    assert!(matches!(before[2].problem, Some(Problem::InvalidFile(_))));
    drop(index);

    for suffix in ["", "-wal", "-shm"] {
        let mut path = env.db().into_os_string();
        path.push(suffix);
        let _ = fs::remove_file(path);
    }
    assert!(!env.db().exists());

    let index = env.open();
    assert_eq!(index.tasks().unwrap(), before);
    assert_eq!(leases(&index), Vec::<String>::new());

    // The Reset local state button does the same while the app runs.
    add_lease(&index, B);
    let index = index.reset().unwrap();
    assert_eq!(index.tasks().unwrap(), before);
    assert_eq!(leases(&index), Vec::<String>::new());
}

#[test]
fn a_file_edited_while_closed_is_picked_up_on_open() {
    let env = Env::new();
    env.write(A, &task(A, "One", "todo", ""));
    env.write(B, &task(B, "Two", "todo", ""));
    env.write(C, &task(C, "Three", "todo", ""));
    drop(env.open());

    env.write(
        A,
        &task(A, "One, edited", "done", "reviewed_at: 2026-10-15\n"),
    );
    env.remove(B);
    env.write(D, &task(D, "Four", "todo", ""));

    let index = env.open();
    assert_eq!(
        titles(&index),
        [
            (id(A), "One, edited".into()),
            (id(D), "Four".into()),
            (id(C), "Three".into())
        ]
    );
    let a = index.task(id(A)).unwrap().unwrap();
    assert_eq!(a.status, Status::Done);
    assert_eq!(a.reviewed_at, Some(Date::constant(2026, 10, 15)));
}

#[test]
fn an_invalid_file_is_indexed_with_its_error_and_never_written() {
    let env = Env::new();
    let broken =
        "---\nid: a7rq4ne\ntitle: Broken\nstatus: started\ncreated: 2026-10-13\n---\r\nBody\r\n";
    env.write(A, broken);
    let mut index = env.open();

    let a = index.task(id(A)).unwrap().unwrap();
    assert_eq!(
        (a.title.as_str(), a.status, a.created, a.body.as_str()),
        ("", Status::Backlog, None, broken)
    );
    match a.problem {
        Some(Problem::InvalidFile(message)) => {
            assert!(message.contains("unknown variant `started`"), "{message}");
            assert!(!message.contains('\n'), "{message}");
        }
        other => panic!("expected an invalid file, got {other:?}"),
    }
    assert_eq!(fs::read_to_string(env.path(A)).unwrap(), broken);

    // Fixing the file clears the error.
    env.write(A, &task(A, "Fixed", "todo", ""));
    assert_eq!(index.rescan().unwrap(), changes(&[A], &[]));
    assert_eq!(problem(&index, A), None);
    assert_eq!(read_task(&env.path(A)).unwrap().title, "Fixed");

    // A file whose id doesn't match its name, or that isn't UTF-8, is invalid.
    env.write(B, &task(C, "Wrong id", "todo", ""));
    fs::write(env.path(C), b"---\xff\n").unwrap();
    index.rescan().unwrap();
    assert!(
        matches!(problem(&index, B), Some(Problem::InvalidFile(m)) if m.contains("does not match"))
    );
    assert!(matches!(problem(&index, C), Some(Problem::InvalidFile(m)) if m.contains("UTF-8")));
}

#[test]
fn every_rescan_checks_the_whole_graph() {
    let env = Env::new();
    env.write(A, &task(A, "Top", "todo", ""));
    env.write(B, &task(B, "Middle", "todo", "parent: a7rq4ne\n"));
    env.write(C, &task(C, "Bottom", "todo", "parent: k3f9x2m\n"));
    let mut index = env.open();
    assert!(index.tasks().unwrap().iter().all(|t| t.problem.is_none()));

    // A hand edit closes a parent loop: A → C → B → A.
    env.write(A, &task(A, "Top", "todo", "parent: p2xmq7d\n"));
    assert_eq!(index.rescan().unwrap(), changes(&[A, B, C], &[]));
    for task in [A, B, C] {
        assert_eq!(
            problem(&index, task),
            Some(Problem::Graph("in a parent loop".into()))
        );
    }

    // Fixing it clears all three, though only one file changed.
    env.write(A, &task(A, "Top", "todo", ""));
    assert_eq!(index.rescan_task(id(A)).unwrap(), changes(&[A, B, C], &[]));
    assert!(index.tasks().unwrap().iter().all(|t| t.problem.is_none()));

    // Deleting a parent leaves its child with a missing parent.
    env.remove(B);
    assert_eq!(index.rescan().unwrap(), changes(&[C], &[B]));
    assert_eq!(
        problem(&index, C),
        Some(Problem::Graph(format!("the parent `{B}` does not exist")))
    );

    // A dependency loop, plus the missing parent.
    env.write(A, &task(A, "Top", "todo", "depends_on: [p2xmq7d]\n"));
    env.write(
        C,
        &task(
            C,
            "Bottom",
            "todo",
            "parent: k3f9x2m\ndepends_on: [a7rq4ne]\n",
        ),
    );
    index.rescan().unwrap();
    assert_eq!(
        problem(&index, A),
        Some(Problem::Graph("in a dependency loop".into()))
    );
    assert_eq!(
        problem(&index, C),
        Some(Problem::Graph(format!(
            "the parent `{B}` does not exist; in a dependency loop"
        )))
    );

    // A parent whose file is invalid still exists.
    env.write(B, "not a task file\n");
    env.write(C, &task(C, "Bottom", "todo", "parent: k3f9x2m\n"));
    env.write(A, &task(A, "Top", "todo", ""));
    index.rescan().unwrap();
    assert_eq!(problem(&index, C), None);
    assert!(matches!(problem(&index, B), Some(Problem::InvalidFile(_))));
}

#[test]
fn leases_are_dropped_when_the_task_is_no_longer_in_progress() {
    let env = Env::new();
    env.write(A, &task(A, "Working", "in_progress", ""));
    env.write(B, &task(B, "Working too", "in_progress", ""));
    env.write(C, &task(C, "Working three", "in_progress", ""));
    let mut index = env.open();
    for task in [A, B, C] {
        add_lease(&index, task);
    }

    // A branch switch: A is finished there, B gets a broken file, C is gone.
    env.write(A, &task(A, "Working", "finished", ""));
    env.write(B, "---\nbroken\n");
    env.remove(C);
    index.rescan().unwrap();
    // A broken file's status is unknown, so its lease stays.
    assert_eq!(leases(&index), [B]);
}

#[test]
fn working_logs_of_missing_tasks_are_pruned_after_30_days() {
    let env = Env::new();
    env.write(A, &task(A, "Still here", "todo", ""));
    let index = env.open();
    let log = |task: &str, at: &str| {
        index
            .connection()
            .execute(
                "INSERT INTO work_log (task_id, kind, message, created_at)
                 VALUES (?1, 'note', 'x', ?2)",
                [task, at],
            )
            .unwrap();
    };
    log(A, "2026-01-01T00:00:00.000Z"); // present task: kept
    log(B, "2026-08-01T00:00:00.000Z"); // missing, last entry old: deleted
    log(B, "2026-09-01T00:00:00.000Z");
    log(C, "2026-08-01T00:00:00.000Z"); // missing, last entry recent: kept
    log(C, "2026-10-01T00:00:00.000Z");

    let now: Timestamp = "2026-10-15T00:00:00Z".parse().unwrap();
    assert_eq!(index.prune_work_log(now).unwrap(), 2);
    let remaining: Vec<String> = {
        let mut stmt = index
            .connection()
            .prepare("SELECT task_id FROM work_log ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(remaining, [A, C, C]);
}

#[test]
fn creates_a_missing_tandem_folder_with_config_and_readme() {
    let env = Env::new();
    fs::remove_dir(env.dir()).unwrap();
    let index = env.open();
    assert_eq!(index.access(), &Access::ReadWrite);
    assert_eq!(
        fs::read_to_string(env.dir().join("config.toml")).unwrap(),
        "format_version = 1\n"
    );
    let readme = fs::read_to_string(env.dir().join("README.md")).unwrap();
    assert!(readme.starts_with("# Tasks\n"));
    // Only the two files, no temp files left behind.
    assert_eq!(fs::read_dir(env.dir()).unwrap().count(), 2);
    assert!(index.tasks().unwrap().is_empty());
}

#[test]
fn a_missing_config_is_written_as_version_1() {
    let env = Env::new();
    env.write(A, &task(A, "One", "todo", ""));
    let index = env.open();
    assert!(!index.is_read_only());
    assert_eq!(
        fs::read_to_string(env.dir().join("config.toml")).unwrap(),
        "format_version = 1\n"
    );
    // The README is only written along with a new folder.
    assert!(!env.dir().join("README.md").exists());
    assert_eq!(titles(&index), [(id(A), "One".into())]);
}

#[test]
fn a_newer_format_opens_read_only() {
    let env = Env::new();
    let config = "# written by Tandem 2\nformat_version = 2\n";
    fs::write(env.dir().join("config.toml"), config).unwrap();
    env.write(A, &task(A, "One", "todo", ""));
    let index = env.open();
    assert!(index.is_read_only());
    assert_eq!(
        index.access(),
        &Access::ReadOnly(ReadOnly::NewerFormat { version: 2 })
    );
    assert_eq!(
        fs::read_to_string(env.dir().join("config.toml")).unwrap(),
        config
    );
    // The board still shows what it can read.
    assert_eq!(titles(&index), [(id(A), "One".into())]);
}

#[test]
fn an_unreadable_config_opens_read_only_and_is_kept() {
    let env = Env::new();
    fs::write(env.dir().join("config.toml"), "format_version = \n").unwrap();
    let index = env.open();
    assert!(matches!(
        index.access(),
        Access::ReadOnly(ReadOnly::InvalidConfig(_))
    ));
    assert_eq!(
        fs::read_to_string(env.dir().join("config.toml")).unwrap(),
        "format_version = \n"
    );
}

#[test]
fn refuses_bad_project_ids_and_newer_schemas() {
    let env = Env::new();
    for bad in ["../evil", "", "A"] {
        assert!(matches!(
            ProjectIndex::open(env.data.path(), bad, env.repo.path()),
            Err(IndexError::InvalidProjectId(_))
        ));
    }

    drop(env.open());
    let conn = Connection::open(env.db()).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(matches!(
        ProjectIndex::open(env.data.path(), "test", env.repo.path()),
        Err(IndexError::NewerSchema {
            version: 99,
            supported: 1,
            ..
        })
    ));
}

#[test]
fn the_app_database_lists_projects() {
    let data = TempDir::new().unwrap();
    let parent = TempDir::new().unwrap();
    let repo = parent.path().join("My Repo");
    let other = parent.path().join("other").join("my-repo");
    fs::create_dir(&repo).unwrap();
    fs::create_dir_all(&other).unwrap();

    let mut app = AppDb::open(data.path()).unwrap();
    let first = app.add_project(&repo).unwrap();
    assert_eq!(
        (first.id.as_str(), first.name.as_str()),
        ("my-repo", "My Repo")
    );
    assert_eq!(first.repo_path, repo.canonicalize().unwrap());
    // Adding the same repo again, by any path, returns it unchanged.
    let again = app
        .add_project(&parent.path().join("other/../My Repo"))
        .unwrap();
    assert_eq!(again, first);
    // Another repo with the same slug gets a suffix.
    let second = app.add_project(&other).unwrap();
    assert_eq!(second.id, "my-repo-2");

    let at: Timestamp = "2026-10-15T09:30:00Z".parse().unwrap();
    assert!(app.mark_opened("my-repo-2", at).unwrap());
    assert!(!app.mark_opened("missing", at).unwrap());
    let projects = app.projects().unwrap();
    assert_eq!(
        projects.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["my-repo-2", "my-repo"]
    );
    assert_eq!(
        projects[0].last_opened_at.as_deref(),
        Some("2026-10-15T09:30:00.000Z")
    );

    drop(app);
    let app = AppDb::open(data.path()).unwrap();
    assert_eq!(app.project("my-repo").unwrap(), Some(first));
    assert!(app.remove_project("my-repo").unwrap());
    assert_eq!(app.project("my-repo").unwrap(), None);
    assert!(repo.is_dir());
}
