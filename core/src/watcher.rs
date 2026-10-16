//! Watches `.tandem/` for outside edits and feeds them into the service,
//! following the spec's rebuild rules:
//!
//! - Events are debounced by about 200 ms, then each changed task file is
//!   re-parsed.
//! - A burst of events, such as a branch switch or `git pull`, triggers a
//!   full scan instead, as does an event on the folder itself or an error
//!   that may have lost events.
//! - Every 60 seconds a stat-only sweep catches what the watcher missed,
//!   for example during sleep or on network drives.
//!
//! The app's own writes need no special handling: the service indexes each
//! file it writes, so the event for it finds the same hash and changes
//! nothing. Changes reach the board through [`Service::subscribe`], and
//! failures through the `on_error` callback given to [`Watcher::start`].

use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

use crate::service::{Service, ServiceError};
use crate::store::{TASK_DIR, TaskId};

/// Settings for a [`Watcher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherConfig {
    /// How long events for a file must settle before it is re-read.
    pub debounce: Duration,
    /// How often the stat-only sweep runs.
    pub sweep_interval: Duration,
    /// More changed task files than this in one debounced batch is a burst,
    /// which runs a full scan.
    pub burst: usize,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(200),
            sweep_interval: Duration::from_secs(60),
            burst: 10,
        }
    }
}

/// Why the watcher could not start.
#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    #[error("cannot watch {}", path.display())]
    Watch {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },
    #[error("cannot start the watcher thread")]
    Thread(#[source] io::Error),
}

