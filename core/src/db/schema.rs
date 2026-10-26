use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use super::{DbError, io_error};

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
/// enforced, and brings its schema up to date. `migrations[n]` takes the
/// schema from version `n` to `n + 1`; the version is kept in
/// `PRAGMA user_version`. Returns whether the database is new.
pub(super) fn open(path: &Path, migrations: &[&str]) -> Result<(Connection, bool), DbError> {
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
    let created = migrate(&mut conn, path, migrations)?;
    Ok((conn, created))
}

/// Runs the migrations the database is missing. Returns whether it had
/// none yet.
pub(super) fn migrate(
    conn: &mut Connection,
    path: &Path,
    migrations: &[&str],
) -> Result<bool, DbError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version = check_version(&tx, path, migrations)?;
    let supported = migrations.len() as i64;
    for migration in &migrations[version as usize..] {
        tx.execute_batch(migration)?;
    }
    tx.pragma_update(None, "user_version", supported)?;
    tx.commit()?;
    Ok(version == 0)
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
