use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use super::{IndexError, io_error};

/// `app.db`: the project list.
pub(super) const APP_MIGRATIONS: &[&str] = &["
CREATE TABLE projects (
  id             TEXT PRIMARY KEY,
  name           TEXT NOT NULL,
  repo_path      TEXT NOT NULL UNIQUE,
  last_opened_at TEXT
);
"];

/// `projects/<project-id>.db`: the task index and in-flight activity.
///
/// A task file that fails to parse still gets a row, so the board can show
/// it: `title` and `created` are empty, `status` is `backlog`, `body` holds
/// the raw file and `parse_error` the reason. A valid file never has an
/// empty `created`, so that is how the two are told apart.
pub(super) const PROJECT_MIGRATIONS: &[&str] = &["
CREATE TABLE tasks (
  id          TEXT PRIMARY KEY,
  path        TEXT NOT NULL,
  title       TEXT NOT NULL,
  status      TEXT NOT NULL CHECK (status IN ('backlog','todo','in_progress','finished','done')),
  priority    TEXT NOT NULL DEFAULT 'medium',
  parent      TEXT,
  labels      TEXT NOT NULL DEFAULT '[]',
  depends_on  TEXT NOT NULL DEFAULT '[]',
  created     TEXT NOT NULL,
  reviewed_at TEXT,
  body        TEXT NOT NULL,
  file_mtime  INTEGER NOT NULL,
  file_size   INTEGER NOT NULL,
  file_hash   TEXT NOT NULL,
  parse_error TEXT
);
CREATE INDEX tasks_by_parent ON tasks (parent);

CREATE TABLE leases (
  task_id    TEXT PRIMARY KEY,
  agent_id   TEXT NOT NULL,
  claimed_at TEXT NOT NULL,
  expires_at TEXT NOT NULL
);

CREATE TABLE task_flags (
  task_id         TEXT PRIMARY KEY,
  blocked_reason  TEXT,
  review_feedback TEXT
);

CREATE TABLE work_log (
  id         INTEGER PRIMARY KEY,
  task_id    TEXT NOT NULL,
  agent_id   TEXT,
  kind       TEXT NOT NULL CHECK (kind IN ('note','blocked','unblocked','feedback','event')),
  message    TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX work_log_by_task ON work_log (task_id, created_at);

CREATE TABLE agents (
  agent_id     TEXT PRIMARY KEY,
  client_name  TEXT,
  last_seen_at TEXT NOT NULL
);
"];

/// Opens or creates the database at `path` in WAL mode and brings its schema
/// up to date. `migrations[n]` takes the schema from version `n` to `n + 1`;
/// the version is kept in `PRAGMA user_version`.
pub(super) fn open(path: &Path, migrations: &[&str]) -> Result<Connection, IndexError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io_error(dir))?;
    }
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(IndexError::NoWal {
            path: path.to_owned(),
            mode,
        });
    }
    // Safe with WAL: a crash can lose the last commits, never corrupt the file.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&mut conn, path, migrations)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection, path: &Path, migrations: &[&str]) -> Result<(), IndexError> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let supported = migrations.len() as i64;
    if !(0..=supported).contains(&version) {
        return Err(IndexError::NewerSchema {
            path: path.to_owned(),
            version,
            supported,
        });
    }
    for migration in &migrations[version as usize..] {
        tx.execute_batch(migration)?;
    }
    tx.pragma_update(None, "user_version", supported)?;
    tx.commit()?;
    Ok(())
}
