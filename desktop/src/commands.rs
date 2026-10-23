//! The Tauri commands the UI calls. Task commands act as you and wrap the
//! project's service; `ui/src/api.ts` is the typed client for them.
//!
//! Every command is async and does its work on a blocking thread, so
//! database work never stalls the window.

use std::path::PathBuf;
use std::sync::Arc;

use jiff::SignedDuration;
use tandem_board_core::db::Theme;
use tandem_board_core::model::TaskId;
use tandem_board_core::service::{Actor, Service};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::build_env::BuildEnv;
use crate::dto::{
    AgentInfo, DeleteCheck, DeletedTask, Finish, NewTask, Saved, TaskDetail, TaskEdit, TaskFilter,
    TaskSummary,
};
use crate::error::Error;
use crate::links::Link;
use crate::projects::{AppSettingsInfo, Opened, ProjectInfo, Projects};

type Result<T> = std::result::Result<T, Error>;

/// Runs `f` on a blocking thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| Error::internal(&e))?
}

/// Runs `f` with an open project's service on a blocking thread.
async fn with_service<T: Send + 'static>(
    projects: State<'_, Arc<Projects>>,
    project: String,
    f: impl FnOnce(&Service) -> Result<T> + Send + 'static,
) -> Result<T> {
    let service = projects.service(&project)?;
    blocking(move || f(&service)).await
}

/// Every project, the most recently opened first.
#[tauri::command]
pub async fn list_projects(projects: State<'_, Arc<Projects>>) -> Result<Vec<ProjectInfo>> {
    let projects = Arc::clone(&projects);
    blocking(move || projects.list()).await
}

/// Opens a folder; see [`Projects::open`].
#[tauri::command]
pub async fn open_project(
    projects: State<'_, Arc<Projects>>,
    path: PathBuf,
    allow_outside_git: bool,
) -> Result<Opened> {
    let projects = Arc::clone(&projects);
    blocking(move || projects.open(&path, allow_outside_git)).await
}

/// Removes a project from the app; see [`Projects::remove`].
#[tauri::command]
pub async fn remove_project(projects: State<'_, Arc<Projects>>, project: String) -> Result<bool> {
    let projects = Arc::clone(&projects);
    blocking(move || projects.remove(&project)).await
}

/// Renames the project; a blank name goes back to the folder name.
#[tauri::command]
pub async fn set_project_name(
    projects: State<'_, Arc<Projects>>,
    project: String,
    name: String,
) -> Result<ProjectInfo> {
    let projects = Arc::clone(&projects);
    blocking(move || projects.set_name(&project, &name)).await
}

/// The app's settings and the MCP server's state, for the Settings page.
#[tauri::command]
pub async fn get_app_settings(projects: State<'_, Arc<Projects>>) -> Result<AppSettingsInfo> {
    let projects = Arc::clone(&projects);
    blocking(move || projects.app_settings()).await
}

/// Moves the MCP server to another port; see [`Projects::set_mcp_port`].
#[tauri::command]
pub async fn set_mcp_port(
    projects: State<'_, Arc<Projects>>,
    port: u16,
) -> Result<AppSettingsInfo> {
    let projects = Arc::clone(&projects);
    blocking(move || projects.set_mcp_port(port)).await
}

/// Sets the theme, `system`, `light` or `dark`, and applies it to the
/// window; the UI applies it to the page.
#[tauri::command]
pub async fn set_theme(
    app: AppHandle,
    projects: State<'_, Arc<Projects>>,
    theme: String,
) -> Result<AppSettingsInfo> {
    let theme: Theme = theme
        .parse()
        .map_err(|e: String| Error::new("invalid_argument", e))?;
    let projects = Arc::clone(&projects);
    let settings = blocking(move || projects.set_theme(theme)).await?;
    crate::apply_theme(&app, theme);
    Ok(settings)
}

/// Asks for a folder with the system dialog. `None` if you cancel.
#[tauri::command]
pub async fn choose_folder(app: AppHandle) -> Result<Option<String>> {
    blocking(move || {
        let Some(folder) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let path = folder
            .into_path()
            .map_err(|e| Error::new("invalid_argument", e.to_string()))?;
        Ok(Some(path.display().to_string()))
    })
    .await
}

/// Opens one of the app's links, such as the release notes, in the browser.
#[tauri::command]
pub async fn open_link(app: AppHandle, link: Link) -> Result<()> {
    let url = link.url(BuildEnv::current(), &app.package_info().version.to_string());
    blocking(move || {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| Error::internal(&e))
    })
    .await
}

#[tauri::command]
pub async fn list_tasks(
    projects: State<'_, Arc<Projects>>,
    project: String,
    filter: Option<TaskFilter>,
) -> Result<Vec<TaskSummary>> {
    let filter = filter.unwrap_or_default().into();
    with_service(projects, project, move |service| {
        let tasks = service.list_tasks(&Actor::User, &filter)?;
        Ok(tasks.into_iter().map(TaskSummary::from).collect())
    })
    .await
}

