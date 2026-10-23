//! Reading the Changelog section of a task file. Each entry is
//!
//! ```text
//! ### 2026-10-14 · claude-code · finished
//! Serialized refresh behind a shared promise and added a parallel-401 test.
//! Files: src/auth/session.ts, src/auth/session.test.ts
//! Follow-ups: none
//! Unmet: 2 (needs a staging account)
//! ```
//!
//! The summary may span several lines; every other value is one line.

use super::text::{find_section, lines};
use crate::model::Unmet;

/// One changelog entry as read from a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// As written in the heading, normally `YYYY-MM-DD`.
    pub date: String,
    /// The agent id, or a person's name.
    pub who: String,
    /// What happened, such as `created`, `started` or `finished`.
    pub event: String,
    /// Empty when the entry has none.
    pub summary: String,
    pub files: Vec<String>,
    pub follow_ups: Option<String>,
    pub unmet: Vec<Unmet>,
}

/// The entries in the Changelog section, oldest first. Headings that don't
/// have three ` · `-separated parts are skipped, along with what follows
/// them up to the next entry.
pub(super) fn entries(body: &str) -> Vec<Entry> {
    let Some(section) = find_section(body, "Changelog") else {
        return Vec::new();
    };
    let content = &body[section.content..section.end];
    let mut out = Vec::new();
    // The current entry's heading parts and the byte range of its text.
    let mut current: Option<([String; 3], usize, usize)> = None;
    for line in lines(content) {
        match line.heading {
            Some(level) if level <= 3 => {
                out.extend(
                    current
                        .take()
                        .map(|(heading, start, end)| entry(heading, &content[start..end])),
                );
                current = (level == 3)
                    .then(|| parse_heading(line.text))
                    .flatten()
                    .map(|heading| (heading, line.end, line.end));
            }
            _ => {
                if let Some((_, _, end)) = &mut current {
                    *end = line.end;
                }
            }
        }
    }
    out.extend(current.map(|(heading, start, end)| entry(heading, &content[start..end])));
    out
}

fn parse_heading(line: &str) -> Option<[String; 3]> {
    let text = line.trim_start().strip_prefix("###")?.trim();
    let mut parts = text.split(" · ");
    let heading = [
        parts.next()?.trim().to_owned(),
        parts.next()?.trim().to_owned(),
        parts.next()?.trim().to_owned(),
    ];
    parts.next().is_none().then_some(heading)
}

/// Splits an entry's text into the summary and its trailing `Files:`,
/// `Follow-ups:` and `Unmet:` lines.
fn entry([date, who, event]: [String; 3], text: &str) -> Entry {
    let mut entry = Entry {
        date,
        who,
        event,
        summary: String::new(),
        files: Vec::new(),
        follow_ups: None,
        unmet: Vec::new(),
    };
    let mut summary: Vec<&str> = Vec::new();
    for line in text.trim_matches('\n').trim_end().lines() {
        if let Some(files) = line.strip_prefix("Files:") {
            entry.files = files
                .split(", ")
                .map(str::trim)
                .filter(|file| !file.is_empty())
                .map(str::to_owned)
                .collect();
        } else if let Some(follow_ups) = line.strip_prefix("Follow-ups:") {
            entry.follow_ups = Some(follow_ups.trim().to_owned());
        } else if let Some(unmet) = line.strip_prefix("Unmet:") {
            entry.unmet = parse_unmet(unmet);
        } else {
            summary.push(line);
        }
    }
    entry.summary = summary.join("\n").trim().to_owned();
    entry
}

/// Parses `2 (needs a staging account); 4 (out of scope)`. A reason may
/// contain `; ` itself, so an item ends only where the next one starts.
fn parse_unmet(value: &str) -> Vec<Unmet> {
    let mut items: Vec<String> = Vec::new();
    for part in value.trim().split("; ") {
        match items.last_mut() {
            Some(item) if !(item.ends_with(')') && starts_item(part)) => {
                item.push_str("; ");
                item.push_str(part);
            }
            _ => items.push(part.to_owned()),
        }
    }
    items
        .iter()
        .filter_map(|item| {
            let (number, reason) = item.split_once(" (")?;
            Some(Unmet {
                criterion: number.parse().ok()?,
                reason: reason.strip_suffix(')')?.to_owned(),
            })
        })
        .collect()
}

fn starts_item(part: &str) -> bool {
    part.split_once(" (")
        .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_entries_and_skips_other_headings() {
        let body = "\n## Description\n### 2026-01-01 · not · changelog\n\n## Changelog\n\n\
                    ### 2026-10-13 · claude-code · started\nGo.\n### a heading\n\
                    ```\n### 2026-10-13 · in · code\n```\n### 2026-10-14 · sam · reopened\n\n## Notes\nN\n";
        let found: Vec<(String, String, String)> = entries(body)
            .into_iter()
            .map(|e| (e.who, e.event, e.summary))
            .collect();
        assert_eq!(
            found,
            [
                ("claude-code".into(), "started".into(), "Go.".into()),
                ("sam".into(), "reopened".into(), String::new()),
            ]
        );
    }

    #[test]
    fn splits_the_summary_from_its_lines() {
        let body = "## Changelog\n\n### 2026-10-15 · a · finished\nDone.\n\n```\n### in code\n```\n\
                    Files: a.rs, b.rs\nFollow-ups: none\n\
                    Unmet: 2 (needs a staging account; or a mock); 4 (out of scope (for now))\n";
        let [entry] = entries(body).try_into().unwrap();
        assert_eq!(entry.date, "2026-10-15");
        assert_eq!(entry.summary, "Done.\n\n```\n### in code\n```");
        assert_eq!(entry.files, ["a.rs", "b.rs"]);
        assert_eq!(entry.follow_ups.as_deref(), Some("none"));
        assert_eq!(
            entry.unmet,
            [
                Unmet {
                    criterion: 2,
                    reason: "needs a staging account; or a mock".into()
                },
                Unmet {
                    criterion: 4,
                    reason: "out of scope (for now)".into()
                },
            ]
        );
    }
}
