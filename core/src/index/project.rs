use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use jiff::{SignedDuration, Timestamp};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use super::sql::{Json, date};
use super::{IndexError, io_error, is_valid_project_id, schema, timestamp_text};
use crate::graph::Graph;
use crate::store::{
    self, Access, Date, Priority, Status, StoreError, TASK_DIR, TaskId, list_task_files,
    parse_task_file, task_path,
};

/// How long working-log rows of a task whose file is gone are kept.
const WORK_LOG_KEEP: SignedDuration = SignedDuration::from_hours(30 * 24);

/// Where a project's database lives: `<data_dir>/projects/<project-id>.db`.
pub fn project_db_path(data_dir: &Path, project_id: &str) -> PathBuf {
    data_dir.join("projects").join(format!("{project_id}.db"))
}

/// One row of the task index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedTask {
    pub id: TaskId,
    /// Relative to the repo root, such as `.tandem/k3f9x2m.md`.
    pub path: String,
    /// Empty if the file is invalid.
    pub title: String,
    /// `backlog` if the file is invalid.
    pub status: Status,
    /// `medium` if the file has no priority.
    pub priority: Priority,
    pub parent: Option<TaskId>,
    pub labels: Vec<String>,
    pub depends_on: Vec<TaskId>,
    /// `None` only if the file is invalid.
    pub created: Option<Date>,
    pub reviewed_at: Option<Date>,
    /// The body after the frontmatter, or the whole file if it is invalid.
    pub body: String,
    pub problem: Option<Problem>,
}

/// Why a task shows an invalid badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The file failed to parse. It is never overwritten.
    InvalidFile(String),
    /// The task is in a parent or dependency loop, or its parent is missing.
    Graph(String),
}

/// What a scan changed in the index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Rows added or changed, including tasks whose graph problem appeared
    /// or cleared because of a change to another task.
    pub changed: BTreeSet<TaskId>,
    /// Rows deleted because their file is gone.
    pub removed: BTreeSet<TaskId>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }
}

/// A project's database, `projects/<project-id>.db`, and the index of its
/// `.tandem/` folder.
///
/// Change detection follows the spec's rebuild rules: every scan compares
/// each file's mtime and size with the index and re-reads only files where
/// either changed; if the hash is unchanged, only the stored mtime and size
/// are updated. After any change the whole graph is checked again, and
/// leases whose task is no longer `in_progress` are dropped.
pub struct ProjectIndex {
    conn: Connection,
    repo: PathBuf,
    dir: PathBuf,
    db_path: PathBuf,
    access: Access,
}

/// What the index holds about a file, for the stat check.
struct FileState {
    mtime: i64,
    size: i64,
    hash: String,
}

enum Outcome {
    Unchanged,
    Changed,
    Gone,
}

impl ProjectIndex {
    /// Opens the project at `repo`, whose database is in `data_dir`:
    ///
    /// - Gets `.tandem/` ready and checks its format version; see
    ///   [`store::prepare_folder`]. A newer format opens read-only.
    /// - Opens or creates the database, then runs a full scan, which also
    ///   catches every change made while the app was closed.
    pub fn open(data_dir: &Path, project_id: &str, repo: &Path) -> Result<Self, IndexError> {
        if !is_valid_project_id(project_id) {
            return Err(IndexError::InvalidProjectId(project_id.to_owned()));
        }
        Self::open_at(project_db_path(data_dir, project_id), repo)
    }

    fn open_at(db_path: PathBuf, repo: &Path) -> Result<Self, IndexError> {
        let access = store::prepare_folder(repo)?;
        let conn = schema::open(&db_path, schema::PROJECT_MIGRATIONS)?;
        let mut index = Self {
            conn,
            repo: repo.to_owned(),
            dir: repo.join(TASK_DIR),
            db_path,
            access,
        };
        index.rescan()?;
        Ok(index)
    }

