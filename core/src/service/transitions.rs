//! Every change to a task, following the transitions table in "State
//! model": each writes exactly the file and SQLite changes listed there.

use std::collections::BTreeSet;

use rusqlite::params;

use super::{
    Actor, Call, Finish, LogKind, NewTask, Service, ServiceError, TaskEdit, Written, read_file,
    written,
};
use crate::index::{graph_in, timestamp_text};
use crate::store::{
    ACCEPTANCE_CRITERIA, ChangelogEntry, DESCRIPTION, EntryHeading, RELEVANT_FILES, Status, Task,
    TaskId, create_task, unused_id,
};

impl Service {
    /// New task: a new file, in `backlog` when an agent creates it and in
    /// `todo` when you do, with a `created` changelog entry. Refuses a
    /// parent or dependency that is missing or would close a loop.
    pub fn create_task(&self, actor: &Actor, new: NewTask) -> Result<Written, ServiceError> {
        self.check_writable()?;
        let title = one_line("title", &new.title)?.to_owned();
        let labels = labels(new.labels)?;
        let criteria = checklist(&new.acceptance_criteria)?;
        let depends_on = dedup(new.depends_on);
        self.call(actor, |call| {
            // A full stat scan first, so the graph check sees every file.
            call.scan_all()?;
            let id = unused_id(call.dir)?;
            graph_in(call.tx)?.check_change(id, new.parent, &depends_on)?;
            let mut task = Task {
                id,
                title,
                status: match actor {
                    Actor::User => Status::Todo,
                    Actor::Agent(_) => Status::Backlog,
                },
                priority: new.priority,
                parent: new.parent,
                labels,
                depends_on,
                created: call.today,
                reviewed_at: None,
                body: String::new(),
            };
            set_section(&mut task, DESCRIPTION, &new.description)?;
            set_section(&mut task, ACCEPTANCE_CRITERIA, &criteria)?;
            append(&mut task, call, "created", "")?;
            create_task(call.dir, &task)?;
            call.scan_task(id)?;
            Ok(written(&task))
        })
    }

    /// Edits the spec: title, sections, priority, parent, labels and
    /// dependencies. Never changes the review, and adds no changelog entry
    /// unless an agent's edit sends the task back to backlog.
    ///
    /// You may edit any task. Agents are limited, so that nothing they
    /// change gets worked on before you approve it:
    ///
    /// - In backlog, agents may edit anything.
    /// - After approval, `relevant_files`, `labels` and `priority` stay open.
    ///   Any other change to a todo task sends it back to backlog, with an
    ///   `edited` changelog entry, until you approve it again. On a task in
    ///   progress, only the lease holder may edit, and other changes are
    ///   refused.
    /// - Finished and done tasks are closed to agents.
    pub fn update_task(
        &self,
        actor: &Actor,
        id: TaskId,
        edit: TaskEdit,
    ) -> Result<Written, ServiceError> {
        self.check_writable()?;
        self.call(actor, |call| {
            call.scan_all()?;
            let original = read_file(call.dir, id)?;
            let is_agent = matches!(actor, Actor::Agent(_));
            if is_agent {
                check_agent_access(call, &original)?;
            }

            // What the task is and when it can start.
            let mut task = original.clone();
            if let Some(title) = &edit.title {
                task.title = one_line("title", title)?.to_owned();
            }
            if let Some(description) = &edit.description {
                set_section(&mut task, DESCRIPTION, description)?;
            }
            if let Some(criteria) = &edit.acceptance_criteria {
                set_section(&mut task, ACCEPTANCE_CRITERIA, &checklist(criteria)?)?;
            }
            let graph = graph_in(call.tx)?;
            if let Some(parent) = edit.parent {
                graph.check_change(id, parent, &[])?;
                task.parent = parent;
            }
            if let Some(depends_on) = edit.depends_on.clone() {
                let depends_on = dedup(depends_on);
                graph.check_change(id, None, &depends_on)?;
                task.depends_on = depends_on;
            }
            let spec_changed = task != original;

            // Details that change neither.
            if let Some(files) = &edit.relevant_files {
                set_section(&mut task, RELEVANT_FILES, &list(files)?)?;
            }
            if let Some(priority) = edit.priority {
                task.priority = Some(priority);
            }
            if let Some(labels_) = edit.labels.clone() {
                task.labels = labels(labels_)?;
            }

            if is_agent && spec_changed {
                match original.status {
                    Status::Backlog => {}
                    Status::Todo => {
                        set_status(&mut task, Status::Backlog);
                        append(
                            &mut task,
                            call,
                            "edited",
                            "Changed after approval; back in backlog until approved again.",
                        )?;
                    }
                    status => {
                        return Err(ServiceError::InvalidTransition {
                            task: id,
                            action: "change the spec of",
                            status,
                        });
                    }
                }
            }
            if task == original {
                return Ok(written(&task));
            }
            call.write(&task)
        })
    }

