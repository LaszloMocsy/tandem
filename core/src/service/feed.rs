//! The change feed, which lets every process that shares a project's
//! database hear of each other's changes. Every call that changes tasks
//! appends their ids to the `changes` table in its own transaction. Each
//! service remembers the last row it has seen, and sends subscribers the
//! tasks of the rows after it: before its own changes in a call, and from
//! a watcher thread that polls `PRAGMA data_version` while it is idle.
//!
//! A service that falls so far behind that rows it hasn't seen were pruned
//! hears of every task instead. It also remembers every task it has sent
//! or found at the start, so that it can still tell subscribers of those
//! that were cleaned up meanwhile, which have no rows left to find.

use std::collections::BTreeSet;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard, PoisonError, Weak};
use std::thread;
use std::time::Duration;

use jiff::SignedDuration;
use rusqlite::{Connection, OptionalExtension, params};

use super::ServiceError;
use crate::db::{Changes, DbError, ProjectDb};
use crate::model::TaskId;

/// How long rows stay in `changes`. A process that falls further behind
/// than that hears of every task instead.
pub const CHANGES_KEPT: SignedDuration = SignedDuration::from_hours(24);

/// A project's database, with how far its service has read the feed.
pub(super) struct State {
    pub db: ProjectDb,
    /// The last row of `changes` sent to subscribers, or written by this
    /// service.
    pub last_change: i64,
    /// `PRAGMA data_version` when the watcher last looked.
    pub data_version: i64,
    /// Every task subscribers may know of: those there were at the start,
    /// and those sent since, less those found cleaned up.
    pub known: BTreeSet<TaskId>,
}

impl State {
    pub fn new(
        db: ProjectDb,
        last_change: i64,
        known: BTreeSet<TaskId>,
    ) -> Result<Self, ServiceError> {
        let data_version = db.data_version()?;
        Ok(Self {
            db,
            last_change,
            data_version,
            known,
        })
    }

    /// The changes other connections committed since the service last
    /// looked, if any did.
    fn poll(&mut self) -> Result<Changes, ServiceError> {
        let data_version = self.db.data_version()?;
        if data_version == self.data_version {
            return Ok(Changes::default());
        }
        let tx = self.db.read()?;
        let (changes, last) = since(&tx, self.last_change, &self.known)?;
        drop(tx);
        self.last_change = last;
        self.data_version = data_version;
        note(self.db.connection(), &mut self.known, &changes);
        Ok(changes)
    }
}

/// The channels [`Service::subscribe`](super::Service::subscribe) handed
/// out.
#[derive(Default)]
pub(super) struct Subscribers(Mutex<Vec<Sender<Changes>>>);

impl Subscribers {
    pub fn add(&self, tx: Sender<Changes>) {
        self.lock().push(tx);
    }

    /// Sends `changes` to every subscriber, dropping those that have gone.
    pub fn emit(&self, changes: Changes) {
        if changes.is_empty() {
            return;
        }
        self.lock().retain(|tx| tx.send(changes.clone()).is_ok());
    }

    /// Drops every channel, which ends each subscriber's iteration.
    fn close(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Sender<Changes>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Starts the thread that sends subscribers what other connections change
/// while the service is idle. It checks every `interval`, and ends with
/// the service. If a newer app takes the database over, it drops every
/// subscriber's channel and ends.
pub(super) fn watch(
    state: Weak<Mutex<State>>,
    subscribers: Weak<Subscribers>,
    interval: Duration,
) -> std::io::Result<()> {
    thread::Builder::new()
        .name("tandem-watch".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(interval);
                let (Some(state), Some(subscribers)) = (state.upgrade(), subscribers.upgrade())
                else {
                    return;
                };
                // Sent under the lock, like a call's changes, so subscribers
                // see changes in commit order.
                let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                match state.poll() {
                    Ok(changes) => subscribers.emit(changes),
                    Err(ServiceError::Db(e)) if matches!(*e, DbError::NewerSchema { .. }) => {
                        subscribers.close();
                        return;
                    }
                    // Most likely the database is busy; try again next time.
                    Err(_) => {}
                }
            }
        })?;
    Ok(())
}

