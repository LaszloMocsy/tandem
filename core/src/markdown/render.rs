//! Writing a task in the Markdown format: frontmatter in a fixed key order
//! with empty optional fields left out, then the sections.

use super::{Entry, TaskDoc, yaml};
use crate::model::TaskId;

impl TaskDoc {
    /// The task as a Markdown file, with LF line endings and one trailing
    /// newline.
    pub fn to_markdown(&self) -> String {
        let mut out = String::from("---\n");
        field(&mut out, "id", &yaml::scalar(self.id.as_str()));
        field(&mut out, "title", &yaml::scalar(&self.title));
        field(&mut out, "status", self.status.as_str());
        if let Some(priority) = self.priority {
            field(&mut out, "priority", priority.as_str());
        }
        if let Some(parent) = self.parent {
            field(&mut out, "parent", &yaml::scalar(parent.as_str()));
        }
        if !self.labels.is_empty() {
            let labels = yaml::flow_seq(self.labels.iter().map(String::as_str));
            field(&mut out, "labels", &labels);
        }
        if !self.depends_on.is_empty() {
            let ids = yaml::flow_seq(self.depends_on.iter().map(TaskId::as_str));
            field(&mut out, "depends_on", &ids);
        }
        field(&mut out, "created", &self.created.to_string());
        if let Some(reviewed_at) = self.reviewed_at {
            field(&mut out, "reviewed_at", &reviewed_at.to_string());
        }
        out.push_str("---\n");

        let description = self.description.trim_matches('\n').trim_end();
        if !description.is_empty() {
            section(&mut out, "Description", description);
        }
        if !self.criteria.is_empty() {
            let items: Vec<String> = self
                .criteria
                .iter()
                .map(|c| format!("- [{}] {}", if c.met { 'x' } else { ' ' }, c.text))
                .collect();
            section(&mut out, "Acceptance criteria", &items.join("\n"));
        }
        if !self.relevant_files.is_empty() {
            let items: Vec<String> = self
                .relevant_files
                .iter()
                .map(|file| format!("- {file}"))
                .collect();
            section(&mut out, "Relevant files", &items.join("\n"));
        }
        if !self.changelog.is_empty() {
            let entries: Vec<String> = self.changelog.iter().map(Entry::to_markdown).collect();
            // Entries start after a blank line, as headings of their own.
            section(
                &mut out,
                "Changelog",
                &format!("\n{}", entries.join("\n\n")),
            );
        }
        out
    }
}

impl Entry {
    /// The entry's heading line, then the summary and its `Files:`,
    /// `Follow-ups:` and `Unmet:` lines, without a trailing newline.
    pub fn to_markdown(&self) -> String {
        let mut out = format!("### {} · {} · {}", self.date, self.who, self.event);
        let text = self.text();
        if !text.is_empty() {
            out.push('\n');
            out.push_str(&text);
        }
        out
    }

    /// The summary and its `Files:`, `Follow-ups:` and `Unmet:` lines,
    /// without a trailing newline. Empty if there are none.
    pub fn text(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        let summary = self.summary.trim();
        if !summary.is_empty() {
            lines.push(summary.to_owned());
        }
        if !self.files.is_empty() {
            lines.push(format!("Files: {}", self.files.join(", ")));
        }
        if let Some(follow_ups) = &self.follow_ups {
            lines.push(format!("Follow-ups: {follow_ups}"));
        }
        if !self.unmet.is_empty() {
            let unmet: Vec<String> = self
                .unmet
                .iter()
                .map(|u| format!("{} ({})", u.criterion, u.reason))
                .collect();
            lines.push(format!("Unmet: {}", unmet.join("; ")));
        }
        lines.join("\n")
    }
}

/// One frontmatter line. Statuses, priorities and dates never need
/// quoting; other values are already rendered by [`yaml`].
fn field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

fn section(out: &mut String, name: &str, content: &str) {
    out.push_str("\n## ");
    out.push_str(name);
    out.push('\n');
    out.push_str(content);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Criterion, Priority, Status, Unmet};

    const FILE: &str = "---
id: k3f9x2m
title: \"Fix: token refresh race\"
status: finished
priority: high
parent: a7rq4ne
labels: [auth, backend]
depends_on: [p2xmq7d]
created: 2026-10-13
---

## Description
Parallel requests that hit a 401 each trigger their own token refresh.

## Notes
Kept.

## Acceptance criteria
- [x] Concurrent requests share one refresh call
- [ ] A test covers two parallel 401 responses

## Relevant files
- src/auth/session.ts

## Changelog

### 2026-10-13 · claude-code-7f2a · started

### 2026-10-14 · claude-code-7f2a · finished
Serialized refresh behind a shared promise.
Files: src/auth/session.ts, src/auth/session.test.ts
Follow-ups: none
Unmet: 2 (needs a staging account)
";

    #[test]
    fn parses_and_renders_a_task() {
        let task = TaskDoc::parse(FILE).unwrap();
        assert_eq!(task.title, "Fix: token refresh race");
        assert_eq!(task.status, Status::Finished);
        assert_eq!(task.priority, Some(Priority::High));
        assert_eq!(
            task.description,
            "Parallel requests that hit a 401 each trigger their own token refresh.\n\n## Notes\nKept."
        );
        assert_eq!(
            task.criteria,
            [
                Criterion {
                    number: 1,
                    met: true,
                    text: "Concurrent requests share one refresh call".into()
                },
                Criterion {
                    number: 2,
                    met: false,
                    text: "A test covers two parallel 401 responses".into()
                },
            ]
        );
        assert_eq!(task.changelog.len(), 2);
        assert_eq!(
            task.changelog[1].unmet,
            [Unmet {
                criterion: 2,
                reason: "needs a staging account".into()
            }]
        );

        // The unknown section moved into the description; the rest renders
        // as it was written.
        let expected = FILE
            .replace("\n## Notes\nKept.\n", "")
            .replace("refresh.\n", "refresh.\n\n## Notes\nKept.\n");
        assert_eq!(task.to_markdown(), expected);
        assert_eq!(TaskDoc::parse(&task.to_markdown()).unwrap(), task);
    }

    #[test]
    fn leaves_out_empty_parts() {
        let task =
            TaskDoc::parse("---\nid: k3f9x2m\ntitle: T\nstatus: todo\ncreated: 2026-10-13\n---\n")
                .unwrap();
        assert_eq!(
            task.to_markdown(),
            "---\nid: k3f9x2m\ntitle: T\nstatus: todo\ncreated: 2026-10-13\n---\n"
        );
    }

    #[test]
    fn refuses_broken_files() {
        for (text, message) in [
            ("no frontmatter", "does not start"),
            ("---\nid: k3f9x2m\n", "no closing"),
            (
                "---\nid: k3f9x2m\ntitle: ' '\nstatus: todo\ncreated: 2026-10-13\n---\n",
                "title is empty",
            ),
            (
                "---\nid: k3f9x2m\ntitle: T\nstatus: doing\ncreated: 2026-10-13\n---\n",
                "invalid frontmatter",
            ),
        ] {
            let error = TaskDoc::parse(text).unwrap_err().to_string();
            assert!(error.contains(message), "{text:?}: {error}");
        }
        let id: TaskId = "aaaaaaa".parse().unwrap();
        assert!(matches!(
            TaskDoc::parse_file(id, FILE.as_bytes()),
            Err(super::super::ParseError::IdMismatch { .. })
        ));
    }
}
