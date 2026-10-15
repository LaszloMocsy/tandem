//! The task service: the one entry point for every change, used by both the
//! board and the MCP server. There is one instance per project, and every
//! call holds the project's lock, so writes are serialized.
//!
//! Transitions follow "State model" in the spec. Each one reads the task
//! file inside a `BEGIN IMMEDIATE` transaction, so the file and the leases
//! are checked and changed together even if another process shares the
//! database. Files are written before the transaction commits; if the
//! commit fails, the next rescan brings the index back in line.

mod error;
mod read;
mod transitions;

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::index::{Changes, Problem, ProjectIndex, rescan_in, rescan_task_in, timestamp_text};
use crate::store::{
    Access, Date, Priority, ReadOnly, Status, StoreError, TASK_DIR, Task, TaskId, Unmet,
    parse_task_file, task_path,
};

pub use error::ServiceError;

/// The default lease length.
pub const LEASE_DURATION: SignedDuration = SignedDuration::from_mins(15);

/// Where the service gets the current time from.
pub type Clock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

/// Settings for a [`Service`].
#[derive(Clone)]
pub struct Config {
    /// How long a lease lasts after the holder's latest call.
    pub lease_duration: SignedDuration,
    /// The name changelog entries use for you, such as your git user name.
    pub user_name: String,
    pub clock: Clock,
    /// The time zone that decides today's date for changelogs, `created`
    /// and `reviewed_at`.
    pub time_zone: TimeZone,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            lease_duration: LEASE_DURATION,
            user_name: "user".to_owned(),
            clock: Arc::new(Timestamp::now),
            time_zone: TimeZone::system(),
        }
    }
}

/// Who is making a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    /// You, from the board.
    User,
    /// An agent, through MCP.
    Agent(Agent),
}

impl Actor {
    /// An agent with this id and no client name.
    pub fn agent(id: impl Into<String>) -> Self {
        Self::Agent(Agent {
            id: id.into(),
            client_name: None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// Such as `claude-code-7f2a`. Shown in changelog headings.
    pub id: String,
    /// From MCP `clientInfo`, such as `claude-code`.
    pub client_name: Option<String>,
}

/// A task's lease.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub agent_id: String,
    /// In [`timestamp_text`] form, like `expires_at`.
    pub claimed_at: String,
    pub expires_at: String,
    /// `false` once the lease has expired: it is stale, and another agent
    /// can take the task over.
    pub live: bool,
}

/// A task as `list_tasks` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    pub id: TaskId,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub parent: Option<TaskId>,
    pub labels: Vec<String>,
    pub depends_on: Vec<TaskId>,
    pub lease: Option<Lease>,
    pub blocked: Option<String>,
    pub problem: Option<Problem>,
}

/// A task as `get_task` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDetail {
    pub summary: TaskSummary,
    /// The task file, as it is on disk.
    pub markdown: String,
    pub parent: Option<TaskSummary>,
    pub children: Vec<TaskSummary>,
    /// Oldest first.
    pub work_log: Vec<LogEntry>,
    /// Your latest reopen feedback, for the next agent.
    pub review_feedback: Option<String>,
}

/// One working-log row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// `None` means you.
    pub agent_id: Option<String>,
    pub kind: LogKind,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    Note,
    Blocked,
    Unblocked,
    Feedback,
    Event,
}

impl LogKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Blocked => "blocked",
            Self::Unblocked => "unblocked",
            Self::Feedback => "feedback",
            Self::Event => "event",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        [
            Self::Note,
            Self::Blocked,
            Self::Unblocked,
            Self::Feedback,
            Self::Event,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == s)
    }
}

/// Filters for `list_tasks`. Every filter that is set must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskFilter {
    pub status: Option<Status>,
    pub label: Option<String>,
    pub parent: Option<TaskId>,
    /// Matches the id, title, labels or body, ignoring case.
    pub query: Option<String>,
    /// Only tasks `start_task` would accept: in todo, every dependency
    /// done, and no live lease.
    pub ready_only: bool,
}

