use crate::graph::GraphError;
use crate::index::IndexError;
use crate::store::{ReadOnly, Status, StoreError, TaskId};

/// Why a service call was refused. [`code`](Self::code) gives the spec's
/// error code, which MCP responses carry so agents can act on it.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("task `{0}` does not exist")]
    NotFound(TaskId),
    #[error("task `{task}` is held by `{holder}` until {expires_at}")]
    ClaimedByOther {
        task: TaskId,
        holder: String,
        expires_at: String,
    },
    #[error("task `{task}` depends on tasks that are not done: {}", list(.pending))]
    DepsNotDone { task: TaskId, pending: Vec<TaskId> },
    #[error("only the lease holder can do this; {}", holder_text(*.task, .holder))]
    NotHolder {
        task: TaskId,
        holder: Option<String>,
    },
    #[error("task `{task}` has an invalid file: {message}")]
    InvalidFile { task: TaskId, message: String },
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
    #[error("{0}")]
    InvalidArgument(String),
    #[error("the project is read-only: {0}")]
    ReadOnly(ReadOnly),
    #[error(transparent)]
    Index(Box<IndexError>),
    #[error(transparent)]
    Store(Box<StoreError>),
}

impl ServiceError {
    /// The error code: one of the spec's `not_found`, `claimed_by_other`,
    /// `deps_not_done`, `not_holder`, `invalid_file`, `invalid_transition`,
    /// `cycle_detected`, `parent_not_found` and `criteria_unmet`, or
    /// `invalid_argument`, `read_only` and `internal`, which the spec
    /// doesn't name.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::ClaimedByOther { .. } => "claimed_by_other",
            Self::DepsNotDone { .. } => "deps_not_done",
            Self::NotHolder { .. } => "not_holder",
            Self::InvalidFile { .. } => "invalid_file",
            Self::InvalidTransition { .. } => "invalid_transition",
            Self::Graph(e) => e.code(),
            Self::CriteriaUnmet { .. } => "criteria_unmet",
            Self::InvalidArgument(_) => "invalid_argument",
            Self::ReadOnly(_) => "read_only",
            Self::Store(e) if matches!(**e, StoreError::Invalid { .. }) => "invalid_file",
            Self::Index(_) | Self::Store(_) => "internal",
        }
    }
}

impl From<IndexError> for ServiceError {
    fn from(e: IndexError) -> Self {
        Self::Index(Box::new(e))
    }
}

impl From<StoreError> for ServiceError {
    fn from(e: StoreError) -> Self {
        Self::Store(Box::new(e))
    }
}

impl From<rusqlite::Error> for ServiceError {
    fn from(e: rusqlite::Error) -> Self {
        IndexError::from(e).into()
    }
}

fn list<T: std::fmt::Display>(items: &[T]) -> String {
    let items: Vec<String> = items.iter().map(ToString::to_string).collect();
    items.join(", ")
}

fn holder_text(task: TaskId, holder: &Option<String>) -> String {
    match holder {
        Some(holder) => format!("task `{task}` is held by `{holder}`"),
        None => format!("no agent holds task `{task}`"),
    }
}
