//! The MCP tools from the spec's tools table, each a thin layer over
//! [`Service`]: parse the arguments, work out who is calling, run the
//! service call on a blocking thread, and shape the result.
//!
//! Agents are identified by ids the server issues: `start_task` returns one,
//! and the tools that only the lease holder may call require it back as
//! `agent`. Other tools accept it optionally, which renews the agent's
//! lease; without it, a call is attributed to the client name from
//! `clientInfo`.

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, schemars, tool, tool_handler, tool_router};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use tandem_board_core::index::Problem;
use tandem_board_core::service::{
    Actor, Agent, Finish, NewTask, Service, ServiceError, TaskDetail, TaskEdit, TaskFilter,
    TaskSummary, Written, client_agent_id,
};
use tandem_board_core::store::{Priority, Status, TaskId, Unmet};

/// The commit rules, repeated in the descriptions of the tools that write
/// task files, so agents follow them even without an `AGENTS.md`.
macro_rules! commit_rules {
    () => {
        " Commit rules: include the task file in your first commit for the task \
         (status in_progress) and in your final commit (status finished), add a \
         `Task: <id>` trailer to every commit for the task, and never edit task \
         frontmatter or the Changelog section by hand. If the tandem server is \
         unreachable, ask the user to open the Tandem app instead of editing files."
    };
}

/// The server's instructions, sent to every client.
const INSTRUCTIONS: &str = concat!(
    "This repo tracks work in `.tandem/` through these tools. Before starting \
     work, call start_task on the task (use next_task if none was given) and \
     keep the agent id it returns: pass it as `agent` to log_progress, \
     set_blocked, add_changelog_entry, finish_task and release_task, and to \
     any other call to keep your lease alive. Use log_progress for notes and \
     set_blocked when you need a decision from the user. Split large work into \
     child tasks with create_task and parent; new tasks wait in backlog until \
     the user approves them, and only tasks in todo can be started. Call \
     finish_task with a short summary before the final commit, ticking each \
     acceptance criterion you met and explaining each one you didn't. Never \
     mark tasks reviewed.",
    commit_rules!()
);

/// The tool handler for one project.
#[derive(Clone)]
pub struct TandemTools {
    service: Arc<Service>,
    tool_router: ToolRouter<Self>,
}

impl TandemTools {
    pub fn new(service: Arc<Service>) -> Self {
        Self {
            service,
            tool_router: Self::tool_router(),
        }
    }

    /// Runs a service call on a blocking thread: the service does file and
    /// database work and waits on the project lock.
    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Service) -> Result<T, ServiceError> + Send + 'static,
    ) -> Result<T, CallToolResult> {
        let service = Arc::clone(&self.service);
        match tokio::task::spawn_blocking(move || f(&service)).await {
            Ok(result) => result.map_err(service_error),
            Err(join) => Err(error("internal", &join.to_string())),
        }
    }

    /// Who is calling. An `agent` id must be one the server issued;
    /// without it, the call is attributed to the client name.
    async fn actor(
        &self,
        context: &RequestContext<RoleServer>,
        agent: Option<String>,
    ) -> Result<Actor, CallToolResult> {
        let client_name = context.client_info().map(|info| info.name);
        let id = match agent {
            Some(id) => {
                let known = id.clone();
                if !self
                    .run(move |service| service.is_known_agent(&known))
                    .await?
                {
                    return Err(error(
                        "invalid_argument",
                        &format!(
                            "`{id}` is not an agent id this server issued; \
                             call start_task to get one"
                        ),
                    ));
                }
                id
            }
            None => client_agent_id(client_name.as_deref().unwrap_or("")),
        };
        Ok(Actor::Agent(Agent { id, client_name }))
    }
}

