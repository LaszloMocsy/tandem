use std::str::FromStr;

use jiff::civil::Date;
use serde::Deserialize;

use super::changelog::{self, ChangelogEntry, ChangelogItem, EntryError, EntryHeading};
use super::id::TaskId;
use super::sections::{self, Criterion, DESCRIPTION, RELEVANT_FILES, SectionError};
use super::yaml;

/// A task's status. These are the only statuses written to task files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Backlog,
    Todo,
    InProgress,
    Finished,
    Done,
}

impl Status {
    pub const ALL: [Self; 5] = [
        Self::Backlog,
        Self::Todo,
        Self::InProgress,
        Self::Finished,
        Self::Done,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Backlog => "backlog",
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Finished => "finished",
            Self::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl Priority {
    pub const ALL: [Self; 3] = [Self::High, Self::Medium, Self::Low];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

impl FromStr for Status {
    type Err = UnknownValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == s)
            .ok_or_else(|| UnknownValue(s.to_owned()))
    }
}

impl FromStr for Priority {
    type Err = UnknownValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|priority| priority.as_str() == s)
            .ok_or_else(|| UnknownValue(s.to_owned()))
    }
}

/// A string that is not a known status or priority.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown value `{0}`")]
pub struct UnknownValue(pub String);

/// One task file: the frontmatter fields in the order they are written,
/// then the Markdown body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub status: Status,
    pub priority: Option<Priority>,
    pub parent: Option<TaskId>,
    pub labels: Vec<String>,
    pub depends_on: Vec<TaskId>,
    pub created: Date,
    pub reviewed_at: Option<Date>,
    /// Everything after the closing `---` line, kept byte for byte apart
    /// from LF line endings and exactly one trailing newline.
    pub body: String,
}

/// Why a task file could not be parsed. A file with any of these errors is
/// shown as invalid and never overwritten.
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

impl Task {
    /// Parses a task file. CRLF line endings are read as LF and the body gets
    /// exactly one trailing newline; nothing else in the body changes.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let text = text.replace("\r\n", "\n");
        let rest = text
            .strip_prefix("---\n")
            .ok_or(ParseError::MissingFrontmatter)?;
        let (yaml, body) = split_frontmatter(rest).ok_or(ParseError::UnclosedFrontmatter)?;
        // Keep the opening `---` (a YAML document marker) so error line
        // numbers match the file, and keep messages to one line for the board.
        let yaml = &text[..4 + yaml.len()];
        let options = serde_saphyr::options! { with_snippet: false };
        let fm: Frontmatter = serde_saphyr::from_str_with_options(yaml, options)
            .map_err(|e| ParseError::Frontmatter(Box::new(e)))?;
        if fm.title.trim().is_empty() {
            return Err(ParseError::EmptyTitle);
        }
        Ok(Self {
            id: fm.id,
            title: fm.title,
            status: fm.status,
            priority: fm.priority,
            parent: fm.parent,
            labels: fm.labels.unwrap_or_default(),
            depends_on: fm.depends_on.unwrap_or_default(),
            created: fm.created,
            reviewed_at: fm.reviewed_at,
            body: normalize_body(body),
        })
    }

    /// Serializes the task: frontmatter in the fixed key order with empty
    /// optional fields omitted, then the body with LF line endings and one
    /// trailing newline.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("---\n");
        push_field(&mut out, "id", &yaml::scalar(self.id.as_str()));
        push_field(&mut out, "title", &yaml::scalar(&self.title));
        push_field(&mut out, "status", self.status.as_str());
        if let Some(priority) = self.priority {
            push_field(&mut out, "priority", priority.as_str());
        }
        if let Some(parent) = self.parent {
            push_field(&mut out, "parent", &yaml::scalar(parent.as_str()));
        }
        if !self.labels.is_empty() {
            let labels = yaml::flow_seq(self.labels.iter().map(String::as_str));
            push_field(&mut out, "labels", &labels);
        }
        if !self.depends_on.is_empty() {
            let ids = yaml::flow_seq(self.depends_on.iter().map(TaskId::as_str));
            push_field(&mut out, "depends_on", &ids);
        }
        push_field(&mut out, "created", &self.created.to_string());
        if let Some(reviewed_at) = self.reviewed_at {
            push_field(&mut out, "reviewed_at", &reviewed_at.to_string());
        }
        out.push_str("---\n");
        out.push_str(&normalize_body(&self.body));
        out
    }

    /// Appends an entry at the end of the Changelog section, adding the
    /// section at the end of the body if it is missing. Existing lines never
    /// change, so in a file whose Changelog is the last section the diff only
    /// adds lines at the end.
    pub fn append_changelog(&mut self, entry: &ChangelogEntry) -> Result<(), EntryError> {
        let entry = entry.render()?;
        self.body = changelog::append(&normalize_body(&self.body), &entry);
        Ok(())
    }

    /// Replaces the content of the Description, Acceptance criteria or
    /// Relevant files section, adding it in the usual order if missing, or
    /// removes the section if `content` is blank. The rest of the body is
    /// kept byte for byte.
    pub fn set_section(&mut self, name: &str, content: &str) -> Result<(), SectionError> {
        self.body = sections::set(&normalize_body(&self.body), name, content)?;
        Ok(())
    }

    /// The headings of the changelog entries, oldest first.
    pub fn changelog(&self) -> Vec<EntryHeading> {
        changelog::headings(&self.body)
    }

    /// The changelog entries with their text, oldest first.
    pub fn changelog_entries(&self) -> Vec<ChangelogItem> {
        changelog::entries(&self.body)
    }

    /// The Description section's text, empty if there is none.
    pub fn description(&self) -> &str {
        sections::text(&self.body, DESCRIPTION).unwrap_or_default()
    }

    /// The items listed in the Relevant files section.
    pub fn relevant_files(&self) -> Vec<String> {
        sections::list_items(&self.body, RELEVANT_FILES)
    }

    /// The checkboxes in the Acceptance criteria section.
    pub fn criteria(&self) -> Vec<Criterion> {
        sections::criteria(&self.body)
    }

    /// Ticks acceptance criteria by number, changing only their checkboxes.
    pub fn tick_criteria(&mut self, numbers: &[u32]) -> Result<(), SectionError> {
        self.body = sections::tick(&self.body, numbers)?;
        Ok(())
    }
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

/// LF line endings and exactly one trailing newline, or empty.
fn normalize_body(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    let trimmed = body.trim_end_matches('\n');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

/// Writes one frontmatter line. Statuses, priorities and dates never need
/// quoting; other values are already rendered by [`yaml`]. Ids go through it
/// too, because an all-digit id such as `2345678` would read as a number.
fn push_field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_and_priorities_round_trip_through_strings() {
        for status in Status::ALL {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        for priority in Priority::ALL {
            assert_eq!(priority.as_str().parse(), Ok(priority));
        }
        assert_eq!(
            "InProgress".parse::<Status>(),
            Err(UnknownValue("InProgress".into()))
        );
    }
}
