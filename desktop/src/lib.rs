mod commands;
mod dto;
mod error;
mod projects;

use std::sync::Arc;

use tandem_board_core::index::Changes;
use tandem_board_mcp::{DEFAULT_ADDR, Endpoints, McpServer};
use tauri::{Emitter, Manager, RunEvent};
use tokio_util::sync::CancellationToken;

use crate::dto::{TASKS_CHANGED, TasksChanged};
use crate::error::report;
use crate::projects::Projects;

pub fn run() {
    // Cancelled when the app exits, which stops the MCP server.
    let cancel = CancellationToken::new();
    let setup_cancel = cancel.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let data_dir = app.path().app_data_dir()?;
            let endpoints = Endpoints::new(&setup_cancel);
            let mcp_addr = start_mcp(endpoints.clone(), setup_cancel.clone());
            let handle = app.handle().clone();
            let on_change = Arc::new(move |project: &str, changes: &Changes| {
                if let Err(e) = handle.emit(TASKS_CHANGED, TasksChanged::new(project, changes)) {
                    report("cannot send tasks-changed", &e);
                }
            });
            let projects = Projects::new(&data_dir, endpoints, mcp_addr, on_change)?;
            projects.open_all()?;
            app.manage(Arc::new(projects));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::open_project,
            commands::remove_project,
            commands::choose_folder,
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

/// Binds the MCP server and runs it in the background. Returns its address,
/// or `None` if it could not start, as when another copy of the app holds
/// the port; the board still works without it.
fn start_mcp(endpoints: Endpoints, cancel: CancellationToken) -> Option<std::net::SocketAddr> {
    let server =
        match tauri::async_runtime::block_on(McpServer::bind(DEFAULT_ADDR, endpoints, cancel)) {
            Ok(server) => server,
            Err(e) => {
                report("MCP server not started", &e);
                return None;
            }
        };
    let addr = server.local_addr().ok()?;
    eprintln!("tandem: MCP server at http://{addr}");
    tauri::async_runtime::spawn(async move {
        if let Err(e) = server.run().await {
            report("MCP server stopped", &e);
        }
    });
    Some(addr)
}
