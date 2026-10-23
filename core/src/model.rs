//! The task model shared by the database, the service and the Markdown
//! format: ids, statuses, priorities and acceptance criteria.

use std::fmt;
use std::str::FromStr;

pub use jiff::civil::Date;
use serde::{Deserialize, Deserializer, de};

/// The 32 characters ids are drawn from: lowercase letters and digits,
/// without `0`, `o`, `1` and `l`.
pub const ID_ALPHABET: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";

/// The number of characters in a task id.
pub const ID_LEN: usize = 7;

/// A task id such as `k3f9x2m`. It never changes, and agents and the board
/// refer to tasks by it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId([u8; ID_LEN]);

impl TaskId {
    /// Generates a random id. It does not check that the id is unused.
    ///
    /// # Panics
    ///
    /// Panics if the operating system's random number generator fails.
    pub fn generate() -> Self {
        let mut bytes = [0u8; ID_LEN];
        getrandom::fill(&mut bytes).expect("the OS random number generator failed");
        // 256 is a multiple of 32, so masking keeps every character equally likely.
        Self(bytes.map(|b| ID_ALPHABET[usize::from(b & 31)]))
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("ids are ASCII")
    }

    /// The file name a Markdown export gives the task, `<id>.md`.
    pub fn file_name(&self) -> String {
        format!("{self}.md")
    }

    /// The id for a file name that is exactly `<id>.md`, otherwise `None`.
    pub fn from_file_name(name: &str) -> Option<Self> {
        name.strip_suffix(".md")?.parse().ok()
    }
}

impl FromStr for TaskId {
    type Err = InvalidId;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes: [u8; ID_LEN] = s
            .as_bytes()
            .try_into()
            .map_err(|_| InvalidId(s.to_owned()))?;
        if bytes.iter().all(|b| ID_ALPHABET.contains(b)) {
            Ok(Self(bytes))
        } else {
            Err(InvalidId(s.to_owned()))
        }
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TaskId({:?})", self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

/// A string that is not a valid task id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a task id: expected 7 characters from a-z and 2-9, without `l` and `o`")]
pub struct InvalidId(pub String);

/// A task's status, one per board column.
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

    /// Sort order: high first.
    pub fn rank(self) -> u8 {
        match self {
            Self::High => 0,
            Self::Medium => 1,
            Self::Low => 2,
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

/// One acceptance criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    /// Counting from 1, in order.
    pub number: u32,
    pub met: bool,
    pub text: String,
}

/// An acceptance criterion the agent did not meet, and why, as
/// `finish_task` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmet {
    /// The criterion's number, counting from 1.
    pub criterion: u32,
    pub reason: String,
}

/// An acceptance criterion a finished task left unmet: still unticked, and
/// explained in the latest `finished` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmetCriterion {
    /// The criterion's number, counting from 1.
    pub number: u32,
    pub text: String,
    pub reason: String,
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

    #[test]
    fn alphabet_has_32_distinct_characters_without_lookalikes() {
        let mut sorted = ID_ALPHABET.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 32);
        for c in b"0o1l" {
            assert!(
                !ID_ALPHABET.contains(c),
                "{} is in the alphabet",
                *c as char
            );
        }
    }

    #[test]
    fn generated_ids_parse() {
        for _ in 0..1000 {
            let id = TaskId::generate();
            assert_eq!(id.as_str().parse::<TaskId>(), Ok(id));
        }
    }

    #[test]
    fn parses_valid_ids() {
        assert_eq!("k3f9x2m".parse::<TaskId>().unwrap().as_str(), "k3f9x2m");
    }

    #[test]
    fn rejects_invalid_ids() {
        for s in [
            "", "k3f9x2", "k3f9x2mm", "K3F9X2M", "k3f9x2o", "k3f9x21", "k3f9-2m", "k3f9x2é",
        ] {
            assert!(s.parse::<TaskId>().is_err(), "{s:?} parsed");
        }
    }

    #[test]
    fn file_names_must_be_exactly_id_dot_md() {
        assert_eq!(
            TaskId::from_file_name("k3f9x2m.md").map(|id| id.to_string()),
            Some("k3f9x2m".to_owned())
        );
        for name in [
            "README.md",
            "config.toml",
            "k3f9x2m.MD",
            "k3f9x2m.md.tmp",
            "k3f9x2m",
            ".k3f9x2m.md",
            "k3f9x2m .md",
        ] {
            assert_eq!(TaskId::from_file_name(name), None, "{name:?} accepted");
        }
    }
}
