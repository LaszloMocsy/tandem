use crate::db::DbError;
use crate::graph::GraphError;
use crate::model::{Status, TaskId};

/// Why a service call was refused. [`code`](Self::code) gives the error
/// code, which MCP responses carry so agents can act on it.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("task `{0}` does not exist")]
    NotFound(TaskId),
    #[error("task `{task}` is held by `{holder}`{}", until(.expires_at))]
    ClaimedByOther {
        task: TaskId,
        holder: String,
        /// `None` when you hold it, or an agent holds it without a lease.
        expires_at: Option<String>,
    },
    #[error("task `{task}` depends on tasks that are not done: {}", list(.pending))]
    DepsNotDone { task: TaskId, pending: Vec<TaskId> },
    #[error("only the lease holder can do this; {}", holder_text(*.task, .holder))]
    NotHolder {
        task: TaskId,
        holder: Option<String>,
    },
    #[error("cannot {action} task `{task}` while it is {}", .status.as_str())]
    InvalidTransition {
        task: TaskId,
        action: &'static str,
        status: Status,
    },
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error(
        "acceptance criteria {} of task `{task}` are neither met nor explained; \
         tick them with `met` or list each in `unmet` with a reason",
        list(.criteria)
    )]
    CriteriaUnmet { task: TaskId, criteria: Vec<u32> },
    #[error(
        "task `{task}` is still linked from other tasks{}{}; unlink them first",
        linked(" as the parent of", .children),
        linked(" as a dependency of", .dependents)
    )]
    Referenced {
        task: TaskId,
        /// Tasks whose `parent` is this task.
        children: Vec<TaskId>,
        /// Tasks that list this task in `depends_on`.
        dependents: Vec<TaskId>,
    },
    #[error(
        "an agent is working on {}; wait until it finishes or releases them",
        tasks_text(.0)
    )]
    AgentsWorking(Vec<TaskId>),
    #[error("{0}")]
    InvalidArgument(String),
    #[error(transparent)]
    Db(Box<DbError>),
}

impl ServiceError {
    /// The error code: `not_found`, `claimed_by_other`, `deps_not_done`,
    /// `not_holder`, `invalid_transition`, `cycle_detected`,
    /// `parent_not_found`, `criteria_unmet`, `task_referenced`,
    /// `agents_working`, `invalid_argument` or `internal`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::ClaimedByOther { .. } => "claimed_by_other",
            Self::DepsNotDone { .. } => "deps_not_done",
            Self::NotHolder { .. } => "not_holder",
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::Graph(e) => e.code(),
            Self::CriteriaUnmet { .. } => "criteria_unmet",
            Self::Referenced { .. } => "task_referenced",
            Self::AgentsWorking(_) => "agents_working",
            Self::InvalidArgument(_) => "invalid_argument",
            Self::Db(_) => "internal",
        }
    }
}

impl From<DbError> for ServiceError {
    fn from(e: DbError) -> Self {
        Self::Db(Box::new(e))
    }
}

impl From<rusqlite::Error> for ServiceError {
    fn from(e: rusqlite::Error) -> Self {
        DbError::from(e).into()
    }
}

fn list<T: std::fmt::Display>(items: &[T]) -> String {
    let items: Vec<String> = items.iter().map(ToString::to_string).collect();
    items.join(", ")
}

fn linked(what: &str, tasks: &[TaskId]) -> String {
    if tasks.is_empty() {
        return String::new();
    }
    format!("{what} {}", list(tasks))
}

fn tasks_text(tasks: &[TaskId]) -> String {
    let noun = if tasks.len() == 1 { "task" } else { "tasks" };
    format!("{noun} {}", list(tasks))
}

fn until(expires_at: &Option<String>) -> String {
    expires_at
        .as_ref()
        .map(|at| format!(" until {at}"))
        .unwrap_or_default()
}

fn holder_text(task: TaskId, holder: &Option<String>) -> String {
    match holder {
        Some(holder) => format!("task `{task}` is held by `{holder}`"),
        None => format!("no agent holds task `{task}`"),
    }
}