/// A new task for `create_task`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewTask {
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub priority: Option<Priority>,
    pub parent: Option<TaskId>,
    pub labels: Vec<String>,
    pub depends_on: Vec<TaskId>,
}

/// The fields `update_task` changes; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskEdit {
    pub title: Option<String>,
    /// A blank value removes the section.
    pub description: Option<String>,
    /// Replaces every criterion, unticked. An empty list removes the
    /// section.
    pub acceptance_criteria: Option<Vec<String>>,
    pub relevant_files: Option<Vec<String>>,
    pub priority: Option<Priority>,
    /// `Some(None)` moves the task to the top level.
    pub parent: Option<Option<TaskId>>,
    pub labels: Option<Vec<String>>,
    pub depends_on: Option<Vec<TaskId>>,
}

/// The arguments of `finish_task`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Finish {
    pub summary: String,
    pub files_changed: Vec<String>,
    pub follow_ups: Option<String>,
    /// Acceptance criteria met, by number; the app ticks them.
    pub met: Vec<u32>,
    /// Every criterion still unticked, with the reason.
    pub unmet: Vec<Unmet>,
}

/// The file a write changed. Every file-writing call returns it, so agents
/// know to include it in their next commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub id: TaskId,
    /// Relative to the repo root, such as `.tandem/k3f9x2m.md`.
    pub path: String,
    pub status: Status,
}

/// The task service for one project.
pub struct Service {
    index: Mutex<ProjectIndex>,
    dir: PathBuf,
    read_only: Option<ReadOnly>,
    config: Config,
    subscribers: Mutex<Vec<Sender<Changes>>>,
}

impl Service {
    /// Opens the project and its index; see [`ProjectIndex::open`].
    pub fn open(
        data_dir: &Path,
        project_id: &str,
        repo: &Path,
        config: Config,
    ) -> Result<Self, ServiceError> {
        Ok(Self::new(
            ProjectIndex::open(data_dir, project_id, repo)?,
            config,
        ))
    }

