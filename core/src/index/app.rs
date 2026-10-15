use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};

use super::{IndexError, io_error, schema, timestamp_text};

/// The longest project id [`AppDb::add_project`] makes, before any `-2`
/// suffix.
const MAX_SLUG_LEN: usize = 32;

/// A project in the project list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// A short slug such as `tandem`, used in the MCP URL and as the name of
    /// the project's database.
    pub id: String,
    pub name: String,
    /// The repo root, canonicalized.
    pub repo_path: PathBuf,
    /// In [`timestamp_text`] form.
    pub last_opened_at: Option<String>,
}

/// `app.db` in the app's data directory: the list of projects.
pub struct AppDb {
    conn: Connection,
}

impl AppDb {
    /// Opens `<data_dir>/app.db`, creating the folder and database if needed.
    pub fn open(data_dir: &Path) -> Result<Self, IndexError> {
        let conn = schema::open(&data_dir.join("app.db"), schema::APP_MIGRATIONS)?;
        Ok(Self { conn })
    }

    /// Adds the repo at `repo_path` to the list, named after its folder,
    /// with an id made from that name. Returns the existing project if the
    /// repo is already in the list.
    pub fn add_project(&mut self, repo_path: &Path) -> Result<Project, IndexError> {
        let repo_path = repo_path.canonicalize().map_err(io_error(repo_path))?;
        let path_text = repo_path
            .to_str()
            .ok_or_else(|| IndexError::NonUtf8Path {
                path: repo_path.clone(),
            })?
            .to_owned();
        let name = repo_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path_text.clone());

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = tx
            .query_row(
                "SELECT id, name, repo_path, last_opened_at FROM projects WHERE repo_path = ?1",
                [&path_text],
                project_from_row,
            )
            .optional()?;
        if let Some(project) = existing {
            return Ok(project);
        }

        let base = slug(&name);
        let mut id = base.clone();
        let mut n = 1;
        while tx
            .query_row("SELECT 1 FROM projects WHERE id = ?1", [&id], |_| Ok(()))
            .optional()?
            .is_some()
        {
            n += 1;
            id = format!("{base}-{n}");
        }
        tx.execute(
            "INSERT INTO projects (id, name, repo_path) VALUES (?1, ?2, ?3)",
            params![id, name, path_text],
        )?;
        tx.commit()?;
        Ok(Project {
            id,
            name,
            repo_path,
            last_opened_at: None,
        })
    }

    /// Every project, the most recently opened first.
    pub fn projects(&self) -> Result<Vec<Project>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, repo_path, last_opened_at FROM projects
             ORDER BY last_opened_at IS NULL, last_opened_at DESC, name, id",
        )?;
        let projects = stmt
            .query_map([], project_from_row)?
            .collect::<Result<_, _>>()?;
        Ok(projects)
    }

    pub fn project(&self, id: &str) -> Result<Option<Project>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, name, repo_path, last_opened_at FROM projects WHERE id = ?1",
                [id],
                project_from_row,
            )
            .optional()?)
    }

    /// Records that the project was opened at `at`. Returns `false` if there
    /// is no such project.
    pub fn mark_opened(&self, id: &str, at: Timestamp) -> Result<bool, IndexError> {
        let updated = self.conn.execute(
            "UPDATE projects SET last_opened_at = ?2 WHERE id = ?1",
            params![id, timestamp_text(at)],
        )?;
        Ok(updated > 0)
    }

    /// Removes the project from the list. Its repo and database are left
    /// alone. Returns `false` if there is no such project.
    pub fn remove_project(&self, id: &str) -> Result<bool, IndexError> {
        Ok(self
            .conn
            .execute("DELETE FROM projects WHERE id = ?1", [id])?
            > 0)
    }
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        repo_path: PathBuf::from(row.get::<_, String>(2)?),
        last_opened_at: row.get(3)?,
    })
}

/// Whether `id` can be a project id: lowercase ASCII letters, digits and
/// dashes, not starting or ending with a dash. Ids name database files and
/// appear in URLs, so nothing else is allowed.
pub fn is_valid_project_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && !id.starts_with('-')
        && !id.ends_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A project id from a name: lowercase ASCII letters and digits, with every
/// other run of characters turned into one dash.
fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.chars() {
        if slug.len() == MAX_SLUG_LEN {
            break;
        }
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-');
    if slug.is_empty() {
        "project".to_owned()
    } else {
        slug.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("tandem"), "tandem");
        assert_eq!(slug("My Repo.rs"), "my-repo-rs");
        assert_eq!(slug("--a__b--"), "a-b");
        assert_eq!(slug("tâche"), "t-che");
        assert_eq!(slug("日本"), "project");
        assert_eq!(slug(&"a".repeat(40)), "a".repeat(MAX_SLUG_LEN));
        assert_eq!(slug(&"abc-".repeat(20)).len(), MAX_SLUG_LEN - 1);
        for name in ["tandem", "My Repo.rs", "--a__b--", "日本", &"a-".repeat(40)] {
            assert!(is_valid_project_id(&slug(name)), "{name}");
        }
    }

    #[test]
    fn project_ids() {
        assert!(is_valid_project_id("tandem-2"));
        for id in ["", "-a", "a-", "A", "a/b", "..", "a b", &"a".repeat(65)] {
            assert!(!is_valid_project_id(id), "{id}");
        }
    }
}
