//! The project registry: the projects in `app.db` and the ones open now.
//!
//! An open project has its service, a thread that forwards its change
//! events, and an MCP endpoint. Every project in the list whose folder
//! still exists is opened on launch and stays open until the app quits or
//! the project is removed, so agents can reach any of them whichever one
//! the board shows.
//!
//! A project is identified by its repo's git common dir, so every worktree
//! of a repo opens the same project, with one board and one MCP URL.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use jiff::SignedDuration;
use serde::Serialize;
use tandem_board_core::db::{
    AppDb, Changes, Project, Theme, delete_project_db, folder_name, normalize_project_name,
};
use tandem_board_core::git;
use tandem_board_core::markdown::TASK_DIR;
use tandem_board_core::service::{Config, Service};
use tandem_board_mcp::{DEFAULT_ADDR, endpoint_path};

use crate::error::{Error, describe, report};
use crate::mcp::Mcp;

/// Called with a project id and what changed in it.
pub type OnChange = Arc<dyn Fn(&str, &Changes) + Send + Sync>;

/// A project as the project switcher shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    /// The checkout or folder last opened.
    pub repo_path: String,
    pub last_opened_at: Option<String>,
    /// Whether the project is open, so its tasks can be listed.
    pub open: bool,
    /// Why the project is not open, when it isn't.
    pub error: Option<String>,
    /// The project's MCP endpoint, while it is open and the server runs.
    pub mcp_url: Option<String>,
}

/// The app's settings, as the Settings page shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsInfo {
    /// The port the MCP server is set to use.
    pub mcp_port: u16,
    pub default_mcp_port: u16,
    /// Where the server listens, such as `http://127.0.0.1:4317`, while it
    /// runs.
    pub mcp_url: Option<String>,
    /// Why the server isn't running, when it isn't.
    pub mcp_error: Option<String>,
    /// `system`, `light` or `dark`.
    pub theme: &'static str,
}

/// What opening a folder did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Opened {
    /// The project is open.
    Project { project: ProjectInfo },
    /// The folder is not in a git repo. Nothing was added to the list; open
    /// it again with `allow_outside_git` to make it a project of its own.
    #[serde(rename_all = "camelCase")]
    OutsideGit { repo_path: String },
}

/// Every project in `app.db`, and the open ones.
pub struct Projects {
    data_dir: PathBuf,
    app_db: Mutex<AppDb>,
    open: Mutex<BTreeMap<String, Arc<Service>>>,
    /// Why a project failed to open on launch.
    failed: Mutex<BTreeMap<String, String>>,
    /// The MCP server, with an endpoint for every open project.
    mcp: Mcp,
    on_change: OnChange,
}

impl Projects {
    /// Opens `app.db` in `data_dir`. Open projects get an endpoint on
    /// `mcp`; [`start_mcp`](Self::start_mcp) starts the server.
    pub fn new(data_dir: &Path, mcp: Mcp, on_change: OnChange) -> Result<Self, Error> {
        Ok(Self {
            data_dir: data_dir.to_owned(),
            app_db: Mutex::new(AppDb::open(data_dir)?),
            open: Mutex::default(),
            failed: Mutex::default(),
            mcp,
            on_change,
        })
    }

    /// Starts the MCP server on the port from the settings, as on launch.
    /// A failure, such as another copy of the app holding the port, is
    /// reported and shown in Settings; the board works without the server.
    pub fn start_mcp(&self) -> Result<(), Error> {
        let port = self.mcp_port()?;
        if let Err(e) = self.mcp.start(port) {
            eprintln!("tandem: MCP server not started: {}", e.message);
        }
        Ok(())
    }

    pub fn app_settings(&self) -> Result<AppSettingsInfo, Error> {
        let settings = lock(&self.app_db).settings()?;
        Ok(AppSettingsInfo {
            mcp_port: settings.mcp_port.unwrap_or(DEFAULT_ADDR.port()),
            default_mcp_port: DEFAULT_ADDR.port(),
            mcp_url: self.mcp.addr().map(|addr| format!("http://{addr}")),
            mcp_error: self.mcp.error(),
            theme: settings.theme.as_str(),
        })
    }

