use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension};

use super::{DbError, backup, io_error};

/// `app.db`: the project list and the app's settings.
///
/// A project is keyed by `git_dir`: the canonical git common dir of the
/// repo, which every worktree of the repo shares, or the canonical folder
/// for a folder outside git. `last_path` is the checkout last opened, which
/// is only shown. Times are milliseconds since the Unix epoch, in UTC.
pub(super) const APP_MIGRATIONS: &[&str] = &["
CREATE TABLE projects (
  id             TEXT PRIMARY KEY,   -- a slug, used in the MCP URL and the database's name
  name           TEXT NOT NULL,
  git_dir        TEXT NOT NULL UNIQUE,
  last_path      TEXT NOT NULL,
  last_opened_at INTEGER
) STRICT;

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT, WITHOUT ROWID;
"];

/// `projects/<project-id>.db`: the project's tasks and their history. Rows
/// are kept for good; nothing is rebuilt from anywhere else.
///
/// - Tasks have an internal integer key, `seq`, which every other table
///   refers to, and the public 7-character `id` that agents and the board
///   use. A deleted task keeps its row, with `deleted_at` set, until you
///   clean up deleted tasks; until then it can be restored.
/// - `actors` are you and the agents. Every event names one.
/// - `events` is each task's timeline, oldest first by `id`: the changelog
///   (`created`, `approved`, `started`, `finished`, …) and the working log
///   (`note`, `blocked`, `unblocked`) in one table. `kind` is open-ended, so
///   it has no CHECK; `data` holds JSON for what only some kinds carry, such
///   as the files and unmet criteria of `finished`.
/// - `leases` change on every agent call, so they live apart from `tasks`.
/// - `changes` names every task each call changed, in commit order, so a
///   process sharing the database can tell what another one changed. It is
///   only a feed: rows are pruned after a day, and `seq` never goes back,
///   not even after a restore.
/// - `restoring` has a row while a backup is being restored, by any
///   process. Every call that could change something is refused until the
///   restore ends, or until `expires_at` if it never does.
/// - Times are milliseconds since the Unix epoch, in UTC.
pub(super) const PROJECT_MIGRATIONS: &[&str] = &[
    "
CREATE TABLE actors (
  id         INTEGER PRIMARY KEY,
  kind       TEXT NOT NULL CHECK (kind IN ('user','agent')),
  name       TEXT NOT NULL,           -- your name, or an agent id such as claude-code-7f2a
  client     TEXT,                    -- an agent's MCP clientInfo name, such as claude-code
  first_seen INTEGER NOT NULL,
  last_seen  INTEGER NOT NULL,
  UNIQUE (kind, name)
) STRICT;

CREATE TABLE tasks (
  seq            INTEGER PRIMARY KEY,
  id             TEXT NOT NULL UNIQUE,
  title          TEXT NOT NULL,
  status         TEXT NOT NULL CHECK (status IN ('backlog','todo','in_progress','finished','done')),
  priority       TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('high','medium','low')),
  parent         INTEGER REFERENCES tasks (seq),
  blocked_reason TEXT,
  created_by     INTEGER NOT NULL REFERENCES actors (id),
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL,
  reviewed_at    INTEGER,             -- set while the task is done
  deleted_at     INTEGER,             -- set while the task is deleted
  relevant_files TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(relevant_files)),
  description    TEXT NOT NULL DEFAULT ''  -- last, so reads that skip it stop early
) STRICT;
CREATE INDEX tasks_by_parent ON tasks (parent);

CREATE TABLE task_deps (
  task       INTEGER NOT NULL REFERENCES tasks (seq) ON DELETE CASCADE,
  depends_on INTEGER NOT NULL REFERENCES tasks (seq) ON DELETE CASCADE,
  PRIMARY KEY (task, depends_on)
) STRICT, WITHOUT ROWID;
CREATE INDEX task_deps_by_target ON task_deps (depends_on);

CREATE TABLE labels (
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE COLLATE NOCASE
) STRICT;

CREATE TABLE task_labels (
  task  INTEGER NOT NULL REFERENCES tasks (seq) ON DELETE CASCADE,
  label INTEGER NOT NULL REFERENCES labels (id),
  PRIMARY KEY (task, label)
) STRICT, WITHOUT ROWID;
CREATE INDEX task_labels_by_label ON task_labels (label);

