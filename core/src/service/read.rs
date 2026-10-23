//! `list_tasks`, `get_task`, `next_task` and the Agents panel.

use std::collections::HashMap;

use jiff::{SignedDuration, Timestamp};
use rusqlite::Connection;
use serde_json::Value;

use super::rows::{self, TaskRow};
use super::{
    Actor, AgentInfo, ChangelogItem, HeldTask, Lease, LogEntry, LogKind, REOPENED, Service,
    ServiceError, TaskContent, TaskDetail, TaskFilter, TaskSummary, date_in, lease,
};
use crate::db::{from_millis, millis, timestamp_text};
use crate::markdown::{Entry, TaskDoc};
use crate::model::{Criterion, Status, TaskId, Unmet, UnmetCriterion};

impl Service {
    /// Summaries of the tasks that match `filter`, sorted by id. Deleted
    /// tasks are left out.
    pub fn list_tasks(
        &self,
        actor: &Actor,
        filter: &TaskFilter,
    ) -> Result<Vec<TaskSummary>, ServiceError> {
        self.call(actor, |call| {
            let board = Board::load(call.tx, call.now, filter.query.is_some())?;
            Ok(board
                .tasks
                .iter()
                .filter(|task| board.matches(task, filter))
                .map(|task| board.summary(self, task))
                .collect())
        })
    }

    /// The agents seen within `seen_within`, and any that still hold a
    /// lease, the most recently seen first, each with the tasks it holds.
    /// Every `start_task` issues a new agent id, so older ones drop off.
    pub fn list_agents(&self, seen_within: SignedDuration) -> Result<Vec<AgentInfo>, ServiceError> {
        let db = self.lock();
        let conn = db.connection();
        let now = (self.config.clock)();

        let mut held: HashMap<i64, Vec<HeldTask>> = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT l.actor, t.id, l.expires_at, t.title
             FROM leases l JOIN tasks t ON t.seq = l.task
             ORDER BY t.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, TaskId>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (actor, id, expires_at, title) = row?;
            held.entry(actor).or_default().push(HeldTask {
                id,
                title,
                live: expires_at > millis(now),
            });
        }

        let mut stmt = conn.prepare(
            "SELECT id, name, client, last_seen FROM actors
             WHERE kind = 'agent' AND (last_seen >= ?1 OR id IN (SELECT actor FROM leases))
             ORDER BY last_seen DESC, name",
        )?;
        let agents = stmt.query_map([millis(now - seen_within)], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                AgentInfo {
                    agent_id: row.get(1)?,
                    client_name: row.get(2)?,
                    last_seen_at: timestamp_text(from_millis(row.get(3)?)),
                    tasks: Vec::new(),
                },
            ))
        })?;
        agents
            .map(|row| {
                let (id, mut agent) = row?;
                agent.tasks = held.remove(&id).unwrap_or_default();
                Ok(agent)
            })
            .collect()
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
            let board = Board::load(call.tx, call.now, false)?;
            Ok(board
                .tasks
                .iter()
                .filter(|task| board.matches(task, &filter))
                .min_by_key(|task| (task.priority.rank(), task.created_at, task.id))
                .map(|task| board.summary(self, task)))
        })
    }

    /// The task with its parent and children, its whole history, and your
    /// latest reopen feedback.
    pub fn get_task(&self, actor: &Actor, id: TaskId) -> Result<TaskDetail, ServiceError> {
        self.call(actor, |call| {
            let board = Board::load(call.tx, call.now, false)?;
            let task = board.get(id).ok_or(ServiceError::NotFound(id))?;
            let (description, relevant_files) = rows::details(call.tx, task.seq)?;
            let criteria = rows::criteria(call.tx, task.seq)?;
            let events = events(call.tx, task.seq)?;

            let tz = &self.config.time_zone;
            let changelog: Vec<&Event> = events.iter().filter(|e| is_changelog(&e.kind)).collect();
            let entries: Vec<Entry> = changelog.iter().map(|event| event.entry(tz)).collect();
            let doc = TaskDoc {
                id,
                title: task.title.clone(),
                status: task.status,
                priority: Some(task.priority),
                parent: task.parent,
                labels: task.labels.clone(),
                depends_on: task.depends_on.clone(),
                created: date_in(tz, task.created_at),
                reviewed_at: task.reviewed_at.map(|ms| date_in(tz, ms)),
                description: description.clone(),
                criteria: criteria.clone(),
                relevant_files: relevant_files.clone(),
                changelog: entries.clone(),
            };
            let work_log = events
                .iter()
                .filter_map(|event| {
                    LogKind::of_event(&event.kind).map(|kind| LogEntry {
                        agent_id: (event.actor_kind == "agent").then(|| event.who.clone()),
                        kind,
                        message: event.summary.clone(),
                        created_at: timestamp_text(from_millis(event.at)),
                    })
                })
                .collect();
            let review_feedback = events
                .iter()
                .rev()
                .find(|event| event.kind == REOPENED)
                .map(|event| event.summary.clone());

            Ok(TaskDetail {
                summary: board.summary(self, task),
                markdown: doc.to_markdown(),
                parent: task
                    .parent
                    .and_then(|parent| board.get(parent))
                    .map(|parent| board.summary(self, parent)),
                children: board
                    .tasks
                    .iter()
                    .filter(|child| child.parent == Some(id))
                    .map(|child| board.summary(self, child))
                    .collect(),
                work_log,
                review_feedback,
                content: TaskContent {
                    description,
                    criteria,
                    relevant_files,
                    changelog: entries
                        .iter()
                        .zip(&changelog)
                        .map(|(entry, event)| ChangelogItem {
                            date: date_in(tz, event.at),
                            who: entry.who.clone(),
                            event: entry.event.clone(),
                            text: entry.text(),
                        })
                        .collect(),
                    reviewed_at: task.reviewed_at.map(|ms| date_in(tz, ms)),
                },
            })
        })
    }
}

