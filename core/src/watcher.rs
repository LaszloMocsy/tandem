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
//! - An event on `config.toml`, or on the folder itself, is passed on with
//!   [`Service::config_changed`], so the app reads the project name again.
//! - Reads are ignored. On Linux, opening a file or listing the folder is an
//!   event too, and every scan lists the folder, so reacting to reads would
//!   scan forever.
//!
//! The app's own writes need no special handling: the service indexes each
//! file it writes, so the event for it finds the same hash and changes
//! nothing. Changes reach the board through [`Service::subscribe`], and
//! failures through the `on_error` callback given to [`Watcher::start`].

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use notify::event::{AccessKind, AccessMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::service::{Service, ServiceError};
use crate::store::{CONFIG_FILE, TASK_DIR, TaskId};

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
    watcher: Option<RecommendedWatcher>,
    stop: Sender<Message>,
    worker: Option<JoinHandle<()>>,
}

enum Message {
    Event(notify::Result<Event>),
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
        // Raw events: the worker debounces them itself. A debouncer that
        // merges events, such as dropping a file created and removed within
        // one window, would hide changes the index has already seen.
        let mut watcher = notify::recommended_watcher(move |result| {
            // Fails only once the worker has stopped.
            let _ = events.send(Message::Event(result));
        })
        .map_err(watch_error)?;
        watcher
            .watch(service.task_dir(), RecursiveMode::NonRecursive)
            .map_err(watch_error)?;
        let worker = thread::Builder::new()
            .name("tandem-watcher".into())
            .spawn(move || run(&service, &rx, &config, on_error))
            .map_err(WatcherError::Thread)?;
        Ok(Self {
            watcher: Some(watcher),
            stop: tx,
            worker: Some(worker),
        })
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // Stop new events first, then the worker.
        self.watcher.take();
        let _ = self.stop.send(Message::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The worker loop: gathers events into a batch and handles it once they
/// settle, and sweeps whenever a sweep interval passes. Failures go to
/// `on_error`; the next sweep retries whatever failed.
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
    let mut batch = Batch::default();
    let mut next_sweep = Instant::now() + config.sweep_interval;
    loop {
        let wake = batch
            .due(config.debounce)
            .map_or(next_sweep, |due| due.min(next_sweep));
        match messages.recv_timeout(wake.saturating_duration_since(Instant::now())) {
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Ok(Message::Event(Ok(event))) => batch.add(&event, Instant::now()),
            // The watcher may have dropped events; the scan covers the batch.
            Ok(Message::Event(Err(e))) => {
                on_error(WatchError::Watch(e));
                batch = Batch::default();
                rescan(&mut on_error);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        if batch.due(config.debounce).is_some_and(|due| due <= now) {
            let batch = std::mem::take(&mut batch);
            if touches_config(batch.paths()) {
                service.config_changed();
            }
            match batch.plan(config.burst) {
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
        if next_sweep <= now {
            rescan(&mut on_error);
            next_sweep = Instant::now() + config.sweep_interval;
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

/// The paths changed since the last batch was handled. Every path counts,
/// however its events add up: a file created and removed again within the
/// batch is re-read too, as the index may have seen it in between.
#[derive(Debug, Default)]
struct Batch {
    paths: BTreeSet<PathBuf>,
    /// The platform watcher lost events, so only a full scan will do.
    full: bool,
    /// When the first and the latest event came.
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Batch {
    /// Adds an event, unless it is a read: opening a file, listing the
    /// folder or closing a file opened for reading. Closing a file opened
    /// for writing counts, as it finishes a write.
    fn add(&mut self, event: &Event, now: Instant) {
        let read = match event.kind {
            EventKind::Access(AccessKind::Close(AccessMode::Write)) => false,
            EventKind::Access(_) => true,
            _ => false,
        };
        if read {
            return;
        }
        self.full |= event.need_rescan();
        self.paths.extend(event.paths.iter().cloned());
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// When to handle the batch: once `debounce` passes without an event,
    /// or five times that after the first, so a steady stream of events
    /// can't hold it back for long. `None` while it is empty.
    fn due(&self, debounce: Duration) -> Option<Instant> {
        Some((self.last? + debounce).min(self.first? + debounce * 5))
    }

    fn paths(&self) -> impl Iterator<Item = &Path> {
        self.paths.iter().map(PathBuf::as_path)
    }

    fn plan(&self, burst: usize) -> Plan {
        if self.full {
            Plan::Full
        } else {
            plan(self.paths(), burst)
        }
    }
}

fn plan<'a>(paths: impl IntoIterator<Item = &'a Path>, burst: usize) -> Plan {
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

/// Whether a batch of events may have changed `config.toml`: an event on
/// the file, or on the folder, which may have been replaced with another.
fn touches_config<'a>(paths: impl IntoIterator<Item = &'a Path>) -> bool {
    paths
        .into_iter()
        .filter_map(|path| path.file_name()?.to_str())
        .any(|name| name == CONFIG_FILE || name == TASK_DIR)
}

#[cfg(test)]
mod tests {
    use std::fs;

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
        assert!(!touches_config(paths.iter().map(PathBuf::as_path)));
        assert!(touches_config([dir.join("config.toml").as_path()]));
        assert!(touches_config([dir]));
        assert!(!touches_config([dir.join("config.toml.bak").as_path()]));
    }

    #[test]
    fn batches_every_change_but_no_reads() {
        use notify::event::{CreateKind, Flag, ModifyKind, RemoveKind};

        let dir = Path::new("/repo/.tandem");
        let event = |kind, name: &str| Event::new(kind).add_path(dir.join(name));
        let debounce = Duration::from_millis(200);
        let start = Instant::now();
        let mut batch = Batch::default();

        // Reads alone leave the batch empty.
        for read in [
            event(EventKind::Access(AccessKind::Open(AccessMode::Any)), ""),
            event(EventKind::Access(AccessKind::Read), "config.toml"),
            event(
                EventKind::Access(AccessKind::Close(AccessMode::Read)),
                "a7rq4ne.md",
            ),
        ] {
            batch.add(&read, start);
        }
        assert_eq!(batch.due(debounce), None);

        // A file created and removed again is still in the batch.
        batch.add(
            &event(EventKind::Create(CreateKind::File), "z9y8x7w.md"),
            start,
        );
        batch.add(
            &event(EventKind::Remove(RemoveKind::File), "z9y8x7w.md"),
            start,
        );
        batch.add(
            &event(
                EventKind::Access(AccessKind::Close(AccessMode::Write)),
                "k3f9x2m.md",
            ),
            start,
        );
        let later = start + Duration::from_millis(150);
        batch.add(
            &event(EventKind::Modify(ModifyKind::Any), "m2p8q4r.md"),
            later,
        );
        assert_eq!(
            batch.paths().collect::<Vec<_>>(),
            [
                dir.join("k3f9x2m.md"),
                dir.join("m2p8q4r.md"),
                dir.join("z9y8x7w.md"),
            ]
        );
        assert_eq!(
            batch.plan(10),
            Plan::Tasks(
                ["k3f9x2m", "m2p8q4r", "z9y8x7w"]
                    .map(|s| s.parse().unwrap())
                    .into()
            )
        );

        // Due once events settle, and at most five debounces after the first.
        assert_eq!(batch.due(debounce), Some(later + debounce));
        batch.add(&event(EventKind::Any, "m2p8q4r.md"), start + debounce * 6);
        assert_eq!(batch.due(debounce), Some(start + debounce * 5));

        // Lost events mean a full scan.
        batch.add(&Event::new(EventKind::Other).set_flag(Flag::Rescan), start);
        assert_eq!(batch.plan(10), Plan::Full);
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
        tx.send(Message::Event(Err(notify::Error::generic("lost events"))))
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
