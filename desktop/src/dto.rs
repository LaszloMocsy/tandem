//! The shapes commands and events exchange with the UI, in camelCase. They
//! mirror the service's types; `ui/src/api.ts` declares the same shapes.

use serde::{Deserialize, Deserializer, Serialize};
use tandem_board_core::index::{Changes, Problem};
use tandem_board_core::service::{self, LogEntry};
use tandem_board_core::store::{Priority, Status, TaskId};

/// The `tasks-changed` event.
pub const TASKS_CHANGED: &str = "tasks-changed";

/// The payload of `tasks-changed`: the tasks of one project that changed,
/// from the board, an agent or an outside edit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TasksChanged {
    pub project: String,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

impl TasksChanged {
    pub fn new(project: &str, changes: &Changes) -> Self {
        Self {
            project: project.to_owned(),
            changed: changes.changed.iter().map(ToString::to_string).collect(),
            removed: changes.removed.iter().map(ToString::to_string).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub id: String,
    pub title: String,
    pub status: &'static str,
    pub priority: &'static str,
    pub parent: Option<String>,
    pub labels: Vec<String>,
    pub depends_on: Vec<String>,
    pub lease: Option<Lease>,
    pub blocked: Option<String>,
    pub problem: Option<TaskProblem>,
}

impl From<service::TaskSummary> for TaskSummary {
    fn from(task: service::TaskSummary) -> Self {
        Self {
            id: task.id.to_string(),
            title: task.title,
            status: task.status.as_str(),
            priority: task.priority.as_str(),
            parent: task.parent.map(|id| id.to_string()),
            labels: task.labels,
            depends_on: task.depends_on.iter().map(ToString::to_string).collect(),
            lease: task.lease.map(Lease::from),
            blocked: task.blocked,
            problem: task.problem.map(TaskProblem::from),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lease {
    pub agent_id: String,
    pub claimed_at: String,
    pub expires_at: String,
    pub live: bool,
}

impl From<service::Lease> for Lease {
    fn from(lease: service::Lease) -> Self {
        Self {
            agent_id: lease.agent_id,
            claimed_at: lease.claimed_at,
            expires_at: lease.expires_at,
            live: lease.live,
        }
    }
}

/// Why a task gets the invalid badge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProblem {
    /// `invalid_file`, or `graph` for a loop or a missing parent.
    pub kind: &'static str,
    pub message: String,
}

impl From<Problem> for TaskProblem {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::InvalidFile(message) => Self {
                kind: "invalid_file",
                message,
            },
            Problem::Graph(message) => Self {
                kind: "graph",
                message,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub summary: TaskSummary,
    pub markdown: String,
    pub parent: Option<TaskSummary>,
    pub children: Vec<TaskSummary>,
    pub work_log: Vec<WorkLogEntry>,
    pub review_feedback: Option<String>,
}

impl From<service::TaskDetail> for TaskDetail {
    fn from(task: service::TaskDetail) -> Self {
        Self {
            summary: task.summary.into(),
            markdown: task.markdown,
            parent: task.parent.map(TaskSummary::from),
            children: task.children.into_iter().map(TaskSummary::from).collect(),
            work_log: task.work_log.into_iter().map(WorkLogEntry::from).collect(),
            review_feedback: task.review_feedback,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkLogEntry {
    /// `None` means you.
    pub agent_id: Option<String>,
    pub kind: &'static str,
    pub message: String,
    pub created_at: String,
}

impl From<LogEntry> for WorkLogEntry {
    fn from(entry: LogEntry) -> Self {
        Self {
            agent_id: entry.agent_id,
            kind: entry.kind.as_str(),
            message: entry.message,
            created_at: entry.created_at,
        }
    }
}

/// The file a write changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    pub id: String,
    pub path: String,
    pub status: &'static str,
}

impl From<service::Written> for Written {
    fn from(written: service::Written) -> Self {
        Self {
            id: written.id.to_string(),
            path: written.path,
            status: written.status.as_str(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct TaskFilter {
    pub status: Option<Status>,
    pub label: Option<String>,
    pub parent: Option<TaskId>,
    pub query: Option<String>,
    pub ready_only: bool,
}

impl From<TaskFilter> for service::TaskFilter {
    fn from(filter: TaskFilter) -> Self {
        Self {
            status: filter.status,
            label: filter.label,
            parent: filter.parent,
            query: filter.query,
            ready_only: filter.ready_only,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewTask {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub parent: Option<TaskId>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<TaskId>,
}

impl From<NewTask> for service::NewTask {
    fn from(task: NewTask) -> Self {
        Self {
            title: task.title,
            description: task.description,
            acceptance_criteria: task.acceptance_criteria,
            priority: task.priority,
            parent: task.parent,
            labels: task.labels,
            depends_on: task.depends_on,
        }
    }
}

/// The fields to change; a missing field stays as it is. `parent: null`
/// moves the task to the top level.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct TaskEdit {
    pub title: Option<String>,
    pub description: Option<String>,
    pub acceptance_criteria: Option<Vec<String>>,
    pub relevant_files: Option<Vec<String>>,
    pub priority: Option<Priority>,
    #[serde(deserialize_with = "present")]
    pub parent: Option<Option<TaskId>>,
    pub labels: Option<Vec<String>>,
    pub depends_on: Option<Vec<TaskId>>,
}

impl From<TaskEdit> for service::TaskEdit {
    fn from(edit: TaskEdit) -> Self {
        Self {
            title: edit.title,
            description: edit.description,
            acceptance_criteria: edit.acceptance_criteria,
            relevant_files: edit.relevant_files,
            priority: edit.priority,
            parent: edit.parent,
            labels: edit.labels,
            depends_on: edit.depends_on,
        }
    }
}

/// Tells a field that is present, even as `null`, from a missing one, which
/// `#[serde(default)]` leaves as `None`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Finish {
    pub summary: String,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub follow_ups: Option<String>,
    #[serde(default)]
    pub met: Vec<u32>,
    #[serde(default)]
    pub unmet: Vec<Unmet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Unmet {
    pub criterion: u32,
    pub reason: String,
}

impl From<Finish> for service::Finish {
    fn from(finish: Finish) -> Self {
        Self {
            summary: finish.summary,
            files_changed: finish.files_changed,
            follow_ups: finish.follow_ups,
            met: finish.met,
            unmet: finish
                .unmet
                .into_iter()
                .map(|u| tandem_board_core::store::Unmet {
                    criterion: u.criterion,
                    reason: u.reason,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_tell_a_null_parent_from_a_missing_one() {
        let edit: TaskEdit = serde_json::from_str(r#"{"title":"T"}"#).unwrap();
        assert_eq!(edit.parent, None);
        let edit: TaskEdit = serde_json::from_str(r#"{"parent":null}"#).unwrap();
        assert_eq!(edit.parent, Some(None));
        let edit: TaskEdit = serde_json::from_str(r#"{"parent":"k3f9x2m"}"#).unwrap();
        assert_eq!(edit.parent, Some(Some("k3f9x2m".parse().unwrap())));
    }

    #[test]
    fn inputs_use_camel_case_and_the_files_values() {
        let task: NewTask = serde_json::from_str(
            r#"{"title":"T","acceptanceCriteria":["a"],"priority":"high","dependsOn":["k3f9x2m"]}"#,
        )
        .unwrap();
        assert_eq!(task.acceptance_criteria, ["a"]);
        assert_eq!(task.priority, Some(Priority::High));
        let filter: TaskFilter =
            serde_json::from_str(r#"{"status":"in_progress","readyOnly":true}"#).unwrap();
        assert_eq!(filter.status, Some(Status::InProgress));
        assert!(filter.ready_only);
        assert!(serde_json::from_str::<TaskEdit>(r#"{"status":"done"}"#).is_err());
    }
}
