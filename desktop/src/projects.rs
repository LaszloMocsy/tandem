//! The project registry: the projects in `app.db` and the ones open now.
//!
//! An open project has its service, its watcher, a thread that forwards
//! its change events, and an MCP endpoint. Every project in the list whose
//! `.tandem/` folder exists is opened on launch and stays open until the
//! app quits or the project is removed, so agents can reach any of them
//! whichever one the board shows.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;

use serde::Serialize;
use tandem_board_core::index::{AppDb, Changes, Project};
use tandem_board_core::service::{Config, Service};
use tandem_board_core::store::TASK_DIR;
use tandem_board_core::watcher::{Watcher, WatcherConfig};
use tandem_board_mcp::{Endpoints, endpoint_path};

use crate::error::{Error, describe, report};

/// Called with a project id and the tasks that changed in it.
pub type OnChange = Arc<dyn Fn(&str, &Changes) + Send + Sync>;

/// A project as the project switcher shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub repo_path: String,
    pub last_opened_at: Option<String>,
    /// Whether the project is open, so its tasks can be listed.
    pub open: bool,
    /// Why the project is not open, when it isn't.
    pub error: Option<String>,
    /// Why the project is read-only, when it is.
    pub read_only: Option<String>,
    /// The project's MCP endpoint, while it is open and the server runs.
    pub mcp_url: Option<String>,
}

/// What opening a folder did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Opened {
    /// The project is open.
    Project { project: ProjectInfo },
    /// The folder has no `.tandem/`. Nothing was created or added to the
    /// list; open it again with `create_folder` to create `.tandem/`.
    #[serde(rename_all = "camelCase")]
    NoTaskFolder { repo_path: String },
}

/// Every project in `app.db`, and the open ones.
pub struct Projects {
    data_dir: PathBuf,
    app_db: Mutex<AppDb>,
    open: Mutex<BTreeMap<String, OpenProject>>,
    /// Why a project failed to open on launch.
    failed: Mutex<BTreeMap<String, String>>,
    endpoints: Endpoints,
    mcp_addr: Option<SocketAddr>,
    on_change: OnChange,
}

/// A running project. Dropping it stops the watcher; the forwarding thread
/// ends once the last handle on the service is gone.
struct OpenProject {
    service: Arc<Service>,
    _watcher: Watcher,
}

impl Projects {
    /// Opens `app.db` in `data_dir`. Open projects get an endpoint in
    /// `endpoints`, served at `mcp_addr` if the server is running.
    pub fn new(
        data_dir: &Path,
        endpoints: Endpoints,
        mcp_addr: Option<SocketAddr>,
        on_change: OnChange,
    ) -> Result<Self, Error> {
        Ok(Self {
            data_dir: data_dir.to_owned(),
            app_db: Mutex::new(AppDb::open(data_dir)?),
            open: Mutex::default(),
            failed: Mutex::default(),
            endpoints,
            mcp_addr,
            on_change,
        })
    }

