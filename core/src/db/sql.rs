//! Conversions between model types and SQLite values.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

use crate::model::{Priority, Status, TaskId};

fn from_text<T: std::str::FromStr>(value: ValueRef<'_>) -> FromSqlResult<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    value
        .as_str()?
        .parse()
        .map_err(|e| FromSqlError::Other(Box::new(e)))
}

impl ToSql for TaskId {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for TaskId {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        from_text(value)
    }
}

impl ToSql for Status {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Status {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        from_text(value)
    }
}

impl ToSql for Priority {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl FromSql for Priority {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        from_text(value)
    }
}
