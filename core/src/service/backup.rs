//! Backing up a project's database and restoring a backup, which only you
//! can do. See `db::backup` for where backups live and how long they stay.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::atomic::Ordering;

use rusqlite::Connection;

use super::{Service, ServiceError, user_row};
use crate::db::backup::{self, Backup};
use crate::db::{Changes, millis};
use crate::model::TaskId;

impl Service {
    /// Copies the project's database into a new backup in `dir`.
    pub fn back_up(&self, dir: &Path) -> Result<Backup, ServiceError> {
        let db = self.lock();
        let now = (self.config.clock)();
        Ok(backup::write(db.connection(), dir, now)?)
    }

    /// Replaces every task, and all their history, with the backup in `dir`
    /// named `name`. The current state is backed up first, so a restore can
    /// be undone by restoring that backup, which is returned. Leases from
    /// the backup are dropped, and subscribers hear of every task that
    /// changed or went.
    ///
    /// Unlike other calls this is not one transaction: it replaces the
    /// database file's contents, which SQLite does as one write. Refused
    /// with `agents_working` while an agent holds a live lease, and with
    /// `invalid_argument` if there is no such backup or it isn't a project
    /// database this app can read.
    pub fn restore_backup(&self, dir: &Path, name: &str) -> Result<Backup, ServiceError> {
        let mut db = self.lock();
        let now = (self.config.clock)();
        let source = backup::find(dir, name)?
            .ok_or_else(|| ServiceError::InvalidArgument(format!("there is no backup `{name}`")))?;
        backup::check(&source.path)
            .map_err(|e| ServiceError::InvalidArgument(format!("cannot restore `{name}`: {e}")))?;
        let working = live_leases(db.connection(), millis(now))?;
        if !working.is_empty() {
            return Err(ServiceError::AgentsWorking(working));
        }

        let before = board_ids(db.connection())?;
        let undo = backup::write(db.connection(), dir, now)?;
        db.restore_from(&source.path)?;
        let conn = db.connection();
        conn.execute("DELETE FROM leases", [])?;
        self.user
            .store(user_row(conn, &self.config)?, Ordering::Relaxed);
        let after = board_ids(conn)?;

        self.emit(Changes {
            removed: before.difference(&after).copied().collect(),
            changed: after,
        });
        Ok(undo)
    }
}

/// The tasks with a live lease, by id.
fn live_leases(conn: &Connection, now_ms: i64) -> Result<Vec<TaskId>, ServiceError> {
    let mut stmt = conn.prepare(
        "SELECT t.id FROM leases l JOIN tasks t ON t.seq = l.task
         WHERE l.expires_at > ?1 ORDER BY t.id",
    )?;
    let ids = stmt.query_map([now_ms], |row| row.get(0))?;
    Ok(ids.collect::<Result<_, _>>()?)
}

/// The tasks on the board, deleted ones left out.
fn board_ids(conn: &Connection) -> Result<BTreeSet<TaskId>, ServiceError> {
    let mut stmt = conn.prepare("SELECT id FROM tasks WHERE deleted_at IS NULL")?;
    let ids = stmt.query_map([], |row| row.get(0))?;
    Ok(ids.collect::<Result<_, _>>()?)
}