    /// Backlog → Todo, by you only: `status: todo`, changelog `approved`.
    pub fn approve(&self, id: TaskId) -> Result<Written, ServiceError> {
        self.check_writable()?;
        self.call(&Actor::User, |call| {
            let mut task = call.read(id)?;
            expect_status(&task, Status::Backlog, "approve")?;
            set_status(&mut task, Status::Todo);
            append(&mut task, call, "approved", "")?;
            call.write(&task)
        })
    }

    /// Todo → In progress: `status: in_progress`, changelog `started`, and a
    /// lease when an agent starts it. Runs as one `BEGIN IMMEDIATE`
    /// transaction, so of many agents starting the same task, exactly one
    /// wins.
    ///
    /// An agent may also start a task that is already in progress if no live
    /// lease is held on it: a stale lease is taken over, which the changelog
    /// notes. Without any lease, an agent may pick up a task you reopened,
    /// or resume one it started itself before the database was rebuilt; a
    /// task you started from the board stays yours. Calling it again while
    /// holding the lease only extends it.
    pub fn start_task(&self, actor: &Actor, id: TaskId) -> Result<Written, ServiceError> {
        self.check_writable()?;
        self.call(actor, |call| {
            let mut task = call.read(id)?;
            let mut summary = String::new();
            match (actor, task.status) {
                (_, Status::Todo) => {}
                (Actor::Agent(agent), Status::InProgress) => match call.lease(id)? {
                    Some(lease) if lease.agent_id == agent.id => return Ok(written(&task)),
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
            let pending: Vec<TaskId> = task
                .depends_on
                .iter()
                .copied()
                .filter(|&dep| !read_file(call.dir, dep).is_ok_and(|d| d.status == Status::Done))
                .collect();
            if !pending.is_empty() {
                return Err(ServiceError::DepsNotDone { task: id, pending });
            }

            set_status(&mut task, Status::InProgress);
            append(&mut task, call, "started", &summary)?;
            let out = call.write(&task)?;
            if let Actor::Agent(agent) = actor {
                call.tx.execute(
                    "INSERT OR REPLACE INTO leases (task_id, agent_id, claimed_at, expires_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        id,
                        agent.id,
                        timestamp_text(call.now),
                        timestamp_text(call.now + call.config.lease_duration)
                    ],
                )?;
            }
            Ok(out)
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
            call.check_exists(id)?;
            call.check_holder(id)?;
            call.log(id, LogKind::Note, message)
        })
    }

    /// Sets the blocked flag with a reason, or clears it when `reason` is
    /// blank, and logs either. Only the lease holder, or you.
    pub fn set_blocked(&self, actor: &Actor, id: TaskId, reason: &str) -> Result<(), ServiceError> {
        let reason = reason.trim();
        self.call(actor, |call| {
            call.check_exists(id)?;
            call.check_holder(id)?;
            let (blocked, kind) = match reason {
                "" => (None, LogKind::Unblocked),
                reason => (Some(reason), LogKind::Blocked),
            };
            call.tx.execute(
                "INSERT INTO task_flags (task_id, blocked_reason) VALUES (?1, ?2)
                 ON CONFLICT (task_id) DO UPDATE SET blocked_reason = excluded.blocked_reason",
                params![id, blocked],
            )?;
            call.log(id, kind, reason)
        })
    }

    /// A milestone entry in the changelog of a task in progress, before an
    /// intermediate commit. Only the lease holder, or you.
    pub fn add_changelog_entry(
        &self,
        actor: &Actor,
        id: TaskId,
        summary: &str,
        files: Vec<String>,
    ) -> Result<Written, ServiceError> {
        self.check_writable()?;
        let summary = non_empty("summary", summary)?;
        self.call(actor, |call| {
            let mut task = call.read(id)?;
            expect_status(&task, Status::InProgress, "add a changelog entry to")?;
            call.check_holder(id)?;
            append_entry(
                &mut task,
                ChangelogEntry {
                    files,
                    ..entry(call, "milestone", summary)
                },
            )?;
            call.write(&task)
        })
    }

    /// In progress → Review: ticks the `met` criteria, sets `finished`,
    /// adds a `finished` changelog entry with the summary, and releases the
    /// lease. Every criterion still unticked must be in `unmet` with a
    /// reason, or the call fails with `criteria_unmet`. Only the lease
    /// holder, or you.
    pub fn finish_task(
        &self,
        actor: &Actor,
        id: TaskId,
        finish: Finish,
    ) -> Result<Written, ServiceError> {
        self.check_writable()?;
        let summary = non_empty("summary", &finish.summary)?;
        self.call(actor, |call| {
            let mut task = call.read(id)?;
            expect_status(&task, Status::InProgress, "finish")?;
            call.check_holder(id)?;

            let met: BTreeSet<u32> = finish.met.iter().copied().collect();
            let mut unmet = BTreeSet::new();
            for item in &finish.unmet {
                non_empty("unmet reason", &item.reason)?;
                if met.contains(&item.criterion) || !unmet.insert(item.criterion) {
                    return Err(invalid(format!(
                        "criterion {} is listed more than once in met and unmet",
                        item.criterion
                    )));
                }
            }
            let criteria = task.criteria();
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

            let met: Vec<u32> = met.into_iter().collect();
            task.tick_criteria(&met)
                .map_err(|e| invalid(e.to_string()))?;
            set_status(&mut task, Status::Finished);
            let mut unmet = finish.unmet.clone();
            unmet.sort_by_key(|u| u.criterion);
            append_entry(
                &mut task,
                ChangelogEntry {
                    files: finish.files_changed.clone(),
                    follow_ups: finish.follow_ups.clone(),
                    unmet,
                    ..entry(call, "finished", summary)
                },
            )?;
            let out = call.write(&task)?;
            call.tx
                .execute("DELETE FROM leases WHERE task_id = ?1", [id])?;
            Ok(out)
        })
    }

    /// Review → Done, by you only: `status: done`, `reviewed_at: <today>`.
    pub fn mark_reviewed(&self, id: TaskId) -> Result<Written, ServiceError> {
        self.check_writable()?;
        self.call(&Actor::User, |call| {
            let mut task = call.read(id)?;
            expect_status(&task, Status::Finished, "mark reviewed")?;
            set_status(&mut task, Status::Done);
            task.reviewed_at = Some(call.today);
            call.write(&task)
        })
    }

    /// Review → In progress, by you only: `status: in_progress`, changelog
    /// `reopened` with your feedback, which is also stored for the next
    /// agent and added to the working log.
    pub fn reopen(&self, id: TaskId, feedback: &str) -> Result<Written, ServiceError> {
        self.check_writable()?;
        let feedback = non_empty("feedback", feedback)?;
        self.call(&Actor::User, |call| {
            let mut task = call.read(id)?;
            expect_status(&task, Status::Finished, "reopen")?;
            set_status(&mut task, Status::InProgress);
            append(&mut task, call, "reopened", feedback)?;
            let out = call.write(&task)?;
            call.tx.execute(
                "INSERT INTO task_flags (task_id, review_feedback) VALUES (?1, ?2)
                 ON CONFLICT (task_id) DO UPDATE SET review_feedback = excluded.review_feedback",
                params![id, feedback],
            )?;
            call.log(id, LogKind::Feedback, feedback)?;
            Ok(out)
        })
    }

    /// In progress → Todo: `status: todo`, changelog `released` with the
    /// note, and the lease removed. Only the lease holder, or you.
    pub fn release_task(
        &self,
        actor: &Actor,
        id: TaskId,
        note: &str,
    ) -> Result<Written, ServiceError> {
        self.check_writable()?;
        self.call(actor, |call| {
            let mut task = call.read(id)?;
            expect_status(&task, Status::InProgress, "release")?;
            call.check_holder(id)?;
            set_status(&mut task, Status::Todo);
            append(&mut task, call, "released", note.trim())?;
            let out = call.write(&task)?;
            call.tx
                .execute("DELETE FROM leases WHERE task_id = ?1", [id])?;
            Ok(out)
        })
    }
}

/// Whether an agent may start an in-progress task that has no lease: only
/// if you reopened it last, or the agent itself started it last.
fn check_unleased_start(call: &Call<'_>, task: &Task, agent: &str) -> Result<(), ServiceError> {
    match last_hand_over(task) {
        Some(entry) if entry.event == "reopened" => Ok(()),
        Some(entry) if entry.event == "started" && entry.who == agent => Ok(()),
        entry => Err(ServiceError::ClaimedByOther {
            task: task.id,
            holder: entry
                .filter(|entry| entry.event == "started")
                .map_or(call.config.user_name.clone(), |entry| entry.who),
            expires_at: None,
        }),
    }
}

/// The latest changelog entry that hands the task over: `started`,
/// `finished`, `released` or `reopened`. Milestones and edits don't.
pub(super) fn last_hand_over(task: &Task) -> Option<EntryHeading> {
    task.changelog().into_iter().rev().find(|entry| {
        matches!(
            entry.event.as_str(),
            "started" | "finished" | "released" | "reopened"
        )
    })
}

/// Which tasks an agent may edit at all: not finished or done ones, and in
/// progress only with the lease.
fn check_agent_access(call: &Call<'_>, task: &Task) -> Result<(), ServiceError> {
    match task.status {
        Status::Backlog | Status::Todo => Ok(()),
        Status::InProgress => call.check_holder(task.id),
        status @ (Status::Finished | Status::Done) => Err(ServiceError::InvalidTransition {
            task: task.id,
            action: "update",
            status,
        }),
    }
}

impl Call<'_> {
    /// Checks that the task has a file, valid or not.
    fn check_exists(&self, id: TaskId) -> Result<(), ServiceError> {
        self.scan_task(id)?;
        match crate::index::task_in(self.tx, id)? {
            Some(_) => Ok(()),
            None => Err(ServiceError::NotFound(id)),
        }
    }
}

