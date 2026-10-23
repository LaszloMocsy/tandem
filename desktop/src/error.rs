use std::error::Error as StdError;

use serde::Serialize;
use tandem_board_core::db::DbError;
use tandem_board_core::service::ServiceError;

/// A failed command, as the UI receives it: a code it can act on and a
/// message to show. Service errors keep their codes, such as
/// `invalid_transition`; the registry adds `project_not_found`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Error {
    pub code: &'static str,
    pub message: String,
}

impl Error {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn project_not_found(id: &str) -> Self {
        Self::new("project_not_found", format!("project `{id}` is not open"))
    }

    pub fn internal(error: &dyn StdError) -> Self {
        Self::new("internal", describe(error))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for Error {}

impl From<ServiceError> for Error {
    fn from(e: ServiceError) -> Self {
        Self::new(e.code(), describe(&e))
    }
}

impl From<DbError> for Error {
    fn from(e: DbError) -> Self {
        Self::internal(&e)
    }
}

/// An error and its causes on one line.
pub fn describe(error: &dyn StdError) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    message
}

/// Writes an error and its causes to stderr.
pub fn report(context: &str, error: &dyn StdError) {
    eprintln!("tandem: {context}: {}", describe(error));
}