    pub fn theme(&self) -> Result<Theme, Error> {
        Ok(lock(&self.app_db).settings()?.theme)
    }

    pub fn set_theme(&self, theme: Theme) -> Result<AppSettingsInfo, Error> {
        lock(&self.app_db).set_theme(theme)?;
        self.app_settings()
    }

    /// Moves the MCP server to `port` and saves it in the settings. Every
    /// project's MCP URL changes with it. If the port can't be used, the
    /// server stays where it was and the setting is unchanged.
    pub fn set_mcp_port(&self, port: u16) -> Result<AppSettingsInfo, Error> {
        if port == 0 {
            return Err(Error::new(
                "invalid_argument",
                "the port must be between 1 and 65535",
            ));
        }
        self.mcp.start(port)?;
        let saved = (port != DEFAULT_ADDR.port()).then_some(port);
        lock(&self.app_db).set_mcp_port(saved)?;
        self.app_settings()
    }

    fn mcp_port(&self) -> Result<u16, Error> {
        let settings = lock(&self.app_db).settings()?;
        Ok(settings.mcp_port.unwrap_or(DEFAULT_ADDR.port()))
    }

    /// Opens every project in the list whose folder still exists, as on
    /// launch. Failures are reported to stderr and shown in the project
    /// list.
    pub fn open_all(&self) -> Result<(), Error> {
        let projects = lock(&self.app_db).projects()?;
        for project in projects {
            if !project.last_path.is_dir() {
                continue;
            }
            if let Err(e) = self.start(&project) {
                eprintln!(
                    "tandem: cannot open {}: {}",
                    project.last_path.display(),
                    e.message
                );
                lock(&self.failed).insert(project.id, e.message);
            }
        }
        Ok(())
    }

    /// Every project, the most recently opened first.
    pub fn list(&self) -> Result<Vec<ProjectInfo>, Error> {
        let projects = lock(&self.app_db).projects()?;
        Ok(projects.iter().map(|project| self.info(project)).collect())
    }

    /// Opens the folder at `path` and records its project as the most
    /// recently opened, adding it to the list if needed. Any folder in a git
    /// repo opens the repo's project, whichever worktree it is in. A folder
    /// outside git becomes a project of its own only with
    /// `allow_outside_git`; otherwise nothing changes and the result says so.
    pub fn open(&self, path: &Path, allow_outside_git: bool) -> Result<Opened, Error> {
        let folder = path.canonicalize().map_err(|e| {
            Error::new(
                "invalid_argument",
                format!("cannot open {}: {e}", path.display()),
            )
        })?;
        if !folder.is_dir() {
            return Err(Error::new(
                "invalid_argument",
                format!("{} is not a folder", folder.display()),
            ));
        }
        let git_dir = match git::common_dir(&folder) {
            Some(git_dir) => git_dir,
            None if allow_outside_git => folder.clone(),
            None => {
                return Ok(Opened::OutsideGit {
                    repo_path: folder.display().to_string(),
                });
            }
        };
        let project = lock(&self.app_db).add_project(&git_dir, &folder, &folder_name(&git_dir))?;
        self.start(&project)?;
        lock(&self.failed).remove(&project.id);
        let app_db = lock(&self.app_db);
        app_db.mark_opened(&project.id, jiff::Timestamp::now())?;
        let project = app_db.project(&project.id)?.unwrap_or(project);
        drop(app_db);
        Ok(Opened::Project {
            project: self.info(&project),
        })
    }