/// The last row `changes` has had, even if it was pruned since.
pub(super) fn latest(conn: &Connection) -> Result<i64, ServiceError> {
    Ok(conn
        .query_row(
            "SELECT seq FROM sqlite_sequence WHERE name = 'changes'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

/// Every task in the database, deleted ones included.
pub(super) fn task_ids(conn: &Connection) -> Result<BTreeSet<TaskId>, ServiceError> {
    Ok(conn
        .prepare("SELECT id FROM tasks")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?)
}

/// The tasks named in `changes` after row `last`, as they are now: changed
/// if they are on the board, removed if deleted or cleaned up. Returns the
/// last row too. If rows after `last` were pruned before they were seen,
/// every task counts, and every one in `known`.
pub(super) fn since(
    conn: &Connection,
    last: i64,
    known: &BTreeSet<TaskId>,
) -> Result<(Changes, i64), ServiceError> {
    let (first, newest): (Option<i64>, Option<i64>) =
        conn.query_row("SELECT min(seq), max(seq) FROM changes", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    let newest = match newest {
        Some(newest) if newest > last => newest,
        _ => return Ok((Changes::default(), last)),
    };
    let ids: BTreeSet<TaskId> = if first.is_some_and(|first| first > last + 1) {
        let mut ids = task_ids(conn)?;
        ids.extend(known);
        ids
    } else {
        conn.prepare_cached("SELECT DISTINCT task FROM changes WHERE seq > ?1")?
            .query_map([last], |row| row.get(0))?
            .collect::<Result<_, _>>()?
    };
    let mut changes = Changes::default();
    let mut on_board = conn.prepare_cached("SELECT deleted_at IS NULL FROM tasks WHERE id = ?1")?;
    for id in ids {
        let shown: Option<bool> = on_board.query_row([id], |row| row.get(0)).optional()?;
        if shown == Some(true) {
            changes.changed.insert(id);
        } else {
            changes.removed.insert(id);
        }
    }
    Ok((changes, newest))
}

/// Adds the tasks in `changes`, just sent to subscribers, to `known`, and
/// drops those that were cleaned up.
pub(super) fn note(conn: &Connection, known: &mut BTreeSet<TaskId>, changes: &Changes) {
    known.extend(&changes.changed);
    for id in &changes.removed {
        let exists = conn
            .prepare_cached("SELECT 1 FROM tasks WHERE id = ?1")
            .and_then(|mut stmt| stmt.exists([id]));
        // Kept if in doubt; it is only sent as removed again.
        if matches!(exists, Ok(false)) {
            known.remove(id);
        } else {
            known.insert(*id);
        }
    }
}

/// Appends a row for every task in `changes`. Returns the last row, or
/// `None` if there were none to add.
pub(super) fn record(
    conn: &Connection,
    changes: &Changes,
    at_ms: i64,
) -> Result<Option<i64>, ServiceError> {
    if changes.is_empty() {
        return Ok(None);
    }
    let mut insert = conn.prepare_cached("INSERT INTO changes (task, at) VALUES (?1, ?2)")?;
    for id in changes.changed.iter().chain(&changes.removed) {
        insert.execute(params![id, at_ms])?;
    }
    Ok(Some(conn.last_insert_rowid()))
}

/// Makes the next row come after `seq`, as after a restore, which brings
/// back the feed of an older state.
pub(super) fn resume_after(conn: &Connection, seq: i64) -> Result<(), ServiceError> {
    let updated = conn.execute(
        "UPDATE sqlite_sequence SET seq = max(seq, ?1) WHERE name = 'changes'",
        [seq],
    )?;
    if updated == 0 {
        conn.execute(
            "INSERT INTO sqlite_sequence (name, seq) VALUES ('changes', ?1)",
            [seq],
        )?;
    }
    Ok(())
}

/// Removes the rows older than [`CHANGES_KEPT`] before `now_ms`.
pub(super) fn prune(conn: &Connection, now_ms: i64) -> Result<(), ServiceError> {
    conn.execute(
        "DELETE FROM changes WHERE at < ?1",
        [now_ms - CHANGES_KEPT.as_millis() as i64],
    )?;
    Ok(())
}
