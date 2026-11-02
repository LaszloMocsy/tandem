//! The shapes commands and events exchange with the UI, in camelCase. They
//! mirror the service's types; `ui/src/api.ts` declares the same shapes.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use tandem_board_core::db::Changes;
use tandem_board_core::features::{Feature, Features};
use tandem_board_core::model::{self, Priority, Status, TaskId};
use tandem_board_core::service::{self, LogEntry};

/// The `tasks-changed` event.
pub const TASKS_CHANGED: &str = "tasks-changed";

/// The payload of `tasks-changed`: the tasks of one project that changed,
/// from the board or an agent.
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

/// Which features are on, by name, such as `{"assistant":false,…}`.
pub type FeaturesInfo = BTreeMap<&'static str, bool>;

pub fn features_info(features: Features) -> FeaturesInfo {
    Feature::ALL
        .into_iter()
        .map(|feature| (feature.as_str(), features.is_on(feature)))
        .collect()
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
    /// `YYYY-MM-DD`.
    pub created: String,
    pub lease: Option<Lease>,
    pub blocked: Option<String>,
    /// Only filled in for tasks in `finished`.
    pub unmet: Vec<UnmetCriterion>,
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
            created: task.created.to_string(),
            lease: task.lease.map(Lease::from),
            blocked: task.blocked,
            unmet: task.unmet.into_iter().map(UnmetCriterion::from).collect(),
        }
    }
}

/// An acceptance criterion the agent left unmet, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnmetCriterion {
    pub criterion: u32,
    pub text: String,
    pub reason: String,
}

impl From<model::UnmetCriterion> for UnmetCriterion {
    fn from(unmet: model::UnmetCriterion) -> Self {
        Self {
            criterion: unmet.number,
            text: unmet.text,
            reason: unmet.reason,
        }
    }
}

/// An agent in the Agents panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub agent_id: String,
    pub client_name: Option<String>,
    pub last_seen_at: String,
    pub tasks: Vec<HeldTask>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldTask {
    pub id: String,
    pub title: String,
    pub live: bool,
}

impl From<service::AgentInfo> for AgentInfo {
    fn from(agent: service::AgentInfo) -> Self {
        Self {
            agent_id: agent.agent_id,
            client_name: agent.client_name,
            last_seen_at: agent.last_seen_at,
            tasks: agent
                .tasks
                .into_iter()
                .map(|task| HeldTask {
                    id: task.id.to_string(),
                    title: task.title,
                    live: task.live,
                })
                .collect(),
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

/// What stands in the way of deleting a task, for its confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCheck {
    pub children: Vec<String>,
    pub dependents: Vec<String>,
    /// The live lease, while an agent is working on the task.
    pub lease: Option<Lease>,
}

impl From<service::DeleteCheck> for DeleteCheck {
    fn from(check: service::DeleteCheck) -> Self {
        Self {
            children: check.children.iter().map(ToString::to_string).collect(),
            dependents: check.dependents.iter().map(ToString::to_string).collect(),
            lease: check.lease.map(Into::into),
        }
    }
}

/// A deleted task, which can still be restored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedTask {
    pub id: String,
    pub title: String,
    pub status: &'static str,
    pub deleted_at: String,
}

impl From<service::DeletedTask> for DeletedTask {
    fn from(task: service::DeletedTask) -> Self {
        Self {
            id: task.id.to_string(),
            title: task.title,
            status: task.status.as_str(),
            deleted_at: task.deleted_at,
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
    pub content: TaskContent,
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
            content: task.content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskContent {
    pub description: String,
    pub criteria: Vec<Criterion>,
    pub relevant_files: Vec<String>,
    pub changelog: Vec<ChangelogEntry>,
    pub reviewed_at: Option<String>,
}

impl From<service::TaskContent> for TaskContent {
    fn from(content: service::TaskContent) -> Self {
        Self {
            description: content.description,
            criteria: content
                .criteria
                .into_iter()
                .map(|c| Criterion {
                    number: c.number,
                    met: c.met,
                    text: c.text,
                })
                .collect(),
            relevant_files: content.relevant_files,
            changelog: content
                .changelog
                .into_iter()
                .map(|entry| ChangelogEntry {
                    date: entry.date.to_string(),
                    who: entry.who,
                    event: entry.event,
                    text: entry.text,
                })
                .collect(),
            reviewed_at: content.reviewed_at.map(|date| date.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Criterion {
    pub number: u32,
    pub met: bool,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogEntry {
    pub date: String,
    pub who: String,
    pub event: String,
    /// The summary and any `Files:`, `Follow-ups:` and `Unmet:` lines.
    pub text: String,
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

/// The task a change saved, and its status after it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub id: String,
    pub status: &'static str,
}

impl From<service::Saved> for Saved {
    fn from(saved: service::Saved) -> Self {
        Self {
            id: saved.id.to_string(),
            status: saved.status.as_str(),
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
                .map(|u| model::Unmet {
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
    fn features_are_sent_by_name() {
        let features = Features::OFF.with(Feature::Canvas, true);
        assert_eq!(
            serde_json::to_string(&features_info(features)).unwrap(),
            r#"{"assistant":false,"automation":false,"canvas":true,"shaping":false,"structure":false}"#
        );
    }

    #[test]
    fn a_delete_check_is_sent_in_camel_case() {
        let check = DeleteCheck::from(service::DeleteCheck {
            children: vec!["k3f9x2m".parse().unwrap()],
            dependents: Vec::new(),
            lease: None,
        });
        assert_eq!(
            serde_json::to_string(&check).unwrap(),
            r#"{"children":["k3f9x2m"],"dependents":[],"lease":null}"#
        );
    }

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
    fn inputs_use_camel_case_and_the_stored_values() {
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
