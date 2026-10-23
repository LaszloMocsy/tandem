//! Reading the body sections of a task file: Description, Acceptance
//! criteria, Relevant files, and any sections the format doesn't know.

use super::text::{find_section, heading_level, heading_text, lines};
use crate::model::Criterion;

pub const DESCRIPTION: &str = "Description";
pub const ACCEPTANCE_CRITERIA: &str = "Acceptance criteria";
pub const RELEVANT_FILES: &str = "Relevant files";
pub const CHANGELOG: &str = "Changelog";

/// The sections the format gives a meaning to.
const KNOWN: [&str; 4] = [DESCRIPTION, ACCEPTANCE_CRITERIA, RELEVANT_FILES, CHANGELOG];

/// The text of section `name` without its heading or surrounding blank
/// lines. `None` if the section is missing.
pub(super) fn text<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    find_section(body, name).map(|section| {
        body[section.content..section.end]
            .trim_matches('\n')
            .trim_end()
    })
}

/// The top-level `-`, `*` or `+` list items of section `name`.
pub(super) fn list_items(body: &str, name: &str) -> Vec<String> {
    let Some(section) = find_section(body, name) else {
        return Vec::new();
    };
    lines(&body[section.content..section.end])
        .into_iter()
        .filter_map(|line| {
            ["- ", "* ", "+ "]
                .iter()
                .find_map(|p| line.text.strip_prefix(p))
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

/// The checkboxes in the Acceptance criteria section: top-level list items
/// starting with `[ ]` or `[x]`.
pub(super) fn criteria(body: &str) -> Vec<Criterion> {
    let Some(section) = find_section(body, ACCEPTANCE_CRITERIA) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in lines(&body[section.content..section.end]) {
        let text = line.text;
        let Some(rest) = ["- [", "* [", "+ ["]
            .iter()
            .find_map(|p| text.strip_prefix(p))
        else {
            continue;
        };
        let (met, after) = match rest.as_bytes() {
            [b' ', b']', ..] => (false, &rest[2..]),
            [b'x' | b'X', b']', ..] => (true, &rest[2..]),
            _ => continue,
        };
        if !(after.is_empty() || after.starts_with(' ')) || heading_level(text).is_some() {
            continue;
        }
        out.push(Criterion {
            number: out.len() as u32 + 1,
            met,
            text: after.trim().to_owned(),
        });
    }
    out
}

/// Every `## ` section the format doesn't know, with its heading, and any
/// text before the first section, in file order. Empty if there are none.
pub(super) fn other_sections(body: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    // The start of the current part, and whether it is kept.
    let mut current: Option<(usize, bool)> = Some((0, true));
    for line in lines(body) {
        if matches!(line.heading, Some(level) if level <= 2) {
            if let Some((start, true)) = current {
                parts.push(&body[start..line.start]);
            }
            let known = line.heading == Some(2)
                && KNOWN
                    .iter()
                    .any(|name| heading_text(line.text).eq_ignore_ascii_case(name));
            current = Some((line.start, !known));
        }
    }
    if let Some((start, true)) = current {
        parts.push(&body[start..]);
    }
    parts
        .iter()
        .map(|part| part.trim_matches('\n').trim_end())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_section_text_and_list_items() {
        let body = "\n## Description\n\nFirst.\n\n### Detail\nSecond.\n\n\
                    ## Relevant files\n- src/a.rs\n* src/b.rs\n  - nested.rs\n-\nprose\n+ c.md\n";
        assert_eq!(
            text(body, DESCRIPTION),
            Some("First.\n\n### Detail\nSecond.")
        );
        assert_eq!(text(body, ACCEPTANCE_CRITERIA), None);
        assert_eq!(
            list_items(body, RELEVANT_FILES),
            ["src/a.rs", "src/b.rs", "c.md"]
        );
        assert_eq!(list_items(body, DESCRIPTION), Vec::<String>::new());
    }

    const BODY: &str = "\n## Description\nOld.\n\n## Acceptance criteria\n- [ ] One\n- [x] Two\n  - [ ] nested\n* [ ] Three\n- [ ]not a box\n\n## Notes\nKeep.\n\n## Changelog\n\n### 2026-10-13 · a · created\n";

    #[test]
    fn reads_criteria() {
        let found = criteria(BODY);
        assert_eq!(
            found
                .iter()
                .map(|c| (c.number, c.met, c.text.as_str()))
                .collect::<Vec<_>>(),
            [(1, false, "One"), (2, true, "Two"), (3, false, "Three")]
        );
        assert!(criteria("\n## Description\n- [ ] not a criterion\n").is_empty());
    }

    #[test]
    fn collects_unknown_sections() {
        assert_eq!(other_sections(BODY), "## Notes\nKeep.");
        assert_eq!(
            other_sections(
                "Intro.\n\n## Description\nD\n\n# Top\nT\n\n## notes\n```\n## code\n```\n"
            ),
            "Intro.\n\n# Top\nT\n\n## notes\n```\n## code\n```"
        );
        assert_eq!(other_sections("\n## Description\nD\n"), "");
    }
}
