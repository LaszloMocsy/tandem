mod build_env;
mod commands;
mod dto;
mod error;
mod links;
mod mcp;
mod projects;

use std::sync::Arc;

use tandem_board_core::db::{Changes, Theme};
use tandem_board_mcp::Endpoints;
use tauri::{Emitter, Manager, RunEvent};
use tokio_util::sync::CancellationToken;

use crate::build_env::BuildEnv;
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
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            open_main_window(app)?;
            let data_dir = app.path().app_data_dir()?;
            let mcp = Mcp::new(Endpoints::new(&setup_cancel), setup_cancel.clone());
            let handle = app.handle().clone();
            let on_change = Arc::new(move |project: &str, changes: &Changes| {
                if let Err(e) = handle.emit(TASKS_CHANGED, TasksChanged::new(project, changes)) {
                    report("cannot send tasks-changed", &e);
                }
            });
            let projects = Projects::new(&data_dir, mcp, on_change)?;
            apply_theme(app.handle(), projects.theme()?);
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
            commands::open_link,
            commands::get_app_settings,
            commands::set_mcp_port,
            commands::set_theme,
            commands::set_project_name,
            commands::list_tasks,
            commands::get_task,
            commands::list_agents,
            commands::create_task,
            commands::update_task,
            commands::delete_check,
            commands::delete_task,
            commands::list_deleted_tasks,
            commands::restore_task,
            commands::purge_deleted_tasks,
            commands::approve_task,
            commands::start_task,
            commands::finish_task,
            commands::release_task,
            commands::mark_reviewed,
            commands::reopen_task,
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

/// Opens the main window from its config, titled with the build environment.
/// It's built here rather than by Tauri, so the plain title never flashes.
fn open_main_window(app: &tauri::App) -> tauri::Result<()> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .expect("tauri.conf.json defines the main window");
    tauri::WebviewWindowBuilder::from_config(app, config)?
        .title(BuildEnv::current().title())
        .build()?;
    Ok(())
}

/// Gives the windows the theme, so their title bars match the page. The
/// system theme lets each window follow the system again.
pub(crate) fn apply_theme(app: &tauri::AppHandle, theme: Theme) {
    let theme = match theme {
        Theme::System => None,
        Theme::Light => Some(tauri::Theme::Light),
        Theme::Dark => Some(tauri::Theme::Dark),
    };
    for window in app.webview_windows().values() {
        if let Err(e) = window.set_theme(theme) {
            report("cannot set the window theme", &e);
        }
    }
}
