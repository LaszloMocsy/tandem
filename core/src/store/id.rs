use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, de};

/// The 32 characters ids are drawn from: lowercase letters and digits,
/// without `0`, `o`, `1` and `l`.
pub const ID_ALPHABET: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";

/// The number of characters in a task id.
pub const ID_LEN: usize = 7;

/// A task id such as `k3f9x2m`. It never changes and is also the file name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId([u8; ID_LEN]);

impl TaskId {
    /// Generates a random id. It does not check that the id is unused; see
    /// [`unused_id`](super::unused_id) for that.
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

    /// The task's file name, `<id>.md`.
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

#[cfg(test)]
mod tests {
    use super::*;

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
