//! Importing tasks from a `.tandem/` folder, where older versions kept one
//! Markdown file per task. The folder is only read, never changed.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use jiff::civil::Date;
use rusqlite::params;
use serde_json::json;

use super::rows;
use super::{Actor, Call, Service, ServiceError};
use crate::db::millis;
use crate::markdown::{TaskDoc, read_folder};
use crate::model::{Priority, TaskId};

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// The tasks imported, by id.
    pub imported: Vec<TaskId>,
    /// What was left out, and why: files that don't parse, tasks whose id
    /// is taken, and links to missing tasks or that would close a loop.
    pub skipped: Vec<String>,
}

impl Service {
    /// Imports every task file in `dir`, with its changelog as events.
    /// People named in a changelog are matched to you by your name; anyone
    /// else becomes an agent, so an agent that started a task in progress
    /// can carry on with its id. Imported tasks have no leases.
    pub fn import_folder(&self, dir: &Path) -> Result<ImportReport, ServiceError> {
        let folder = read_folder(dir).map_err(|e| io_error(dir, e))?;
        let mut report = ImportReport {
            skipped: folder
                .skipped
                .into_iter()
                .map(|(name, reason)| format!("{name}: {reason}"))
                .collect(),
            ..ImportReport::default()
        };
        self.call(&Actor::User, |call| {
            let tz = &self.config.time_zone;
            let start_of = |date: Date| {
                date.to_zoned(tz.clone())
                    .map(|zoned| millis(zoned.timestamp()))
                    .unwrap_or_else(|_| call.now_ms())
            };
            let mut actors: HashMap<String, i64> = HashMap::new();
            let mut seqs: Vec<(i64, &TaskDoc)> = Vec::new();

            for doc in &folder.tasks {
                let taken = call
                    .tx
                    .prepare_cached("SELECT 1 FROM tasks WHERE id = ?1")?
                    .exists([doc.id])?;
                if taken {
                    report
                        .skipped
                        .push(format!("{}: a task with this id exists already", doc.id));
                    continue;
                }
                let created_at = start_of(doc.created);
                let created_by = match doc.changelog.iter().find(|e| e.event == "created") {
                    Some(entry) => actor(call, &mut actors, &entry.who, created_at)?,
                    None => call.actor_id,
                };
                let seq: i64 = call.tx.query_row(
                    "INSERT INTO tasks (id, title, status, priority, created_by, created_at,
                                        updated_at, reviewed_at, relevant_files, description)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8, ?9) RETURNING seq",
                    params![
                        doc.id,
                        doc.title,
                        doc.status,
                        doc.priority.unwrap_or(Priority::Medium),
                        created_by,
                        created_at,
                        doc.reviewed_at.map(start_of),
                        serde_json::to_string(&doc.relevant_files).expect("strings serialize"),
                        doc.description
                    ],
                    |row| row.get(0),
                )?;
                rows::set_labels(call.tx, seq, &doc.labels)?;
                let criteria: Vec<(String, bool)> = doc
                    .criteria
                    .iter()
                    .map(|c| (c.text.clone(), c.met))
                    .collect();
                rows::set_criteria(call.tx, seq, &criteria)?;
                let mut updated_at = created_at;
                for entry in &doc.changelog {
                    let at = entry
                        .date
                        .parse::<Date>()
                        .map(start_of)
                        .unwrap_or(created_at);
                    updated_at = updated_at.max(at);
                    let who = actor(call, &mut actors, &entry.who, at)?;
                    let data = (!entry.files.is_empty()
                        || entry.follow_ups.is_some()
                        || !entry.unmet.is_empty())
                    .then(|| {
                        json!({
                            "files": entry.files,
                            "follow_ups": entry.follow_ups,
                            "unmet": entry
                                .unmet
                                .iter()
                                .map(|u| json!({ "criterion": u.criterion, "reason": u.reason }))
                                .collect::<Vec<_>>(),
                        })
                    });
                    rows::add_event(
                        call.tx,
                        seq,
                        who,
                        at,
                        &entry.event,
                        &entry.summary,
                        data.as_ref(),
                    )?;
                }
                call.tx.execute(
                    "UPDATE tasks SET updated_at = ?2 WHERE seq = ?1",
                    params![seq, updated_at],
                )?;
                seqs.push((seq, doc));
                report.imported.push(doc.id);
            }

            // Links once every task is in, keeping only those that point at
            // a task and close no loop.
            let mut graph = rows::graph(call.tx)?;
            for &(seq, doc) in &seqs {
                let mut parent = None;
                if let Some(p) = doc.parent {
                    match graph.check_change(doc.id, Some(p), &[]) {
                        Ok(()) => parent = Some(p),
                        Err(e) => report
                            .skipped
                            .push(format!("{}: parent left out: {e}", doc.id)),
                    }
                }
                let mut depends_on = Vec::new();
                for &dep in &doc.depends_on {
                    let mut with = depends_on.clone();
                    with.push(dep);
                    match graph.check_change(doc.id, None, &with) {
                        Ok(()) => depends_on = with,
                        Err(e) => report
                            .skipped
                            .push(format!("{}: dependency left out: {e}", doc.id)),
                    }
                }
                graph.insert(doc.id, parent, depends_on.clone());
                rows::set_parent(call.tx, seq, parent)?;
                rows::set_depends_on(call.tx, seq, &depends_on)?;
                call.changes.borrow_mut().changed.insert(doc.id);
            }
            Ok(())
        })?;
        Ok(report)
    }
}

/// The actor a changelog names: you if it is your name, ignoring case, and
/// otherwise an agent by that name, added if new.
fn actor(
    call: &Call<'_>,
    cache: &mut HashMap<String, i64>,
    who: &str,
    at: i64,
) -> Result<i64, ServiceError> {
    if who
        .trim()
        .eq_ignore_ascii_case(call.config.user_name.trim())
    {
        // An import is your call, so the caller is you.
        return Ok(call.actor_id);
    }
    if let Some(&id) = cache.get(who) {
        return Ok(id);
    }
    let id: i64 = call.tx.query_row(
        "INSERT INTO actors (kind, name, first_seen, last_seen) VALUES ('agent', ?1, ?2, ?2)
         ON CONFLICT (kind, name) DO UPDATE SET
           first_seen = min(first_seen, excluded.first_seen),
           last_seen = max(last_seen, excluded.last_seen)
         RETURNING id",
        params![who, at],
        |row| row.get(0),
    )?;
    cache.insert(who.to_owned(), id);
    Ok(id)
}

fn io_error(dir: &Path, source: io::Error) -> ServiceError {
    crate::db::DbError::Io {
        path: dir.to_owned(),
        source,
    }
    .into()
}
