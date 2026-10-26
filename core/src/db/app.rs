use std::path::{Path, PathBuf};

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, TransactionBehavior, params};

use super::backup::{backup_dir, backups_owner};
use super::{DB_DIR, DbError, from_millis, io_error, millis, schema, timestamp_text};

/// The longest project id [`AppDb::add_project`] makes, before any `-2`
/// suffix.
const MAX_SLUG_LEN: usize = 32;

/// The columns [`project_from_row`] reads.
const PROJECT_COLUMNS: &str = "id, name, git_dir, last_path, last_opened_at";

/// A project in the project list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// A short slug such as `tandem`, used in the MCP URL and as the name of
    /// the project's database.
    pub id: String,
    pub name: String,
    /// What identifies the project: the repo's git common dir, shared by
    /// all its worktrees, or the folder itself outside git. Canonicalized.
    pub git_dir: PathBuf,
    /// The checkout or folder last opened, canonicalized.
    pub last_path: PathBuf,
    /// In [`timestamp_text`] form.
    pub last_opened_at: Option<String>,
}

/// `app.db` in the app's data directory: the list of projects and the app's
/// settings.
pub struct AppDb {
    conn: Connection,
    path: PathBuf,
    data_dir: PathBuf,
}

/// Where the app's database lives: `<data_dir>/db/app.db`.
pub fn app_db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DB_DIR).join("app.db")
}

impl AppDb {
    /// Opens [`app_db_path`], creating the folder and database if needed.
    pub fn open(data_dir: &Path) -> Result<Self, DbError> {
        let path = app_db_path(data_dir);
        let (conn, _) = schema::open(&path, schema::APP_MIGRATIONS)?;
        Ok(Self {
            conn,
            path,
            data_dir: data_dir.to_owned(),
        })
    }

    /// Starts a transaction, refused with [`DbError::NewerSchema`] once a
    /// newer app has migrated `app.db`.
    fn begin(&self, behavior: TransactionBehavior) -> Result<Transaction<'_>, DbError> {
        let tx = Transaction::new_unchecked(&self.conn, behavior)?;
        schema::check_version(&tx, &self.path, schema::APP_MIGRATIONS)?;
        Ok(tx)
    }

    /// Runs one statement in a write transaction. Returns the rows changed.
    fn execute(&self, sql: &str, params: impl rusqlite::Params) -> Result<usize, DbError> {
        let tx = self.begin(TransactionBehavior::Immediate)?;
        let changed = tx.execute(sql, params)?;
        tx.commit()?;
        Ok(changed)
    }

    /// Adds the project identified by `git_dir`, opened at `path`, to the
    /// list, named `name` with an id made from it. If a project with that
    /// `git_dir` is in the list already, as when you open another worktree
    /// of the same repo, returns it instead, with `last_path` set to `path`.
    /// A new project never takes an id whose backups belong to another
    /// project, as a removed project's last backup does.
    pub fn add_project(
        &mut self,
        git_dir: &Path,
        path: &Path,
        name: &str,
    ) -> Result<Project, DbError> {
        let git_dir = canonical_text(git_dir)?;
        let path = canonical_text(path)?;

        let tx = self.begin(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "UPDATE projects SET last_path = ?2 WHERE git_dir = ?1 RETURNING id",
                params![git_dir, path],
                |row| row.get(0),
            )
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                let base = slug(name);
                let mut id = base.clone();
                let mut n = 1;
                let taken = |id: &str| -> Result<bool, DbError> {
                    let listed = tx
                        .query_row("SELECT 1 FROM projects WHERE id = ?1", [id], |_| Ok(()))
                        .optional()?
                        .is_some();
                    let backups = backups_owner(&backup_dir(&self.data_dir, id));
                    Ok(listed || backups.is_some_and(|owner| owner != git_dir))
                };
                while taken(&id)? {
                    n += 1;
                    id = format!("{base}-{n}");
                }
                tx.execute(
                    "INSERT INTO projects (id, name, git_dir, last_path) VALUES (?1, ?2, ?3, ?4)",
                    params![id, name, git_dir, path],
                )?;
                id
            }
        };
        let project = tx.query_row(
            &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"),
            [&id],
            project_from_row,
        )?;
        tx.commit()?;
        Ok(project)
    }

    /// Every project, the most recently opened first.
    pub fn projects(&self) -> Result<Vec<Project>, DbError> {
        let tx = self.begin(TransactionBehavior::Deferred)?;
        let mut stmt = tx.prepare(&format!(
            "SELECT {PROJECT_COLUMNS} FROM projects
                 ORDER BY last_opened_at IS NULL, last_opened_at DESC, name, id"
        ))?;
        let projects = stmt
            .query_map([], project_from_row)?
            .collect::<Result<_, _>>()?;
        Ok(projects)
    }

    pub fn project(&self, id: &str) -> Result<Option<Project>, DbError> {
        Ok(self
            .begin(TransactionBehavior::Deferred)?
            .query_row(
                &format!("SELECT {PROJECT_COLUMNS} FROM projects WHERE id = ?1"),
                [id],
                project_from_row,
            )
            .optional()?)
    }

    /// Records that the project was opened at `at`. Returns `false` if there
    /// is no such project.
    pub fn mark_opened(&self, id: &str, at: Timestamp) -> Result<bool, DbError> {
        let updated = self.execute(
            "UPDATE projects SET last_opened_at = ?2 WHERE id = ?1",
            params![id, millis(at)],
        )?;
        Ok(updated > 0)
    }

    /// Renames the project. Returns `false` if there is no such project.
    pub fn set_name(&self, id: &str, name: &str) -> Result<bool, DbError> {
        let updated = self.execute(
            "UPDATE projects SET name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        Ok(updated > 0)
    }

    /// The app's settings. A missing or unreadable value gets its default.
    pub fn settings(&self) -> Result<AppSettings, DbError> {
        let mut settings = AppSettings::default();
        let tx = self.begin(TransactionBehavior::Deferred)?;
        let mut stmt = tx.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (key, value) = row?;
            match key.as_str() {
                MCP_PORT => settings.mcp_port = value.parse().ok().filter(|&port| port != 0),
                THEME => settings.theme = value.parse().unwrap_or_default(),
                _ => {}
            }
        }
        Ok(settings)
    }

    /// Sets the MCP server's port; `None` goes back to the default.
    pub fn set_mcp_port(&self, port: Option<u16>) -> Result<(), DbError> {
        self.set_setting(MCP_PORT, port.map(|port| port.to_string()).as_deref())
    }

    pub fn set_theme(&self, theme: Theme) -> Result<(), DbError> {
        let value = (theme != Theme::System).then(|| theme.as_str());
        self.set_setting(THEME, value)
    }

    /// Stores a setting, or deletes it when `value` is `None`.
    fn set_setting(&self, key: &str, value: Option<&str>) -> Result<(), DbError> {
        match value {
            Some(value) => self.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?,
            None => self.execute("DELETE FROM settings WHERE key = ?1", [key])?,
        };
        Ok(())
    }

    /// Removes the project from the list. Its database is left alone.
    /// Returns `false` if there is no such project.
    pub fn remove_project(&self, id: &str) -> Result<bool, DbError> {
        Ok(self.execute("DELETE FROM projects WHERE id = ?1", [id])? > 0)
    }
}