    /// Deletes the project database and builds it again from the files, for
    /// the Reset local state button. Leases, flags and working logs are lost.
    pub fn reset(self) -> Result<Self, IndexError> {
        let Self {
            conn,
            repo,
            db_path,
            ..
        } = self;
        conn.close().map_err(|(_, e)| e)?;
        for suffix in ["", "-wal", "-shm"] {
            let mut path = db_path.clone().into_os_string();
            path.push(suffix);
            let path = PathBuf::from(path);
            match fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(io_error(&path)(e)),
                _ => {}
            }
        }
        Self::open_at(db_path, &repo)
    }

    pub fn access(&self) -> &Access {
        &self.access
    }

    /// Whether the app must not write to `.tandem/`, because its format is
    /// newer than this app or unknown.
    pub fn is_read_only(&self) -> bool {
        self.access != Access::ReadWrite
    }

    pub fn repo(&self) -> &Path {
        &self.repo
    }

    /// The `.tandem/` folder.
    pub fn task_dir(&self) -> &Path {
        &self.dir
    }

    /// The database, for the service's leases, flags and working log.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// A full scan of `.tandem/`: on project open, after a burst of watcher
    /// events, and for the periodic sweep. Re-reads files whose mtime or
    /// size changed, deletes rows whose file is gone, and prunes old working
    /// logs of missing tasks.
    pub fn rescan(&mut self) -> Result<Changes, IndexError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changes = rescan_in(&tx, &self.dir)?;
        prune_work_log(&tx, Timestamp::now())?;
        tx.commit()?;
        Ok(changes)
    }

    /// Re-reads one task's file, for a watcher event or after the app writes
    /// it. Uses the same stat and hash checks as [`rescan`](Self::rescan),
    /// so an event for a file the app just wrote and indexed changes nothing.
    pub fn rescan_task(&mut self, id: TaskId) -> Result<Changes, IndexError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changes = rescan_task_in(&tx, &self.dir, id)?;
        tx.commit()?;
        Ok(changes)
    }

    /// A `BEGIN IMMEDIATE` transaction, which takes the write lock up front.
    pub(crate) fn begin(&mut self) -> Result<Transaction<'_>, IndexError> {
        Ok(self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?)
    }

    /// Every indexed task, sorted by id.
    pub fn tasks(&self) -> Result<Vec<IndexedTask>, IndexError> {
        tasks_in(&self.conn)
    }

    pub fn task(&self, id: TaskId) -> Result<Option<IndexedTask>, IndexError> {
        task_in(&self.conn, id)
    }

    /// Deletes the working logs of tasks whose file is gone, once their
    /// latest entry is more than 30 days older than `now`. Every
    /// [`rescan`](Self::rescan) does this with the current time. Returns the
    /// number of rows deleted.
    pub fn prune_work_log(&self, now: Timestamp) -> Result<usize, IndexError> {
        prune_work_log(&self.conn, now)
    }
}

/// [`ProjectIndex::rescan`] inside a transaction, without the working-log
/// pruning.
pub(crate) fn rescan_in(tx: &Transaction<'_>, dir: &Path) -> Result<Changes, IndexError> {
    // A folder that is gone, or a dangling `.tandem` link, holds no tasks.
    let files = match list_task_files(dir) {
        Err(StoreError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
            Vec::new()
        }
        files => files?,
    };
    let known = file_states(tx, None)?;
    let mut changes = Changes::default();
    let mut present = HashSet::with_capacity(files.len());
    for (id, path) in files {
        match index_file(tx, id, &path, known.get(&id))? {
            Outcome::Gone => {}
            Outcome::Unchanged => {
                present.insert(id);
            }
            Outcome::Changed => {
                present.insert(id);
                changes.changed.insert(id);
            }
        }
    }
    for &id in known.keys() {
        if !present.contains(&id) {
            tx.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
            changes.removed.insert(id);
        }
    }
    after_scan(tx, &mut changes)?;
    Ok(changes)
}

/// [`ProjectIndex::rescan_task`] inside a transaction.
pub(crate) fn rescan_task_in(
    tx: &Transaction<'_>,
    dir: &Path,
    id: TaskId,
) -> Result<Changes, IndexError> {
    let path = task_path(dir, id);
    let known = file_states(tx, Some(id))?;
    let is_file = match fs::symlink_metadata(&path) {
        Ok(meta) => meta.is_file(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(io_error(&path)(e)),
    };
    let outcome = if is_file {
        index_file(tx, id, &path, known.get(&id))?
    } else {
        Outcome::Gone
    };
    let mut changes = Changes::default();
    match outcome {
        Outcome::Unchanged => {}
        Outcome::Changed => {
            changes.changed.insert(id);
        }
        Outcome::Gone => {
            if tx.execute("DELETE FROM tasks WHERE id = ?1", [id])? > 0 {
                changes.removed.insert(id);
            }
        }
    }
    after_scan(tx, &mut changes)?;
    Ok(changes)
}

/// The parent and dependency graph of every indexed task.
pub(crate) fn graph_in(conn: &Connection) -> Result<Graph, IndexError> {
    Ok(graph_rows(conn)?.0)
}

/// The graph, plus each task with a valid file and its stored graph problem.
#[allow(clippy::type_complexity)]
fn graph_rows(conn: &Connection) -> Result<(Graph, Vec<(TaskId, Option<String>)>), IndexError> {
    let mut graph = Graph::new();
    let mut valid = Vec::new();
    let mut stmt =
        conn.prepare("SELECT id, parent, depends_on, created = '', parse_error FROM tasks")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: TaskId = row.get(0)?;
        if row.get(3)? {
            // An invalid file has no edges, but still exists as a parent.
            graph.insert(id, None, Vec::new());
            continue;
        }
        let Json(depends_on) = row.get(2)?;
        graph.insert(id, row.get(1)?, depends_on);
        valid.push((id, row.get::<_, Option<String>>(4)?));
    }
    Ok((graph, valid))
}