/// Whether an event belongs in the changelog: all but working-log notes
/// and blocked flags.
fn is_changelog(kind: &str) -> bool {
    !matches!(kind, "note" | "blocked" | "unblocked")
}

/// One row of `events`, with its actor.
struct Event {
    at: i64,
    actor_kind: String,
    who: String,
    kind: String,
    summary: String,
    data: Option<Value>,
}

impl Event {
    /// The event as a changelog entry.
    fn entry(&self, tz: &jiff::tz::TimeZone) -> Entry {
        let data = self.data.as_ref();
        let files = data
            .and_then(|d| d.get("files"))
            .and_then(Value::as_array)
            .map(|files| {
                files
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Entry {
            date: date_in(tz, self.at).to_string(),
            who: self.who.clone(),
            event: self.kind.clone(),
            summary: self.summary.clone(),
            files,
            follow_ups: data
                .and_then(|d| d.get("follow_ups"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            unmet: data.map(unmet_of).unwrap_or_default(),
        }
    }
}

/// The `unmet` list of a `finished` event's data.
fn unmet_of(data: &Value) -> Vec<Unmet> {
    data.get("unmet")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(Unmet {
                        criterion: u32::try_from(item.get("criterion")?.as_u64()?).ok()?,
                        reason: item.get("reason")?.as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The task's events, oldest first.
fn events(conn: &Connection, seq: i64) -> Result<Vec<Event>, ServiceError> {
    let mut stmt = conn.prepare_cached(
        "SELECT e.at, a.kind, a.name, e.kind, e.summary, e.data
         FROM events e JOIN actors a ON a.id = e.actor
         WHERE e.task = ?1 ORDER BY e.id",
    )?;
    let rows = stmt.query_map([seq], |row| {
        Ok(Event {
            at: row.get(0)?,
            actor_kind: row.get(1)?,
            who: row.get(2)?,
            kind: row.get(3)?,
            summary: row.get(4)?,
            data: row
                .get::<_, Option<String>>(5)?
                .and_then(|data| serde_json::from_str(&data).ok()),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// The tasks with their leases, flags and unmet criteria, for one call.
struct Board {
    tasks: Vec<TaskRow>,
    by_id: HashMap<TaskId, usize>,
    leases: HashMap<i64, Lease>,
    /// For tasks in `finished`: the criteria the latest `finished` event
    /// left unmet and that are still unticked.
    unmet: HashMap<i64, Vec<UnmetCriterion>>,
    /// Lowercase descriptions, when a text query needs them.
    descriptions: HashMap<i64, String>,
}

impl Board {
    fn load(
        conn: &Connection,
        now: Timestamp,
        with_descriptions: bool,
    ) -> Result<Self, ServiceError> {
        let tasks = rows::task_rows(conn, None)?;
        let by_id = tasks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();

        let mut stmt = conn.prepare_cached(
            "SELECT l.task, a.name, l.claimed_at, l.expires_at
             FROM leases l JOIN actors a ON a.id = l.actor",
        )?;
        let leases = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    lease(row.get(1)?, row.get(2)?, row.get(3)?, now),
                ))
            })?
            .collect::<Result<_, _>>()?;

        let mut unmet: HashMap<i64, Vec<UnmetCriterion>> = HashMap::new();
        let mut stmt = conn.prepare_cached(
            "SELECT e.task, e.data FROM events e JOIN tasks t ON t.seq = e.task
             WHERE t.status = 'finished' AND t.deleted_at IS NULL
               AND e.id = (SELECT max(id) FROM events WHERE task = e.task AND kind = 'finished')",
        )?;
        let finished = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (seq, data) in finished {
            let listed = data
                .and_then(|data| serde_json::from_str::<Value>(&data).ok())
                .map(|data| unmet_of(&data))
                .unwrap_or_default();
            if listed.is_empty() {
                continue;
            }
            let criteria: Vec<Criterion> = rows::criteria(conn, seq)?;
            let items: Vec<UnmetCriterion> = listed
                .into_iter()
                .filter_map(|u| {
                    let criterion = criteria
                        .iter()
                        .find(|c| c.number == u.criterion && !c.met)?;
                    Some(UnmetCriterion {
                        number: u.criterion,
                        text: criterion.text.clone(),
                        reason: u.reason,
                    })
                })
                .collect();
            unmet.insert(seq, items);
        }

        let mut descriptions = HashMap::new();
        if with_descriptions {
            let mut stmt =
                conn.prepare_cached("SELECT seq, description FROM tasks WHERE deleted_at IS NULL")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (seq, description) = row?;
                descriptions.insert(seq, description.to_lowercase());
            }
        }
        Ok(Self {
            tasks,
            by_id,
            leases,
            unmet,
            descriptions,
        })
    }

    fn get(&self, id: TaskId) -> Option<&TaskRow> {
        self.by_id.get(&id).map(|&i| &self.tasks[i])
    }

    fn summary(&self, service: &Service, task: &TaskRow) -> TaskSummary {
        TaskSummary {
            id: task.id,
            title: task.title.clone(),
            status: task.status,
            priority: task.priority,
            parent: task.parent,
            labels: task.labels.clone(),
            depends_on: task.depends_on.clone(),
            created: service.date(task.created_at),
            lease: self.leases.get(&task.seq).cloned(),
            blocked: task.blocked.clone(),
            unmet: self.unmet.get(&task.seq).cloned().unwrap_or_default(),
        }
    }

    fn is_ready(&self, task: &TaskRow) -> bool {
        task.status == Status::Todo
            && task
                .depends_on
                .iter()
                .all(|&dep| self.get(dep).is_some_and(|dep| dep.status == Status::Done))
            && !self.leases.get(&task.seq).is_some_and(|lease| lease.live)
    }

    fn matches(&self, task: &TaskRow, filter: &TaskFilter) -> bool {
        filter.status.is_none_or(|status| task.status == status)
            && filter
                .label
                .as_ref()
                .is_none_or(|label| task.labels.iter().any(|l| l.eq_ignore_ascii_case(label)))
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
                    || self
                        .descriptions
                        .get(&task.seq)
                        .is_some_and(|d| d.contains(&query))
            })
            && (!filter.ready_only || self.is_ready(task))
    }
}