/// The name a new project gets: its folder's name. For a git common dir,
/// that is the folder of the main checkout, `repo` for `repo/.git`, or the
/// bare repo's name without `.git`.
pub fn folder_name(git_dir: &Path) -> String {
    let folder = match git_dir.file_name() {
        Some(name) if name == ".git" => git_dir.parent().unwrap_or(git_dir),
        _ => git_dir,
    };
    match folder.file_name() {
        Some(name) => {
            let name = name.to_string_lossy();
            match name.strip_suffix(".git") {
                Some(bare) if !bare.is_empty() => bare.to_owned(),
                _ => name.into_owned(),
            }
        }
        None => folder.display().to_string(),
    }
}

/// The longest project name, in characters.
pub const MAX_NAME_LEN: usize = 100;

/// Why a project name cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("the project name is longer than {MAX_NAME_LEN} characters")]
    TooLong,
    #[error("the project name cannot contain line breaks or control characters")]
    ControlCharacter,
}

/// A project name as it is stored: trimmed, and `None` if blank, which
/// means the folder name.
pub fn normalize_project_name(name: &str) -> Result<Option<String>, NameError> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().any(char::is_control) {
        return Err(NameError::ControlCharacter);
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(NameError::TooLong);
    }
    Ok(Some(name.to_owned()))
}

/// `path` canonicalized, as text.
fn canonical_text(path: &Path) -> Result<String, DbError> {
    let path = path.canonicalize().map_err(io_error(path))?;
    path.to_str()
        .map(str::to_owned)
        .ok_or(DbError::NonUtf8Path { path })
}

const MCP_PORT: &str = "mcp_port";
const THEME: &str = "theme";

/// The settings for the whole app, kept in `app.db`, never in a repo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppSettings {
    /// The MCP server's port. `None` means the default.
    pub mcp_port: Option<u16>,
    pub theme: Theme,
}

/// The app's colour theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Theme {
    /// Follow the system's light or dark appearance.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

impl std::str::FromStr for Theme {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "system" => Ok(Self::System),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            _ => Err(format!("`{s}` is not a theme; use system, light or dark")),
        }
    }
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        git_dir: PathBuf::from(row.get::<_, String>(2)?),
        last_path: PathBuf::from(row.get::<_, String>(3)?),
        last_opened_at: row
            .get::<_, Option<i64>>(4)?
            .map(|ms| timestamp_text(from_millis(ms))),
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
    fn project_names() {
        assert_eq!(
            normalize_project_name("  Tandem "),
            Ok(Some("Tandem".into()))
        );
        assert_eq!(normalize_project_name(" \t"), Ok(None));
        assert_eq!(
            normalize_project_name("a\nb"),
            Err(NameError::ControlCharacter)
        );
        assert_eq!(
            normalize_project_name(&"é".repeat(MAX_NAME_LEN + 1)),
            Err(NameError::TooLong)
        );
        assert!(normalize_project_name(&"é".repeat(MAX_NAME_LEN)).is_ok());
    }

    #[test]
    fn folder_names() {
        assert_eq!(folder_name(Path::new("/code/tandem/.git")), "tandem");
        assert_eq!(folder_name(Path::new("/code/tandem.git")), "tandem");
        assert_eq!(folder_name(Path::new("/code/notes")), "notes");
        assert_eq!(folder_name(Path::new("/code/.git.git")), ".git");
    }

    #[test]
    fn project_ids() {
        assert!(is_valid_project_id("tandem-2"));
        for id in ["", "-a", "a-", "A", "a/b", "..", "a b", &"a".repeat(65)] {
            assert!(!is_valid_project_id(id), "{id}");
        }
    }
}
