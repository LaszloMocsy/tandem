//! The task service: the one entry point for every change, used by both the
//! board and the MCP server. There is one instance per project, and every
//! call holds the project's lock and runs in one `BEGIN IMMEDIATE`
//! transaction, so checks and changes land together even if another
//! process shares the database.

mod backup;
mod delete;
mod error;
mod export;
mod import;
mod read;
mod rows;
mod transitions;

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::db::{Changes, ProjectDb, from_millis, millis, timestamp_text};
use crate::model::{Criterion, Date, Priority, Status, TaskId, Unmet, UnmetCriterion};

pub use delete::{DeleteCheck, DeletedTask};
pub use error::ServiceError;
pub use export::{EXPORT_FORMAT, EXPORT_VERSION};
pub use import::ImportReport;

/// The default lease length.
pub const LEASE_DURATION: SignedDuration = SignedDuration::from_mins(15);

/// Where the service gets the current time from.
pub type Clock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

/// Settings for a [`Service`].
#[derive(Clone)]
pub struct Config {
    /// How long a lease lasts after the holder's latest call.
    pub lease_duration: SignedDuration,
    /// The name events use for you, such as your git user name.
    pub user_name: String,
    pub clock: Clock,
    /// The time zone that decides the dates shown for events, `created`
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
    /// Such as `claude-code-7f2a`. Shown in the task's history.
    pub id: String,
    /// From MCP `clientInfo`, such as `claude-code`.
    pub client_name: Option<String>,
}

/// An agent as the Agents panel shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentInfo {
    pub agent_id: String,
    /// From MCP `clientInfo`, such as `claude-code`.
    pub client_name: Option<String>,
    /// In [`timestamp_text`] form.
    pub last_seen_at: String,
    /// The tasks it holds a lease on, by id.
    pub tasks: Vec<HeldTask>,
}

/// A task an agent holds a lease on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldTask {
    pub id: TaskId,
    pub title: String,
    /// `false` once the lease has expired.
    pub live: bool,
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
    pub created: Date,
    pub lease: Option<Lease>,
    pub blocked: Option<String>,
    /// The criteria the agent left unmet, with reasons. Only filled in for
    /// tasks in `finished`, where the Review card shows them.
    pub unmet: Vec<UnmetCriterion>,
}

/// A task as `get_task` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDetail {
    pub summary: TaskSummary,
    /// The whole task in the Markdown format, changelog included.
    pub markdown: String,
    pub parent: Option<TaskSummary>,
    pub children: Vec<TaskSummary>,
    /// Oldest first.
    pub work_log: Vec<LogEntry>,
    /// Your latest reopen feedback, for the next agent.
    pub review_feedback: Option<String>,
    pub content: TaskContent,
}

/// What the detail pane shows of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskContent {
    /// Markdown; empty if there is none.
    pub description: String,
    pub criteria: Vec<Criterion>,
    pub relevant_files: Vec<String>,
    /// Oldest first.
    pub changelog: Vec<ChangelogItem>,
    pub reviewed_at: Option<Date>,
}

/// One entry of a task's changelog: every event except working-log notes
/// and blocked flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogItem {
    pub date: Date,
    /// The agent id, or your name.
    pub who: String,
    /// Such as `created`, `started` or `finished`.
    pub event: String,
    /// The summary and any `Files:`, `Follow-ups:` and `Unmet:` lines.
    pub text: String,
}

/// One working-log entry.
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
    /// Your reopen feedback.
    Feedback,
}

impl LogKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Blocked => "blocked",
            Self::Unblocked => "unblocked",
            Self::Feedback => "feedback",
        }
    }

    /// The working-log kind of an event, for the events the log shows.
    fn of_event(kind: &str) -> Option<Self> {
        match kind {
            NOTE => Some(Self::Note),
            BLOCKED => Some(Self::Blocked),
            UNBLOCKED => Some(Self::Unblocked),
            REOPENED => Some(Self::Feedback),
            _ => None,
        }
    }
}

/// Event kinds the working log shows and the changelog doesn't.
const NOTE: &str = "note";
const BLOCKED: &str = "blocked";
const UNBLOCKED: &str = "unblocked";
/// Shown in both: the changelog entry, and your feedback in the log.
const REOPENED: &str = "reopened";

/// Filters for `list_tasks`. Every filter that is set must match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskFilter {
    pub status: Option<Status>,
    pub label: Option<String>,
    pub parent: Option<TaskId>,
    /// Matches the id, title, labels or description, ignoring case.
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
    /// `medium` when not given.
    pub priority: Option<Priority>,
    pub parent: Option<TaskId>,
    pub labels: Vec<String>,
    pub depends_on: Vec<TaskId>,
}

/// The fields `update_task` changes; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskEdit {
    pub title: Option<String>,
    /// A blank value clears it.
    pub description: Option<String>,
    /// Replaces every criterion. One with the same text as a ticked
    /// criterion stays ticked; new or reworded ones start unticked.
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

