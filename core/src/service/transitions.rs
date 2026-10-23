//! Every change to a task. Each transition sets the status and adds its
//! event in one transaction: `created`, `approved`, `started`, `finished`,
//! `reviewed`, `reopened`, `released`, plus `edited` and `milestone`.

use std::collections::BTreeSet;

use rusqlite::params;
use serde_json::json;

use super::rows::{self, TaskRow};
use super::{
    Actor, BLOCKED, Call, Finish, NOTE, NewTask, REOPENED, Saved, Service, ServiceError, TaskEdit,
    UNBLOCKED,
};
use crate::model::{Criterion, Priority, Status, TaskId};

impl Service {
    /// New task: in `backlog` when an agent creates it and in `todo` when
    /// you do, with a `created` event. Refuses a parent or dependency that
    /// is missing or would close a loop.
    pub fn create_task(&self, actor: &Actor, new: NewTask) -> Result<Saved, ServiceError> {
        let title = one_line("title", &new.title)?.to_owned();
        let labels = labels(new.labels)?;
        let criteria = checklist(&new.acceptance_criteria, &[])?;
        let depends_on = dedup(new.depends_on);
        let status = match actor {
            Actor::User => Status::Todo,
            Actor::Agent(_) => Status::Backlog,
        };
        self.call(actor, |call| {
            let id = unused_id(call)?;
            rows::graph(call.tx)?.check_change(id, new.parent, &depends_on)?;
            let seq: i64 = call.tx.query_row(
                "INSERT INTO tasks (id, title, status, priority, created_by, created_at,
                                    updated_at, description)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7) RETURNING seq",
                params![
                    id,
                    title,
                    status,
                    new.priority.unwrap_or(Priority::Medium),
                    call.actor_id,
                    call.now_ms(),
                    new.description.trim()
                ],
                |row| row.get(0),
            )?;
            rows::set_parent(call.tx, seq, new.parent)?;
            rows::set_labels(call.tx, seq, &labels)?;
            rows::set_depends_on(call.tx, seq, &depends_on)?;
            rows::set_criteria(call.tx, seq, &criteria)?;
            call.event(seq, id, "created", "", None)?;
            Ok(Saved { id, status })
        })
    }

    /// Edits the spec: title, description, criteria, relevant files,
    /// priority, parent, labels and dependencies. Never changes the review,
    /// and adds no event unless an agent's edit sends the task back to
    /// backlog.
    ///
    /// You may edit any task. Agents are limited, so that nothing they
    /// change gets worked on before you approve it:
    ///
    /// - In backlog, agents may edit anything.
    /// - After approval, `relevant_files`, `labels` and `priority` stay open.
    ///   Any other change to a todo task sends it back to backlog, with an
    ///   `edited` event, until you approve it again. On a task in progress,
    ///   only the lease holder may edit, and other changes are refused.
    /// - Finished and done tasks are closed to agents.
    pub fn update_task(
        &self,
        actor: &Actor,
        id: TaskId,
        edit: TaskEdit,
    ) -> Result<Saved, ServiceError> {
        self.call(actor, |call| {
            let task = rows::task_row(call.tx, id)?;
            let is_agent = matches!(actor, Actor::Agent(_));
            if is_agent {
                check_agent_access(call, &task)?;
            }
            let (description, files) = rows::details(call.tx, task.seq)?;
            let criteria = rows::criteria(call.tx, task.seq)?;

            // What the task is and when it can start.
            let title = match &edit.title {
                Some(title) => one_line("title", title)?.to_owned(),
                None => task.title.clone(),
            };
            let new_description = match &edit.description {
                Some(text) => text.trim().to_owned(),
                None => description.clone(),
            };
            let new_criteria = match &edit.acceptance_criteria {
                Some(items) => checklist(items, &criteria)?,
                None => criteria.iter().map(|c| (c.text.clone(), c.met)).collect(),
            };
            let parent = edit.parent.unwrap_or(task.parent);
            let depends_on = match edit.depends_on.clone() {
                Some(ids) => dedup(ids),
                None => task.depends_on.clone(),
            };
            let graph = rows::graph(call.tx)?;
            if edit.parent.is_some() {
                graph.check_change(id, parent, &[])?;
            }
            if edit.depends_on.is_some() {
                graph.check_change(id, None, &depends_on)?;
            }
            let mut sorted_deps = depends_on.clone();
            sorted_deps.sort_unstable();
            let old_criteria: Vec<(String, bool)> =
                criteria.iter().map(|c| (c.text.clone(), c.met)).collect();
            let spec_changed = title != task.title
                || new_description != description
                || new_criteria != old_criteria
                || parent != task.parent
                || sorted_deps != task.depends_on;

            // Details that change neither.
            let new_files = match &edit.relevant_files {
                Some(items) => one_lines("relevant file", items)?,
                None => files.clone(),
            };
            let priority = edit.priority.unwrap_or(task.priority);
            let new_labels = match edit.labels.clone() {
                Some(items) => labels(items)?,
                None => task.labels.clone(),
            };
            let details_changed = new_files != files
                || priority != task.priority
                || !same_labels(&new_labels, &task.labels);

            let mut status = task.status;
            if is_agent && spec_changed {
                match task.status {
                    Status::Backlog => {}
                    Status::Todo => status = Status::Backlog,
                    status => {
                        return Err(ServiceError::InvalidTransition {
                            task: id,
                            action: "change the spec of",
                            status,
                        });
                    }
                }
            }
            if !spec_changed && !details_changed {
                return Ok(Saved { id, status });
            }

            call.tx.execute(
                "UPDATE tasks SET title = ?2, status = ?3, priority = ?4, description = ?5,
                                  relevant_files = ?6
                 WHERE seq = ?1",
                params![
                    task.seq,
                    title,
                    status,
                    priority,
                    new_description,
                    serde_json::to_string(&new_files).expect("strings serialize")
                ],
            )?;
            if parent != task.parent {
                rows::set_parent(call.tx, task.seq, parent)?;
            }
            if sorted_deps != task.depends_on {
                rows::set_depends_on(call.tx, task.seq, &depends_on)?;
            }
            if new_criteria != old_criteria {
                rows::set_criteria(call.tx, task.seq, &new_criteria)?;
            }
            if !same_labels(&new_labels, &task.labels) {
                rows::set_labels(call.tx, task.seq, &new_labels)?;
            }
            if status != task.status {
                call.event(
                    task.seq,
                    id,
                    "edited",
                    "Changed after approval; back in backlog until approved again.",
                    None,
                )?;
            }
            call.changed(task.seq, id)?;
            Ok(Saved { id, status })
        })
    }

    /// Backlog → Todo, by you only, with an `approved` event.
    pub fn approve(&self, id: TaskId) -> Result<Saved, ServiceError> {
        self.call(&Actor::User, |call| {
            let task = rows::task_row(call.tx, id)?;
            expect_status(&task, Status::Backlog, "approve")?;
            set_status(call, &task, Status::Todo)?;
            call.event(task.seq, id, "approved", "", None)?;
            Ok(saved(&task, Status::Todo))
        })
    }

    /// Todo → In progress, with a `started` event, and a lease when an agent
    /// starts it. Runs in one `BEGIN IMMEDIATE` transaction, so of many
    /// agents starting the same task, exactly one wins.
    ///
    /// An agent may also start a task that is already in progress if no live
    /// lease is held on it: a stale lease is taken over, which the event
    /// notes. Without any lease, an agent may pick up a task you reopened,
    /// or resume one it started itself; a task you started from the board
    /// stays yours. Calling it again while holding the lease only extends
    /// it.
    pub fn start_task(&self, actor: &Actor, id: TaskId) -> Result<Saved, ServiceError> {
        self.call(actor, |call| {
            let task = rows::task_row(call.tx, id)?;
            let mut summary = String::new();
            match (actor, task.status) {
                (_, Status::Todo) => {}
                (Actor::Agent(agent), Status::InProgress) => match call.lease(task.seq)? {
                    Some(lease) if lease.agent_id == agent.id => {
                        return Ok(saved(&task, task.status));
                    }
                    Some(lease) if lease.live => {
                        return Err(ServiceError::ClaimedByOther {
                            task: id,
                            holder: lease.agent_id,
                            expires_at: Some(lease.expires_at),
                        });
                    }
                    Some(lease) => {
                        summary =
                            format!("Took over from {} after its lease expired.", lease.agent_id);
                    }
                    None => check_unleased_start(call, &task, &agent.id)?,
                },
                (_, status) => {
                    return Err(ServiceError::InvalidTransition {
                        task: id,
                        action: "start",
                        status,
                    });
                }
            }
            let pending: Vec<TaskId> = call
                .tx
                .prepare_cached(
                    "SELECT t.id FROM task_deps d JOIN tasks t ON t.seq = d.depends_on
                     WHERE d.task = ?1 AND t.status != 'done' ORDER BY t.id",
                )?
                .query_map([task.seq], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            if !pending.is_empty() {
                return Err(ServiceError::DepsNotDone { task: id, pending });
            }

            set_status(call, &task, Status::InProgress)?;
            call.event(task.seq, id, "started", &summary, None)?;
            if let Actor::Agent(_) = actor {
                call.claim(task.seq)?;
            }
            Ok(saved(&task, Status::InProgress))
        })
    }

    /// Adds a note to the working log. Only the lease holder, or you.
    pub fn log_progress(
        &self,
        actor: &Actor,
        id: TaskId,
        message: &str,
    ) -> Result<(), ServiceError> {
        let message = non_empty("message", message)?;
        self.call(actor, |call| {
            let task = rows::task_row(call.tx, id)?;
            call.check_holder(&task)?;
            call.event(task.seq, id, NOTE, message, None)
        })
    }

    /// Sets the blocked flag with a reason, or clears it when `reason` is
    /// blank, and logs either. Only the lease holder, or you.
    pub fn set_blocked(&self, actor: &Actor, id: TaskId, reason: &str) -> Result<(), ServiceError> {
        let reason = reason.trim();
        self.call(actor, |call| {
            let task = rows::task_row(call.tx, id)?;
            call.check_holder(&task)?;
            let (blocked, kind) = match reason {
                "" => (None, UNBLOCKED),
                reason => (Some(reason), BLOCKED),
            };
            call.tx.execute(
                "UPDATE tasks SET blocked_reason = ?2 WHERE seq = ?1",
                params![task.seq, blocked],
            )?;
            call.event(task.seq, id, kind, reason, None)
        })
    }

    /// A milestone event for a task in progress, before an intermediate
    /// commit. Only the lease holder, or you.
    pub fn add_changelog_entry(
        &self,
        actor: &Actor,
        id: TaskId,
        summary: &str,
        files: Vec<String>,
    ) -> Result<Saved, ServiceError> {
        let summary = non_empty("summary", summary)?;
        let files = one_lines("file", &files)?;
        self.call(actor, |call| {
            let task = rows::task_row(call.tx, id)?;
            expect_status(&task, Status::InProgress, "add a changelog entry to")?;
            call.check_holder(&task)?;
            let data = (!files.is_empty()).then(|| json!({ "files": files }));
            call.event(task.seq, id, "milestone", summary, data.as_ref())?;
            Ok(saved(&task, task.status))
        })
    }

    /// In progress → Review: ticks the `met` criteria, sets `finished`,
    /// adds a `finished` event with the summary, and releases the lease.
    /// Every criterion still unticked must be in `unmet` with a reason, or
    /// the call fails with `criteria_unmet`. Only the lease holder, or you.
    pub fn finish_task(
        &self,
        actor: &Actor,
        id: TaskId,
        finish: Finish,
    ) -> Result<Saved, ServiceError> {
        let summary = non_empty("summary", &finish.summary)?;
        let files = one_lines("file", &finish.files_changed)?;
        let follow_ups = match &finish.follow_ups {
            Some(text) => Some(one_line("follow_ups", text)?.to_owned()),
            None => None,
        };
        self.call(actor, |call| {
            let task = rows::task_row(call.tx, id)?;
            expect_status(&task, Status::InProgress, "finish")?;
            call.check_holder(&task)?;

            let met: BTreeSet<u32> = finish.met.iter().copied().collect();
            let mut unmet = BTreeSet::new();
            for item in &finish.unmet {
                one_line("unmet reason", &item.reason)?;
                if met.contains(&item.criterion) || !unmet.insert(item.criterion) {
                    return Err(invalid(format!(
                        "criterion {} is listed more than once in met and unmet",
                        item.criterion
                    )));
                }
            }
            let criteria = rows::criteria(call.tx, task.seq)?;
            for &number in met.iter().chain(&unmet) {
                let Some(criterion) = criteria.iter().find(|c| c.number == number) else {
                    return Err(invalid(format!("criterion {number} does not exist")));
                };
                if criterion.met && unmet.contains(&number) {
                    return Err(invalid(format!("criterion {number} is already ticked")));
                }
            }
            let missing: Vec<u32> = criteria
                .iter()
                .filter(|c| !c.met && !met.contains(&c.number) && !unmet.contains(&c.number))
                .map(|c| c.number)
                .collect();
            if !missing.is_empty() {
                return Err(ServiceError::CriteriaUnmet {
                    task: id,
                    criteria: missing,
                });
            }

            for &number in &met {
                call.tx
                    .prepare_cached(
                        "UPDATE criteria SET met = 1 WHERE task = ?1 AND position = ?2",
                    )?
                    .execute(params![task.seq, number])?;
            }
            set_status(call, &task, Status::Finished)?;
            let mut unmet = finish.unmet.clone();
            unmet.sort_by_key(|u| u.criterion);
            let data = json!({
                "files": files,
                "follow_ups": follow_ups,
                "unmet": unmet
                    .iter()
                    .map(|u| json!({ "criterion": u.criterion, "reason": u.reason.trim() }))
                    .collect::<Vec<_>>(),
            });
            call.event(task.seq, id, "finished", summary, Some(&data))?;
            call.release(task.seq)?;
            Ok(saved(&task, Status::Finished))
        })
    }

    /// Review → Done, by you only: sets `done` and `reviewed_at`, with a
    /// `reviewed` event.
    pub fn mark_reviewed(&self, id: TaskId) -> Result<Saved, ServiceError> {
        self.call(&Actor::User, |call| {
            let task = rows::task_row(call.tx, id)?;
            expect_status(&task, Status::Finished, "mark reviewed")?;
            set_status(call, &task, Status::Done)?;
            call.event(task.seq, id, "reviewed", "", None)?;
            Ok(saved(&task, Status::Done))
        })
    }

    /// Review → In progress, by you only, with a `reopened` event carrying
    /// your feedback, which the next agent sees.
    pub fn reopen(&self, id: TaskId, feedback: &str) -> Result<Saved, ServiceError> {
        let feedback = non_empty("feedback", feedback)?;
        self.call(&Actor::User, |call| {
            let task = rows::task_row(call.tx, id)?;
            expect_status(&task, Status::Finished, "reopen")?;
            set_status(call, &task, Status::InProgress)?;
            call.event(task.seq, id, REOPENED, feedback, None)?;
            Ok(saved(&task, Status::InProgress))
        })
    }

    /// In progress → Todo, with a `released` event carrying the note, and
    /// the lease removed. Only the lease holder, or you.
    pub fn release_task(
        &self,
        actor: &Actor,
        id: TaskId,
        note: &str,
    ) -> Result<Saved, ServiceError> {
        self.call(actor, |call| {
            let task = rows::task_row(call.tx, id)?;
            expect_status(&task, Status::InProgress, "release")?;
            call.check_holder(&task)?;
            set_status(call, &task, Status::Todo)?;
            call.event(task.seq, id, "released", note.trim(), None)?;
            call.release(task.seq)?;
            Ok(saved(&task, Status::Todo))
        })
    }
}