    /// Closes the project, removes it from the list and deletes its
    /// database, with every task in it. The folder is left alone. Refused
    /// with `agents_working` while an agent holds a live lease on any of
    /// its tasks. Returns `false` if there is no such project.
    pub fn remove(&self, id: &str) -> Result<bool, Error> {
        let service = lock(&self.open).get(id).cloned();
        if let Some(service) = service {
            check_no_agents_working(&service)?;
            // Close the endpoint so no agent can claim a task, then check
            // again for a claim that landed in between.
            self.mcp.endpoints().remove(id);
            if let Err(e) = check_no_agents_working(&service) {
                self.mcp.endpoints().insert(id, service);
                return Err(e);
            }
        }
        lock(&self.open).remove(id);
        lock(&self.failed).remove(id);
        self.mcp.endpoints().remove(id);
        let removed = lock(&self.app_db).remove_project(id)?;
        if removed && let Err(e) = delete_project_db(&self.data_dir, id) {
            // The project is gone from the list either way.
            report(&format!("cannot delete the database of {id}"), &e);
        }
        Ok(removed)
    }

    /// Renames the project, or gives it back its folder's name when `name`
    /// is blank. Returns the project as the list shows it.
    pub fn set_name(&self, id: &str, name: &str) -> Result<ProjectInfo, Error> {
        let name = normalize_project_name(name)
            .map_err(|e| Error::new("invalid_argument", e.to_string()))?;
        let app_db = lock(&self.app_db);
        let project = app_db
            .project(id)?
            .ok_or_else(|| Error::project_not_found(id))?;
        let name = name.unwrap_or_else(|| folder_name(&project.git_dir));
        app_db.set_name(id, &name)?;
        let project = app_db.project(id)?.unwrap_or(project);
        drop(app_db);
        Ok(self.info(&project))
    }

    /// The service of an open project.
    pub fn service(&self, id: &str) -> Result<Arc<Service>, Error> {
        lock(&self.open)
            .get(id)
            .cloned()
            .ok_or_else(|| Error::project_not_found(id))
    }

    /// Opens the project unless it is open already: its service, change
    /// forwarding and MCP endpoint. A project opened for the first time
    /// imports the tasks of an old `.tandem/` folder in the checkout, if it
    /// has one; the folder is only read.
    fn start(&self, project: &Project) -> Result<(), Error> {
        let mut open = lock(&self.open);
        if open.contains_key(&project.id) {
            return Ok(());
        }
        let config = Config {
            user_name: git::user_name(&project.last_path).unwrap_or_else(|| "user".to_owned()),
            ..Config::default()
        };
        let service = Arc::new(Service::open(&self.data_dir, &project.id, config)?);
        let task_dir = project.last_path.join(TASK_DIR);
        if service.is_new() && task_dir.is_dir() {
            let report = service.import_folder(&task_dir)?;
            for skipped in &report.skipped {
                eprintln!("tandem: importing {}: {skipped}", task_dir.display());
            }
        }

        // The thread holds the receiver only, so it ends with the project.
        let changes = service.subscribe();
        let on_change = Arc::clone(&self.on_change);
        let id = project.id.clone();
        thread::Builder::new()
            .name(format!("tandem-events-{id}"))
            .spawn(move || {
                for changes in changes {
                    on_change(&id, &changes);
                }
            })
            .map_err(|e| Error::internal(&e))?;

        self.mcp
            .endpoints()
            .insert(&project.id, Arc::clone(&service));
        open.insert(project.id.clone(), service);
        Ok(())
    }

    fn info(&self, project: &Project) -> ProjectInfo {
        let open = lock(&self.open).contains_key(&project.id);
        ProjectInfo {
            id: project.id.clone(),
            name: project.name.clone(),
            repo_path: project.last_path.display().to_string(),
            last_opened_at: project.last_opened_at.clone(),
            open,
            error: (!open).then(|| self.closed_reason(project)),
            mcp_url: self
                .mcp
                .addr()
                .filter(|_| open)
                .map(|addr| format!("http://{addr}{}", endpoint_path(&project.id))),
        }
    }