/// How long an agent stays in the Agents panel after its last call, unless
/// it still holds a lease.
const AGENTS_SEEN_WITHIN: SignedDuration = SignedDuration::from_hours(24);

/// The Agents panel: recent agents and the tasks each holds.
#[tauri::command]
pub async fn list_agents(
    projects: State<'_, Arc<Projects>>,
    project: String,
) -> Result<Vec<AgentInfo>> {
    with_service(projects, project, move |service| {
        let agents = service.list_agents(AGENTS_SEEN_WITHIN)?;
        Ok(agents.into_iter().map(AgentInfo::from).collect())
    })
    .await
}

#[tauri::command]
pub async fn get_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
) -> Result<TaskDetail> {
    with_service(projects, project, move |service| {
        Ok(service.get_task(&Actor::User, id)?.into())
    })
    .await
}

/// New task dialog: a new task in todo.
#[tauri::command]
pub async fn create_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    task: NewTask,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service.create_task(&Actor::User, task.into())?.into())
    })
    .await
}

/// Edits the spec from the detail pane.
#[tauri::command]
pub async fn update_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
    edit: TaskEdit,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service.update_task(&Actor::User, id, edit.into())?.into())
    })
    .await
}

/// What stands in the way of deleting a task.
#[tauri::command]
pub async fn delete_check(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
) -> Result<DeleteCheck> {
    with_service(projects, project, move |service| {
        Ok(service.delete_check(id)?.into())
    })
    .await
}

/// Deletes a task, at any status, until restored or cleaned up; see
/// `Service::delete_task`.
#[tauri::command]
pub async fn delete_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
) -> Result<()> {
    with_service(projects, project, move |service| {
        Ok(service.delete_task(id)?)
    })
    .await
}

/// The deleted tasks, which can still be restored, newest first.
#[tauri::command]
pub async fn list_deleted_tasks(
    projects: State<'_, Arc<Projects>>,
    project: String,
) -> Result<Vec<DeletedTask>> {
    with_service(projects, project, move |service| {
        let tasks = service.deleted_tasks()?;
        Ok(tasks.into_iter().map(DeletedTask::from).collect())
    })
    .await
}

/// Brings a deleted task back.
#[tauri::command]
pub async fn restore_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
) -> Result<()> {
    with_service(projects, project, move |service| {
        Ok(service.restore_task(id)?)
    })
    .await
}

/// Removes every deleted task for good, so none can be restored. Returns
/// how many went.
#[tauri::command]
pub async fn purge_deleted_tasks(
    projects: State<'_, Arc<Projects>>,
    project: String,
) -> Result<usize> {
    with_service(projects, project, move |service| {
        Ok(service.purge_deleted()?.len())
    })
    .await
}

/// Backlog → Todo.
#[tauri::command]
pub async fn approve_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service.approve(id)?.into())
    })
    .await
}

/// Todo → In progress, held by you rather than an agent.
#[tauri::command]
pub async fn start_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service.start_task(&Actor::User, id)?.into())
    })
    .await
}

/// In progress → Review.
#[tauri::command]
pub async fn finish_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
    finish: Finish,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service.finish_task(&Actor::User, id, finish.into())?.into())
    })
    .await
}

/// In progress → Todo.
#[tauri::command]
pub async fn release_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
    note: String,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service.release_task(&Actor::User, id, &note)?.into())
    })
    .await
}

/// Review → Done.
#[tauri::command]
pub async fn mark_reviewed(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service.mark_reviewed(id)?.into())
    })
    .await
}

/// Review → In progress, with feedback for the next agent.
#[tauri::command]
pub async fn reopen_task(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
    feedback: String,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service.reopen(id, &feedback)?.into())
    })
    .await
}

/// Sets the blocked flag, or clears it when `reason` is blank.
#[tauri::command]
pub async fn set_blocked(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
    reason: String,
) -> Result<()> {
    with_service(projects, project, move |service| {
        Ok(service.set_blocked(&Actor::User, id, &reason)?)
    })
    .await
}

/// Adds a note to the working log.
#[tauri::command]
pub async fn log_progress(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
    message: String,
) -> Result<()> {
    with_service(projects, project, move |service| {
        Ok(service.log_progress(&Actor::User, id, &message)?)
    })
    .await
}

/// A milestone entry in the changelog of a task in progress.
#[tauri::command]
pub async fn add_changelog_entry(
    projects: State<'_, Arc<Projects>>,
    project: String,
    id: TaskId,
    summary: String,
    files: Option<Vec<String>>,
) -> Result<Saved> {
    with_service(projects, project, move |service| {
        Ok(service
            .add_changelog_entry(&Actor::User, id, &summary, files.unwrap_or_default())?
            .into())
    })
    .await
}