/// A random id no task has, deleted ones included.
fn unused_id(call: &Call<'_>) -> Result<TaskId, ServiceError> {
    loop {
        let id = TaskId::generate();
        let taken = call
            .tx
            .prepare_cached("SELECT 1 FROM tasks WHERE id = ?1")?
            .exists([id])?;
        if !taken {
            return Ok(id);
        }
    }
}

/// Whether an agent may start an in-progress task that has no lease: only
/// if you reopened it last, or the agent itself started it last.
fn check_unleased_start(call: &Call<'_>, task: &TaskRow, agent: &str) -> Result<(), ServiceError> {
    match rows::last_hand_over(call.tx, task.seq)? {
        Some((kind, _, _)) if kind == REOPENED => Ok(()),
        Some((kind, actor_kind, name))
            if kind == "started" && actor_kind == "agent" && name == agent =>
        {
            Ok(())
        }
        last => Err(ServiceError::ClaimedByOther {
            task: task.id,
            holder: last
                .filter(|(kind, _, _)| kind == "started")
                .map_or(call.config.user_name.clone(), |(_, _, name)| name),
            expires_at: None,
        }),
    }
}

/// Which tasks an agent may edit at all: not finished or done ones, and in
/// progress only with the lease.
fn check_agent_access(call: &Call<'_>, task: &TaskRow) -> Result<(), ServiceError> {
    match task.status {
        Status::Backlog | Status::Todo => Ok(()),
        Status::InProgress => call.check_holder(task),
        status @ (Status::Finished | Status::Done) => Err(ServiceError::InvalidTransition {
            task: task.id,
            action: "update",
            status,
        }),
    }
}

