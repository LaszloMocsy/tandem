//! Parsing a task file: YAML frontmatter, then the Markdown body.

use std::fs;
use std::io;
use std::path::Path;

use jiff::civil::Date;
use serde::Deserialize;

use super::changelog;
use super::sections::{self, DESCRIPTION, RELEVANT_FILES};
use super::{FolderTasks, TaskDoc};
use crate::model::{Priority, Status, TaskId};

/// Why a task file could not be parsed.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("the file is not valid UTF-8")]
    NotUtf8,
    #[error("the file does not start with a `---` frontmatter line")]
    MissingFrontmatter,
    #[error("the frontmatter has no closing `---` line")]
    UnclosedFrontmatter,
    #[error("invalid frontmatter: {0}")]
    Frontmatter(Box<serde_saphyr::Error>),
    #[error("the title is empty")]
    EmptyTitle,
    #[error("the id `{found}` does not match the file name `{expected}.md`")]
    IdMismatch { expected: TaskId, found: TaskId },
    #[error("the file is a symbolic link or not a regular file")]
    NotRegularFile,
    #[error("cannot read the file: {0}")]
    Io(#[from] io::Error),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    id: TaskId,
    title: String,
    status: Status,
    priority: Option<Priority>,
    parent: Option<TaskId>,
    labels: Option<Vec<String>>,
    depends_on: Option<Vec<TaskId>>,
    created: Date,
    reviewed_at: Option<Date>,
}

impl TaskDoc {
    /// Parses a task file. CRLF line endings are read as LF. Sections the
    /// format doesn't know are kept at the end of the description.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let text = text.replace("\r\n", "\n");
        let rest = text
            .strip_prefix("---\n")
            .ok_or(ParseError::MissingFrontmatter)?;
        let (yaml, body) = split_frontmatter(rest).ok_or(ParseError::UnclosedFrontmatter)?;
        // Keep the opening `---` (a YAML document marker) so error line
        // numbers match the file, and keep messages to one line.
        let yaml = &text[..4 + yaml.len()];
        let options = serde_saphyr::options! { with_snippet: false };
        let fm: Frontmatter = serde_saphyr::from_str_with_options(yaml, options)
            .map_err(|e| ParseError::Frontmatter(Box::new(e)))?;
        if fm.title.trim().is_empty() {
            return Err(ParseError::EmptyTitle);
        }
        let description = sections::text(body, DESCRIPTION).unwrap_or_default();
        let others = sections::other_sections(body);
        let description = match (description.is_empty(), others.is_empty()) {
            (_, true) => description.to_owned(),
            (true, false) => others,
            (false, false) => format!("{description}\n\n{others}"),
        };
        Ok(Self {
            id: fm.id,
            title: fm.title.trim().to_owned(),
            status: fm.status,
            priority: fm.priority,
            parent: fm.parent,
            labels: fm.labels.unwrap_or_default(),
            depends_on: fm.depends_on.unwrap_or_default(),
            created: fm.created,
            reviewed_at: fm.reviewed_at,
            description,
            criteria: sections::criteria(body),
            relevant_files: sections::list_items(body, RELEVANT_FILES),
            changelog: changelog::entries(body),
        })
    }

    /// Parses the file for task `id`, checking that the `id` in its
    /// frontmatter matches.
    pub fn parse_file(id: TaskId, bytes: &[u8]) -> Result<Self, ParseError> {
        let text = std::str::from_utf8(bytes).map_err(|_| ParseError::NotUtf8)?;
        let task = Self::parse(text)?;
        if task.id != id {
            return Err(ParseError::IdMismatch {
                expected: id,
                found: task.id,
            });
        }
        Ok(task)
    }
}

/// Reads every task file in `dir`: regular files named exactly `<id>.md`,
/// sorted by id. Symbolic links are never followed. Files that fail to
/// parse are listed in `skipped`.
pub fn read_folder(dir: &Path) -> io::Result<FolderTasks> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = TaskId::from_file_name(&name) {
            files.push((id, name, entry.path()));
        }
    }
    files.sort_unstable_by_key(|(id, ..)| *id);
    let mut out = FolderTasks::default();
    for (id, name, path) in files {
        let read = match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => fs::read(&path)
                .map_err(ParseError::from)
                .and_then(|bytes| TaskDoc::parse_file(id, &bytes)),
            Ok(_) => Err(ParseError::NotRegularFile),
            Err(e) => Err(e.into()),
        };
        match read {
            Ok(task) => out.tasks.push(task),
            Err(e) => out.skipped.push((name, e.to_string())),
        }
    }
    Ok(out)
}

/// Splits what follows the opening `---` line at the closing `---` line,
/// into the YAML and the body.
fn split_frontmatter(rest: &str) -> Option<(&str, &str)> {
    let mut start = 0;
    loop {
        let end = rest[start..].find('\n').map_or(rest.len(), |i| start + i);
        if &rest[start..end] == "---" {
            return Some((&rest[..start], rest.get(end + 1..).unwrap_or("")));
        }
        if end == rest.len() {
            return None;
        }
        start = end + 1;
    }
}