/// The task a change saved, and its status after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub id: TaskId,
    pub status: Status,
}

/// The task service for one project.
pub struct Service {
    db: Mutex<ProjectDb>,
    config: Config,
    /// Your row in `actors`. Looked up again after a restore.
    user: AtomicI64,
    subscribers: Mutex<Vec<Sender<Changes>>>,
}

impl Service {
    /// Opens the project's database in `data_dir`, creating it if needed.
    pub fn open(data_dir: &Path, project_id: &str, config: Config) -> Result<Self, ServiceError> {
        Self::new(ProjectDb::open(data_dir, project_id)?, config)
    }

    /// The service for an open database. Your row in `actors` takes the
    /// current [`Config::user_name`].
    pub fn new(db: ProjectDb, config: Config) -> Result<Self, ServiceError> {
        let user = user_row(db.connection(), &config)?;
        Ok(Self {
            db: Mutex::new(db),
            config,
            user: AtomicI64::new(user),
            subscribers: Mutex::new(Vec::new()),
        })
    }

    /// Whether opening created the project's database, so it has no tasks
    /// yet and an old `.tandem/` folder may be worth importing.
    pub fn is_new(&self) -> bool {
        self.lock().created()
    }

    /// A channel that receives the tasks every call changes from then on.
    /// A working-log note or blocked flag counts as a change to its task.
    /// Dropping the receiver unsubscribes.
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
    /// random suffix, such as `claude-code-7f2a`. The id is recorded, so
    /// [`is_known_agent`](Self::is_known_agent) accepts it.
    pub fn register_agent(&self, client_name: Option<&str>) -> Result<String, ServiceError> {
        let base = client_agent_id(client_name.unwrap_or(""));
        let db = self.lock();
        let now = millis((self.config.clock)());
        loop {
            let mut suffix = [0u8; 2];
            getrandom::fill(&mut suffix).expect("the OS random number generator failed");
            let id = format!("{base}-{:02x}{:02x}", suffix[0], suffix[1]);
            if check_agent_id(&id, &self.config.user_name).is_err() {
                continue;
            }
            let inserted = db.connection().execute(
                "INSERT OR IGNORE INTO actors (kind, name, client, first_seen, last_seen)
                 VALUES ('agent', ?1, ?2, ?3, ?3)",
                params![id, client_name, now],
            )?;
            if inserted == 1 {
                return Ok(id);
            }
        }
    }

