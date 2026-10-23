//! Reading and writing task rows: the SQL every service call shares.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::ServiceError;
use crate::graph::Graph;
use crate::model::{Criterion, Priority, Status, TaskId};

/// A task that isn't deleted, with what the board shows of it. The
/// description, criteria and relevant files are read separately.
#[derive(Debug, Clone)]
pub(super) struct TaskRow {
    pub seq: i64,
    pub id: TaskId,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub parent: Option<TaskId>,
    /// Sorted by name, ignoring case.
    pub labels: Vec<String>,
    /// Sorted by id.
    pub depends_on: Vec<TaskId>,
    pub created_at: i64,
    pub reviewed_at: Option<i64>,
    pub blocked: Option<String>,
}

/// Every task that isn't deleted, sorted by id; or only the one with
/// internal key `only`.
pub(super) fn task_rows(
    conn: &Connection,
    only: Option<i64>,
) -> Result<Vec<TaskRow>, ServiceError> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.seq, t.id, t.title, t.status, t.priority, p.id,
                t.created_at, t.reviewed_at, t.blocked_reason
         FROM tasks t LEFT JOIN tasks p ON p.seq = t.parent
         WHERE t.deleted_at IS NULL AND (?1 IS NULL OR t.seq = ?1)
         ORDER BY t.id",
    )?;
    let mut tasks: Vec<TaskRow> = stmt
        .query_map([only], |row| {
            Ok(TaskRow {
                seq: row.get(0)?,
                id: row.get(1)?,
                title: row.get(2)?,
                status: row.get(3)?,
                priority: row.get(4)?,
                parent: row.get(5)?,
                labels: Vec::new(),
                depends_on: Vec::new(),
                created_at: row.get(6)?,
                reviewed_at: row.get(7)?,
                blocked: row.get(8)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let index: HashMap<i64, usize> = tasks.iter().enumerate().map(|(i, t)| (t.seq, i)).collect();

    let mut stmt = conn.prepare_cached(
        "SELECT tl.task, l.name FROM task_labels tl JOIN labels l ON l.id = tl.label
         WHERE ?1 IS NULL OR tl.task = ?1
         ORDER BY l.name COLLATE NOCASE",
    )?;
    let labels = stmt.query_map([only], |row| Ok((row.get::<_, i64>(0)?, row.get(1)?)))?;
    for row in labels {
        let (task, label) = row?;
        if let Some(&i) = index.get(&task) {
            tasks[i].labels.push(label);
        }
    }

    let mut stmt = conn.prepare_cached(
        "SELECT d.task, t.id FROM task_deps d JOIN tasks t ON t.seq = d.depends_on
         WHERE ?1 IS NULL OR d.task = ?1
         ORDER BY t.id",
    )?;
    let deps = stmt.query_map([only], |row| Ok((row.get::<_, i64>(0)?, row.get(1)?)))?;
    for row in deps {
        let (task, dep) = row?;
        if let Some(&i) = index.get(&task) {
            tasks[i].depends_on.push(dep);
        }
    }
    Ok(tasks)
}

/// The task with this id, unless it is missing or deleted.
pub(super) fn task_row(conn: &Connection, id: TaskId) -> Result<TaskRow, ServiceError> {
    let seq = live_seq(conn, id)?.ok_or(ServiceError::NotFound(id))?;
    task_rows(conn, Some(seq))?
        .pop()
        .ok_or(ServiceError::NotFound(id))
}

/// The internal key of the task with this id, unless it is missing or
/// deleted.
pub(super) fn live_seq(conn: &Connection, id: TaskId) -> Result<Option<i64>, ServiceError> {
    Ok(conn
        .prepare_cached("SELECT seq FROM tasks WHERE id = ?1 AND deleted_at IS NULL")?
        .query_row([id], |row| row.get(0))
        .optional()?)
}

/// The parent and dependency graph of the tasks that aren't deleted.
pub(super) fn graph(conn: &Connection) -> Result<Graph, ServiceError> {
    let mut graph = Graph::new();
    for task in task_rows(conn, None)? {
        graph.insert(task.id, task.parent, task.depends_on);
    }
    Ok(graph)
}

/// The task's description, and its relevant files.
pub(super) fn details(conn: &Connection, seq: i64) -> Result<(String, Vec<String>), ServiceError> {
    let (description, files): (String, String) = conn
        .prepare_cached("SELECT description, relevant_files FROM tasks WHERE seq = ?1")?
        .query_row([seq], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok((
        description,
        serde_json::from_str(&files).unwrap_or_default(),
    ))
}

/// The task's acceptance criteria, in order.
pub(super) fn criteria(conn: &Connection, seq: i64) -> Result<Vec<Criterion>, ServiceError> {
    let mut stmt = conn.prepare_cached(
        "SELECT position, met, text FROM criteria WHERE task = ?1 ORDER BY position",
    )?;
    let rows = stmt.query_map([seq], |row| {
        Ok(Criterion {
            number: row.get(0)?,
            met: row.get(1)?,
            text: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Replaces the task's labels, adding any label not used before.
pub(super) fn set_labels(
    conn: &Connection,
    seq: i64,
    labels: &[String],
) -> Result<(), ServiceError> {
    conn.prepare_cached("DELETE FROM task_labels WHERE task = ?1")?
        .execute([seq])?;
    for label in labels {
        conn.prepare_cached("INSERT INTO labels (name) VALUES (?1) ON CONFLICT DO NOTHING")?
            .execute([label])?;
        conn.prepare_cached(
            "INSERT OR IGNORE INTO task_labels (task, label)
             SELECT ?1, id FROM labels WHERE name = ?2",
        )?
        .execute(params![seq, label])?;
    }
    Ok(())
}

/// Replaces the task's dependencies. Each must be a task that isn't
/// deleted; the graph check has made sure of that.
pub(super) fn set_depends_on(
    conn: &Connection,
    seq: i64,
    depends_on: &[TaskId],
) -> Result<(), ServiceError> {
    conn.prepare_cached("DELETE FROM task_deps WHERE task = ?1")?
        .execute([seq])?;
    for &dep in depends_on {
        let target = live_seq(conn, dep)?.ok_or(ServiceError::NotFound(dep))?;
        conn.prepare_cached("INSERT OR IGNORE INTO task_deps (task, depends_on) VALUES (?1, ?2)")?
            .execute([seq, target])?;
    }
    Ok(())
}

/// Sets the task's parent; `None` moves it to the top level.
pub(super) fn set_parent(
    conn: &Connection,
    seq: i64,
    parent: Option<TaskId>,
) -> Result<(), ServiceError> {
    let parent = match parent {
        Some(id) => Some(live_seq(conn, id)?.ok_or(ServiceError::NotFound(id))?),
        None => None,
    };
    conn.prepare_cached("UPDATE tasks SET parent = ?2 WHERE seq = ?1")?
        .execute(params![seq, parent])?;
    Ok(())
}

/// Replaces the task's acceptance criteria.
pub(super) fn set_criteria(
    conn: &Connection,
    seq: i64,
    criteria: &[(String, bool)],
) -> Result<(), ServiceError> {
    conn.prepare_cached("DELETE FROM criteria WHERE task = ?1")?
        .execute([seq])?;
    for (i, (text, met)) in criteria.iter().enumerate() {
        conn.prepare_cached(
            "INSERT INTO criteria (task, position, text, met) VALUES (?1, ?2, ?3, ?4)",
        )?
        .execute(params![seq, i as i64 + 1, text, met])?;
    }
    Ok(())
}

/// Adds an event to the task's timeline.
pub(super) fn add_event(
    conn: &Connection,
    seq: i64,
    actor: i64,
    at: i64,
    kind: &str,
    summary: &str,
    data: Option<&Value>,
) -> Result<(), ServiceError> {
    conn.prepare_cached(
        "INSERT INTO events (task, at, actor, kind, summary, data) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        seq,
        at,
        actor,
        kind,
        summary,
        data.map(Value::to_string)
    ])?;
    Ok(())
}

/// The latest event of the task that hands it over (`started`,
/// `finished`, `released` or `reopened`): its kind, and the actor's kind
/// and name.
pub(super) fn last_hand_over(
    conn: &Connection,
    seq: i64,
) -> Result<Option<(String, String, String)>, ServiceError> {
    Ok(conn
        .prepare_cached(
            "SELECT e.kind, a.kind, a.name FROM events e JOIN actors a ON a.id = e.actor
             WHERE e.task = ?1 AND e.kind IN ('started','finished','released','reopened')
             ORDER BY e.id DESC LIMIT 1",
        )?
        .query_row([seq], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .optional()?)
}
