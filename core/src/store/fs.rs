use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::id::TaskId;
use super::task::{ParseError, Task};

/// The folder at the repo root that holds the task files.
pub const TASK_DIR: &str = ".tandem";

/// An error reading or writing task files.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot access {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{} is not a valid task file", path.display())]
    Invalid {
        path: PathBuf,
        #[source]
        source: ParseError,
    },
    #[error("{} is not named `<id>.md`", path.display())]
    NotTaskFile { path: PathBuf },
    #[error("{} already exists", path.display())]
    AlreadyExists { path: PathBuf },
}

pub(super) fn io_error(path: &Path) -> impl FnOnce(io::Error) -> StoreError + '_ {
    move |source| StoreError::Io {
        path: path.to_owned(),
        source,
    }
}

/// The path of a task's file in `dir`.
pub fn task_path(dir: &Path, id: TaskId) -> PathBuf {
    dir.join(id.file_name())
}

/// Every task file in `dir`, sorted by id. Only regular files named exactly
/// `<id>.md` count; everything else in the folder is ignored.
pub fn list_task_files(dir: &Path) -> Result<Vec<(TaskId, PathBuf)>, StoreError> {
    let mut tasks = Vec::new();
    for entry in fs::read_dir(dir).map_err(io_error(dir))? {
        let entry = entry.map_err(io_error(dir))?;
        let Some(id) = entry.file_name().to_str().and_then(TaskId::from_file_name) else {
            continue;
        };
        let path = entry.path();
        if entry.file_type().map_err(io_error(&path))?.is_file() {
            tasks.push((id, path));
        }
    }
    tasks.sort_unstable_by_key(|(id, _)| *id);
    Ok(tasks)
}

/// Reads and parses a task file. Fails with [`StoreError::Invalid`] if the
/// file cannot be parsed or its `id` does not match its name.
pub fn read_task(path: &Path) -> Result<Task, StoreError> {
    let id = id_for_path(path)?;
    let bytes = fs::read(path).map_err(io_error(path))?;
    parse_task_file(id, &bytes).map_err(|source| StoreError::Invalid {
        path: path.to_owned(),
        source,
    })
}

/// Writes a task to `dir`, creating or replacing its file atomically.
/// Refuses to replace a file that does not parse, so a broken file is never
/// overwritten.
pub fn write_task(dir: &Path, task: &Task) -> Result<PathBuf, StoreError> {
    let path = task_path(dir, task.id);
    match fs::read(&path) {
        Ok(existing) => {
            parse_task_file(task.id, &existing).map_err(|source| StoreError::Invalid {
                path: path.clone(),
                source,
            })?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(&path)(e)),
    }
    write_atomic(&path, &serialize(&path, task)?, Mode::Replace)?;
    Ok(path)
}

/// Writes a new task to `dir` atomically. Fails with
/// [`StoreError::AlreadyExists`] if a file with its id is already there.
pub fn create_task(dir: &Path, task: &Task) -> Result<PathBuf, StoreError> {
    let path = task_path(dir, task.id);
    write_atomic(&path, &serialize(&path, task)?, Mode::CreateNew)?;
    Ok(path)
}

/// A random id with no file in `dir` yet.
pub fn unused_id(dir: &Path) -> Result<TaskId, StoreError> {
    loop {
        let id = TaskId::generate();
        let path = task_path(dir, id);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(id),
            Err(e) => return Err(io_error(&path)(e)),
            Ok(_) => {}
        }
    }
}

fn id_for_path(path: &Path) -> Result<TaskId, StoreError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(TaskId::from_file_name)
        .ok_or_else(|| StoreError::NotTaskFile {
            path: path.to_owned(),
        })
}

/// Parses the contents of the file for task `id`, checking that the `id` in
/// its frontmatter matches.
pub fn parse_task_file(id: TaskId, bytes: &[u8]) -> Result<Task, ParseError> {
    let text = std::str::from_utf8(bytes).map_err(|_| ParseError::NotUtf8)?;
    let task = Task::parse(text)?;
    if task.id != id {
        return Err(ParseError::IdMismatch {
            expected: id,
            found: task.id,
        });
    }
    Ok(task)
}

/// Serializes `task` and checks that the result parses, so the store never
/// writes a file it would later report as invalid (an empty title, say).
fn serialize(path: &Path, task: &Task) -> Result<String, StoreError> {
    let markdown = task.to_markdown();
    Task::parse(&markdown).map_err(|source| StoreError::Invalid {
        path: path.to_owned(),
        source,
    })?;
    Ok(markdown)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Replace,
    CreateNew,
}

/// Writes a temp file next to `path`, flushes it to disk, then renames it
/// into place, so readers see either the old file or the new one. The temp
/// name never matches `<id>.md`, so listing and the watcher ignore it.
pub(super) fn write_atomic(path: &Path, contents: &str, mode: Mode) -> Result<(), StoreError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut builder = tempfile::Builder::new();
    builder.prefix(".tmp-").suffix(".tandem");
    // Temp files are owner-only by default. A replaced file keeps its mode;
    // a new one gets the usual 0o666 minus the umask, like any editor's.
    #[cfg(unix)]
    let existing = fs::metadata(path).map(|m| m.permissions()).ok();
    #[cfg(unix)]
    if existing.is_none() {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o666));
    }
    let mut file = builder.tempfile_in(dir).map_err(io_error(dir))?;
    #[cfg(unix)]
    if let Some(permissions) = existing {
        file.as_file()
            .set_permissions(permissions)
            .map_err(io_error(file.path()))?;
    }
    file.write_all(contents.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(io_error(file.path()))?;

    let persisted = match mode {
        Mode::Replace => file.persist(path),
        Mode::CreateNew => file.persist_noclobber(path),
    };
    // On failure the temp file is dropped with the error, which deletes it.
    persisted.map_err(|e| match e.error.kind() {
        io::ErrorKind::AlreadyExists if mode == Mode::CreateNew => StoreError::AlreadyExists {
            path: path.to_owned(),
        },
        _ => io_error(path)(e.error),
    })?;

    // Make the rename itself durable.
    #[cfg(unix)]
    fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(io_error(dir))?;
    Ok(())
}
