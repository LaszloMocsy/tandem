use jiff::civil::Date;

use super::markdown::{find_section, has_unclosed_fence, heading_level, lines};
use super::sections::criteria;

/// One changelog entry:
///
/// ```text
/// ### 2026-10-14 · claude-code · finished
/// Serialized refresh behind a shared promise and added a parallel-401 test.
/// Files: src/auth/session.ts, src/auth/session.test.ts
/// Follow-ups: none
/// Unmet: 2 (needs a staging account)
/// ```
///
/// The summary may span several lines; every other value is one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogEntry {
    pub date: Date,
    /// The agent id, or the user's name.
    pub who: String,
    /// What happened, such as `created`, `started` or `finished`.
    pub event: String,
    /// Left out when empty.
    pub summary: String,
    /// The `Files:` line, left out when empty.
    pub files: Vec<String>,
    /// The `Follow-ups:` line, left out when `None`.
    pub follow_ups: Option<String>,
    /// The `Unmet:` line, left out when empty.
    pub unmet: Vec<Unmet>,
}

/// An acceptance criterion the agent did not meet, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmet {
    /// The criterion's number, counting from 1.
    pub criterion: u32,
    pub reason: String,
}

/// An acceptance criterion a finished task left unmet: still unticked, and
/// explained on the `Unmet:` line of the latest `finished` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmetCriterion {
    /// The criterion's number, counting from 1.
    pub number: u32,
    pub text: String,
    pub reason: String,
}

