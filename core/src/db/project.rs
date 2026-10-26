use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, MAIN_DB, Transaction, TransactionBehavior};

use super::{DB_DIR, DbError, io_error, is_valid_project_id, schema};
use crate::model::TaskId;

/// Where a project's database lives:
/// `<data_dir>/db/projects/<project-id>.db`.
pub fn project_db_path(data_dir: &Path, project_id: &str) -> PathBuf {
    data_dir
        .join(DB_DIR)
        .join("projects")
        .join(format!("{project_id}.db"))
}

/// Deletes a project's database, with its WAL and shared-memory files, as
/// removing the project from the app does. A missing file is fine.
pub fn delete_project_db(data_dir: &Path, project_id: &str) -> Result<(), DbError> {
    let db_path = project_db_path(data_dir, project_id);
    for suffix in ["", "-wal", "-shm"] {
        let mut path = db_path.as_os_str().to_owned();
        path.push(suffix);
        let path = PathBuf::from(path);
        match fs::remove_file(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(io_error(&path)(e)),
            _ => {}
        }
    }
    Ok(())
}

/// What a call changed, for the board to refetch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Tasks added or changed, including a new note or flag, or a restore.
    pub changed: BTreeSet<TaskId>,
    /// Tasks deleted, or cleaned up for good.
    pub removed: BTreeSet<TaskId>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }

    /// Adds later changes: a task removed and then changed again counts as
    /// changed, and the other way round.
    pub fn merge(&mut self, later: Changes) {
        for id in later.removed {
            self.changed.remove(&id);
            self.removed.insert(id);
        }
        for id in later.changed {
            self.removed.remove(&id);
            self.changed.insert(id);
        }
    }
}

/// A project's database, `projects/<project-id>.db`.
pub struct ProjectDb {
    conn: Connection,
    path: PathBuf,
    created: bool,
}

impl ProjectDb {
    /// Opens or creates the database of project `project_id` in `data_dir`.
    pub fn open(data_dir: &Path, project_id: &str) -> Result<Self, DbError> {
        if !is_valid_project_id(project_id) {
            return Err(DbError::InvalidProjectId(project_id.to_owned()));
        }
        Self::open_at(&project_db_path(data_dir, project_id))
    }

    /// Opens or creates the database at `path`.
    pub fn open_at(path: &Path) -> Result<Self, DbError> {
        let (conn, created) = schema::open(path, schema::PROJECT_MIGRATIONS)?;
        Ok(Self {
            conn,
            path: path.to_owned(),
            created,
        })
    }

    /// Whether opening created the database.
    pub fn created(&self) -> bool {
        self.created
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Replaces the whole database with the backup at `path`. The backup is
    /// copied to a scratch file next to the database first, brought up to
    /// date, and adjusted by `prepare` in a transaction; that file is then
    /// copied over the database in one write, so a failure leaves the
    /// database as it was, and other connections see either the old state
    /// or the restored one with `prepare`'s changes.
    pub(crate) fn restore_from<T, E: From<DbError>>(
        &mut self,
        path: &Path,
        prepare: impl FnOnce(&Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut scratch = self.path.as_os_str().to_owned();
        scratch.push("-restore");
        let scratch = PathBuf::from(scratch);
        let result = self.restore_through(path, &scratch, prepare);
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let mut file = scratch.as_os_str().to_owned();
            file.push(suffix);
            // Left over at worst, and replaced by the next restore.
            let _ = fs::remove_file(PathBuf::from(file));
        }
        result
    }

    fn restore_through<T, E: From<DbError>>(
        &mut self,
        path: &Path,
        scratch: &Path,
        prepare: impl FnOnce(&Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        fs::copy(path, scratch).map_err(io_error(scratch))?;
        let mut conn = Connection::open(scratch).map_err(DbError::from)?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(DbError::from)?;
        schema::migrate(&mut conn, scratch, schema::PROJECT_MIGRATIONS)?;
        let tx = conn.transaction().map_err(DbError::from)?;
        let out = prepare(&tx)?;
        tx.commit().map_err(DbError::from)?;
        drop(conn);
        self.conn
            .restore(MAIN_DB, scratch, None::<fn(rusqlite::backup::Progress)>)
            .map_err(DbError::from)?;
        // The copy brings the scratch file's header along, which may not
        // be in WAL mode.
        self.conn
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(DbError::from)?;
        Ok(out)
    }

    /// Starts a `BEGIN IMMEDIATE` transaction, which takes the write lock
    /// at once, so checks and changes inside it can't interleave with
    /// another writer's, even in another process. Refused with
    /// [`DbError::NewerSchema`] once a newer app has migrated the database.
    pub(crate) fn begin(&mut self) -> Result<Transaction<'_>, DbError> {
        self.transaction(TransactionBehavior::Immediate)
    }

    /// Starts a read transaction, which sees one state of the database
    /// throughout, whatever other processes commit meanwhile. Refused like
    /// [`begin`](Self::begin).
    pub(crate) fn read(&mut self) -> Result<Transaction<'_>, DbError> {
        self.transaction(TransactionBehavior::Deferred)
    }

    fn transaction(&mut self, behavior: TransactionBehavior) -> Result<Transaction<'_>, DbError> {
        let tx = self.conn.transaction_with_behavior(behavior)?;
        schema::check_version(&tx, &self.path, schema::PROJECT_MIGRATIONS)?;
        Ok(tx)
    }

    /// `PRAGMA data_version`, which changes whenever another connection,
    /// in this process or another, commits to the database.
    pub(crate) fn data_version(&self) -> Result<i64, DbError> {
        Ok(self
            .conn
            .pragma_query_value(None, "data_version", |row| row.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(s: &str) -> TaskId {
        s.parse().unwrap()
    }

    #[test]
    fn merging_keeps_the_latest_state() {
        let mut changes = Changes {
            changed: [id("aaaaaaa")].into(),
            removed: [id("bbbbbbb")].into(),
        };
        changes.merge(Changes {
            changed: [id("bbbbbbb")].into(),
            removed: [id("aaaaaaa")].into(),
        });
        assert_eq!(changes.changed, [id("bbbbbbb")].into());
        assert_eq!(changes.removed, [id("aaaaaaa")].into());
    }
}