fn expect_status(task: &Task, expected: Status, action: &'static str) -> Result<(), ServiceError> {
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

/// Changes the status. Any move out of `done` removes `reviewed_at`.
fn set_status(task: &mut Task, status: Status) {
    if task.status == Status::Done && status != Status::Done {
        task.reviewed_at = None;
    }
    task.status = status;
}

fn entry(call: &Call<'_>, event: &str, summary: &str) -> ChangelogEntry {
    ChangelogEntry {
        date: call.today,
        who: call.who().to_owned(),
        event: event.to_owned(),
        summary: summary.to_owned(),
        files: Vec::new(),
        follow_ups: None,
        unmet: Vec::new(),
    }
}

fn append(
    task: &mut Task,
    call: &Call<'_>,
    event: &str,
    summary: &str,
) -> Result<(), ServiceError> {
    append_entry(task, entry(call, event, summary))
}

fn append_entry(task: &mut Task, entry: ChangelogEntry) -> Result<(), ServiceError> {
    task.append_changelog(&entry)
        .map_err(|e| invalid(e.to_string()))
}

fn set_section(task: &mut Task, name: &str, content: &str) -> Result<(), ServiceError> {
    task.set_section(name, content)
        .map_err(|e| invalid(format!("{name}: {e}")))
}

fn invalid(message: String) -> ServiceError {
    ServiceError::InvalidArgument(message)
}

fn non_empty<'a>(field: &str, value: &'a str) -> Result<&'a str, ServiceError> {
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

/// Labels: single lines, without duplicates.
fn labels(labels: Vec<String>) -> Result<Vec<String>, ServiceError> {
    let mut out: Vec<String> = Vec::with_capacity(labels.len());
    for label in &labels {
        let label = one_line("label", label)?;
        if !out.iter().any(|l| l == label) {
            out.push(label.to_owned());
        }
    }
    Ok(out)
}

/// Acceptance criteria as unticked checkboxes.
fn checklist(items: &[String]) -> Result<String, ServiceError> {
    items
        .iter()
        .map(|item| {
            Ok(format!(
                "- [ ] {}\n",
                one_line("acceptance criterion", item)?
            ))
        })
        .collect()
}

/// Relevant files as a bulleted list.
fn list(items: &[String]) -> Result<String, ServiceError> {
    items
        .iter()
        .map(|item| Ok(format!("- {}\n", one_line("relevant file", item)?)))
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
