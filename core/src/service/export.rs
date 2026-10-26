//! Exporting a project's tasks, as a manual backup you keep yourself: JSON
//! with everything, or the Markdown task format, one file per task.

use rusqlite::Connection;
use serde_json::{Value, json};

use super::{Actor, Service, ServiceError, TaskFilter};
use crate::db::{from_millis, timestamp_text};
use crate::model::TaskId;

/// The `format` of a JSON export.
pub const EXPORT_FORMAT: &str = "tandem-tasks";

/// The `version` of the JSON export format.
pub const EXPORT_VERSION: u32 = 1;

impl Service {
    /// Every task, deleted ones included, with its criteria, links, labels
    /// and whole history, as JSON. Tasks and events refer to each other and
    /// to people by id and name, not by internal keys, so the file stands
    /// on its own. Leases are left out.
    pub fn export_json(&self) -> Result<Value, ServiceError> {
        let mut state = self.lock();
        let tx = state.db.read()?;
        let conn: &Connection = &tx;
        let now = (self.config.clock)();
        let mut stmt = conn.prepare(
            "SELECT t.seq, t.id, t.title, t.status, t.priority, p.id, t.blocked_reason,
                    a.kind, a.name, t.created_at, t.updated_at, t.reviewed_at,
                    t.deleted_at, t.relevant_files, t.description
             FROM tasks t JOIN actors a ON a.id = t.created_by
             LEFT JOIN tasks p ON p.seq = t.parent
             ORDER BY t.created_at, t.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                json!({
                    "id": row.get::<_, String>(1)?,
                    "title": row.get::<_, String>(2)?,
                    "status": row.get::<_, String>(3)?,
                    "priority": row.get::<_, String>(4)?,
                    "parent": row.get::<_, Option<String>>(5)?,
                    "blocked": row.get::<_, Option<String>>(6)?,
                    "created_by": actor(row.get(7)?, row.get(8)?),
                    "created_at": time(row.get(9)?),
                    "updated_at": time(row.get(10)?),
                    "reviewed_at": row.get::<_, Option<i64>>(11)?.map(time),
                    "deleted_at": row.get::<_, Option<i64>>(12)?.map(time),
                    "relevant_files": serde_json::from_str::<Value>(&row.get::<_, String>(13)?)
                        .unwrap_or(Value::Array(Vec::new())),
                    "description": row.get::<_, String>(14)?,
                }),
            ))
        })?;
        let mut tasks = Vec::new();
        for row in rows {
            let (seq, mut task) = row?;
            task["labels"] = strings(
                conn,
                "SELECT l.name FROM task_labels tl JOIN labels l ON l.id = tl.label
                 WHERE tl.task = ?1 ORDER BY l.name",
                seq,
            )?;
            task["depends_on"] = strings(
                conn,
                "SELECT t.id FROM task_deps d JOIN tasks t ON t.seq = d.depends_on
                 WHERE d.task = ?1 ORDER BY t.id",
                seq,
            )?;
            task["criteria"] = criteria(conn, seq)?;
            task["events"] = events(conn, seq)?;
            tasks.push(task);
        }
        Ok(json!({
            "format": EXPORT_FORMAT,
            "version": EXPORT_VERSION,
            "exported_at": time(now.as_millisecond()),
            "tasks": tasks,
        }))
    }

    /// Every task on the board in the Markdown task format, as `get_task`
    /// shows it, with its file name, `<id>.md`. Deleted tasks are left out.
    pub fn export_markdown(&self) -> Result<Vec<(String, String)>, ServiceError> {
        let ids: Vec<TaskId> = self
            .list_tasks(&Actor::User, &TaskFilter::default())?
            .into_iter()
            .map(|task| task.id)
            .collect();
        ids.into_iter()
            .map(|id| {
                let detail = self.get_task(&Actor::User, id)?;
                Ok((id.file_name(), detail.markdown))
            })
            .collect()
    }
}

fn time(ms: i64) -> String {
    timestamp_text(from_millis(ms))
}

fn actor(kind: String, name: String) -> Value {
    json!({ "kind": kind, "name": name })
}

fn strings(conn: &Connection, sql: &str, seq: i64) -> Result<Value, ServiceError> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map([seq], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?.into())
}

fn criteria(conn: &Connection, seq: i64) -> Result<Value, ServiceError> {
    let mut stmt =
        conn.prepare_cached("SELECT text, met FROM criteria WHERE task = ?1 ORDER BY position")?;
    let rows = stmt.query_map([seq], |row| {
        Ok(json!({ "text": row.get::<_, String>(0)?, "met": row.get::<_, bool>(1)? }))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?.into())
}

fn events(conn: &Connection, seq: i64) -> Result<Value, ServiceError> {
    let mut stmt = conn.prepare_cached(
        "SELECT e.at, a.kind, a.name, e.kind, e.summary, e.data
         FROM events e JOIN actors a ON a.id = e.actor WHERE e.task = ?1 ORDER BY e.id",
    )?;
    let rows = stmt.query_map([seq], |row| {
        let data: Option<String> = row.get(5)?;
        Ok(json!({
            "at": time(row.get(0)?),
            "actor": actor(row.get(1)?, row.get(2)?),
            "kind": row.get::<_, String>(3)?,
            "summary": row.get::<_, String>(4)?,
            "data": data.and_then(|data| serde_json::from_str::<Value>(&data).ok()),
        }))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?.into())
}
