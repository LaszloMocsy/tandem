//! Deleting a task, which only you can do: it removes the task file. Git
//! keeps the last committed version, and the deletion lands in your next
//! commit like any other change.

use std::fs;
use std::path::Path;

use rusqlite::Connection;

use super::{Actor, Lease, Service, ServiceError, check_folder, check_regular_file, leases};
use crate::git;
use crate::index::tasks_in;
use crate::store::{StoreError, TASK_DIR, TaskId, task_path};

/// What stands in the way of deleting a task, and what deleting it loses,
/// for the confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteCheck {
    /// Tasks whose `parent` is this task.
    pub children: Vec<TaskId>,
    /// Tasks that list this task in `depends_on`.
    pub dependents: Vec<TaskId>,
    /// The live lease on the task, while an agent is working on it.
    pub lease: Option<Lease>,
    /// Whether the latest commit has the file, so git history keeps it.
    pub committed: bool,
}

impl DeleteCheck {
    /// Whether [`Service::delete_task`] would refuse.
    pub fn blocked(&self) -> bool {
        !self.children.is_empty() || !self.dependents.is_empty() || self.lease.is_some()
    }
}

impl Service {
    /// What deleting the task would run into; see [`DeleteCheck`].
    pub fn delete_check(&self, id: TaskId) -> Result<DeleteCheck, ServiceError> {
        let index = self.lock();
        let conn = index.connection();
        check_regular_file_or_link(&task_path(&self.dir, id), id)?;
        let (children, dependents) = references(conn, id)?;
        let now = (self.config.clock)();
        let lease = leases(conn, now)?.remove(&id).filter(|lease| lease.live);
        drop(index);
        Ok(DeleteCheck {
            children,
            dependents,
            lease,
            committed: git::is_committed(self.repo(), &format!("{TASK_DIR}/{}", id.file_name())),
        })
    }

    /// Deletes the task file, at any status. Refused with `claimed_by_other`
    /// while an agent holds a live lease on the task, and with
    /// `task_referenced` while another task names it as its parent or a
    /// dependency, since that task would be left invalid or locked for good.
    /// An invalid file can be deleted too.
    pub fn delete_task(&self, id: TaskId) -> Result<(), ServiceError> {
        self.check_writable()?;
        self.call(&Actor::User, |call| {
            // A full stat scan, so links added in another editor count.
            call.scan_all()?;
            let path = task_path(call.dir, id);
            check_regular_file_or_link(&path, id)?;
            let (children, dependents) = references(call.tx, id)?;
            if !children.is_empty() || !dependents.is_empty() {
                return Err(ServiceError::Referenced {
                    task: id,
                    children,
                    dependents,
                });
            }
            if let Some(lease) = call.lease(id)?.filter(|lease| lease.live) {
                return Err(ServiceError::ClaimedByOther {
                    task: id,
                    holder: lease.agent_id,
                    expires_at: Some(lease.expires_at),
                });
            }
            check_folder(call.dir)?;
            // Removes a symlinked task file itself, never what it points at.
            fs::remove_file(&path).map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
            // Drops the index row and any stale lease, and rechecks the
            // graph, so the watcher event for the deletion changes nothing.
            call.scan_task(id)
        })
    }
}

/// `NotFound` if the task has no file. Unlike reading, a symlink or other
/// odd file is fine here: deleting removes the entry itself.
fn check_regular_file_or_link(path: &Path, id: TaskId) -> Result<(), ServiceError> {
    match check_regular_file(path, id) {
        Err(ServiceError::InvalidFile { .. }) => Ok(()),
        other => other,
    }
}

/// The tasks that name `id` as their parent, and those that depend on it,
/// each sorted by id.
fn references(conn: &Connection, id: TaskId) -> Result<(Vec<TaskId>, Vec<TaskId>), ServiceError> {
    let tasks = tasks_in(conn)?;
    let children = tasks
        .iter()
        .filter(|task| task.parent == Some(id))
        .map(|task| task.id)
        .collect();
    let dependents = tasks
        .iter()
        .filter(|task| task.depends_on.contains(&id))
        .map(|task| task.id)
        .collect();
    Ok((children, dependents))
}
