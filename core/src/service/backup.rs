//! Backing up a project's database and restoring a backup, which only you
//! can do. See `db::backup` for where backups live and how long they stay.

use std::collections::BTreeSet;
use std::path::Path;

use jiff::SignedDuration;
use rusqlite::Connection;

use super::feed::{self, State};
use super::{Service, ServiceError, user_row};
use crate::db::backup::{self, Backup};
use crate::db::{Changes, millis};
use crate::model::TaskId;

/// How long a restore keeps every other call out at most, in case it never
/// finishes, such as when the app quits halfway.
pub const RESTORE_TIMEOUT: SignedDuration = SignedDuration::from_mins(1);

impl Service {
    /// Copies the project's database into a new backup in `dir`, after
    /// pruning the change feed. Refused with `restoring` while a backup is
    /// being restored.
    pub fn back_up(&self, dir: &Path) -> Result<Backup, ServiceError> {
        let mut state = self.lock();
        let now = (self.config.clock)();
        let tx = state.db.begin()?;
        check_not_restoring(&tx, millis(now))?;
        feed::prune(&tx, millis(now))?;
        tx.commit()?;
        Ok(backup::write(state.db.connection(), dir, now)?)
    }

    /// Replaces every task, and all their history, with the backup in `dir`
    /// named `name`. The current state is backed up first, so a restore can
    /// be undone by restoring that backup, which is returned. Leases from
    /// the backup are dropped, and subscribers hear of every task that
    /// changed or went.
    ///
    /// Unlike other calls this is not one transaction, but nothing else
    /// gets in: the transaction that checks for agents also marks the
    /// database as restoring, which refuses every other call, in any
    /// process, with `restoring` until the copy, which clears the mark,
    /// replaces the database's contents in one write. Refused with
    /// `agents_working` while an agent holds a live lease, with `restoring`
    /// during another restore, and with `invalid_argument` if there is no
    /// such backup or it isn't a project database this app can read.
    pub fn restore_backup(&self, dir: &Path, name: &str) -> Result<Backup, ServiceError> {
        let mut state = self.lock();
        let now = (self.config.clock)();
        let source = backup::find(dir, name)?
            .ok_or_else(|| ServiceError::InvalidArgument(format!("there is no backup `{name}`")))?;
        backup::check(&source.path)
            .map_err(|e| ServiceError::InvalidArgument(format!("cannot restore `{name}`: {e}")))?;
        let (before, seq) = {
            let tx = state.db.begin()?;
            check_not_restoring(&tx, millis(now))?;
            let working = live_leases(&tx, millis(now))?;
            if !working.is_empty() {
                return Err(ServiceError::AgentsWorking(working));
            }
            tx.execute(
                "INSERT INTO restoring (id, expires_at) VALUES (1, ?1)
                 ON CONFLICT (id) DO UPDATE SET expires_at = excluded.expires_at",
                [millis(now + RESTORE_TIMEOUT)],
            )?;
            let before = (board_ids(&tx)?, feed::latest(&tx)?);
            tx.commit()?;
            before
        };

        let restored = self.restore_marked(&mut state, dir, &source.path, before, seq);
        if restored.is_err() {
            // Let other calls back in at once. If even this fails, the mark
            // expires.
            let cleared = state.db.begin().and_then(|tx| {
                tx.execute("DELETE FROM restoring", [])?;
                Ok(tx.commit()?)
            });
            if let Err(e) = cleared {
                eprintln!("tandem: cannot end the failed restore: {e}");
            }
        }
        restored
    }

    /// The rest of [`restore_backup`](Self::restore_backup), once the
    /// database is marked as restoring.
    fn restore_marked(
        &self,
        state: &mut State,
        dir: &Path,
        source: &Path,
        before: BTreeSet<TaskId>,
        seq: i64,
    ) -> Result<Backup, ServiceError> {
        let now = (self.config.clock)();
        let undo = backup::write(state.db.connection(), dir, now)?;
        // Done to the copy before it replaces the database, so the restored
        // state arrives with all of it at once.
        let (changes, recorded, known) = state.db.restore_from(source, |tx| {
            tx.execute("DELETE FROM restoring", [])?;
            tx.execute("DELETE FROM leases", [])?;
            user_row(tx, &self.config)?;
            let after = board_ids(tx)?;
            let changes = Changes {
                removed: before.difference(&after).copied().collect(),
                changed: after,
            };
            // The backup's feed ends earlier; carry on after this one's,
            // so other processes see the restore as new rows.
            feed::resume_after(tx, seq)?;
            let recorded = feed::record(tx, &changes, millis(now))?;
            Ok::<_, ServiceError>((changes, recorded, feed::task_ids(tx)?))
        })?;
        state.last_change = recorded.unwrap_or(seq);
        state.known = known;
        self.subscribers.emit(changes);
        Ok(undo)
    }
}

/// Refuses with `restoring` while a backup is being restored.
pub(super) fn check_not_restoring(conn: &Connection, now_ms: i64) -> Result<(), ServiceError> {
    let restoring = conn
        .prepare_cached("SELECT 1 FROM restoring WHERE expires_at > ?1")?
        .exists([now_ms])?;
    if restoring {
        return Err(ServiceError::Restoring);
    }
    Ok(())
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