    /// Opens every project in the list whose `.tandem/` folder still
    /// exists, as on launch. Projects without one are left closed, so
    /// nothing is created behind your back. Failures are reported to
    /// stderr and shown in the project list.
    pub fn open_all(&self) -> Result<(), Error> {
        let projects = lock(&self.app_db).projects()?;
        for project in projects {
            if !has_task_folder(&project.repo_path) {
                continue;
            }
            if let Err(e) = self.start(&project) {
                eprintln!(
                    "tandem: cannot open {}: {}",
                    project.repo_path.display(),
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

    /// Opens the repo at `path` and records it as the most recently opened
    /// project, adding it to the list if needed. If the folder has no
    /// `.tandem/`, creates one when `create_folder` is set, and otherwise
    /// changes nothing and says so.
    pub fn open(&self, path: &Path, create_folder: bool) -> Result<Opened, Error> {
        let repo = path.canonicalize().map_err(|e| {
            Error::new(
                "invalid_argument",
                format!("cannot open {}: {e}", path.display()),
            )
        })?;
        if !repo.is_dir() {
            return Err(Error::new(
                "invalid_argument",
                format!("{} is not a folder", repo.display()),
            ));
        }
        if !create_folder && !has_task_folder(&repo) {
            return Ok(Opened::NoTaskFolder {
                repo_path: repo.display().to_string(),
            });
        }
        let project = lock(&self.app_db).add_project(&repo)?;
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

    /// Closes the project and removes it from the list. Its repo and
    /// database are left alone. Returns `false` if there is no such project.
    pub fn remove(&self, id: &str) -> Result<bool, Error> {
        lock(&self.open).remove(id);
        lock(&self.failed).remove(id);
        self.endpoints.remove(id);
        Ok(lock(&self.app_db).remove_project(id)?)
    }

    /// The service of an open project.
    pub fn service(&self, id: &str) -> Result<Arc<Service>, Error> {
        lock(&self.open)
            .get(id)
            .map(|project| Arc::clone(&project.service))
            .ok_or_else(|| Error::project_not_found(id))
    }

    /// Opens the project unless it is open already: its service, watcher,
    /// change forwarding and MCP endpoint.
    fn start(&self, project: &Project) -> Result<(), Error> {
        let mut open = lock(&self.open);
        if open.contains_key(&project.id) {
            return Ok(());
        }
        let config = Config {
            user_name: git_user_name(&project.repo_path).unwrap_or_else(|| "user".to_owned()),
            ..Config::default()
        };
        let service = Arc::new(Service::open(
            &self.data_dir,
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

        // Subscribe before the watcher starts, so no change is missed.
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

        let path = project.repo_path.clone();
        let watcher = Watcher::start(Arc::clone(&service), WatcherConfig::default(), move |e| {
            report(&format!("watching {}", path.display()), &e);
        })?;
        self.endpoints.insert(&project.id, Arc::clone(&service));
        open.insert(
            project.id.clone(),
            OpenProject {
                service,
                _watcher: watcher,
            },
        );
        Ok(())
    }

    fn info(&self, project: &Project) -> ProjectInfo {
        let open = lock(&self.open);
        let service = open.get(&project.id).map(|open| &open.service);
        let error = match service {
            Some(_) => None,
            None => Some(self.closed_reason(project)),
        };
        ProjectInfo {
            id: project.id.clone(),
            name: project.name.clone(),
            repo_path: project.repo_path.display().to_string(),
            last_opened_at: project.last_opened_at.clone(),
            open: service.is_some(),
            error,
            read_only: service
                .and_then(|service| service.read_only())
                .map(ToString::to_string),
            mcp_url: service
                .and(self.mcp_addr)
                .map(|addr| format!("http://{addr}{}", endpoint_path(&project.id))),
        }
    }

    fn closed_reason(&self, project: &Project) -> String {
        if let Some(message) = lock(&self.failed).get(&project.id) {
            return message.clone();
        }
        match fs::metadata(&project.repo_path) {
            Ok(meta) if meta.is_dir() => format!("the folder has no {TASK_DIR} folder"),
            Ok(_) => "the project path is not a folder".to_owned(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => "the folder is missing".to_owned(),
            Err(e) => describe(&e),
        }
    }
}

/// Whether `repo` has a `.tandem` entry. A symlinked one counts; it opens
/// read-only.
fn has_task_folder(repo: &Path) -> bool {
    fs::symlink_metadata(repo.join(TASK_DIR)).is_ok()
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use tandem_board_core::service::{Actor, NewTask};
    use tandem_board_core::store::TaskId;
    use tempfile::TempDir;
    use tokio_util::sync::CancellationToken;

    use super::*;

    struct Env {
        data: TempDir,
        cancel: CancellationToken,
        endpoints: Endpoints,
        events: mpsc::Receiver<(String, Changes)>,
        projects: Projects,
    }

    impl Env {
        fn new(data: TempDir) -> Self {
            let cancel = CancellationToken::new();
            let endpoints = Endpoints::new(&cancel);
            let (tx, events) = mpsc::channel();
            let tx = Mutex::new(tx);
            let projects = Projects::new(
                data.path(),
                endpoints.clone(),
                Some("127.0.0.1:4317".parse().unwrap()),
                Arc::new(move |id: &str, changes: &Changes| {
                    let _ = lock(&tx).send((id.to_owned(), changes.clone()));
                }),
            )
            .unwrap();
            Self {
                data,
                cancel,
                endpoints,
                events,
                projects,
            }
        }

        fn open(&self, repo: &Path, create_folder: bool) -> Opened {
            self.projects.open(repo, create_folder).unwrap()
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
    }

    fn project(opened: Opened) -> ProjectInfo {
        match opened {
            Opened::Project { project } => project,
            other => panic!("expected an open project, got {other:?}"),
        }
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
    fn a_folder_without_tandem_is_only_opened_when_asked_to_create_it() {
        let env = Env::new(TempDir::new().unwrap());
        let repo = TempDir::new().unwrap();

        let opened = env.open(repo.path(), false);
        let repo_path = repo.path().canonicalize().unwrap().display().to_string();
        assert_eq!(opened, Opened::NoTaskFolder { repo_path });
        assert!(!repo.path().join(TASK_DIR).exists());
        assert!(env.projects.list().unwrap().is_empty());

        let info = project(env.open(repo.path(), true));
        assert!(repo.path().join(TASK_DIR).join("config.toml").is_file());
        assert!(repo.path().join(TASK_DIR).join("README.md").is_file());
        assert!(info.open);
        assert_eq!(info.error, None);
        assert!(info.last_opened_at.is_some());
        assert_eq!(
            info.mcp_url.as_deref(),
            Some(format!("http://127.0.0.1:4317/p/{}/mcp", info.id).as_str())
        );
        assert_eq!(env.projects.list().unwrap(), std::slice::from_ref(&info));
        assert_eq!(env.endpoints.project_ids(), [info.id]);
    }

    #[test]
    fn opening_a_missing_folder_fails() {
        let env = Env::new(TempDir::new().unwrap());
        let repo = TempDir::new().unwrap();
        let missing = repo.path().join("nope");
        let error = env.projects.open(&missing, true).unwrap_err();
        assert_eq!(error.code, "invalid_argument");
        let file = repo.path().join("file");
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
        let a_id = project(env.open(a.path(), true)).id;
        let gone_id = project(env.open(gone.path(), true)).id;
        thread::sleep(Duration::from_millis(5));
        let b_id = project(env.open(b.path(), true)).id;
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
        project(env.open(a.path(), false));
        assert_eq!(env.projects.list().unwrap()[0].id, a_id);

        fs::remove_dir_all(gone.path().join(TASK_DIR)).unwrap();
        let env = env.relaunch();
        let list = env.projects.list().unwrap();
        let by_id = |id: &str| list.iter().find(|p| p.id == id).unwrap().clone();
        assert!(by_id(&a_id).open && by_id(&b_id).open);
        let closed = by_id(&gone_id);
        assert!(!closed.open);
        assert_eq!(closed.mcp_url, None);
        assert_eq!(
            closed.error.as_deref(),
            Some("the folder has no .tandem folder")
        );
        // Launch doesn't bring the folder back.
        assert!(!gone.path().join(TASK_DIR).exists());
        assert_eq!(
            env.projects.service(&gone_id).err().unwrap().code,
            "project_not_found"
        );
        let mut served = vec![a_id, b_id];
        served.sort();
        assert_eq!(env.endpoints.project_ids(), served);
    }

    #[test]
    fn changes_are_forwarded_with_the_project_id() {
        let env = Env::new(TempDir::new().unwrap());
        let repo = TempDir::new().unwrap();
        let id = project(env.open(repo.path(), true)).id;
        let service = env.projects.service(&id).unwrap();

        let task = create(&service, "From the board");
        let (project, changes) = env.events.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(project, id);
        assert!(changes.changed.contains(&task));

        // An outside edit reaches the UI through the watcher.
        fs::remove_file(service.task_dir().join(task.file_name())).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let (project, changes) = env
                .events
                .recv_timeout(deadline - std::time::Instant::now())
                .expect("the watcher reports the deleted file");
            if project == id && changes.removed.contains(&task) {
                break;
            }
        }
    }

    #[test]
    fn removing_a_project_closes_it_and_keeps_its_files() {
        let env = Env::new(TempDir::new().unwrap());
        let repo = TempDir::new().unwrap();
        let id = project(env.open(repo.path(), true)).id;
        let task = create(&env.projects.service(&id).unwrap(), "Kept");

        assert!(env.projects.remove(&id).unwrap());
        assert!(!env.projects.remove(&id).unwrap());
        assert!(env.projects.list().unwrap().is_empty());
        assert!(env.endpoints.project_ids().is_empty());
        assert_eq!(
            env.projects.service(&id).err().unwrap().code,
            "project_not_found"
        );
        assert!(repo.path().join(TASK_DIR).join(task.file_name()).is_file());
    }
}
