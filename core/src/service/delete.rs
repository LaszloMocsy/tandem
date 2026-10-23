//! Deleting a task, which only you can do. A deleted task keeps its row and
//! history, hidden from the board and the agents, and can be restored until
//! you clean up deleted tasks, which removes them for good.

use rusqlite::{Connection, params};

use super::rows::{self, TaskRow};
use super::{Actor, Lease, Service, ServiceError};
use crate::db::{from_millis, timestamp_text};
use crate::model::{Status, TaskId};

/// What stands in the way of deleting a task, for the confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteCheck {
    /// Tasks whose `parent` is this task.
    pub children: Vec<TaskId>,
    /// Tasks that list this task in `depends_on`.
    pub dependents: Vec<TaskId>,
    /// The live lease on the task, while an agent is working on it.
    pub lease: Option<Lease>,
}

impl DeleteCheck {
    /// Whether [`Service::delete_task`] would refuse.
    pub fn blocked(&self) -> bool {
        !self.children.is_empty() || !self.dependents.is_empty() || self.lease.is_some()
    }
}

/// A deleted task, which can still be restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedTask {
    pub id: TaskId,
    pub title: String,
    /// Its status when it was deleted.
    pub status: Status,
    /// In [`timestamp_text`] form.
    pub deleted_at: String,
}

impl Service {
    /// What deleting the task would run into; see [`DeleteCheck`].
    pub fn delete_check(&self, id: TaskId) -> Result<DeleteCheck, ServiceError> {
        self.call(&Actor::User, |call| {
            let task = rows::task_row(call.tx, id)?;
            let (children, dependents) = references(call.tx, &task)?;
            Ok(DeleteCheck {
                children,
                dependents,
                lease: call.lease(task.seq)?.filter(|lease| lease.live),
            })
        })
    }

    /// Deletes the task, at any status: it leaves the board and the agents'
    /// view, with a `deleted` event, until restored or cleaned up. Refused
    /// with `claimed_by_other` while an agent holds a live lease on it, and
    /// with `task_referenced` while another task names it as its parent or
    /// a dependency.
    pub fn delete_task(&self, id: TaskId) -> Result<(), ServiceError> {
        self.call(&Actor::User, |call| {
            let task = rows::task_row(call.tx, id)?;
            let (children, dependents) = references(call.tx, &task)?;
            if !children.is_empty() || !dependents.is_empty() {
                return Err(ServiceError::Referenced {
                    task: id,
                    children,
                    dependents,
                });
            }
            if let Some(lease) = call.lease(task.seq)?.filter(|lease| lease.live) {
                return Err(ServiceError::ClaimedByOther {
                    task: id,
                    holder: lease.agent_id,
                    expires_at: Some(lease.expires_at),
                });
            }
            call.release(task.seq)?;
            call.tx.execute(
                "UPDATE tasks SET deleted_at = ?2 WHERE seq = ?1",
                params![task.seq, call.now_ms()],
            )?;
            rows::add_event(
                call.tx,
                task.seq,
                call.actor_id,
                call.now_ms(),
                "deleted",
                "",
                None,
            )?;
            call.tx.execute(
                "UPDATE tasks SET updated_at = ?2 WHERE seq = ?1",
                params![task.seq, call.now_ms()],
            )?;
            call.changes.borrow_mut().removed.insert(id);
            Ok(())
        })
    }

    /// The deleted tasks, the most recently deleted first.
    pub fn deleted_tasks(&self) -> Result<Vec<DeletedTask>, ServiceError> {
        let db = self.lock();
        let mut stmt = db.connection().prepare(
            "SELECT id, title, status, deleted_at FROM tasks
             WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DeletedTask {
                id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                deleted_at: timestamp_text(from_millis(row.get(3)?)),
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Brings a deleted task back where it was, with a `restored` event.
    /// Refused with `parent_not_found` or `not_found` while its parent or
    /// a dependency is deleted itself; restore that first.
    pub fn restore_task(&self, id: TaskId) -> Result<(), ServiceError> {
        self.call(&Actor::User, |call| {
            let seq: i64 = call
                .tx
                .query_row(
                    "SELECT seq FROM tasks WHERE id = ?1 AND deleted_at IS NOT NULL",
                    [id],
                    |row| row.get(0),
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => ServiceError::NotFound(id),
                    e => e.into(),
                })?;
            let (parent, depends_on) = links(call.tx, seq)?;
            rows::graph(call.tx)?.check_change(id, parent, &depends_on)?;
            call.tx
                .execute("UPDATE tasks SET deleted_at = NULL WHERE seq = ?1", [seq])?;
            call.event(seq, id, "restored", "", None)
        })
    }

    /// Removes every deleted task for good, with its whole history, so none
    /// of them can be restored. Labels no task uses any more go too.
    /// Returns the ids removed.
    pub fn purge_deleted(&self) -> Result<Vec<TaskId>, ServiceError> {
        self.call(&Actor::User, |call| {
            let ids: Vec<TaskId> = call
                .tx
                .prepare("DELETE FROM tasks WHERE deleted_at IS NOT NULL RETURNING id")?
                .query_map([], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            call.tx.execute(
                "DELETE FROM labels WHERE id NOT IN (SELECT label FROM task_labels)",
                [],
            )?;
            call.changes
                .borrow_mut()
                .removed
                .extend(ids.iter().copied());
            Ok(ids)
        })
    }
}

/// The tasks that aren't deleted and name `task` as their parent, and those
/// that depend on it, each sorted by id.
fn references(
    conn: &Connection,
    task: &TaskRow,
) -> Result<(Vec<TaskId>, Vec<TaskId>), ServiceError> {
    let ids = |sql: &str| -> Result<Vec<TaskId>, ServiceError> {
        let mut stmt = conn.prepare_cached(sql)?;
        let rows = stmt.query_map([task.seq], |row| row.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    };
    Ok((
        ids("SELECT id FROM tasks WHERE parent = ?1 AND deleted_at IS NULL ORDER BY id")?,
        ids("SELECT t.id FROM task_deps d JOIN tasks t ON t.seq = d.task
             WHERE d.depends_on = ?1 AND t.deleted_at IS NULL ORDER BY t.id")?,
    ))
}

/// A task's parent and dependencies by id, deleted ones included.
fn links(conn: &Connection, seq: i64) -> Result<(Option<TaskId>, Vec<TaskId>), ServiceError> {
    let parent = conn.query_row(
        "SELECT p.id FROM tasks t LEFT JOIN tasks p ON p.seq = t.parent WHERE t.seq = ?1",
        [seq],
        |row| row.get(0),
    )?;
    let mut stmt = conn.prepare(
        "SELECT t.id FROM task_deps d JOIN tasks t ON t.seq = d.depends_on
         WHERE d.task = ?1 ORDER BY t.id",
    )?;
    let depends_on = stmt
        .query_map([seq], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    Ok((parent, depends_on))
}