/// [`ProjectIndex::tasks`] on any connection or transaction.
pub(crate) fn tasks_in(conn: &Connection) -> Result<Vec<IndexedTask>, IndexError> {
    let mut stmt = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY id"))?;
    let tasks = stmt
        .query_map([], task_from_row)?
        .collect::<Result<_, _>>()?;
    Ok(tasks)
}

/// [`ProjectIndex::task`] on any connection or transaction.
pub(crate) fn task_in(conn: &Connection, id: TaskId) -> Result<Option<IndexedTask>, IndexError> {
    Ok(conn
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
            [id],
            task_from_row,
        )
        .optional()?)
}

/// The stored stat and hash of every row, or of one task.
fn file_states(
    tx: &Transaction<'_>,
    only: Option<TaskId>,
) -> Result<HashMap<TaskId, FileState>, IndexError> {
    let mut stmt = tx.prepare(
        "SELECT id, file_mtime, file_size, file_hash FROM tasks
         WHERE ?1 IS NULL OR id = ?1",
    )?;
    let rows = stmt.query_map([only], |row| {
        Ok((
            row.get(0)?,
            FileState {
                mtime: row.get(1)?,
                size: row.get(2)?,
                hash: row.get(3)?,
            },
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Brings one file's row up to date: skipped if its mtime and size match,
/// only the stat updated if its hash matches, otherwise parsed and written.
fn index_file(
    tx: &Transaction<'_>,
    id: TaskId,
    path: &Path,
    known: Option<&FileState>,
) -> Result<Outcome, IndexError> {
    let not_found = |e: &io::Error| e.kind() == io::ErrorKind::NotFound;
    // Stat before reading, so the stored stat is never newer than the
    // content: a write in between shows up as a change on the next scan.
    let (mtime, size) = match fs::metadata(path).and_then(|meta| stat(&meta)) {
        Ok(stat) => stat,
        Err(e) if not_found(&e) => return Ok(Outcome::Gone),
        Err(e) => return Err(io_error(path)(e)),
    };
    if let Some(known) = known
        && (known.mtime, known.size) == (mtime, size)
    {
        return Ok(Outcome::Unchanged);
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if not_found(&e) => return Ok(Outcome::Gone),
        Err(e) => return Err(io_error(path)(e)),
    };
    let hash = sha256_hex(&bytes);
    if let Some(known) = known
        && known.hash == hash
    {
        tx.execute(
            "UPDATE tasks SET file_mtime = ?2, file_size = ?3 WHERE id = ?1",
            params![id, mtime, size],
        )?;
        return Ok(Outcome::Unchanged);
    }

    let rel_path = format!("{TASK_DIR}/{}", id.file_name());
    let sql = "
        INSERT INTO tasks (id, path, title, status, priority, parent, labels, depends_on,
                           created, reviewed_at, body, file_mtime, file_size, file_hash,
                           parse_error)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
        ON CONFLICT (id) DO UPDATE SET
          path = excluded.path, title = excluded.title, status = excluded.status,
          priority = excluded.priority, parent = excluded.parent, labels = excluded.labels,
          depends_on = excluded.depends_on, created = excluded.created,
          reviewed_at = excluded.reviewed_at, body = excluded.body,
          file_mtime = excluded.file_mtime, file_size = excluded.file_size,
          file_hash = excluded.file_hash, parse_error = excluded.parse_error";
    match parse_task_file(id, &bytes) {
        Ok(task) => {
            let depends_on: Vec<&str> = task.depends_on.iter().map(TaskId::as_str).collect();
            // `parse_error` is filled in by the graph check that follows.
            tx.execute(
                sql,
                params![
                    id,
                    rel_path,
                    task.title,
                    task.status,
                    task.priority.unwrap_or(Priority::Medium),
                    task.parent,
                    serde_json::to_string(&task.labels).expect("strings serialize"),
                    serde_json::to_string(&depends_on).expect("strings serialize"),
                    task.created.to_string(),
                    task.reviewed_at.map(|d| d.to_string()),
                    task.body,
                    mtime,
                    size,
                    hash,
                    None::<String>,
                ],
            )?;
        }
        Err(error) => {
            tx.execute(
                sql,
                params![
                    id,
                    rel_path,
                    "",
                    Status::Backlog,
                    Priority::Medium,
                    None::<TaskId>,
                    "[]",
                    "[]",
                    "",
                    None::<String>,
                    String::from_utf8_lossy(&bytes),
                    mtime,
                    size,
                    hash,
                    error.to_string(),
                ],
            )?;
        }
    }
    Ok(Outcome::Changed)
}

/// Runs after every scan: if anything changed, checks the whole graph again
/// and records each task's loop or missing parent in `parse_error`. Then
/// drops leases whose task is no longer `in_progress` in its file. Leases on
/// invalid files are kept, since their status is unknown.
fn after_scan(tx: &Transaction<'_>, changes: &mut Changes) -> Result<(), IndexError> {
    if !changes.is_empty() {
        check_graph(tx, changes)?;
    }
    tx.execute(
        "DELETE FROM leases WHERE task_id NOT IN
           (SELECT id FROM tasks WHERE status = 'in_progress' OR created = '')",
        [],
    )?;
    Ok(())
}

/// The full-graph pass, linear in the number of tasks and edges.
fn check_graph(tx: &Transaction<'_>, changes: &mut Changes) -> Result<(), IndexError> {
    let (graph, valid) = graph_rows(tx)?;
    let invalid = graph.validate();
    let mut update = tx.prepare("UPDATE tasks SET parse_error = ?2 WHERE id = ?1")?;
    for (id, old) in valid {
        let new = invalid.get(&id).map(|reasons| {
            let reasons: Vec<String> = reasons.iter().map(ToString::to_string).collect();
            reasons.join("; ")
        });
        if new != old {
            update.execute(params![id, new])?;
            changes.changed.insert(id);
        }
    }
    Ok(())
}

fn prune_work_log(conn: &Connection, now: Timestamp) -> Result<usize, IndexError> {
    let cutoff = timestamp_text(
        now.saturating_sub(WORK_LOG_KEEP)
            .expect("no calendar units"),
    );
    Ok(conn.execute(
        "DELETE FROM work_log WHERE task_id IN (
           SELECT task_id FROM work_log
           WHERE task_id NOT IN (SELECT id FROM tasks)
           GROUP BY task_id
           HAVING MAX(created_at) < ?1)",
        [cutoff],
    )?)
}

const TASK_COLUMNS: &str = "id, path, title, status, priority, parent, labels, depends_on, \
                            created, reviewed_at, body, parse_error";

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<IndexedTask> {
    let created: String = row.get(8)?;
    let reviewed_at: Option<String> = row.get(9)?;
    let parse_error: Option<String> = row.get(11)?;
    let (created, problem) = if created.is_empty() {
        (
            None,
            Some(Problem::InvalidFile(parse_error.unwrap_or_default())),
        )
    } else {
        (Some(date(&created)?), parse_error.map(Problem::Graph))
    };
    Ok(IndexedTask {
        id: row.get(0)?,
        path: row.get(1)?,
        title: row.get(2)?,
        status: row.get(3)?,
        priority: row.get(4)?,
        parent: row.get(5)?,
        labels: row.get::<_, Json<_>>(6)?.0,
        depends_on: row.get::<_, Json<_>>(7)?.0,
        created,
        reviewed_at: reviewed_at.as_deref().map(date).transpose()?,
        body: row.get(10)?,
        problem,
    })
}

/// A file's mtime in ms since the epoch, and its size in bytes.
fn stat(meta: &fs::Metadata) -> io::Result<(i64, i64)> {
    let mtime = match meta.modified()?.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_millis() as i64,
        Err(before) => -(before.duration().as_millis() as i64),
    };
    Ok((mtime, meta.len() as i64))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(hex, "{byte:02x}").expect("writing to a String never fails");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_as_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn timestamps_have_a_fixed_width() {
        let at: Timestamp = "2026-10-15T09:30:00Z".parse().unwrap();
        assert_eq!(timestamp_text(at), "2026-10-15T09:30:00.000Z");
        let at: Timestamp = "2026-10-15T09:30:00.5Z".parse().unwrap();
        assert_eq!(timestamp_text(at), "2026-10-15T09:30:00.500Z");
    }
}
