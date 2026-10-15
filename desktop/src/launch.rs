//! Opens the project on launch and starts its watcher and MCP server.
//!
//! For now the project is the repo named by the `TANDEM_PROJECT`
//! environment variable; the project switcher comes later. Problems are
//! written to stderr, and the app keeps running without MCP.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tandem_board_core::index::AppDb;
use tandem_board_core::service::{Config, Service};
use tandem_board_core::watcher::{Watcher, WatcherConfig};
use tandem_board_mcp::{DEFAULT_ADDR, McpServer, endpoint_path};
use tokio_util::sync::CancellationToken;

/// The running project, kept in Tauri's state for the app's lifetime.
/// Dropping it stops the watcher and the MCP server.
pub struct Running {
    _watcher: Watcher,
    cancel: CancellationToken,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Opens the repo in `TANDEM_PROJECT`, if set, and starts serving it.
pub fn start(data_dir: &Path) -> Option<Running> {
    let Some(repo) = std::env::var_os("TANDEM_PROJECT").map(PathBuf::from) else {
        eprintln!("tandem: TANDEM_PROJECT is not set, so no project is open and MCP is off");
        return None;
    };
    match open(data_dir, &repo) {
        Ok(running) => Some(running),
        Err(e) => {
            report(&format!("cannot open {}", repo.display()), e.as_ref());
            None
        }
    }
}

fn open(data_dir: &Path, repo: &Path) -> Result<Running, Box<dyn Error>> {
    let mut app_db = AppDb::open(data_dir)?;
    let project = app_db.add_project(repo)?;
    app_db.mark_opened(&project.id, jiff::Timestamp::now())?;

    let config = Config {
        user_name: git_user_name(&project.repo_path).unwrap_or_else(|| "user".to_owned()),
        ..Config::default()
    };
    let service = Arc::new(Service::open(
        data_dir,
        &project.id,
        &project.repo_path,
        config,
    )?);
    if let Some(reason) = service.read_only() {
        eprintln!(
            "tandem: {} is read-only: {reason}",
            project.repo_path.display()
        );
    }
    let watcher = Watcher::start(Arc::clone(&service), WatcherConfig::default(), |e| {
        report("watcher", &e);
    })?;

    let cancel = CancellationToken::new();
    let projects = BTreeMap::from([(project.id.clone(), Arc::clone(&service))]);
    let server_cancel = cancel.clone();
    let id = project.id.clone();
    tauri::async_runtime::spawn(async move {
        match McpServer::bind(DEFAULT_ADDR, &projects, server_cancel).await {
            Ok(server) => {
                eprintln!(
                    "tandem: MCP server for `{id}` at http://{DEFAULT_ADDR}{}",
                    endpoint_path(&id)
                );
                if let Err(e) = server.run().await {
                    report("MCP server stopped", &e);
                }
            }
            Err(e) => report("MCP server not started", &e),
        }
    });

    Ok(Running {
        _watcher: watcher,
        cancel,
    })
}

/// Your name for changelog entries: `git config user.name` in the repo.
fn git_user_name(repo: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "user.name"])
        .current_dir(repo)
        .output()
        .ok()?;
    let name = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !name.is_empty()).then_some(name)
}

/// Writes an error and its causes to stderr on one line.
fn report(context: &str, error: &dyn Error) {
    let mut message = format!("tandem: {context}: {error}");
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    eprintln!("{message}");
}