/// A failure while watching, passed to the `on_error` callback. None is
/// fatal: the watcher keeps running, and the next sweep retries whatever
/// failed, so a lasting problem is reported again on every sweep.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// A full scan failed, after a burst, a watcher error or on a sweep.
    #[error("scanning the task folder failed")]
    Rescan(#[source] ServiceError),
    /// Re-reading one changed task file failed.
    #[error("re-reading task `{id}` failed")]
    RescanTask {
        id: TaskId,
        #[source]
        source: ServiceError,
    },
    /// The platform watcher reported an error, so events may have been
    /// lost. A full scan follows.
    #[error("the file watcher failed; running a full scan")]
    Watch(#[source] notify::Error),
}

/// A running watcher for one project. Dropping it stops the watching and
/// the sweep, and waits for the worker thread to finish.
pub struct Watcher {
    debouncer: Option<Debouncer<RecommendedWatcher>>,
    stop: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

enum Message {
    Events(DebounceEventResult),
    Stop,
}

impl Watcher {
    /// Starts watching the service's `.tandem/` folder. `on_error` runs on
    /// the watcher's thread for every failure, so the app can show it.
    pub fn start(
        service: Arc<Service>,
        config: WatcherConfig,
        on_error: impl FnMut(WatchError) + Send + 'static,
    ) -> Result<Self, WatcherError> {
        let (tx, rx) = mpsc::channel();
        let events = tx.clone();
        let watch_error = |source| WatcherError::Watch {
            path: service.task_dir().to_owned(),
            source,
        };
        let mut debouncer = new_debouncer(config.debounce, move |result| {
            // Fails only once the worker has stopped.
            let _ = events.send(Message::Events(result));
        })
        .map_err(watch_error)?;
        debouncer
            .watcher()
            .watch(service.task_dir(), RecursiveMode::NonRecursive)
            .map_err(watch_error)?;
        let worker = thread::Builder::new()
            .name("tandem-watcher".into())
            .spawn(move || run(&service, &rx, &config, on_error))
            .map_err(WatcherError::Thread)?;
        Ok(Self {
            debouncer: Some(debouncer),
            stop: tx,
            worker: Some(worker),
        })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // Stop new events first, then the worker.
        self.debouncer.take();
        let _ = self.stop.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The worker loop: handles debounced batches as they come, and sweeps
/// whenever a sweep interval passes. Failures go to `on_error`; the next
/// sweep retries whatever failed.
fn run(
    service: &Service,
    messages: &mpsc::Receiver<Message>,
    config: &WatcherConfig,
    mut on_error: impl FnMut(WatchError),
) {
    let rescan = |on_error: &mut dyn FnMut(WatchError)| {
        if let Err(e) = service.rescan() {
            on_error(WatchError::Rescan(e));
        }
    };
    let mut next_sweep = Instant::now() + config.sweep_interval;
    loop {
        let wait = next_sweep.saturating_duration_since(Instant::now());
        match messages.recv_timeout(wait) {
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Message::Events(Ok(events))) => {
                match plan(events.iter().map(|e| e.path.as_path()), config.burst) {
                    Plan::Full => rescan(&mut on_error),
                    Plan::Tasks(ids) => {
                        for id in ids {
                            if let Err(source) = service.rescan_task(id) {
                                on_error(WatchError::RescanTask { id, source });
                            }
                        }
                    }
                }
            }
            // The watcher may have dropped events.
            Ok(Message::Events(Err(e))) => {
                on_error(WatchError::Watch(e));
                rescan(&mut on_error);
            }
            Err(RecvTimeoutError::Timeout) => {
                rescan(&mut on_error);
                next_sweep = Instant::now() + config.sweep_interval;
            }
        }
    }
}

/// What to do about one debounced batch of events.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// A full scan.
    Full,
    /// Re-read these task files. Other files in the folder are ignored.
    Tasks(BTreeSet<TaskId>),
}

fn plan<'a>(paths: impl IntoIterator<Item = &'a std::path::Path>, burst: usize) -> Plan {
    let mut ids = BTreeSet::new();
    for path in paths {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == TASK_DIR {
            // The folder itself was created, removed or replaced.
            return Plan::Full;
        }
        if let Some(id) = TaskId::from_file_name(name) {
            ids.insert(id);
        }
    }
    if ids.len() > burst {
        Plan::Full
    } else {
        Plan::Tasks(ids)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::service::{Actor, Config, NewTask};

    #[test]
    fn plans_task_files_and_bursts() {
        let dir = Path::new("/repo/.tandem");
        let paths = [
            dir.join("a7rq4ne.md"),
            dir.join("a7rq4ne.md"),
            dir.join("k3f9x2m.md"),
            dir.join("README.md"),
            dir.join(".tmp-x1y2.tandem"),
        ];
        assert_eq!(
            plan(paths.iter().map(PathBuf::as_path), 10),
            Plan::Tasks(["a7rq4ne", "k3f9x2m"].map(|s| s.parse().unwrap()).into())
        );
        assert_eq!(plan(paths.iter().map(PathBuf::as_path), 1), Plan::Full);
        assert_eq!(plan([dir], 10), Plan::Full);
        assert_eq!(
            plan([dir.join("notes.txt").as_path()], 10),
            Plan::Tasks(BTreeSet::new())
        );
    }

    #[test]
    fn the_sweep_finds_edits_the_watcher_missed() {
        let data = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let service =
            Arc::new(Service::open(data.path(), "test", repo.path(), Config::default()).unwrap());
        let id = service
            .create_task(
                &Actor::User,
                NewTask {
                    title: "Before".into(),
                    ..NewTask::default()
                },
            )
            .unwrap()
            .id;
        let changes = service.subscribe();

        // The worker alone, with no file events at all.
        let (tx, rx) = mpsc::channel();
        let config = WatcherConfig {
            sweep_interval: Duration::from_millis(100),
            ..WatcherConfig::default()
        };
        let worker = {
            let service = Arc::clone(&service);
            thread::spawn(move || run(&service, &rx, &config, |e| panic!("{e}")))
        };
        let path = service.task_dir().join(id.file_name());
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("Before", "After!");
        fs::write(&path, text).unwrap();

        let found = changes.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(found.changed, [id].into());
        tx.send(Message::Stop).unwrap();
        worker.join().unwrap();
        let tasks = service
            .list_tasks(&Actor::User, &Default::default())
            .unwrap();
        assert_eq!(tasks[0].title, "After!");
    }

    #[test]
    fn failures_go_to_the_error_callback() {
        let data = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let service =
            Arc::new(Service::open(data.path(), "test", repo.path(), Config::default()).unwrap());
        let (errors_tx, errors) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let config = WatcherConfig {
            sweep_interval: Duration::from_millis(100),
            ..WatcherConfig::default()
        };
        let worker = {
            let service = Arc::clone(&service);
            thread::spawn(move || run(&service, &rx, &config, move |e| errors_tx.send(e).unwrap()))
        };

        // A watcher error is reported, then a full scan runs.
        tx.send(Message::Events(Err(notify::Error::generic("lost events"))))
            .unwrap();
        let error = errors.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(error, WatchError::Watch(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            "the file watcher failed; running a full scan"
        );

        // A sweep that cannot read the folder reports it, on every sweep.
        let dir = service.task_dir().to_owned();
        fs::remove_dir_all(&dir).unwrap();
        fs::write(&dir, "not a folder").unwrap();
        for _ in 0..2 {
            let error = errors.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(matches!(error, WatchError::Rescan(_)), "{error:?}");
        }
        tx.send(Message::Stop).unwrap();
        worker.join().unwrap();
    }
}
