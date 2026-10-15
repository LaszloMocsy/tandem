use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;

use super::fs::{Mode, StoreError, TASK_DIR, io_error, write_atomic};

/// The version of the `.tandem/` file format this app reads and writes.
pub const FORMAT_VERSION: i64 = 1;

/// The file in `.tandem/` that records the format version.
pub const CONFIG_FILE: &str = "config.toml";

/// The format reference for humans and agents, written when `.tandem/` is
/// created.
pub const README_FILE: &str = "README.md";

const README: &str = include_str!("folder_readme.md");

/// Whether the app may write to a project's `.tandem/` folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Access {
    ReadWrite,
    ReadOnly(ReadOnly),
}

/// Why a project is open read-only.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadOnly {
    /// Written by a newer app. Updating the app fixes it.
    #[error(
        "the tasks use format version {version}, but this app supports only up to \
         {FORMAT_VERSION}; update Tandem to edit them"
    )]
    NewerFormat { version: i64 },
    /// `config.toml` cannot be read, so the format is unknown. It is never
    /// overwritten.
    #[error("{CONFIG_FILE} is invalid: {0}")]
    InvalidConfig(String),
}

#[derive(Deserialize)]
struct Config {
    format_version: i64,
}

/// Gets `repo`'s `.tandem/` folder ready to open and checks its format
/// version:
///
/// - If the folder is missing, creates it with `config.toml` and `README.md`.
/// - If `config.toml` is missing, treats the folder as version 1 and writes it.
/// - If the version is newer than [`FORMAT_VERSION`], or the file cannot be
///   read, the project opens read-only and the file is left as it is.
pub fn prepare_folder(repo: &Path) -> Result<Access, StoreError> {
    let dir = repo.join(TASK_DIR);
    match fs::metadata(&dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(StoreError::Io {
                source: io::Error::new(io::ErrorKind::NotADirectory, "not a directory"),
                path: dir,
            });
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            create_folder(&dir)?;
        }
        Err(e) => return Err(io_error(&dir)(e)),
    }

    let config = dir.join(CONFIG_FILE);
    let text = match fs::read(&config) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            create_if_missing(&config, &config_text())?;
            return Ok(Access::ReadWrite);
        }
        Err(e) => return Err(io_error(&config)(e)),
    };
    Ok(check_config(&text))
}

fn check_config(bytes: &[u8]) -> Access {
    let invalid = |message: String| Access::ReadOnly(ReadOnly::InvalidConfig(message));
    let Ok(text) = std::str::from_utf8(bytes) else {
        return invalid("the file is not valid UTF-8".into());
    };
    let config: Config = match toml::from_str(text) {
        Ok(config) => config,
        // TOML errors span several lines with a snippet; keep the first.
        Err(e) => return invalid(e.message().to_owned()),
    };
    match config.format_version {
        FORMAT_VERSION => Access::ReadWrite,
        version if version > FORMAT_VERSION => Access::ReadOnly(ReadOnly::NewerFormat { version }),
        version => invalid(format!("format_version {version} does not exist")),
    }
}

fn config_text() -> String {
    format!("format_version = {FORMAT_VERSION}\n")
}

/// Creates `.tandem/` with its config and README. Another process creating
/// it at the same time is fine.
fn create_folder(dir: &Path) -> Result<(), StoreError> {
    match fs::create_dir(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io_error(dir)(e)),
    }
    create_if_missing(&dir.join(CONFIG_FILE), &config_text())?;
    create_if_missing(&dir.join(README_FILE), README)
}

fn create_if_missing(path: &Path, contents: &str) -> Result<(), StoreError> {
    match write_atomic(path, contents, Mode::CreateNew) {
        Err(StoreError::AlreadyExists { .. }) => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_the_format_version() {
        assert_eq!(check_config(b"format_version = 1\n"), Access::ReadWrite);
        // Keys a later version might add don't matter for version 1.
        assert_eq!(
            check_config(b"format_version = 1\nother = true\n"),
            Access::ReadWrite
        );
        assert_eq!(
            check_config(b"format_version = 2\n"),
            Access::ReadOnly(ReadOnly::NewerFormat { version: 2 })
        );
        for (text, message) in [
            (
                &b"format_version = 0\n"[..],
                "format_version 0 does not exist",
            ),
            (b"", "missing field `format_version`"),
            (
                b"format_version = \"1\"\n",
                "invalid type: string \"1\", expected i64",
            ),
            (b"\xff", "the file is not valid UTF-8"),
        ] {
            assert_eq!(
                check_config(text),
                Access::ReadOnly(ReadOnly::InvalidConfig(message.into())),
                "{text:?}"
            );
        }
    }

    #[test]
    fn readme_and_config_end_in_one_newline() {
        for text in [README, &config_text()] {
            assert!(text.ends_with('\n') && !text.ends_with("\n\n"));
            assert!(!text.contains('\r'));
        }
    }
}
