//! The Markdown task format: one file per task, YAML frontmatter, then
//! Description, Acceptance criteria, Relevant files and Changelog sections.
//!
//! The database is where tasks live. This format is how `get_task` shows a
//! task to agents, and how tasks from an old `.tandem/` folder are imported.

mod changelog;
mod parse;
mod render;
mod sections;
mod text;
mod yaml;

pub use changelog::Entry;
pub use parse::{ParseError, read_folder};

use crate::model::{Criterion, Date, Priority, Status, TaskId};

/// The folder older versions kept task files in, at the repo root.
pub const TASK_DIR: &str = ".tandem";

/// One task in the Markdown format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDoc {
    pub id: TaskId,
    pub title: String,
    pub status: Status,
    pub priority: Option<Priority>,
    pub parent: Option<TaskId>,
    pub labels: Vec<String>,
    pub depends_on: Vec<TaskId>,
    pub created: Date,
    pub reviewed_at: Option<Date>,
    /// Markdown; empty if there is none.
    pub description: String,
    pub criteria: Vec<Criterion>,
    pub relevant_files: Vec<String>,
    /// Oldest first.
    pub changelog: Vec<Entry>,
}

/// What [`read_folder`] found.
#[derive(Debug, Default)]
pub struct FolderTasks {
    pub tasks: Vec<TaskDoc>,
    /// Files that could not be read, by name, with the reason.
    pub skipped: Vec<(String, String)>,
}
