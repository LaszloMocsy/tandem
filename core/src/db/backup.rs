//! Backups of a project's database, in the app's data directory:
//! `<data_dir>/backups/<project-id>/<time>.db`, each a full copy made with
//! `VACUUM INTO`, named by the UTC time it was made, such as
//! `20261023T093000.123Z.db`.
//!
//! The folder also holds a `git-dir` file naming the project's git common
//! dir, so the backups of a removed project are never shown to a
//! different project that later gets the same id.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use jiff::civil::DateTime;
use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use rusqlite::{Connection, OpenFlags};

use super::{DbError, io_error, schema};

/// The folder in the app's data directory that holds every project's
/// backups.
const BACKUP_DIR: &str = "backups";

/// The file in a project's backup folder that names its git common dir.
const OWNER_FILE: &str = "git-dir";

/// How long every backup is kept.
pub const KEEP_ALL: SignedDuration = SignedDuration::from_hours(24);

/// How long the newest backup of each day is kept.
pub const KEEP_DAILY: SignedDuration = SignedDuration::from_hours(30 * 24);

/// The time format of a backup's name, without the `Z`.
const NAME_FORMAT: &str = "%Y%m%dT%H%M%S%.3f";

/// A backup of a project's database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    /// The file name without `.db`, such as `20261023T093000.123Z`. It
    /// identifies the backup.
    pub name: String,
    pub path: PathBuf,
    /// When it was made.
    pub at: Timestamp,
    /// In bytes.
    pub size: u64,
    /// The tasks on its board, deleted ones not counted; `None` if the
    /// file can't be read.
    pub tasks: Option<u64>,
}

/// Where the backups of project `project_id` live:
/// `<data_dir>/backups/<project-id>`.
pub fn backup_dir(data_dir: &Path, project_id: &str) -> PathBuf {
    data_dir.join(BACKUP_DIR).join(project_id)
}

/// The git common dir, or folder, the backups in `dir` were made for, if
/// it is recorded.
pub fn backups_owner(dir: &Path) -> Option<String> {
    let owner = fs::read_to_string(dir.join(OWNER_FILE)).ok()?;
    Some(owner.trim_end_matches('\n').to_owned())
}

/// Records that the backups in `dir` belong to the project identified by
/// `git_dir`, creating the folder if needed.
pub fn set_backups_owner(dir: &Path, git_dir: &Path) -> Result<(), DbError> {
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    let path = dir.join(OWNER_FILE);
    let text = git_dir
        .to_str()
        .ok_or_else(|| DbError::NonUtf8Path {
            path: git_dir.to_owned(),
        })?
        .to_owned();
    fs::write(&path, text + "\n").map_err(io_error(&path))
}

/// Copies the database on `conn` into a new backup in `dir`, named by
/// `at`, or a millisecond later for each backup with that name already.
/// The copy is written to a hidden file first, so a backup is never seen
/// half written.
pub fn write(conn: &Connection, dir: &Path, at: Timestamp) -> Result<Backup, DbError> {
    fs::create_dir_all(dir).map_err(io_error(dir))?;
    let mut at = Timestamp::from_millisecond(at.as_millisecond()).unwrap_or(at);
    let (name, path) = loop {
        let name = name_for(at);
        let path = dir.join(format!("{name}.db"));
        if !path.exists() {
            break (name, path);
        }
        at += SignedDuration::from_millis(1);
    };
    let tmp = dir.join(format!(".{name}.db.tmp"));
    match fs::remove_file(&tmp) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(io_error(&tmp)(e)),
        _ => {}
    }
    let tmp_text = tmp
        .to_str()
        .ok_or_else(|| DbError::NonUtf8Path { path: tmp.clone() })?;
    conn.execute("VACUUM INTO ?1", [tmp_text])?;
    fs::rename(&tmp, &path).map_err(io_error(&path))?;
    let size = fs::metadata(&path).map_err(io_error(&path))?.len();
    Ok(Backup {
        name,
        path,
        at,
        size,
        tasks: Some(count_tasks(conn)?),
    })
}

/// The backups in `dir`, the newest first. Other files are ignored, and a
/// missing folder has none.
pub fn list(dir: &Path) -> Result<Vec<Backup>, DbError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(io_error(dir)(e)),
    };
    let mut backups = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_error(dir))?;
        let path = entry.path();
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".db"))
        else {
            continue;
        };
        let Some(at) = parse_name(name) else {
            continue;
        };
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        backups.push(Backup {
            name: name.to_owned(),
            at,
            size: meta.len(),
            tasks: open_read_only(&path)
                .ok()
                .and_then(|conn| count_tasks(&conn).ok()),
            path,
        });
    }
    backups.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| b.name.cmp(&a.name)));
    Ok(backups)
}

/// The backup in `dir` named `name`, if there is one.
pub fn find(dir: &Path, name: &str) -> Result<Option<Backup>, DbError> {
    Ok(list(dir)?.into_iter().find(|backup| backup.name == name))
}

/// Checks that the backup at `path` can be restored: a sound SQLite file
/// with a project schema this app supports.
pub(crate) fn check(path: &Path) -> Result<(), DbError> {
    let conn = open_read_only(path)?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let supported = schema::PROJECT_MIGRATIONS.len() as i64;
    if version > supported {
        return Err(DbError::NewerSchema {
            path: path.to_owned(),
            version,
            supported,
        });
    }
    let result: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if version < 1 || result != "ok" {
        return Err(DbError::NotABackup {
            path: path.to_owned(),
        });
    }
    Ok(())
}

