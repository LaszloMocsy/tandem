use std::fs;
use std::io;
use std::path::Path;

use serde::Deserialize;

use super::fs::{Mode, StoreError, TASK_DIR, io_error, write_atomic};

/// The version of the `.tandem/` file format this app reads and writes.
pub const FORMAT_VERSION: i64 = 1;

/// The file in `.tandem/` that records the format version and the project
/// name.
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
    /// `.tandem` is a symbolic link. Following it could write task files
    /// outside the repo, such as wherever a cloned repo's link points, so
    /// the app only reads through it.
    #[error("{TASK_DIR} is a symbolic link; make it a real folder in the repo to edit tasks")]
    SymlinkedFolder,
}

/// The longest project name [`set_project_name`] writes, in characters.
pub const MAX_NAME_LEN: usize = 100;

/// Why a project name cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("the project name is longer than {MAX_NAME_LEN} characters")]
    TooLong,
    #[error("the project name cannot contain line breaks or control characters")]
    ControlCharacter,
}

#[derive(Deserialize)]
struct Config {
    format_version: i64,
    /// The project's display name. Without it, the app uses the repo
    /// folder's name.
    name: Option<String>,
}

/// Gets `repo`'s `.tandem/` folder ready to open and checks its format
/// version:
///
/// - If the folder is missing, creates it with `config.toml` and `README.md`.
/// - If `config.toml` is missing, treats the folder as version 1 and writes it.
/// - If the version is newer than [`FORMAT_VERSION`], or the file cannot be
///   read, the project opens read-only and the file is left as it is.
/// - If `.tandem` is a symbolic link, the project opens read-only and
///   nothing is written through it.
pub fn prepare_folder(repo: &Path) -> Result<Access, StoreError> {
    let dir = repo.join(TASK_DIR);
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Ok(Access::ReadOnly(ReadOnly::SymlinkedFolder));
        }
        _ => {}
    }
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
    match parse_config(bytes) {
        Ok(_) => Access::ReadWrite,
        Err(reason) => Access::ReadOnly(reason),
    }
}

/// Parses `config.toml`, or says why the project must open read-only.
fn parse_config(bytes: &[u8]) -> Result<Config, ReadOnly> {
    let invalid = |message: String| ReadOnly::InvalidConfig(message);
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(invalid("the file is not valid UTF-8".into()));
    };
    let config: Config = match toml::from_str(text) {
        Ok(config) => config,
        // TOML errors span several lines with a snippet; keep the first.
        Err(e) => return Err(invalid(e.message().to_owned())),
    };
    match config.format_version {
        FORMAT_VERSION => Ok(config),
        version if version > FORMAT_VERSION => Err(ReadOnly::NewerFormat { version }),
        version => Err(invalid(format!("format_version {version} does not exist"))),
    }
}

/// The project name `repo`'s `config.toml` sets, trimmed. `None` if the
/// file is missing or invalid, or the name is missing or blank; the app
/// then uses the repo folder's name.
pub fn project_name(repo: &Path) -> Result<Option<String>, StoreError> {
    let path = repo.join(TASK_DIR).join(CONFIG_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(&path)(e)),
    };
    Ok(parse_config(&bytes)
        .ok()
        .and_then(|config| config.name)
        .and_then(|name| normalize_project_name(&name).ok().flatten()))
}

/// A project name as [`set_project_name`] writes it: trimmed, and `None`
/// if blank.
pub fn normalize_project_name(name: &str) -> Result<Option<String>, NameError> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().any(char::is_control) {
        return Err(NameError::ControlCharacter);
    }
    if name.chars().count() > MAX_NAME_LEN {
        return Err(NameError::TooLong);
    }
    Ok(Some(name.to_owned()))
}

/// Sets the project name in `repo`'s `config.toml`, or removes it when
/// `name` is blank or `None`. Like task-file writes, this keeps the diff
/// minimal: every other key, comment and the key order stay as they are,
/// only the `name` line changes, and the file is written atomically with
/// LF line endings and one trailing newline. A config the app can't read,
/// or one from a newer format, is never overwritten.
///
/// Returns whether the file changed.
pub fn set_project_name(repo: &Path, name: Option<&str>) -> Result<bool, StoreError> {
    let name = match name {
        Some(name) => normalize_project_name(name)?,
        None => None,
    };
    let dir = repo.join(TASK_DIR);
    let path = dir.join(CONFIG_FILE);
    let read_only = |reason| StoreError::ConfigReadOnly {
        path: path.clone(),
        reason,
    };
    if fs::symlink_metadata(&dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(read_only(ReadOnly::SymlinkedFolder));
    }
    let bytes = match fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() => return Err(StoreError::NotRegularFile { path }),
        Ok(_) => fs::read(&path).map_err(io_error(&path))?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => config_text().into_bytes(),
        Err(e) => return Err(io_error(&path)(e)),
    };
    let config = parse_config(&bytes).map_err(read_only)?;
    if config.name == name {
        return Ok(false);
    }
    // `parse_config` checked that the file is UTF-8 and valid TOML.
    let text = String::from_utf8(bytes).map_err(|e| read_only(invalid_config(e)))?;
    let text = with_name(&text, name.as_deref()).map_err(|e| read_only(invalid_config(e)))?;
    write_atomic(&path, &text, Mode::Replace)?;
    Ok(true)
}

