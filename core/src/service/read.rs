//! `list_tasks`, `get_task` and `next_task`, answered from the index.

use std::collections::HashMap;
use std::fs;
use std::io;

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension};

use super::{
    Actor, Lease, LogEntry, LogKind, Service, ServiceError, TaskDetail, TaskFilter, TaskSummary,
    check_regular_file, leases,
};
use crate::index::{IndexedTask, Problem, rescan_task_in, tasks_in};
use crate::store::{Priority, Status, StoreError, TaskId, task_path};

impl Service {
    /// Summaries of the tasks that match `filter`, sorted by id.
    pub fn list_tasks(
        &self,
        actor: &Actor,
        filter: &TaskFilter,
    ) -> Result<Vec<TaskSummary>, ServiceError> {
        self.call(actor, |call| {
            let board = Board::load(call.tx, call.now)?;
            Ok(board
                .tasks
                .iter()
                .filter(|task| board.matches(task, filter))
                .map(|task| board.summary(task))
                .collect())
        })
    }

    /// The highest-priority task `start_task` would accept: in todo, every
    /// dependency done, and no live lease. Ties go to the oldest task.
    pub fn next_task(
        &self,
        actor: &Actor,
        label: Option<&str>,
        parent: Option<TaskId>,
    ) -> Result<Option<TaskSummary>, ServiceError> {
        let filter = TaskFilter {
            label: label.map(str::to_owned),
            parent,
            ready_only: true,
            ..TaskFilter::default()
        };
        self.call(actor, |call| {
            let board = Board::load(call.tx, call.now)?;
            Ok(board
                .tasks
                .iter()
                .filter(|task| board.matches(task, &filter))
                .min_by_key(|task| (priority_rank(task.priority), task.created, task.id))
                .map(|task| board.summary(task)))
        })
    }

    /// The task file, its parent and children, its working log and your
    /// latest reopen feedback.
    pub fn get_task(&self, actor: &Actor, id: TaskId) -> Result<TaskDetail, ServiceError> {
        self.call(actor, |call| {
            rescan_task_in(call.tx, call.dir, id)?;
            let board = Board::load(call.tx, call.now)?;
            let task = board.get(id).ok_or(ServiceError::NotFound(id))?;
            let path = task_path(call.dir, id);
            check_regular_file(&path, id)?;
            let markdown = match fs::read(&path) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    return Err(ServiceError::NotFound(id));
                }
                Err(source) => return Err(StoreError::Io { path, source }.into()),
            };
            Ok(TaskDetail {
                summary: board.summary(task),
                markdown,
                parent: task
                    .parent
                    .and_then(|parent| board.get(parent))
                    .map(|parent| board.summary(parent)),
                children: board
                    .tasks
                    .iter()
                    .filter(|child| child.parent == Some(id))
                    .map(|child| board.summary(child))
                    .collect(),
                work_log: work_log(call.tx, id)?,
                review_feedback: call
                    .tx
                    .query_row(
                        "SELECT review_feedback FROM task_flags WHERE task_id = ?1",
                        [id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten(),
            })
        })
    }
}

/// The index joined with the overlays, for one call.
struct Board {
    tasks: Vec<IndexedTask>,
    by_id: HashMap<TaskId, usize>,
    leases: HashMap<TaskId, Lease>,
    blocked: HashMap<TaskId, String>,
}

impl Board {
    fn load(conn: &Connection, now: Timestamp) -> Result<Self, ServiceError> {
        let tasks = tasks_in(conn)?;
        let by_id = tasks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        let mut stmt = conn.prepare(
            "SELECT task_id, blocked_reason FROM task_flags WHERE blocked_reason IS NOT NULL",
        )?;
        let blocked = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        Ok(Self {
            tasks,
            by_id,
            leases: leases(conn, now)?,
            blocked,
        })
    }

    fn get(&self, id: TaskId) -> Option<&IndexedTask> {
        self.by_id.get(&id).map(|&i| &self.tasks[i])
    }

    fn summary(&self, task: &IndexedTask) -> TaskSummary {
        TaskSummary {
            id: task.id,
            title: task.title.clone(),
            status: task.status,
            priority: task.priority,
            parent: task.parent,
            labels: task.labels.clone(),
            depends_on: task.depends_on.clone(),
            lease: self.leases.get(&task.id).cloned(),
            blocked: self.blocked.get(&task.id).cloned(),
            problem: task.problem.clone(),
        }
    }

    fn is_ready(&self, task: &IndexedTask) -> bool {
        task.status == Status::Todo
            && !matches!(task.problem, Some(Problem::InvalidFile(_)))
            && task
                .depends_on
                .iter()
                .all(|&dep| self.get(dep).is_some_and(|dep| dep.status == Status::Done))
            && !self.leases.get(&task.id).is_some_and(|lease| lease.live)
    }

    fn matches(&self, task: &IndexedTask, filter: &TaskFilter) -> bool {
        filter.status.is_none_or(|status| task.status == status)
            && filter
                .label
                .as_ref()
                .is_none_or(|label| task.labels.contains(label))
            && filter
                .parent
                .is_none_or(|parent| task.parent == Some(parent))
            && filter.query.as_ref().is_none_or(|query| {
                let query = query.to_lowercase();
                task.id.as_str().contains(&query)
                    || task.title.to_lowercase().contains(&query)
                    || task
                        .labels
                        .iter()
                        .any(|l| l.to_lowercase().contains(&query))
                    || task.body.to_lowercase().contains(&query)
            })
            && (!filter.ready_only || self.is_ready(task))
    }
}

fn priority_rank(priority: Priority) -> u8 {
    match priority {
        Priority::High => 0,
        Priority::Medium => 1,
        Priority::Low => 2,
    }
}

fn work_log(conn: &Connection, id: TaskId) -> Result<Vec<LogEntry>, ServiceError> {
    let mut stmt = conn.prepare(
        "SELECT agent_id, kind, message, created_at FROM work_log
         WHERE task_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([id], |row| {
        let kind: String = row.get(1)?;
        Ok(LogEntry {
            agent_id: row.get(0)?,
            kind: LogKind::parse(&kind).unwrap_or(LogKind::Event),
            message: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