fn expect_status(
    task: &TaskRow,
    expected: Status,
    action: &'static str,
) -> Result<(), ServiceError> {
    if task.status == expected {
        Ok(())
    } else {
        Err(ServiceError::InvalidTransition {
            task: task.id,
            action,
            status: task.status,
        })
    }
}

/// Changes the status. Moving into `done` sets `reviewed_at`; any move out
/// of it clears it.
fn set_status(call: &Call<'_>, task: &TaskRow, status: Status) -> Result<(), ServiceError> {
    let reviewed_at = match status {
        Status::Done => Some(call.now_ms()),
        _ => None,
    };
    call.tx
        .prepare_cached("UPDATE tasks SET status = ?2, reviewed_at = ?3 WHERE seq = ?1")?
        .execute(params![task.seq, status, reviewed_at])?;
    Ok(())
}

fn saved(task: &TaskRow, status: Status) -> Saved {
    Saved {
        id: task.id,
        status,
    }
}

fn invalid(message: String) -> ServiceError {
    ServiceError::InvalidArgument(message)
}

pub(super) fn non_empty<'a>(field: &str, value: &'a str) -> Result<&'a str, ServiceError> {
    match value.trim() {
        "" => Err(invalid(format!("`{field}` must not be empty"))),
        value => Ok(value),
    }
}