/// Deletes the backups `prune_plan` doesn't keep. Returns how many went.
pub fn prune(dir: &Path, now: Timestamp, time_zone: &TimeZone) -> Result<usize, DbError> {
    let backups = list(dir)?;
    let doomed = prune_plan(&backups, now, time_zone);
    for backup in &doomed {
        remove(&backup.path)?;
    }
    Ok(doomed.len())
}

/// The backups to delete, from `backups` sorted newest first: every backup
/// from the last [`KEEP_ALL`] is kept, and the newest of each day, in
/// `time_zone`, for [`KEEP_DAILY`].
pub fn prune_plan(backups: &[Backup], now: Timestamp, time_zone: &TimeZone) -> Vec<Backup> {
    let mut days = Vec::new();
    let mut doomed = Vec::new();
    for backup in backups {
        let age = now.duration_since(backup.at);
        let day = backup.at.to_zoned(time_zone.clone()).date();
        let newest_of_day = !days.contains(&day);
        days.push(day);
        let keep = age < KEEP_ALL || (age < KEEP_DAILY && newest_of_day);
        if !keep {
            doomed.push(backup.clone());
        }
    }
    doomed
}

/// Deletes every backup in `dir` except the one named `keep`.
pub fn keep_only(dir: &Path, keep: &str) -> Result<(), DbError> {
    for backup in list(dir)? {
        if backup.name != keep {
            remove(&backup.path)?;
        }
    }
    Ok(())
}

/// Deletes `dir` with every backup in it. A missing folder is fine.
pub fn delete_all(dir: &Path) -> Result<(), DbError> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_error(dir)(e)),
        _ => Ok(()),
    }
}

/// Whether the database at `db_path`, or its WAL, was written after `at`.
/// A database that can't be read counts as changed.
pub fn changed_since(db_path: &Path, at: Timestamp) -> bool {
    let at = SystemTime::from(at);
    ["", "-wal"].iter().any(|suffix| {
        let mut path = db_path.as_os_str().to_owned();
        path.push(suffix);
        match fs::metadata(PathBuf::from(path)).and_then(|meta| meta.modified()) {
            Ok(modified) => modified > at,
            Err(e) => e.kind() != io::ErrorKind::NotFound,
        }
    })
}

fn remove(path: &Path) -> Result<(), DbError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_error(path)(e)),
        _ => Ok(()),
    }
}

fn open_read_only(path: &Path) -> Result<Connection, DbError> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn count_tasks(conn: &Connection) -> Result<u64, DbError> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM tasks WHERE deleted_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    Ok(count.unsigned_abs())
}

fn name_for(at: Timestamp) -> String {
    format!("{}Z", at.to_zoned(TimeZone::UTC).strftime(NAME_FORMAT))
}

fn parse_name(name: &str) -> Option<Timestamp> {
    let civil = DateTime::strptime(NAME_FORMAT, name.strip_suffix('Z')?).ok()?;
    let at = civil.to_zoned(TimeZone::UTC).ok()?.timestamp();
    (name_for(at) == name).then_some(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn backup(when: &str) -> Backup {
        let at = at(when);
        Backup {
            name: name_for(at),
            path: PathBuf::from(format!("{}.db", name_for(at))),
            at,
            size: 0,
            tasks: None,
        }
    }

    #[test]
    fn names_round_trip() {
        let t = at("2026-10-23T09:30:00.123Z");
        assert_eq!(name_for(t), "20261023T093000.123Z");
        assert_eq!(parse_name("20261023T093000.123Z"), Some(t));
        for bad in [
            "20261023T093000Z",
            "20261023T093000.123",
            "notes",
            "20261323T093000.123Z",
        ] {
            assert_eq!(parse_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn pruning_keeps_a_day_of_everything_and_a_month_of_dailies() {
        let now = at("2026-10-23T12:00:00Z");
        let backups: Vec<Backup> = [
            "2026-10-23T11:00:00Z", // the last 24 hours: all kept
            "2026-10-23T01:00:00Z",
            "2026-10-22T13:00:00Z",
            "2026-10-22T09:00:00Z", // older, and 22 October has a newer one
            "2026-10-21T20:00:00Z", // the newest of 21 October
            "2026-10-21T08:00:00Z",
            "2026-09-24T08:00:00Z", // the newest of its day, within 30 days
            "2026-09-23T08:00:00Z", // over 30 days
        ]
        .into_iter()
        .map(backup)
        .collect();
        let doomed: Vec<String> = prune_plan(&backups, now, &TimeZone::UTC)
            .into_iter()
            .map(|b| b.at.to_string())
            .collect();
        assert_eq!(
            doomed,
            [
                "2026-10-22T09:00:00Z",
                "2026-10-21T08:00:00Z",
                "2026-09-23T08:00:00Z"
            ]
        );
    }

    #[test]
    fn days_follow_the_time_zone() {
        let now = at("2026-10-25T12:00:00Z");
        let backups = [
            backup("2026-10-23T23:30:00Z"),
            backup("2026-10-23T22:30:00Z"),
        ];
        assert_eq!(prune_plan(&backups, now, &TimeZone::UTC).len(), 1);
        // An hour ahead, the newer one falls on 24 October.
        let ahead = TimeZone::fixed(jiff::tz::offset(1));
        assert!(prune_plan(&backups, now, &ahead).is_empty());
    }
}