    /// Whether `id` is an agent the project has seen: issued by
    /// [`register_agent`](Self::register_agent), or named in imported
    /// history.
    pub fn is_known_agent(&self, id: &str) -> Result<bool, ServiceError> {
        let db = self.lock();
        Ok(db
            .connection()
            .query_row(
                "SELECT 1 FROM actors WHERE kind = 'agent' AND name = ?1",
                [id],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    fn lock(&self) -> MutexGuard<'_, ProjectDb> {
        // A panic mid-call rolls its transaction back, so the database is
        // still consistent.
        self.db.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `f` under the project lock: records the call (which extends the
    /// caller's leases), then runs `f` in a `BEGIN IMMEDIATE` transaction
    /// that commits if `f` succeeds.
    fn call<T>(
        &self,
        actor: &Actor,
        f: impl FnOnce(&Call<'_>) -> Result<T, ServiceError>,
    ) -> Result<T, ServiceError> {
        let mut db = self.lock();
        let now = (self.config.clock)();
        // Outside the transaction, so even a refused call extends leases.
        let actor_id = self.touch(db.connection(), actor, now)?;
        let tx = db.begin()?;
        let call = Call {
            tx: &tx,
            actor,
            actor_id,
            now,
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

    /// Records a call and returns the caller's row in `actors`. For an
    /// agent, updates its row and extends every lease it holds, as any tool
    /// call by the lease holder does.
    fn touch(&self, conn: &Connection, actor: &Actor, now: Timestamp) -> Result<i64, ServiceError> {
        let Actor::Agent(agent) = actor else {
            return Ok(self.user.load(Ordering::Relaxed));
        };
        check_agent_id(&agent.id, &self.config.user_name)?;
        let id: i64 = conn.query_row(
            "INSERT INTO actors (kind, name, client, first_seen, last_seen)
             VALUES ('agent', ?1, ?2, ?3, ?3)
             ON CONFLICT (kind, name) DO UPDATE SET
               client = coalesce(excluded.client, client),
               last_seen = excluded.last_seen
             RETURNING id",
            params![agent.id, agent.client_name, millis(now)],
            |row| row.get(0),
        )?;
        conn.execute(
            "UPDATE leases SET expires_at = ?2 WHERE actor = ?1",
            params![id, millis(now + self.config.lease_duration)],
        )?;
        Ok(id)
    }

    /// The date `ms` falls on in the configured time zone.
    fn date(&self, ms: i64) -> Date {
        date_in(&self.config.time_zone, ms)
    }
}

/// Your row in `actors`, added if missing, with its name set to
/// [`Config::user_name`].
fn user_row(conn: &Connection, config: &Config) -> Result<i64, ServiceError> {
    let now = millis((config.clock)());
    let existing: Option<i64> = conn
        .query_row("SELECT id FROM actors WHERE kind = 'user'", [], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(match existing {
        Some(id) => {
            // Only when it differs, so opening writes nothing otherwise.
            conn.execute(
                "UPDATE actors SET name = ?2 WHERE id = ?1 AND name IS NOT ?2",
                params![id, config.user_name],
            )?;
            id
        }
        None => conn.query_row(
            "INSERT INTO actors (kind, name, first_seen, last_seen)
             VALUES ('user', ?1, ?2, ?2) RETURNING id",
            params![config.user_name, now],
            |row| row.get(0),
        )?,
    })
}

/// One call's view of the project, inside its transaction.
struct Call<'a> {
    tx: &'a Transaction<'a>,
    actor: &'a Actor,
    /// The caller's row in `actors`.
    actor_id: i64,
    now: Timestamp,
    config: &'a Config,
    /// What the call changed, sent to subscribers once it commits.
    changes: RefCell<Changes>,
}

impl Call<'_> {
    fn now_ms(&self) -> i64 {
        millis(self.now)
    }

    /// Records that the task changed: sets its `updated_at` and tells
    /// subscribers once the call commits.
    fn changed(&self, seq: i64, id: TaskId) -> Result<(), ServiceError> {
        self.tx
            .prepare_cached("UPDATE tasks SET updated_at = ?2 WHERE seq = ?1")?
            .execute([seq, self.now_ms()])?;
        self.changes.borrow_mut().changed.insert(id);
        Ok(())
    }

    /// Adds an event by the caller to the task's timeline, and records the
    /// change.
    fn event(
        &self,
        seq: i64,
        id: TaskId,
        kind: &str,
        summary: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<(), ServiceError> {
        rows::add_event(
            self.tx,
            seq,
            self.actor_id,
            self.now_ms(),
            kind,
            summary,
            data,
        )?;
        self.changed(seq, id)
    }

    fn lease(&self, seq: i64) -> Result<Option<Lease>, ServiceError> {
        Ok(self
            .tx
            .prepare_cached(
                "SELECT a.name, l.claimed_at, l.expires_at
                 FROM leases l JOIN actors a ON a.id = l.actor WHERE l.task = ?1",
            )?
            .query_row([seq], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .optional()?
            .map(|(agent_id, claimed_at, expires_at)| {
                lease(agent_id, claimed_at, expires_at, self.now)
            }))
    }

    /// Gives the caller a fresh lease on the task.
    fn claim(&self, seq: i64) -> Result<(), ServiceError> {
        self.tx
            .prepare_cached(
                "INSERT OR REPLACE INTO leases (task, actor, claimed_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![
                seq,
                self.actor_id,
                self.now_ms(),
                millis(self.now + self.config.lease_duration)
            ])?;
        Ok(())
    }

    fn release(&self, seq: i64) -> Result<(), ServiceError> {
        self.tx
            .prepare_cached("DELETE FROM leases WHERE task = ?1")?
            .execute([seq])?;
        Ok(())
    }

    /// Checks that an agent holds the task's lease. You may always act.
    fn check_holder(&self, task: &rows::TaskRow) -> Result<(), ServiceError> {
        let Actor::Agent(agent) = self.actor else {
            return Ok(());
        };
        match self.lease(task.seq)? {
            Some(lease) if lease.agent_id == agent.id => Ok(()),
            None if task.status == Status::InProgress
                && self.started_last(task.seq, &agent.id)? =>
            {
                // The lease is gone, but this agent started the task and
                // nothing handed it over since: it carries on.
                self.claim(task.seq)
            }
            other => Err(ServiceError::NotHolder {
                task: task.id,
                holder: other.map(|lease| lease.agent_id),
            }),
        }
    }

    /// Whether `agent` started the task, with nothing handing it over since.
    fn started_last(&self, seq: i64, agent: &str) -> Result<bool, ServiceError> {
        Ok(
            rows::last_hand_over(self.tx, seq)?.is_some_and(|(kind, actor_kind, name)| {
                kind == "started" && actor_kind == "agent" && name == agent
            }),
        )
    }
}

fn lease(agent_id: String, claimed_at: i64, expires_at: i64, now: Timestamp) -> Lease {
    Lease {
        agent_id,
        claimed_at: timestamp_text(from_millis(claimed_at)),
        expires_at: timestamp_text(from_millis(expires_at)),
        live: expires_at > millis(now),
    }
}

fn date_in(time_zone: &TimeZone, ms: i64) -> Date {
    from_millis(ms).to_zoned(time_zone.clone()).date()
}

/// Agent ids appear in a task's history next to your name, so they are
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
