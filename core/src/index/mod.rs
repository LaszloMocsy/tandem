//! The SQLite index: `app.db` with the project list, and one database per
//! project with the task index and in-flight activity (leases, flags, the
//! working log). Both live in the app's data directory, never in the repo.
//!
//! Files are the source of truth. A project database can be deleted at any
//! time and is rebuilt from `.tandem/` on the next open; only leases and
//! working logs are lost.

mod app;
mod project;
mod schema;
mod sql;

use std::io;
use std::path::{Path, PathBuf};

use jiff::Timestamp;

use crate::store::StoreError;

pub use app::{AppDb, Project, is_valid_project_id};
pub use project::{Changes, IndexedTask, Problem, ProjectIndex, project_db_path};

/// An error opening or updating a database.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("cannot access {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{} cannot use WAL mode (journal mode is `{mode}`)", path.display())]
    NoWal { path: PathBuf, mode: String },
    #[error(
        "{} has schema version {version}, but this app supports only up to {supported}",
        path.display()
    )]
    NewerSchema {
        path: PathBuf,
        version: i64,
        supported: i64,
    },
    #[error("`{0}` is not a valid project id")]
    InvalidProjectId(String),
    #[error("{} is not valid UTF-8", path.display())]
    NonUtf8Path { path: PathBuf },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> IndexError + '_ {
    move |source| IndexError::Io {
        path: path.to_owned(),
        source,
    }
}

/// The text form of a timestamp in the databases: UTC with milliseconds,
/// such as `2026-10-15T09:30:00.000Z`. Every timestamp has the same length,
/// so comparing the text compares the times.
pub fn timestamp_text(at: Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}
