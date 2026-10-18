mod commands;
mod dto;
mod error;
mod mcp;
mod projects;

use std::sync::Arc;

use tandem_board_core::index::Changes;
use tandem_board_mcp::Endpoints;
use tauri::{Emitter, Manager, RunEvent};
use tokio_util::sync::CancellationToken;

use crate::dto::{TASKS_CHANGED, TasksChanged};
use crate::error::report;
use crate::mcp::Mcp;
use crate::projects::Projects;

pub fn run() {
    // Cancelled when the app exits, which stops the MCP server.
    let cancel = CancellationToken::new();
    let setup_cancel = cancel.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let data_dir = app.path().app_data_dir()?;
            let mcp = Mcp::new(Endpoints::new(&setup_cancel), setup_cancel.clone());
            let handle = app.handle().clone();
            let on_change = Arc::new(move |project: &str, changes: &Changes| {
                if let Err(e) = handle.emit(TASKS_CHANGED, TasksChanged::new(project, changes)) {
                    report("cannot send tasks-changed", &e);
                }
            });
            let projects = Projects::new(&data_dir, mcp, on_change)?;
            projects.start_mcp()?;
            projects.open_all()?;
            app.manage(Arc::new(projects));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::open_project,
            commands::remove_project,
            commands::choose_folder,
            commands::get_app_settings,
            commands::set_mcp_port,
            commands::list_tasks,
            commands::get_task,
            commands::list_agents,
            commands::create_task,
            commands::update_task,
            commands::delete_check,
            commands::delete_task,
            commands::approve_task,
            commands::start_task,
            commands::finish_task,
            commands::release_task,
            commands::mark_reviewed,
            commands::reopen_task,
            commands::has_uncommitted_changes,
            commands::commit_review,
            commands::set_blocked,
            commands::log_progress,
            commands::add_changelog_entry,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_, event| {
            if let RunEvent::Exit = event {
                cancel.cancel();
            }
        });
}