fn one_line<'a>(field: &str, value: &'a str) -> Result<&'a str, ServiceError> {
    let value = non_empty(field, value)?;
    if value.contains(['\n', '\r']) {
        return Err(invalid(format!("`{field}` must be a single line")));
    }
    Ok(value)
}

fn one_lines(field: &str, items: &[String]) -> Result<Vec<String>, ServiceError> {
    items
        .iter()
        .map(|item| one_line(field, item).map(str::to_owned))
        .collect()
}

/// Labels: single lines, without duplicates, ignoring case.
fn labels(labels: Vec<String>) -> Result<Vec<String>, ServiceError> {
    let mut out: Vec<String> = Vec::with_capacity(labels.len());
    for label in &labels {
        let label = one_line("label", label)?;
        if !out.iter().any(|l| l.eq_ignore_ascii_case(label)) {
            out.push(label.to_owned());
        }
    }
    Ok(out)
}

/// Whether two label lists hold the same labels, in any order and case.
fn same_labels(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|x| b.iter().any(|y| x.eq_ignore_ascii_case(y)))
}

/// Acceptance criteria with their ticks. An item with the same text as a
/// ticked criterion in `existing` stays ticked, each one matched at most
/// once; new or reworded items start unticked.
fn checklist(
    items: &[String],
    existing: &[Criterion],
) -> Result<Vec<(String, bool)>, ServiceError> {
    let mut ticked: Vec<&str> = existing
        .iter()
        .filter(|c| c.met)
        .map(|c| c.text.as_str())
        .collect();
    items
        .iter()
        .map(|item| {
            let text = one_line("acceptance criterion", item)?;
            let met = match ticked.iter().position(|&t| t == text) {
                Some(i) => {
                    ticked.swap_remove(i);
                    true
                }
                None => false,
            };
            Ok((text.to_owned(), met))
        })
        .collect()
}

fn dedup(ids: Vec<TaskId>) -> Vec<TaskId> {
    let mut out: Vec<TaskId> = Vec::with_capacity(ids.len());
    for id in ids {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}
