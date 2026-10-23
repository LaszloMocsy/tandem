//! The SQLite databases, in the app's data directory and never in a repo:
//! `app.db` with the project list and settings, and one database per
//! project with its tasks, their history and in-flight activity.
//!
//! The schema is in `schema.rs`: each list of migrations, read in order,
//! builds the current schema from nothing.

mod app;
mod project;
mod schema;
mod sql;

use std::io;
use std::path::{Path, PathBuf};

use jiff::Timestamp;

pub use app::{
    AppDb, AppSettings, MAX_NAME_LEN, NameError, Project, Theme, folder_name, is_valid_project_id,
    normalize_project_name,
};
pub use project::{Changes, ProjectDb, delete_project_db, project_db_path};

/// The folder in the app's data directory that holds the databases.
const DB_DIR: &str = "db";

/// An error opening or updating a database.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
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

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> DbError + '_ {
    move |source| DbError::Io {
        path: path.to_owned(),
        source,
    }
}

/// A time as the databases store it: milliseconds since the Unix epoch.
pub fn millis(at: Timestamp) -> i64 {
    at.as_millisecond()
}

/// The time a stored value of [`millis`] stands for.
pub fn from_millis(ms: i64) -> Timestamp {
    Timestamp::from_millisecond(ms).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// The text form of a time the service hands out: UTC with milliseconds,
/// such as `2026-10-15T09:30:00.000Z`. Every one has the same length, so
/// comparing the text compares the times.
pub fn timestamp_text(at: Timestamp) -> String {
    at.strftime("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}