/// Why a changelog entry cannot be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EntryError {
    #[error("`{0}` must not be empty")]
    Empty(&'static str),
    #[error("`{0}` must be a single line")]
    MultiLine(&'static str),
    #[error("`{0}` must not contain ` · `")]
    Separator(&'static str),
    #[error("the summary must not contain Markdown headings")]
    Heading,
    #[error("the summary has an unclosed code fence")]
    UnclosedFence,
    #[error("summary lines must not start with `{0}`, which the entry writes itself")]
    ReservedLine(&'static str),
}

/// The lines an entry writes after its summary.
const RESERVED: [&str; 3] = ["Files:", "Follow-ups:", "Unmet:"];

/// The heading of a changelog entry: `### <date> · <who> · <event>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryHeading {
    pub date: String,
    pub who: String,
    pub event: String,
}

impl ChangelogEntry {
    /// Renders the entry as Markdown lines, each ending in `\n`.
    pub fn render(&self) -> Result<String, EntryError> {
        let who = heading_part("who", &self.who)?;
        let event = heading_part("event", &self.event)?;
        let mut out = format!("### {} · {who} · {event}\n", self.date);

        let summary = self.summary.replace("\r\n", "\n");
        let summary = summary.trim();
        if summary.lines().any(is_heading) {
            return Err(EntryError::Heading);
        }
        // An open fence would turn the rest of the file into code, and a
        // fake `Unmet:` line would change what the Review card shows.
        if has_unclosed_fence(summary) {
            return Err(EntryError::UnclosedFence);
        }
        if let Some(prefix) = summary.lines().find_map(|line| {
            RESERVED
                .into_iter()
                .find(|prefix| line.trim_start().starts_with(prefix))
        }) {
            return Err(EntryError::ReservedLine(prefix));
        }
        if !summary.is_empty() {
            out.push_str(summary);
            out.push('\n');
        }

        if !self.files.is_empty() {
            for file in &self.files {
                single_line("files", file)?;
            }
            out.push_str(&format!("Files: {}\n", self.files.join(", ")));
        }
        if let Some(follow_ups) = &self.follow_ups {
            out.push_str(&format!(
                "Follow-ups: {}\n",
                single_line("follow_ups", follow_ups)?
            ));
        }
        if !self.unmet.is_empty() {
            let unmet = self
                .unmet
                .iter()
                .map(|u| {
                    Ok(format!(
                        "{} ({})",
                        u.criterion,
                        single_line("unmet", &u.reason)?
                    ))
                })
                .collect::<Result<Vec<_>, EntryError>>()?;
            out.push_str(&format!("Unmet: {}\n", unmet.join("; ")));
        }
        Ok(out)
    }
}

fn single_line<'a>(field: &'static str, value: &'a str) -> Result<&'a str, EntryError> {
    let value = value.trim();
    if value.is_empty() {
        Err(EntryError::Empty(field))
    } else if value.contains(['\n', '\r']) {
        Err(EntryError::MultiLine(field))
    } else {
        Ok(value)
    }
}

fn heading_part<'a>(field: &'static str, value: &'a str) -> Result<&'a str, EntryError> {
    let value = single_line(field, value)?;
    if value.contains(" · ") {
        Err(EntryError::Separator(field))
    } else {
        Ok(value)
    }
}

/// One entry as read back from the Changelog section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangelogItem {
    pub heading: EntryHeading,
    /// Everything after the heading line, up to the next heading of level 3
    /// or above, without surrounding blank lines: the summary and the
    /// `Files:`, `Follow-ups:` and `Unmet:` lines.
    pub text: String,
}

/// The entries in the Changelog section, oldest first. Headings that don't
/// have three ` · `-separated parts are skipped, along with what follows
/// them up to the next entry.
pub(crate) fn entries(body: &str) -> Vec<ChangelogItem> {
    let Some(section) = find_section(body, "Changelog") else {
        return Vec::new();
    };
    let content = &body[section.content..section.end];
    let mut out = Vec::new();
    // The current entry's heading and the byte range of its text.
    let mut current: Option<(EntryHeading, usize, usize)> = None;
    for line in lines(content) {
        match line.heading {
            Some(level) if level <= 3 => {
                out.extend(current.take().map(|(heading, start, end)| ChangelogItem {
                    heading,
                    text: entry_text(&content[start..end]),
                }));
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
    out.extend(current.map(|(heading, start, end)| ChangelogItem {
        heading,
        text: entry_text(&content[start..end]),
    }));
    out
}

fn entry_text(text: &str) -> String {
    text.trim_matches('\n').trim_end().to_owned()
}

/// The headings of the entries in the Changelog section, oldest first.
pub(crate) fn headings(body: &str) -> Vec<EntryHeading> {
    entries(body)
        .into_iter()
        .map(|entry| entry.heading)
        .collect()
}

fn parse_heading(line: &str) -> Option<EntryHeading> {
    let text = line.trim_start().strip_prefix("###")?.trim();
    let mut parts = text.split(" · ");
    let heading = EntryHeading {
        date: parts.next()?.trim().to_owned(),
        who: parts.next()?.trim().to_owned(),
        event: parts.next()?.trim().to_owned(),
    };
    parts.next().is_none().then_some(heading)
}

/// The criteria the latest `finished` entry lists as unmet that are still
/// unticked, with their text and the agent's reason. A criterion ticked by
/// hand since, or one that no longer exists, is left out.
pub fn unmet_criteria(body: &str) -> Vec<UnmetCriterion> {
    let criteria = criteria(body);
    latest_unmet(body)
        .into_iter()
        .filter_map(|unmet| {
            let criterion = criteria
                .iter()
                .find(|c| c.number == unmet.criterion && !c.met)?;
            Some(UnmetCriterion {
                number: unmet.criterion,
                text: criterion.text.clone(),
                reason: unmet.reason,
            })
        })
        .collect()
}

/// The `Unmet:` line of the latest `finished` entry, parsed.
fn latest_unmet(body: &str) -> Vec<Unmet> {
    entries(body)
        .into_iter()
        .rev()
        .find(|entry| entry.heading.event == "finished")
        .and_then(|entry| {
            entry
                .text
                .lines()
                .rev()
                .find_map(|line| line.strip_prefix("Unmet:"))
                .map(parse_unmet)
        })
        .unwrap_or_default()
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

/// Appends a rendered entry to `body`, which must have LF line endings and
/// end in one newline (or be empty). The entry goes at the end of the
/// `## Changelog` section; without one, a new section is added at the end.
pub(crate) fn append(body: &str, entry: &str) -> String {
    let mut out = String::with_capacity(body.len() + entry.len() + 16);
    match changelog_end(body) {
        None => {
            out.push_str(body);
            out.push_str("\n## Changelog\n\n");
            out.push_str(entry);
        }
        Some(end) => {
            let (before, after) = body.split_at(end);
            out.push_str(before);
            if !before.ends_with("\n\n") {
                out.push('\n');
            }
            out.push_str(entry);
            if !after.is_empty() {
                out.push('\n');
            }
            out.push_str(after);
        }
    }
    out
}

/// The byte offset where the `## Changelog` section ends: the start of the
/// next heading of level 1 or 2, or the end of the body. `None` if there is
/// no such section. Headings inside fenced code blocks don't count.
fn changelog_end(body: &str) -> Option<usize> {
    find_section(body, "Changelog").map(|section| section.end)
}

fn is_heading(line: &str) -> bool {
    heading_level(line).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> ChangelogEntry {
        ChangelogEntry {
            date: Date::constant(2026, 10, 14),
            who: "claude-code".into(),
            event: "finished".into(),
            summary: "Did the thing.".into(),
            files: vec![],
            follow_ups: None,
            unmet: vec![],
        }
    }

    #[test]
    fn renders_all_lines() {
        let e = ChangelogEntry {
            files: vec!["src/a.rs".into(), "src/b.rs".into()],
            follow_ups: Some("none".into()),
            unmet: vec![
                Unmet {
                    criterion: 2,
                    reason: "needs a staging account".into(),
                },
                Unmet {
                    criterion: 4,
                    reason: "out of scope".into(),
                },
            ],
            ..entry()
        };
        assert_eq!(
            e.render().unwrap(),
            "### 2026-10-14 · claude-code · finished\n\
             Did the thing.\n\
             Files: src/a.rs, src/b.rs\n\
             Follow-ups: none\n\
             Unmet: 2 (needs a staging account); 4 (out of scope)\n"
        );
    }

    #[test]
    fn renders_heading_only_without_summary() {
        let e = ChangelogEntry {
            summary: "  \n".into(),
            ..entry()
        };
        assert_eq!(
            e.render().unwrap(),
            "### 2026-10-14 · claude-code · finished\n"
        );
    }

    #[test]
    fn keeps_multi_line_summaries() {
        let e = ChangelogEntry {
            summary: "First.\r\n\r\nSecond.\n".into(),
            ..entry()
        };
        assert_eq!(
            e.render().unwrap(),
            "### 2026-10-14 · claude-code · finished\nFirst.\n\nSecond.\n"
        );
    }

    #[test]
    fn rejects_entries_that_would_break_the_file() {
        let cases = [
            (
                ChangelogEntry {
                    who: " ".into(),
                    ..entry()
                },
                EntryError::Empty("who"),
            ),
            (
                ChangelogEntry {
                    event: "a\nb".into(),
                    ..entry()
                },
                EntryError::MultiLine("event"),
            ),
            (
                ChangelogEntry {
                    who: "a · b".into(),
                    ..entry()
                },
                EntryError::Separator("who"),
            ),
            (
                ChangelogEntry {
                    summary: "ok\n## Oops".into(),
                    ..entry()
                },
                EntryError::Heading,
            ),
            (
                ChangelogEntry {
                    summary: "### Oops".into(),
                    ..entry()
                },
                EntryError::Heading,
            ),
            (
                ChangelogEntry {
                    files: vec!["a\nb".into()],
                    ..entry()
                },
                EntryError::MultiLine("files"),
            ),
            (
                ChangelogEntry {
                    follow_ups: Some(String::new()),
                    ..entry()
                },
                EntryError::Empty("follow_ups"),
            ),
        ];
        for (e, expected) in cases {
            assert_eq!(e.render(), Err(expected));
        }
    }

    #[test]
    fn rejects_summaries_that_fake_structure() {
        for (summary, expected) in [
            ("ok\n```\ncode", EntryError::UnclosedFence),
            ("Done.\nUnmet: 1 (fake)", EntryError::ReservedLine("Unmet:")),
            ("  Files: a.rs", EntryError::ReservedLine("Files:")),
            ("Follow-ups: none", EntryError::ReservedLine("Follow-ups:")),
        ] {
            let e = ChangelogEntry {
                summary: summary.into(),
                ..entry()
            };
            assert_eq!(e.render(), Err(expected), "{summary:?}");
        }
        let e = ChangelogEntry {
            summary: "Closed fence:\n```\nlet x = 1;\n```\nThe files: a.rs".into(),
            ..entry()
        };
        assert!(e.render().is_ok());
    }

    #[test]
    fn reads_entry_headings() {
        let body = "\n## Description\n### 2026-01-01 · not · changelog\n\n## Changelog\n\n\
                    ### 2026-10-13 · claude-code · started\nGo.\n### a heading\n\
                    ```\n### 2026-10-13 · in · code\n```\n### 2026-10-14 · sam · reopened\n";
        let found: Vec<(String, String)> = headings(body)
            .into_iter()
            .map(|h| (h.who, h.event))
            .collect();
        assert_eq!(
            found,
            [
                ("claude-code".to_owned(), "started".to_owned()),
                ("sam".to_owned(), "reopened".to_owned())
            ]
        );
    }

    fn unmet(body: &str) -> Vec<(u32, String, String)> {
        unmet_criteria(body)
            .into_iter()
            .map(|u| (u.number, u.text, u.reason))
            .collect()
    }

    #[test]
    fn reads_unmet_criteria_from_the_latest_finished_entry() {
        let criteria = "\n## Acceptance criteria\n- [x] One\n- [ ] Two\n- [ ] Three\n- [ ] Four\n";
        let body = format!(
            "{criteria}\n## Changelog\n\n\
             ### 2026-10-13 · a · finished\nFirst try.\nUnmet: 3 (old reason)\n\
             ### 2026-10-14 · sam · reopened\nAgain.\n\
             ### 2026-10-15 · a · finished\nDone.\nFiles: a.rs\n\
             Unmet: 2 (needs a staging account; or a mock); 4 (out of scope (for now))\n"
        );
        assert_eq!(
            unmet(&body),
            [
                (
                    2,
                    "Two".to_owned(),
                    "needs a staging account; or a mock".to_owned()
                ),
                (4, "Four".to_owned(), "out of scope (for now)".to_owned()),
            ]
        );

        // Ticked by hand since, or no longer there.
        let ticked = body
            .replace("- [ ] Two", "- [x] Two")
            .replace("- [ ] Four\n", "");
        assert_eq!(unmet(&ticked), []);

        // A later finish without an `Unmet:` line clears the list.
        let all_met = format!("{body}\n### 2026-10-16 · a · finished\nAll done.\n");
        assert_eq!(unmet(&all_met), []);

        // Only the Changelog section counts.
        assert_eq!(unmet(&format!("{criteria}Unmet: 2 (x)\n")), []);
    }

    #[test]
    fn reads_entries_with_their_text() {
        let body = "\n## Changelog\n\n### 2026-10-13 · a · created\n\n\
                    ### 2026-10-14 · a · finished\nFirst line.\n\n```\n### in code\n```\n\
                    Files: a.rs\n\n### not an entry\nSkipped.\n#### a sub-heading\nStill skipped.\n\
                    ### 2026-10-15 · sam · reviewed\nOk.\n\n## Notes\nNot changelog.\n";
        let found: Vec<(String, String)> = entries(body)
            .into_iter()
            .map(|e| (e.heading.event, e.text))
            .collect();
        assert_eq!(
            found,
            [
                ("created".to_owned(), String::new()),
                (
                    "finished".to_owned(),
                    "First line.\n\n```\n### in code\n```\nFiles: a.rs".to_owned()
                ),
                ("reviewed".to_owned(), "Ok.".to_owned()),
            ]
        );
    }

    #[test]
    fn hashtags_are_not_headings() {
        let e = ChangelogEntry {
            summary: "#123 fixed\n#hashtag".into(),
            ..entry()
        };
        assert!(e.render().is_ok());
    }

    #[test]
    fn appends_after_the_last_entry() {
        let body = "\n## Description\nX\n\n## Changelog\n\n### 2026-10-13 · a · started\nGo.\n";
        assert_eq!(append(body, "### new\n"), format!("{body}\n### new\n"));
    }

    #[test]
    fn appends_into_an_empty_changelog_section() {
        assert_eq!(
            append("## Changelog\n", "### new\n"),
            "## Changelog\n\n### new\n"
        );
    }

    #[test]
    fn adds_a_missing_changelog_section_at_the_end() {
        assert_eq!(
            append("\n## Description\nX\n", "### new\n"),
            "\n## Description\nX\n\n## Changelog\n\n### new\n"
        );
        assert_eq!(append("", "### new\n"), "\n## Changelog\n\n### new\n");
    }

    #[test]
    fn appends_before_a_section_after_the_changelog() {
        let body = "## Changelog\n\n### old\n\n## Notes\nN\n";
        assert_eq!(
            append(body, "### new\n"),
            "## Changelog\n\n### old\n\n### new\n\n## Notes\nN\n"
        );
    }

    #[test]
    fn ignores_headings_in_code_fences() {
        let body = "## Changelog\n\n### old\n```\n## not a heading\n```\n";
        assert_eq!(append(body, "### new\n"), format!("{body}\n### new\n"));
        let body = "## Notes\n~~~md\n## Changelog\n~~~\n";
        assert_eq!(changelog_end(body), None);
    }
}