CREATE TABLE criteria (
  task     INTEGER NOT NULL REFERENCES tasks (seq) ON DELETE CASCADE,
  position INTEGER NOT NULL,          -- from 1, the number finish_task's `met` uses
  text     TEXT NOT NULL,
  met      INTEGER NOT NULL DEFAULT 0 CHECK (met IN (0, 1)),
  PRIMARY KEY (task, position)
) STRICT, WITHOUT ROWID;

CREATE TABLE events (
  id      INTEGER PRIMARY KEY,
  task    INTEGER NOT NULL REFERENCES tasks (seq) ON DELETE CASCADE,
  at      INTEGER NOT NULL,
  actor   INTEGER NOT NULL REFERENCES actors (id),
  kind    TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT '',
  data    TEXT CHECK (data IS NULL OR json_valid(data))
) STRICT;
CREATE INDEX events_by_task ON events (task, id);

CREATE TABLE leases (
  task       INTEGER PRIMARY KEY REFERENCES tasks (seq) ON DELETE CASCADE,
  actor      INTEGER NOT NULL REFERENCES actors (id),
  claimed_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
) STRICT;
CREATE INDEX leases_by_actor ON leases (actor);
",
    "
CREATE TABLE changes (
  seq  INTEGER PRIMARY KEY AUTOINCREMENT,
  task TEXT NOT NULL,                 -- the task's id; it may have been cleaned up since
  at   INTEGER NOT NULL
) STRICT;
",
    "
CREATE TABLE restoring (
  id         INTEGER PRIMARY KEY CHECK (id = 1),  -- one row at most
  expires_at INTEGER NOT NULL
) STRICT;
",
];

/// Opens or creates the database at `path` in WAL mode, with foreign keys
/// enforced, and brings its schema up to date; see [`migrate`]. Returns
/// whether the database is new.
pub(super) fn open(
    path: &Path,
    migrations: &[&str],
    backups: Option<&Path>,
) -> Result<(Connection, bool), DbError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_error(dir))?;
    }
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(DbError::NoWal {
            path: path.to_owned(),
            mode,
        });
    }
    // Safe with WAL: a crash can lose the last commits, never corrupt the file.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let created = migrate(&mut conn, path, migrations, backups)?;
    Ok((conn, created))
}

/// Runs the migrations the database is missing. `migrations[n]` takes the
/// schema from version `n` to `n + 1`; the version is kept in
/// `PRAGMA user_version`. Returns whether the database had none yet.
///
/// The migrations run in one `BEGIN IMMEDIATE` transaction with foreign
/// keys off, so one can rebuild a table, as changing a column's `CHECK`
/// takes, without `DROP TABLE` cascading into the tables that refer to it.
/// `PRAGMA foreign_key_check` then has to find nothing, or the whole
/// transaction is rolled back with [`DbError::DanglingForeignKey`]. Foreign
/// keys are enforced afterwards as they were before.
///
/// An existing database is first backed up into `backups`, if given, with
/// [`backup::write`]. `VACUUM INTO` can't run in a transaction, so the
/// backup is taken just before it: another process's commit in between is
/// not in the backup.
pub(super) fn migrate(
    conn: &mut Connection,
    path: &Path,
    migrations: &[&str],
    backups: Option<&Path>,
) -> Result<bool, DbError> {
    let version = check_version(conn, path, migrations)?;
    if version == migrations.len() as i64 {
        return Ok(false);
    }
    if let Some(dir) = backups
        && version > 0
    {
        backup::write(conn, dir, Timestamp::now())?;
    }
    let enforced: bool = conn.pragma_query_value(None, "foreign_keys", |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", false)?;
    // A failed transaction has been rolled back by now, so the pragma
    // takes effect either way.
    let applied = apply(conn, path, migrations);
    let restored = conn.pragma_update(None, "foreign_keys", enforced);
    let version = applied?;
    restored?;
    Ok(version == 0)
}

/// Runs the missing migrations in one transaction and checks the foreign
/// keys they leave. Returns the version the database had.
fn apply(conn: &mut Connection, path: &Path, migrations: &[&str]) -> Result<i64, DbError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    // Again: another process may have migrated it meanwhile.
    let version = check_version(&tx, path, migrations)?;
    for migration in &migrations[version as usize..] {
        tx.execute_batch(migration)?;
    }
    let dangling = tx
        .query_row("PRAGMA foreign_key_check", [], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(2)?))
        })
        .optional()?;
    if let Some((table, parent)) = dangling {
        return Err(DbError::DanglingForeignKey {
            path: path.to_owned(),
            table,
            parent,
        });
    }
    tx.pragma_update(None, "user_version", migrations.len() as i64)?;
    tx.commit()?;
    Ok(version)
}