    fn closed_reason(&self, project: &Project) -> String {
        if let Some(message) = lock(&self.failed).get(&project.id) {
            return message.clone();
        }
        match fs::metadata(&project.last_path) {
            Ok(meta) if meta.is_dir() => "the project is not open".to_owned(),
            Ok(_) => "the project path is not a folder".to_owned(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => "the folder is missing".to_owned(),
            Err(e) => describe(&e),
        }
    }
}

/// Refuses with `agents_working` if an agent holds a live lease on any of
/// the project's tasks. Stale leases don't count: their agent has stopped.
fn check_no_agents_working(service: &Service) -> Result<(), Error> {
    let working: Vec<String> = service
        .list_agents(SignedDuration::ZERO)?
        .into_iter()
        .flat_map(|agent| agent.tasks)
        .filter(|task| task.live)
        .map(|task| task.id.to_string())
        .collect();
    if working.is_empty() {
        return Ok(());
    }
    let (noun, verb) = if working.len() == 1 {
        ("task", "it")
    } else {
        ("tasks", "them")
    };
    Err(Error::new(
        "agents_working",
        format!(
            "an agent is working on {noun} {}; wait until the agent finishes or releases {verb}, then remove the project",
            working.join(", ")
        ),
    ))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::process::Command;
    use std::sync::mpsc;
    use std::time::Duration;

    use std::net::SocketAddr;
    use tandem_board_core::db::project_db_path;
    use tandem_board_core::model::TaskId;
    use tandem_board_core::service::{Actor, NewTask, TaskFilter};
    use tempfile::TempDir;

    use tandem_board_mcp::Endpoints;
    use tokio_util::sync::CancellationToken;

    use super::*;

    struct Env {
        data: TempDir,
        cancel: CancellationToken,
        endpoints: Endpoints,
        /// Where the MCP server listens, on a free port.
        addr: SocketAddr,
        events: mpsc::Receiver<(String, Changes)>,
        projects: Projects,
    }

    impl Env {
        fn new(data: TempDir) -> Self {
            let cancel = CancellationToken::new();
            let endpoints = Endpoints::new(&cancel);
            let mcp = Mcp::new(endpoints.clone(), cancel.clone());
            let addr = mcp.start(0).unwrap();
            let (tx, events) = mpsc::channel();
            let tx = Mutex::new(tx);
            let projects = Projects::new(
                data.path(),
                mcp,
                Arc::new(move |id: &str, changes: &Changes| {
                    let _ = lock(&tx).send((id.to_owned(), changes.clone()));
                }),
            )
            .unwrap();
            Self {
                data,
                cancel,
                endpoints,
                addr,
                events,
                projects,
            }
        }

        /// Opens a folder, as a project of its own if it is outside git.
        fn open(&self, folder: &Path) -> ProjectInfo {
            match self.projects.open(folder, true).unwrap() {
                Opened::Project { project } => project,
                other => panic!("expected an open project, got {other:?}"),
            }
        }

        /// Closes everything and starts again on the same data directory,
        /// as relaunching the app does.
        fn relaunch(self) -> Self {
            let data = self.data;
            self.cancel.cancel();
            drop(self.projects);
            let env = Self::new(data);
            env.projects.open_all().unwrap();
            env
        }

        fn titles(&self, id: &str) -> Vec<String> {
            self.projects
                .service(id)
                .unwrap()
                .list_tasks(&Actor::User, &TaskFilter::default())
                .unwrap()
                .into_iter()
                .map(|task| task.title)
                .collect()
        }
    }

    fn git(folder: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(folder)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// A repo `app` with one commit, in a new folder.
    fn repo() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("app");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "--quiet"]);
        for (key, value) in [
            ("user.name", "Sam"),
            ("user.email", "sam@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git(&repo, &["config", key, value]);
        }
        fs::write(repo.join("README.md"), "readme\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "--quiet", "-m", "Initial"]);
        (dir, repo)
    }

    fn create(service: &Service, title: &str) -> TaskId {
        service
            .create_task(
                &Actor::User,
                NewTask {
                    title: title.into(),
                    ..NewTask::default()
                },
            )
            .unwrap()
            .id
    }

    #[test]
    fn a_folder_outside_git_is_only_opened_when_allowed() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();

        let opened = env.projects.open(folder.path(), false).unwrap();
        let repo_path = folder.path().canonicalize().unwrap().display().to_string();
        assert_eq!(opened, Opened::OutsideGit { repo_path });
        assert!(env.projects.list().unwrap().is_empty());

        let info = env.open(folder.path());
        assert!(info.open);
        assert_eq!(info.error, None);
        assert!(info.last_opened_at.is_some());
        assert_eq!(
            info.mcp_url.as_deref(),
            Some(format!("http://{}/p/{}/mcp", env.addr, info.id).as_str())
        );
        assert_eq!(env.projects.list().unwrap(), std::slice::from_ref(&info));
        assert_eq!(env.endpoints.project_ids(), [info.id]);
        // Nothing is written into the folder.
        assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 0);
    }

    #[test]
    fn every_worktree_of_a_repo_opens_the_same_project() {
        let env = Env::new(TempDir::new().unwrap());
        let (dir, repo) = repo();
        let worktree = dir.path().join("app-feature");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "feature",
                worktree.to_str().unwrap(),
            ],
        );
        let sub = repo.join("src");
        fs::create_dir(&sub).unwrap();

        // A repo opens without asking, from any folder in it.
        let Opened::Project { project: main } = env.projects.open(&sub, false).unwrap() else {
            panic!("a repo opens without asking");
        };
        assert_eq!(main.id, "app");
        assert_eq!(main.name, "app");
        let task = create(&env.projects.service(&main.id).unwrap(), "Shared");

        let other = env.open(&worktree);
        assert_eq!(other.id, main.id);
        assert_eq!(other.mcp_url, main.mcp_url);
        assert_eq!(
            other.repo_path,
            worktree.canonicalize().unwrap().display().to_string()
        );
        assert_eq!(env.projects.list().unwrap().len(), 1);
        assert_eq!(env.endpoints.project_ids(), ["app"]);
        assert_eq!(env.titles("app"), ["Shared"]);
        let found = env
            .projects
            .service("app")
            .unwrap()
            .get_task(&Actor::User, task)
            .unwrap();
        assert_eq!(found.content.changelog[0].who, "Sam");
    }

    #[test]
    fn opening_a_missing_folder_fails() {
        let env = Env::new(TempDir::new().unwrap());
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope");
        let error = env.projects.open(&missing, true).unwrap_err();
        assert_eq!(error.code, "invalid_argument");
        let file = dir.path().join("file");
        fs::write(&file, "").unwrap();
        assert_eq!(
            env.projects.open(&file, true).unwrap_err().code,
            "invalid_argument"
        );
        assert!(env.projects.list().unwrap().is_empty());
    }

    #[test]
    fn recent_projects_come_most_recent_first_and_reopen_on_launch() {
        let env = Env::new(TempDir::new().unwrap());
        let (a, b, gone) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        let a_id = env.open(a.path()).id;
        let gone_id = env.open(gone.path()).id;
        thread::sleep(Duration::from_millis(5));
        let b_id = env.open(b.path()).id;
        let ids: Vec<String> = env
            .projects
            .list()
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids, [b_id.clone(), gone_id.clone(), a_id.clone()]);
        // Opening a listed project again moves it to the top.
        thread::sleep(Duration::from_millis(5));
        env.open(a.path());
        assert_eq!(env.projects.list().unwrap()[0].id, a_id);

        let gone_path = gone.path().to_owned();
        drop(gone);
        let env = env.relaunch();
        let list = env.projects.list().unwrap();
        let by_id = |id: &str| list.iter().find(|p| p.id == id).unwrap().clone();
        assert!(by_id(&a_id).open && by_id(&b_id).open);
        let closed = by_id(&gone_id);
        assert!(!closed.open);
        assert_eq!(closed.mcp_url, None);
        assert_eq!(closed.error.as_deref(), Some("the folder is missing"));
        // Launch doesn't bring the folder back.
        assert!(!gone_path.exists());
        assert_eq!(
            env.projects.service(&gone_id).err().unwrap().code,
            "project_not_found"
        );
        let mut served = vec![a_id, b_id];
        served.sort();
        assert_eq!(env.endpoints.project_ids(), served);
    }

    #[test]
    fn tasks_stay_in_the_database_across_launches() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();
        let id = env.open(folder.path()).id;
        create(&env.projects.service(&id).unwrap(), "Kept");
        let env = env.relaunch();
        assert_eq!(env.titles(&id), ["Kept"]);
    }

    #[test]
    fn a_first_open_imports_an_old_tandem_folder_once() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();
        let dir = folder.path().join(TASK_DIR);
        fs::create_dir(&dir).unwrap();
        let task = |id: &str, title: &str| {
            format!("---\nid: {id}\ntitle: {title}\nstatus: todo\ncreated: 2026-10-13\n---\n")
        };
        fs::write(dir.join("k3f9x2m.md"), task("k3f9x2m", "From the folder")).unwrap();

        let id = env.open(folder.path()).id;
        assert_eq!(env.titles(&id), ["From the folder"]);

        // Files added later are not picked up: the database is the board.
        fs::write(dir.join("p2xmq7d.md"), task("p2xmq7d", "Added later")).unwrap();
        let env = env.relaunch();
        env.open(folder.path());
        assert_eq!(env.titles(&id), ["From the folder"]);
    }

    #[test]
    fn changes_are_forwarded_with_the_project_id() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();
        let id = env.open(folder.path()).id;
        let service = env.projects.service(&id).unwrap();

        let task = create(&service, "From the board");
        let (project, changes) = env.events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(project, id);
        assert!(changes.changed.contains(&task));

        service.delete_task(task).unwrap();
        let (project, changes) = env.events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(project, id);
        assert!(changes.removed.contains(&task));
    }

    #[test]
    fn removing_a_project_deletes_its_database_and_leaves_the_folder() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();
        fs::write(folder.path().join("notes.md"), "mine\n").unwrap();
        let id = env.open(folder.path()).id;
        create(&env.projects.service(&id).unwrap(), "Gone with it");
        let db = project_db_path(env.data.path(), &id);
        assert!(db.is_file());

        assert!(env.projects.remove(&id).unwrap());
        assert!(!db.exists());
        assert!(!env.projects.remove(&id).unwrap());
        assert!(env.projects.list().unwrap().is_empty());
        assert!(env.endpoints.project_ids().is_empty());
        assert_eq!(
            env.projects.service(&id).err().unwrap().code,
            "project_not_found"
        );
        assert!(folder.path().join("notes.md").is_file());
    }

    #[test]
    fn a_project_cannot_be_removed_while_an_agent_works_on_it() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();
        let id = env.open(folder.path()).id;
        let service = env.projects.service(&id).unwrap();
        let task = create(&service, "Busy");
        let agent = Actor::agent("claude-code-7f2a");
        service.start_task(&agent, task).unwrap();

        let error = env.projects.remove(&id).unwrap_err();
        assert_eq!(error.code, "agents_working");
        assert!(
            error.message.contains(&task.to_string()),
            "{}",
            error.message
        );
        // Nothing changed: still listed, open and reachable by agents.
        assert_eq!(env.projects.list().unwrap()[0].id, id);
        assert!(env.projects.service(&id).is_ok());
        assert_eq!(env.endpoints.project_ids(), std::slice::from_ref(&id));
        assert!(project_db_path(env.data.path(), &id).is_file());

        service.release_task(&agent, task, "").unwrap();
        drop(service);
        assert!(env.projects.remove(&id).unwrap());
        assert!(env.projects.list().unwrap().is_empty());
    }

    #[test]
    fn a_project_that_is_not_open_can_be_removed() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();
        let id = env.open(folder.path()).id;
        drop(folder);
        let env = env.relaunch();
        assert!(!env.projects.list().unwrap()[0].open);

        assert!(env.projects.remove(&id).unwrap());
        assert!(env.projects.list().unwrap().is_empty());
        assert!(!project_db_path(env.data.path(), &id).exists());
    }

    #[test]
    fn changing_the_port_moves_every_mcp_url_and_is_saved() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();
        let id = env.open(folder.path()).id;
        let settings = env.projects.app_settings().unwrap();
        assert_eq!(settings.mcp_port, DEFAULT_ADDR.port());
        assert_eq!(settings.default_mcp_port, DEFAULT_ADDR.port());
        assert_eq!(settings.mcp_url, Some(format!("http://{}", env.addr)));

        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);
        let settings = env.projects.set_mcp_port(port).unwrap();
        assert_eq!(settings.mcp_port, port);
        let url = format!("http://127.0.0.1:{port}");
        assert_eq!(settings.mcp_url.as_deref(), Some(url.as_str()));
        assert_eq!(settings.mcp_error, None);
        let info = env.projects.list().unwrap().remove(0);
        assert_eq!(info.mcp_url, Some(format!("{url}/p/{id}/mcp")));
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());

        // The port is kept for the next launch.
        let env = env.relaunch();
        env.projects.start_mcp().unwrap();
        assert_eq!(env.projects.app_settings().unwrap().mcp_url, Some(url));
        let app = AppDb::open(env.data.path()).unwrap();
        assert_eq!(app.settings().unwrap().mcp_port, Some(port));
    }

    #[test]
    fn a_port_that_cannot_be_used_changes_nothing() {
        let env = Env::new(TempDir::new().unwrap());
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();

        let error = env.projects.set_mcp_port(port).unwrap_err();
        assert_eq!(error.code, "port_in_use");
        let settings = env.projects.app_settings().unwrap();
        assert_eq!(settings.mcp_port, DEFAULT_ADDR.port());
        assert_eq!(settings.mcp_url, Some(format!("http://{}", env.addr)));
        assert_eq!(
            env.projects.set_mcp_port(0).unwrap_err().code,
            "invalid_argument"
        );
    }

    #[test]
    fn the_theme_is_saved() {
        let env = Env::new(TempDir::new().unwrap());
        assert_eq!(env.projects.app_settings().unwrap().theme, "system");
        assert_eq!(env.projects.set_theme(Theme::Dark).unwrap().theme, "dark");
        let env = env.relaunch();
        assert_eq!(env.projects.theme().unwrap(), Theme::Dark);
    }

    #[test]
    fn the_name_starts_as_the_folder_name_and_can_be_changed() {
        let env = Env::new(TempDir::new().unwrap());
        let folder = TempDir::new().unwrap();
        let folder_name = folder_name(&folder.path().canonicalize().unwrap());

        let info = env.open(folder.path());
        assert_eq!(info.name, folder_name);
        let id = info.id;

        let info = env.projects.set_name(&id, " Renamed ").unwrap();
        assert_eq!(info.name, "Renamed");
        assert_eq!(env.projects.list().unwrap()[0].name, "Renamed");
        // The id stays, so agent configs keep working.
        assert_eq!(info.id, id);

        assert_eq!(
            env.projects.set_name(&id, "a\nb").unwrap_err().code,
            "invalid_argument"
        );
        assert_eq!(env.projects.set_name(&id, "").unwrap().name, folder_name);
        assert_eq!(
            env.projects.set_name("missing", "x").unwrap_err().code,
            "project_not_found"
        );
    }
}