    pub fn new(index: ProjectIndex, config: Config) -> Self {
        let read_only = match index.access() {
            Access::ReadWrite => None,
            Access::ReadOnly(reason) => Some(reason.clone()),
        };
        Self {
            dir: index.task_dir().to_owned(),
            index: Mutex::new(index),
            read_only,
            config,
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// A channel that receives the tasks changed by every index change
    /// from then on: writes through the service, and outside edits found
    /// by the watcher or a rescan. A working-log note or blocked flag
    /// counts as a change to its task. Dropping the receiver unsubscribes.
    pub fn subscribe(&self) -> Receiver<Changes> {
        let (tx, rx) = mpsc::channel();
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(tx);
        rx
    }

    fn emit(&self, changes: Changes) {
        if changes.is_empty() {
            return;
        }
        self.subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|tx| tx.send(changes.clone()).is_ok());
    }

    /// Issues a new agent id for an MCP client that starts a task: the
    /// client's name in lowercase ASCII letters, digits and dashes, plus a
    /// random suffix, such as `claude-code-7f2a`. The id is recorded in
    /// `agents`, so [`is_known_agent`](Self::is_known_agent) accepts it.
    pub fn register_agent(&self, client_name: Option<&str>) -> Result<String, ServiceError> {
        let base = client_agent_id(client_name.unwrap_or(""));
        let index = self.lock();
        let now = timestamp_text((self.config.clock)());
        loop {
            let mut suffix = [0u8; 2];
            getrandom::fill(&mut suffix).expect("the OS random number generator failed");
            let id = format!("{base}-{:02x}{:02x}", suffix[0], suffix[1]);
            if check_agent_id(&id, &self.config.user_name).is_err() {
                continue;
            }
            let inserted = index.connection().execute(
                "INSERT OR IGNORE INTO agents (agent_id, client_name, last_seen_at)
                 VALUES (?1, ?2, ?3)",
                params![id, client_name, now],
            )?;
            if inserted == 1 {
                return Ok(id);
            }
        }
    }

    /// Whether `id` was issued by [`register_agent`](Self::register_agent).
    pub fn is_known_agent(&self, id: &str) -> Result<bool, ServiceError> {
        Ok(self
            .lock()
            .connection()
            .query_row("SELECT 1 FROM agents WHERE agent_id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// The `.tandem/` folder.
    pub fn task_dir(&self) -> &Path {
        &self.dir
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Why the project is read-only, if it is.
    pub fn read_only(&self) -> Option<&ReadOnly> {
        self.read_only.as_ref()
    }

    /// A full rescan, for a burst of watcher events or the periodic sweep.
    pub fn rescan(&self) -> Result<Changes, ServiceError> {
        let mut index = self.lock();
        let changes = index.rescan()?;
        self.emit(changes.clone());
        Ok(changes)
    }

    /// Re-reads one task's file, for a watcher event. An event for a file
    /// the service just wrote finds the same hash and changes nothing, so
    /// the app's own writes never come back as changes.
    pub fn rescan_task(&self, id: TaskId) -> Result<Changes, ServiceError> {
        let mut index = self.lock();
        let changes = index.rescan_task(id)?;
        self.emit(changes.clone());
        Ok(changes)
    }

    fn lock(&self) -> MutexGuard<'_, ProjectIndex> {
        // A panic mid-call rolls its transaction back, so the index is
        // still consistent.
        self.index.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` under the project lock: records the call (which extends the
    /// caller's leases), then runs `f` in a `BEGIN IMMEDIATE` transaction
    /// that commits if `f` succeeds.
    fn call<T>(
        &self,
        actor: &Actor,
        f: impl FnOnce(&Call<'_>) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        let mut index = self.lock();
        let now = (self.config.clock)();
        // Outside the transaction, so even a refused call extends leases.
        self.touch(index.connection(), actor, now)?;
        let tx = index.begin()?;
        let call = Call {
            tx: &tx,
            dir: &self.dir,
            actor,
            now,
            today: now.to_zoned(self.config.time_zone.clone()).date(),
            config: &self.config,
            changes: RefCell::default(),
        };
        let out = f(&call)?;
        let changes = call.changes.take();
        tx.commit()?;
        // Still under the lock, so subscribers see changes in commit order.
        self.emit(changes);
        Ok(out)
    }

    /// Records an agent's call: updates `agents` and extends every lease the
    /// agent holds, as "any tool call by the lease holder" does.
    fn touch(&self, conn: &Connection, actor: &Actor, now: Timestamp) -> Result<(), ServiceError> {
        let Actor::Agent(agent) = actor else {
            return Ok(());
        };
        check_agent_id(&agent.id, &self.config.user_name)?;
        conn.execute(
            "INSERT INTO agents (agent_id, client_name, last_seen_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (agent_id) DO UPDATE SET
               client_name = coalesce(excluded.client_name, client_name),
               last_seen_at = excluded.last_seen_at",
            params![agent.id, agent.client_name, timestamp_text(now)],
        )?;
        conn.execute(
            "UPDATE leases SET expires_at = ?2 WHERE agent_id = ?1",
            params![agent.id, timestamp_text(now + self.config.lease_duration)],
        )?;
        Ok(())
    }

    fn check_writable(&self) -> Result<(), ServiceError> {
        match &self.read_only {
            Some(reason) => Err(ServiceError::ReadOnly(reason.clone())),
            None => check_folder(&self.dir),
        }
    }
}

/// One call's view of the project, inside its transaction.
struct Call<'a> {
    tx: &'a Transaction<'a>,
    dir: &'a Path,
    actor: &'a Actor,
    now: Timestamp,
    today: Date,
    config: &'a Config,
    /// What the call changed, sent to subscribers once it commits.
    changes: RefCell<Changes>,
}

impl Call<'_> {
    /// Re-indexes one task's file.
    fn scan_task(&self, id: TaskId) -> Result<(), ServiceError> {
        let changes = rescan_task_in(self.tx, self.dir, id)?;
        self.changes.borrow_mut().merge(changes);
        Ok(())
    }

    /// A full stat scan.
    fn scan_all(&self) -> Result<(), ServiceError> {
        let changes = rescan_in(self.tx, self.dir)?;
        self.changes.borrow_mut().merge(changes);
        Ok(())
    }

    /// Records a change to a task's overlays: its working log or flags.
    fn touched(&self, id: TaskId) {
        self.changes.borrow_mut().merge(Changes {
            changed: [id].into(),
            removed: Default::default(),
        });
    }

    /// Re-indexes the task, then reads and parses its file.
    fn read(&self, id: TaskId) -> Result<Task, ServiceError> {
        self.scan_task(id)?;
        read_file(self.dir, id)
    }

    /// Writes the task file and indexes it, so the watcher event for this
    /// write finds the same hash and changes nothing.
    fn write(&self, task: &Task) -> Result<Written, ServiceError> {
        check_folder(self.dir)?;
        crate::store::write_task(self.dir, task)?;
        self.scan_task(task.id)?;
        Ok(written(task))
    }

    fn lease(&self, id: TaskId) -> Result<Option<Lease>, ServiceError> {
        Ok(self
            .tx
            .query_row(
                "SELECT agent_id, claimed_at, expires_at FROM leases WHERE task_id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .map(|(agent_id, claimed_at, expires_at)| {
                lease(agent_id, claimed_at, expires_at, self.now)
            }))
    }

    /// Checks that an agent holds the task's lease. You may always act.
    fn check_holder(&self, id: TaskId) -> Result<(), ServiceError> {
        let Actor::Agent(agent) = self.actor else {
            return Ok(());
        };
        match self.lease(id)? {
            Some(lease) if lease.agent_id == agent.id => Ok(()),
            None if self.started_last(id, &agent.id) => {
                // The lease was lost, as in a database rebuild, but the
                // changelog shows this agent started the task: it carries on.
                self.tx.execute(
                    "INSERT INTO leases (task_id, agent_id, claimed_at, expires_at)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        id,
                        agent.id,
                        timestamp_text(self.now),
                        timestamp_text(self.now + self.config.lease_duration)
                    ],
                )?;
                Ok(())
            }
            other => Err(ServiceError::NotHolder {
                task: id,
                holder: other.map(|lease| lease.agent_id),
            }),
        }
    }

    /// Whether the task is in progress and `agent` started it, with nothing
    /// handing it over since.
    fn started_last(&self, id: TaskId, agent: &str) -> bool {
        read_file(self.dir, id).is_ok_and(|task| {
            task.status == Status::InProgress
                && transitions::last_hand_over(&task)
                    .is_some_and(|entry| entry.event == "started" && entry.who == agent)
        })
    }

    /// The name changelog entries use for the caller.
    fn who(&self) -> &str {
        match self.actor {
            Actor::User => &self.config.user_name,
            Actor::Agent(agent) => &agent.id,
        }
    }

    fn agent_id(&self) -> Option<&str> {
        match self.actor {
            Actor::User => None,
            Actor::Agent(agent) => Some(&agent.id),
        }
    }

    fn log(&self, id: TaskId, kind: LogKind, message: &str) -> Result<(), ServiceError> {
        self.touched(id);
        self.tx.execute(
            "INSERT INTO work_log (task_id, agent_id, kind, message, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                id,
                self.agent_id(),
                kind.as_str(),
                message,
                timestamp_text(self.now)
            ],
        )?;
        Ok(())
    }
}

/// Reads and parses a task file. A symlink or anything else that isn't a
/// regular file is invalid and never followed, since it could point
/// outside the repo.
fn read_file(dir: &Path, id: TaskId) -> Result<Task, ServiceError> {
    let path = task_path(dir, id);
    check_regular_file(&path, id)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(ServiceError::NotFound(id)),
        Err(source) => return Err(StoreError::Io { path, source }.into()),
    };
    parse_task_file(id, &bytes).map_err(|e| ServiceError::InvalidFile {
        task: id,
        message: e.to_string(),
    })
}

fn check_regular_file(path: &Path, id: TaskId) -> Result<(), ServiceError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(_) => Err(ServiceError::InvalidFile {
            task: id,
            message: "the file is a symbolic link or not a regular file".into(),
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Err(ServiceError::NotFound(id)),
        Err(source) => Err(StoreError::Io {
            path: path.to_owned(),
            source,
        }
        .into()),
    }
}

fn written(task: &Task) -> Written {
    Written {
        id: task.id,
        path: format!("{TASK_DIR}/{}", task.id.file_name()),
        status: task.status,
    }
}

fn lease(agent_id: String, claimed_at: String, expires_at: String, now: Timestamp) -> Lease {
    let live = expires_at > timestamp_text(now);
    Lease {
        agent_id,
        claimed_at,
        expires_at,
        live,
    }
}

/// Every lease, by task.
fn leases(conn: &Connection, now: Timestamp) -> Result<HashMap<TaskId, Lease>, ServiceError> {
    let mut stmt = conn.prepare("SELECT task_id, agent_id, claimed_at, expires_at FROM leases")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, TaskId>(0)?,
            lease(row.get(1)?, row.get(2)?, row.get(3)?, now),
        ))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Agent ids appear in changelog headings next to your name, so they are
/// limited to ASCII letters, digits, `-`, `_` and `.`, up to 64 characters,
/// which rules out lookalike letters and invisible characters, and must not
/// be your name in any case.
fn check_agent_id(id: &str, user_name: &str) -> Result<(), ServiceError> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    if !(1..=64).contains(&id.len()) || !id.chars().all(allowed) {
        return Err(ServiceError::InvalidArgument(format!(
            "`{}` is not a valid agent id: use 1 to 64 ASCII letters, digits, `-`, `_` or `.`",
            id.escape_debug()
        )));
    }
    if id.to_lowercase() == user_name.trim().to_lowercase() {
        return Err(ServiceError::InvalidArgument(format!(
            "the agent id `{id}` is the user's name"
        )));
    }
    Ok(())
}

/// The agent id for an MCP call that carries no issued id: the client name
/// in lowercase ASCII letters and digits, with every other run of
/// characters as one dash, such as `claude-code`, or `agent` without a name.
/// Issued ids add a random suffix to it.
pub fn client_agent_id(client_name: &str) -> String {
    let mut base = String::new();
    for c in client_name.chars() {
        if base.len() >= 40 {
            break;
        }
        if c.is_ascii_alphanumeric() {
            base.push(c.to_ascii_lowercase());
        } else if !base.is_empty() && !base.ends_with('-') {
            base.push('-');
        }
    }
    match base.trim_end_matches('-') {
        "" => "agent".to_owned(),
        base => base.to_owned(),
    }
}

/// Checks that `.tandem` is still a real folder before writing to it: it
/// may have been replaced by a symlink since the project was opened.
fn check_folder(dir: &Path) -> Result<(), ServiceError> {
    match fs::symlink_metadata(dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            Err(ServiceError::ReadOnly(ReadOnly::SymlinkedFolder))
        }
        Ok(meta) if !meta.is_dir() => Err(StoreError::Io {
            path: dir.to_owned(),
            source: io::Error::new(io::ErrorKind::NotADirectory, "not a directory"),
        }
        .into()),
        Ok(_) => Ok(()),
        Err(source) => Err(StoreError::Io {
            path: dir.to_owned(),
            source,
        }
        .into()),
    }
}