fn invalid_config(e: impl std::fmt::Display) -> ReadOnly {
    ReadOnly::InvalidConfig(e.to_string())
}

/// `text` with its top-level `name` set to `name`, or removed when `None`.
/// Everything else is kept as written, apart from line endings, which
/// become LF, and the end of the file, which gets one trailing newline.
fn with_name(text: &str, name: Option<&str>) -> Result<String, toml_edit::TomlError> {
    let mut doc: toml_edit::DocumentMut = text.parse()?;
    match name {
        Some(name) => match doc.get_mut("name").and_then(toml_edit::Item::as_value_mut) {
            // Keep the spacing and any comment after the old value.
            Some(value) => {
                let decor = value.decor().clone();
                *value = name.into();
                *value.decor_mut() = decor;
            }
            None => {
                doc.insert("name", toml_edit::value(name));
            }
        },
        None => {
            doc.remove("name");
        }
    }
    let text = doc.to_string().replace("\r\n", "\n");
    Ok(format!("{}\n", text.trim_end_matches('\n')))
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
    fn a_name_of_the_wrong_type_makes_the_config_invalid() {
        assert_eq!(
            check_config(b"format_version = 1\nname = \"Tandem\"\n"),
            Access::ReadWrite
        );
        assert_eq!(
            check_config(b"format_version = 1\nname = 5\n"),
            Access::ReadOnly(ReadOnly::InvalidConfig(
                "invalid type: integer `5`, expected a string".into()
            ))
        );
    }

    #[test]
    fn normalizes_names() {
        assert_eq!(
            normalize_project_name("  Tandem "),
            Ok(Some("Tandem".into()))
        );
        assert_eq!(normalize_project_name(" \t "), Ok(None));
        assert_eq!(
            normalize_project_name("a\nb"),
            Err(NameError::ControlCharacter)
        );
        let long = "é".repeat(MAX_NAME_LEN);
        assert_eq!(normalize_project_name(&long), Ok(Some(long.clone())));
        assert_eq!(
            normalize_project_name(&format!("{long}a")),
            Err(NameError::TooLong)
        );
    }

    fn set(text: &str, name: Option<&str>) -> String {
        with_name(text, name).unwrap()
    }

    #[test]
    fn setting_a_name_adds_one_line_after_the_top_level_keys() {
        assert_eq!(
            set("format_version = 1\n", Some("Tandem")),
            "format_version = 1\nname = \"Tandem\"\n"
        );
        assert_eq!(
            set(
                "# Tandem settings\nformat_version = 1 # never change\n\n[later]\nkey = true\n",
                Some("Tandem")
            ),
            "# Tandem settings\nformat_version = 1 # never change\nname = \"Tandem\"\n\n[later]\nkey = true\n"
        );
    }

    #[test]
    fn changing_a_name_keeps_its_line_in_place() {
        assert_eq!(
            set(
                "name = \"Old\"   # shown in the switcher\n# the format\nformat_version = 1\n",
                Some("New \"one\"")
            ),
            "name = 'New \"one\"'   # shown in the switcher\n# the format\nformat_version = 1\n"
        );
    }

    #[test]
    fn removing_a_name_removes_only_its_line_and_comments_above_it() {
        assert_eq!(
            set("format_version = 1\nname = \"Old\"\nother = 2\n", None),
            "format_version = 1\nother = 2\n"
        );
        assert_eq!(
            set("# Settings\nformat_version = 1\n\nname = 'Old'\n", None),
            "# Settings\nformat_version = 1\n"
        );
        assert_eq!(
            set(
                "format_version = 1\n# Shown in the switcher\nname = \"Old\"\nother = 2\n",
                None
            ),
            "format_version = 1\nother = 2\n"
        );
    }

    #[test]
    fn writes_lf_and_one_trailing_newline() {
        assert_eq!(
            set("format_version = 1\r\nother = 2\r\n\r\n\r\n", Some("T")),
            "format_version = 1\nother = 2\nname = \"T\"\n"
        );
        assert_eq!(
            set("format_version = 1", Some("T")),
            "format_version = 1\nname = \"T\"\n"
        );
    }

    #[test]
    fn readme_and_config_end_in_one_newline() {
        for text in [README, &config_text()] {
            assert!(text.ends_with('\n') && !text.ends_with("\n\n"));
            assert!(!text.contains('\r'));
        }
    }
}