/// The schema version of the database on `conn`, refused with
/// [`DbError::NewerSchema`] if a newer app has migrated it past
/// `migrations`. Every transaction checks it, so an app that is already
/// running stops using a database a newer one has taken over.
pub(super) fn check_version(
    conn: &Connection,
    path: &Path,
    migrations: &[&str],
) -> Result<i64, DbError> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let supported = migrations.len() as i64;
    if !(0..=supported).contains(&version) {
        return Err(DbError::NewerSchema {
            path: path.to_owned(),
            version,
            supported,
        });
    }
    Ok(version)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    /// Rebuilds `tasks` with a wider `status` CHECK, as SQLite needs to
    /// change one.
    const REBUILD_TASKS: &str = "
CREATE TABLE tasks_new (
  seq            INTEGER PRIMARY KEY,
  id             TEXT NOT NULL UNIQUE,
  title          TEXT NOT NULL,
  status         TEXT NOT NULL CHECK (status IN ('backlog','todo','in_progress','review','finished','done')),
  priority       TEXT NOT NULL DEFAULT 'medium' CHECK (priority IN ('high','medium','low')),
  parent         INTEGER REFERENCES tasks (seq),
  blocked_reason TEXT,
  created_by     INTEGER NOT NULL REFERENCES actors (id),
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL,
  reviewed_at    INTEGER,
  deleted_at     INTEGER,
  relevant_files TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(relevant_files)),
  description    TEXT NOT NULL DEFAULT ''
) STRICT;
INSERT INTO tasks_new SELECT * FROM tasks;
DROP TABLE tasks;
ALTER TABLE tasks_new RENAME TO tasks;
CREATE INDEX tasks_by_parent ON tasks (parent);
";

    /// The tables that refer to `tasks`, each with the rows they hold.
    const CHILD_ROWS: &[(&str, i64)] = &[
        ("task_deps", 1),
        ("criteria", 2),
        ("events", 3),
        ("leases", 1),
        ("task_labels", 2),
    ];

    struct Env {
        _dir: TempDir,
        path: std::path::PathBuf,
        backups: std::path::PathBuf,
    }

    impl Env {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            Self {
                path: dir.path().join("test.db"),
                backups: dir.path().join("backups"),
                _dir: dir,
            }
        }

        /// A database at the current schema with two tasks, the second a
        /// child of the first and depending on it, and rows in every
        /// table that refers to them.
        fn seeded(self) -> Self {
            let (conn, created) = open(&self.path, PROJECT_MIGRATIONS, None).unwrap();
            assert!(created);
            conn.execute_batch(
                "
INSERT INTO actors (id, kind, name, first_seen, last_seen) VALUES (1, 'user', 'me', 0, 0);
INSERT INTO tasks (seq, id, title, status, created_by, created_at, updated_at)
  VALUES (5, 'aaaaaaa', 'Parent', 'todo', 1, 0, 0);
INSERT INTO tasks (seq, id, title, status, parent, created_by, created_at, updated_at)
  VALUES (9, 'bbbbbbb', 'Child', 'in_progress', 5, 1, 0, 0);
INSERT INTO task_deps (task, depends_on) VALUES (9, 5);
INSERT INTO labels (id, name) VALUES (1, 'ui'), (2, 'core');
INSERT INTO task_labels (task, label) VALUES (5, 1), (9, 2);
INSERT INTO criteria (task, position, text) VALUES (9, 1, 'one'), (9, 2, 'two');
INSERT INTO events (task, at, actor, kind) VALUES (5, 0, 1, 'created'), (9, 0, 1, 'created'),
  (9, 1, 1, 'started');
INSERT INTO leases (task, actor, claimed_at, expires_at) VALUES (9, 1, 0, 1);
",
            )
            .unwrap();
            self
        }

        fn migrations(extra: &'static str) -> Vec<&'static str> {
            let mut migrations = PROJECT_MIGRATIONS.to_vec();
            migrations.push(extra);
            migrations
        }

        fn open(&self, migrations: &[&str]) -> Result<(Connection, bool), DbError> {
            open(&self.path, migrations, Some(&self.backups))
        }
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    fn pragma(conn: &Connection, name: &str) -> i64 {
        conn.pragma_query_value(None, name, |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn rebuilding_tasks_keeps_the_rows_that_refer_to_them() {
        let env = Env::new().seeded();
        let migrations = Env::migrations(REBUILD_TASKS);
        let (conn, created) = env.open(&migrations).unwrap();
        assert!(!created);
        assert_eq!(pragma(&conn, "user_version"), 4);
        assert_eq!(pragma(&conn, "foreign_keys"), 1);
        for &(table, rows) in CHILD_ROWS {
            assert_eq!(count(&conn, table), rows, "{table}");
        }
        let tasks: Vec<(i64, String, Option<i64>)> = conn
            .prepare("SELECT seq, id, parent FROM tasks ORDER BY seq")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            tasks,
            [
                (5, "aaaaaaa".to_owned(), None),
                (9, "bbbbbbb".to_owned(), Some(5))
            ]
        );
        // The new CHECK holds, and the foreign keys still cascade.
        conn.execute("UPDATE tasks SET status = 'review' WHERE seq = 9", [])
            .unwrap();
        conn.execute("DELETE FROM tasks WHERE seq = 9", []).unwrap();
        assert_eq!(count(&conn, "criteria"), 0);
        assert_eq!(count(&conn, "leases"), 0);
    }

    #[test]
    fn a_migration_that_leaves_a_dangling_foreign_key_is_rolled_back() {
        let env = Env::new().seeded();
        let migrations = Env::migrations("DELETE FROM tasks WHERE seq = 5;");
        let err = env.open(&migrations).unwrap_err();
        assert!(
            matches!(&err, DbError::DanglingForeignKey { table, parent, .. }
                if parent == "tasks" && CHILD_ROWS.iter().any(|(t, _)| t == table)),
            "{err:?}"
        );
        let (conn, _) = open(&env.path, PROJECT_MIGRATIONS, None).unwrap();
        assert_eq!(pragma(&conn, "user_version"), 3);
        assert_eq!(count(&conn, "tasks"), 2);
        for &(table, rows) in CHILD_ROWS {
            assert_eq!(count(&conn, table), rows, "{table}");
        }
    }

    #[test]
    fn foreign_keys_are_enforced_again_after_a_failed_migration() {
        let env = Env::new().seeded();
        let mut conn = Connection::open(&env.path).unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        for bad in ["DELETE FROM tasks WHERE seq = 5;", "NOT SQL;"] {
            let migrations = Env::migrations(bad);
            assert!(migrate(&mut conn, &env.path, &migrations, None).is_err());
            assert_eq!(pragma(&conn, "foreign_keys"), 1);
            assert_eq!(pragma(&conn, "user_version"), 3);
        }
        // And stay off for a connection that had them off.
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        let migrations = Env::migrations(REBUILD_TASKS);
        migrate(&mut conn, &env.path, &migrations, None).unwrap();
        assert_eq!(pragma(&conn, "foreign_keys"), 0);
    }

    #[test]
    fn an_existing_database_is_backed_up_before_it_is_migrated() {
        let env = Env::new().seeded();
        let migrations = Env::migrations(REBUILD_TASKS);
        drop(env.open(&migrations).unwrap());
        let backups = backup::list(&env.backups).unwrap();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].tasks, Some(2));
        // The backup has the schema from before the migration.
        let saved = Connection::open(&backups[0].path).unwrap();
        assert_eq!(pragma(&saved, "user_version"), 3);
        drop(saved);

        // Opening it again migrates nothing, so backs up nothing.
        drop(env.open(&migrations).unwrap());
        assert_eq!(backup::list(&env.backups).unwrap().len(), 1);
    }

    #[test]
    fn a_new_database_is_not_backed_up() {
        let env = Env::new();
        let (_, created) = env.open(PROJECT_MIGRATIONS).unwrap();
        assert!(created);
        drop(env.open(PROJECT_MIGRATIONS).unwrap());
        assert!(!env.backups.exists());
    }
}