#[tool_router]
impl TandemTools {
    #[tool(
        description = "List task summaries: id, title, status, priority, parent, labels, \
                       lease holder and blocked reason. Every filter given must match; \
                       ready_only keeps tasks start_task would accept."
    )]
    async fn list_tasks(
        &self,
        Parameters(args): Parameters<ListTasksArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let actor = self.actor(&context, args.agent).await?;
        let filter = TaskFilter {
            status: args.status.map(Status::from),
            label: args.label,
            parent: args.parent.as_deref().map(task_id).transpose()?,
            query: args.query,
            ready_only: args.ready_only.unwrap_or(false),
        };
        let tasks = self
            .run(move |service| service.list_tasks(&actor, &filter))
            .await?;
        Ok(success(json!({
            "tasks": tasks.iter().map(summary).collect::<Vec<_>>()
        })))
    }

    #[tool(
        description = "Get a task: its full Markdown, its parent and children, the \
                       working log and the user's latest review feedback."
    )]
    async fn get_task(
        &self,
        Parameters(args): Parameters<GetTaskArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let id = task_id(&args.id)?;
        let actor = self.actor(&context, args.agent).await?;
        let detail = self
            .run(move |service| service.get_task(&actor, id))
            .await?;
        Ok(success(detail_json(&detail)))
    }

    #[tool(
        description = "The highest-priority task in todo whose dependencies are all \
                       done and that no agent holds, or null. Start it with start_task."
    )]
    async fn next_task(
        &self,
        Parameters(args): Parameters<NextTaskArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let parent = args.parent.as_deref().map(task_id).transpose()?;
        let actor = self.actor(&context, args.agent).await?;
        let next = self
            .run(move |service| service.next_task(&actor, args.label.as_deref(), parent))
            .await?;
        Ok(success(json!({ "task": next.as_ref().map(summary) })))
    }

    #[tool(
        description = concat!(
            "Create a task. It starts in backlog and waits until the user approves it \
             into todo; only tasks in todo can be started. Use parent to split large \
             work into child tasks. Refused with cycle_detected, parent_not_found or \
             not_found when parent or depends_on is missing or would close a loop. \
             Writes the task file and returns its path.",
            commit_rules!()
        )
    )]
    async fn create_task(
        &self,
        Parameters(args): Parameters<CreateTaskArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let new = NewTask {
            title: args.title,
            description: args.description,
            acceptance_criteria: args.acceptance_criteria,
            priority: args.priority.map(Priority::from),
            parent: args.parent.as_deref().map(task_id).transpose()?,
            labels: args.labels.unwrap_or_default(),
            depends_on: task_ids(args.depends_on.unwrap_or_default())?,
        };
        let actor = self.actor(&context, args.agent).await?;
        let written = self
            .run(move |service| service.create_task(&actor, new))
            .await?;
        Ok(written_result(&written, None, Reminder::Next))
    }

    #[tool(
        description = concat!(
            "Edit a task's spec: title, description, acceptance criteria, relevant \
             files, priority, parent (null moves it to the top level), labels and \
             depends_on. Cannot change status or review. In backlog anything can \
             change. After approval, relevant_files, labels and priority stay open; \
             any other change sends a todo task back to backlog until the user \
             approves it again, and is refused on a task in progress, which only \
             its lease holder may edit. Writes the task file and returns its path.",
            commit_rules!()
        )
    )]
    async fn update_task(
        &self,
        Parameters(args): Parameters<UpdateTaskArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let id = task_id(&args.id)?;
        let edit = TaskEdit {
            title: args.title,
            description: args.description,
            acceptance_criteria: args.acceptance_criteria,
            relevant_files: args.relevant_files,
            priority: args.priority.map(Priority::from),
            parent: args
                .parent
                .map(|parent| parent.as_deref().map(task_id).transpose())
                .transpose()?,
            labels: args.labels,
            depends_on: args.depends_on.map(task_ids).transpose()?,
        };
        let actor = self.actor(&context, args.agent).await?;
        let written = self
            .run(move |service| service.update_task(&actor, id, edit))
            .await?;
        Ok(written_result(&written, None, Reminder::Next))
    }

    #[tool(
        description = concat!(
            "Claim a task in todo and set it in_progress, adding a started changelog \
             entry. Returns your agent id for this task: pass it as agent to \
             log_progress, set_blocked, add_changelog_entry, finish_task and \
             release_task, and to any other call to keep your lease alive. The lease \
             lasts 15 minutes after your latest call. Refused with claimed_by_other, \
             deps_not_done or invalid_transition. Writes the task file and returns \
             its path.",
            commit_rules!()
        )
    )]
    async fn start_task(
        &self,
        Parameters(args): Parameters<StartTaskArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let id = task_id(&args.id)?;
        let client_name = context.client_info().map(|info| info.name);
        let (agent, written) = self
            .run(move |service| {
                let agent = service.register_agent(client_name.as_deref())?;
                let actor = Actor::Agent(Agent {
                    id: agent.clone(),
                    client_name,
                });
                Ok((agent, service.start_task(&actor, id)?))
            })
            .await?;
        Ok(written_result(&written, Some(&agent), Reminder::First))
    }

    #[tool(
        description = "Add a note to the task's working log, shown on the board and \
                       never committed. Requires your agent id from start_task."
    )]
    async fn log_progress(
        &self,
        Parameters(args): Parameters<LogProgressArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let id = task_id(&args.id)?;
        let actor = self.actor(&context, Some(args.agent)).await?;
        self.run(move |service| service.log_progress(&actor, id, &args.message))
            .await?;
        Ok(success(json!({ "id": id.as_str(), "logged": true })))
    }

    #[tool(
        description = "Set the blocked flag with a question or reason for the user, or \
                       clear it with an empty reason. Requires your agent id from \
                       start_task."
    )]
    async fn set_blocked(
        &self,
        Parameters(args): Parameters<SetBlockedArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let id = task_id(&args.id)?;
        let actor = self.actor(&context, Some(args.agent)).await?;
        let blocked = !args.reason.trim().is_empty();
        self.run(move |service| service.set_blocked(&actor, id, &args.reason))
            .await?;
        Ok(success(json!({ "id": id.as_str(), "blocked": blocked })))
    }

    #[tool(
        description = concat!(
            "Add a milestone entry to the changelog of a long task, before a \
             meaningful intermediate commit. Optional. Requires your agent id from \
             start_task. Writes the task file and returns its path.",
            commit_rules!()
        )
    )]
    async fn add_changelog_entry(
        &self,
        Parameters(args): Parameters<AddChangelogEntryArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let id = task_id(&args.id)?;
        let actor = self.actor(&context, Some(args.agent)).await?;
        let files = args.files.unwrap_or_default();
        let written = self
            .run(move |service| service.add_changelog_entry(&actor, id, &args.summary, files))
            .await?;
        Ok(written_result(&written, None, Reminder::Intermediate))
    }

    #[tool(
        description = concat!(
            "Finish the task: set it finished, add a finished changelog entry with your \
             summary and files_changed, and release your lease. Tick or explain: list \
             the acceptance criteria you met by number (counting from 1) in met, and \
             put every other unticked criterion in unmet with a reason, or the call \
             fails with criteria_unmet. Never mark tasks reviewed; the user does. \
             Requires your agent id from start_task. Writes the task file and returns \
             its path.",
            commit_rules!()
        )
    )]
    async fn finish_task(
        &self,
        Parameters(args): Parameters<FinishTaskArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let id = task_id(&args.id)?;
        let actor = self.actor(&context, Some(args.agent)).await?;
        let finish = Finish {
            summary: args.summary,
            files_changed: args.files_changed,
            follow_ups: args.follow_ups,
            met: args.met.unwrap_or_default(),
            unmet: args
                .unmet
                .unwrap_or_default()
                .into_iter()
                .map(|u| Unmet {
                    criterion: u.criterion,
                    reason: u.reason,
                })
                .collect(),
        };
        let written = self
            .run(move |service| service.finish_task(&actor, id, finish))
            .await?;
        Ok(written_result(&written, None, Reminder::Final))
    }

    #[tool(
        description = concat!(
            "Give the task back: set it to todo, add a released changelog entry with \
             your note, and remove your lease. Requires your agent id from start_task. \
             Writes the task file and returns its path.",
            commit_rules!()
        )
    )]
    async fn release_task(
        &self,
        Parameters(args): Parameters<ReleaseTaskArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, CallToolResult> {
        let id = task_id(&args.id)?;
        let actor = self.actor(&context, Some(args.agent)).await?;
        let written = self
            .run(move |service| service.release_task(&actor, id, &args.note))
            .await?;
        Ok(written_result(&written, None, Reminder::Next))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for TandemTools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("tandem", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    /// Routes the call to its tool. Arguments that don't fit the tool's
    /// schema come back from rmcp as a plain-text error; they are turned
    /// into the same structured `invalid_argument` error as every other
    /// refusal, so agents can handle all errors one way.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let call = ToolCallContext::new(self, request, context);
        match self.tool_router.call(call).await? {
            CallToolResponse::Complete(result)
                if result.is_error == Some(true) && result.structured_content.is_none() =>
            {
                let message: Vec<&str> = result
                    .content
                    .iter()
                    .filter_map(|block| block.as_text().map(|text| text.text.as_str()))
                    .collect();
                Ok(error("invalid_argument", &message.join(" ")).into())
            }
            response => Ok(response),
        }
    }
}

// ---- Arguments, exactly as in the spec's tools table, plus `agent`.

/// A status, as written in task files.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum StatusArg {
    Backlog,
    Todo,
    InProgress,
    Finished,
    Done,
}

