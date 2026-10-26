mod build_env;
mod commands;
mod dto;
mod error;
mod links;
mod mcp;
mod projects;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use jiff::SignedDuration;
use tandem_board_core::db::{Changes, Theme, data_dir};
use tandem_board_mcp::Endpoints;
use tauri::{Emitter, Manager, RunEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tokio_util::sync::CancellationToken;

use crate::build_env::BuildEnv;
use crate::dto::{TASKS_CHANGED, TasksChanged};
use crate::error::{NEWER_SCHEMA, report};
use crate::mcp::Mcp;
use crate::projects::{BACKUP_INTERVAL, Projects};

pub fn run() {
    // Cancelled when the app exits, which stops the MCP server.
    let cancel = CancellationToken::new();
    let setup_cancel = cancel.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // From core, so every process using Tandem finds the same one.
            let data_dir = data_dir().ok_or("the system has no data directory")?;
            let mcp = Mcp::new(Endpoints::new(&setup_cancel), setup_cancel.clone());
            let handle = app.handle().clone();
            let on_change = Arc::new(move |project: &str, changes: &Changes| {
                if let Err(e) = handle.emit(TASKS_CHANGED, TasksChanged::new(project, changes)) {
                    report("cannot send tasks-changed", &e);
                }
            });
            let projects = match Projects::new(&data_dir, mcp, on_change) {
                Err(e) if e.code == NEWER_SCHEMA => {
                    refuse_to_start(app.handle(), e.message);
                    return Ok(());
                }
                projects => projects?,
            };
            open_main_window(app)?;
            apply_theme(app.handle(), projects.theme()?);
            projects.start_mcp()?;
            projects.open_all()?;
            let projects = Arc::new(projects);
            start_backups(Arc::clone(&projects));
            app.manage(projects);
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
            commands::list_backups,
            commands::back_up_project,
            commands::restore_backup,
            commands::export_tasks,
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
        .run(move |app, event| {
            if let RunEvent::Exit = event {
                cancel.cancel();
                if let Some(projects) = app.try_state::<Arc<Projects>>() {
                    projects.back_up_changed(SignedDuration::ZERO);
                }
            }
        });
}

/// How often the backup thread checks for changed projects.
const BACKUP_CHECK: Duration = Duration::from_secs(10 * 60);

/// Backs up every changed project now, as on launch, then each one that
/// keeps changing at most once per [`BACKUP_INTERVAL`], on a thread of its
/// own that lasts as long as the app.
fn start_backups(projects: Arc<Projects>) {
    let spawned = thread::Builder::new()
        .name("tandem-backups".to_owned())
        .spawn(move || {
            projects.back_up_changed(SignedDuration::ZERO);
            loop {
                thread::sleep(BACKUP_CHECK);
                projects.back_up_changed(BACKUP_INTERVAL);
            }
        });
    if let Err(e) = spawned {
        report("cannot start backups", &e);
    }
}

/// Tells you that a newer version of Tandem has taken the app's data over,
/// then quits once you close the message. No window opens.
fn refuse_to_start(app: &tauri::AppHandle, message: String) {
    let handle = app.clone();
    app.dialog()
        .message(message)
        .title("Tandem cannot start")
        .kind(MessageDialogKind::Error)
        .show(move |_| handle.exit(1));
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

#[cfg(test)]
mod tests {
    use tandem_board_core::db::APP_IDENTIFIER;

    #[test]
    fn core_finds_the_data_directory_tauri_uses() {
        // Tauri's app data directory is the identifier in the platform's
        // data directory, as `data_dir` builds it.
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], APP_IDENTIFIER);
    }
}