impl From<StatusArg> for Status {
    fn from(status: StatusArg) -> Self {
        match status {
            StatusArg::Backlog => Self::Backlog,
            StatusArg::Todo => Self::Todo,
            StatusArg::InProgress => Self::InProgress,
            StatusArg::Finished => Self::Finished,
            StatusArg::Done => Self::Done,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(rename_all = "snake_case")]
pub enum PriorityArg {
    High,
    Medium,
    Low,
}

impl From<PriorityArg> for Priority {
    fn from(priority: PriorityArg) -> Self {
        match priority {
            PriorityArg::High => Self::High,
            PriorityArg::Medium => Self::Medium,
            PriorityArg::Low => Self::Low,
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ListTasksArgs {
    pub status: Option<StatusArg>,
    pub label: Option<String>,
    /// Only children of this task id.
    pub parent: Option<String>,
    /// Matches the id, title, labels or body, ignoring case.
    pub query: Option<String>,
    /// Only tasks start_task would accept: in todo, every dependency done,
    /// no live lease.
    pub ready_only: Option<bool>,
    /// Your agent id from start_task, to keep your lease alive.
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct GetTaskArgs {
    /// The task id, such as k3f9x2m.
    pub id: String,
    /// Your agent id from start_task, to keep your lease alive.
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct NextTaskArgs {
    pub label: Option<String>,
    /// Only children of this task id.
    pub parent: Option<String>,
    /// Your agent id from start_task, to keep your lease alive.
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct CreateTaskArgs {
    pub title: String,
    /// Markdown for the Description section.
    pub description: String,
    /// One line each; written as unticked checkboxes.
    pub acceptance_criteria: Vec<String>,
    pub priority: Option<PriorityArg>,
    /// The parent task id, to make this a child task.
    pub parent: Option<String>,
    pub labels: Option<Vec<String>>,
    /// Task ids that must be done before this task can start.
    pub depends_on: Option<Vec<String>>,
    /// Your agent id from start_task, to keep your lease alive.
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct UpdateTaskArgs {
    /// The task id, such as k3f9x2m.
    pub id: String,
    pub title: Option<String>,
    /// Markdown for the Description section; blank removes it.
    pub description: Option<String>,
    /// Replaces every criterion, unticked; an empty list removes them.
    pub acceptance_criteria: Option<Vec<String>>,
    pub relevant_files: Option<Vec<String>>,
    pub priority: Option<PriorityArg>,
    /// The parent task id, or null to move the task to the top level.
    #[serde(default, deserialize_with = "present")]
    pub parent: Option<Option<String>>,
    pub labels: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
    /// Your agent id from start_task. Required to edit a task you hold.
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct StartTaskArgs {
    /// The task id, such as k3f9x2m.
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct LogProgressArgs {
    /// The task id, such as k3f9x2m.
    pub id: String,
    pub message: String,
    /// Your agent id from start_task.
    pub agent: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct SetBlockedArgs {
    /// The task id, such as k3f9x2m.
    pub id: String,
    /// What you need from the user; empty clears the flag.
    pub reason: String,
    /// Your agent id from start_task.
    pub agent: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct AddChangelogEntryArgs {
    /// The task id, such as k3f9x2m.
    pub id: String,
    pub summary: String,
    /// Files changed so far, for the entry's Files: line.
    pub files: Option<Vec<String>>,
    /// Your agent id from start_task.
    pub agent: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct FinishTaskArgs {
    /// The task id, such as k3f9x2m.
    pub id: String,
    /// What you did, for the finished changelog entry.
    pub summary: String,
    pub files_changed: Vec<String>,
    pub follow_ups: Option<String>,
    /// Numbers of the acceptance criteria you met, counting from 1.
    pub met: Option<Vec<u32>>,
    /// Every other unticked criterion, with the reason it isn't met.
    pub unmet: Option<Vec<UnmetArg>>,
    /// Your agent id from start_task.
    pub agent: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct UnmetArg {
    pub criterion: u32,
    pub reason: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
#[serde(deny_unknown_fields)]
pub struct ReleaseTaskArgs {
    /// The task id, such as k3f9x2m.
    pub id: String,
    /// Why you are giving the task back.
    pub note: String,
    /// Your agent id from start_task.
    pub agent: String,
}

/// Tells a present `null` apart from a missing field: `Some(None)` for
/// null, `None` (through `#[serde(default)]`) when absent.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

// ---- Results.

/// What to remind the agent about after a file write.
#[derive(Clone, Copy)]
enum Reminder {
    First,
    Intermediate,
    Final,
    Next,
}

fn written_result(written: &Written, agent: Option<&str>, reminder: Reminder) -> CallToolResult {
    let (path, id) = (&written.path, written.id);
    let reminder = match reminder {
        Reminder::First => format!(
            "Include {path} (status in_progress) in your first commit for this task, \
             with a `Task: {id}` trailer."
        ),
        Reminder::Intermediate => format!(
            "Include {path} in the intermediate commit this entry describes, with a \
             `Task: {id}` trailer."
        ),
        Reminder::Final => format!(
            "Include {path} (status finished) in your final commit for this task, with \
             a `Task: {id}` trailer."
        ),
        Reminder::Next => format!("Include {path} in your next commit."),
    };
    let mut value = json!({
        "id": id.as_str(),
        "path": path,
        "status": written.status.as_str(),
        "reminder": reminder,
    });
    if let Some(agent) = agent {
        value["agent"] = json!(agent);
    }
    let text = match agent {
        Some(agent) => format!("{reminder} Your agent id for this task is {agent}.\n{value}"),
        None => format!("{reminder}\n{value}"),
    };
    with_text(CallToolResult::structured(value), text)
}

fn success(value: Value) -> CallToolResult {
    CallToolResult::structured(value)
}

fn error(code: &str, message: &str) -> CallToolResult {
    let value = json!({ "error": { "code": code, "message": message } });
    with_text(
        CallToolResult::structured_error(value),
        format!("{code}: {message}"),
    )
}

fn service_error(e: ServiceError) -> CallToolResult {
    error(e.code(), &e.to_string())
}

fn with_text(mut result: CallToolResult, text: String) -> CallToolResult {
    result.content = vec![ContentBlock::text(text)];
    result
}

fn task_id(s: &str) -> Result<TaskId, CallToolResult> {
    s.parse()
        .map_err(|e: tandem_board_core::store::InvalidId| error("invalid_argument", &e.to_string()))
}

fn task_ids(ids: Vec<String>) -> Result<Vec<TaskId>, CallToolResult> {
    ids.iter().map(|id| task_id(id)).collect()
}

fn summary(task: &TaskSummary) -> Value {
    json!({
        "id": task.id.as_str(),
        "title": task.title,
        "status": task.status.as_str(),
        "priority": task.priority.as_str(),
        "parent": task.parent.map(|p| p.as_str().to_owned()),
        "labels": task.labels,
        "depends_on": task.depends_on.iter().map(|d| d.as_str()).collect::<Vec<_>>(),
        "lease_holder": task.lease.as_ref().map(|l| l.agent_id.clone()),
        "lease_stale": task.lease.as_ref().map(|l| !l.live),
        "blocked": task.blocked,
        "invalid": task.problem.as_ref().map(|p| match p {
            Problem::InvalidFile(message) | Problem::Graph(message) => message.clone(),
        }),
    })
}

fn detail_json(detail: &TaskDetail) -> Value {
    json!({
        "task": summary(&detail.summary),
        "markdown": detail.markdown,
        "parent": detail.parent.as_ref().map(summary),
        "children": detail.children.iter().map(summary).collect::<Vec<_>>(),
        "work_log": detail.work_log.iter().map(|entry| json!({
            "agent": entry.agent_id,
            "kind": entry.kind.as_str(),
            "message": entry.message,
            "at": entry.created_at,
        })).collect::<Vec<_>>(),
        "review_feedback": detail.review_feedback,
    })
}
