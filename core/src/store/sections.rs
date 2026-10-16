//! Editing the body sections the app writes: Description, Acceptance
//! criteria and Relevant files. Every other line of the body is kept byte
//! for byte.

use super::markdown::{Section, find_section, has_unclosed_fence, heading_level, lines};

pub const DESCRIPTION: &str = "Description";
pub const ACCEPTANCE_CRITERIA: &str = "Acceptance criteria";
pub const RELEVANT_FILES: &str = "Relevant files";
pub const CHANGELOG: &str = "Changelog";

/// The order sections are written in when the app adds one.
const ORDER: [&str; 4] = [DESCRIPTION, ACCEPTANCE_CRITERIA, RELEVANT_FILES, CHANGELOG];

/// Why a section cannot be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SectionError {
    #[error("the {0} section is written only through changelog entries")]
    Changelog(&'static str),
    #[error("`{0}` is not a section the app writes")]
    Unknown(String),
    #[error("the text must not contain level 1 or 2 headings")]
    Heading,
    #[error("the text has an unclosed code fence")]
    UnclosedFence,
    #[error("acceptance criterion {0} does not exist")]
    NoCriterion(u32),
}

/// One checkbox in the Acceptance criteria section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    /// Counting from 1, in the order they appear.
    pub number: u32,
    pub met: bool,
    pub text: String,
}

/// Replaces the content of section `name`, adding the section in the usual
/// order if it is missing, or removes it if `content` is blank. `body` has
/// LF line endings and ends in one newline, or is empty.
pub(super) fn set(body: &str, name: &str, content: &str) -> Result<String, SectionError> {
    let name = ORDER[..3]
        .iter()
        .copied()
        .find(|known| known.eq_ignore_ascii_case(name))
        .ok_or_else(|| match name.eq_ignore_ascii_case(CHANGELOG) {
            true => SectionError::Changelog(CHANGELOG),
            false => SectionError::Unknown(name.to_owned()),
        })?;
    let content = content.replace("\r\n", "\n");
    let content = content.trim_matches('\n').trim_end();
    if lines(content)
        .iter()
        .any(|line| line.heading.is_some_and(|level| level <= 2))
    {
        return Err(SectionError::Heading);
    }
    if has_unclosed_fence(content) {
        return Err(SectionError::UnclosedFence);
    }

    let mut out = String::with_capacity(body.len() + content.len() + 32);
    match find_section(body, name) {
        Some(Section {
            heading,
            content: start,
            end,
        }) => {
            if content.is_empty() {
                out.push_str(&body[..heading]);
            } else {
                out.push_str(&body[..start]);
                out.push_str(content);
                out.push('\n');
                if end < body.len() {
                    out.push('\n');
                }
            }
            out.push_str(&body[end..]);
        }
        None if content.is_empty() => out.push_str(body),
        None => {
            let block = format!("## {name}\n{content}\n");
            let later = ORDER
                .iter()
                .skip_while(|&&known| known != name)
                .skip(1)
                .find_map(|later| find_section(body, later));
            match later {
                Some(section) => {
                    out.push_str(&body[..section.heading]);
                    out.push_str(&block);
                    out.push('\n');
                    out.push_str(&body[section.heading..]);
                }
                None => {
                    out.push_str(body);
                    out.push('\n');
                    out.push_str(&block);
                }
            }
        }
    }
    Ok(out)
}

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
    checkboxes(body)
        .into_iter()
        .map(|(number, _, met, text)| Criterion {
            number,
            met,
            text: text.to_owned(),
        })
        .collect()
}

/// Ticks the given criteria, changing only the character inside their
/// brackets. Ticking a ticked criterion changes nothing.
pub(super) fn tick(body: &str, numbers: &[u32]) -> Result<String, SectionError> {
    let boxes = checkboxes(body);
    let mut out = body.to_owned();
    for &number in numbers {
        let &(_, mark, met, _) = boxes
            .iter()
            .find(|(n, ..)| *n == number)
            .ok_or(SectionError::NoCriterion(number))?;
        if !met {
            out.replace_range(mark..mark + 1, "x");
        }
    }
    Ok(out)
}

/// `(number, byte offset of the mark, ticked, text)` for each checkbox.
fn checkboxes(body: &str) -> Vec<(u32, usize, bool, &str)> {
    let Some(section) = find_section(body, ACCEPTANCE_CRITERIA) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let content = &body[section.content..section.end];
    for line in lines(content) {
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
        let number = out.len() as u32 + 1;
        out.push((number, section.content + line.start + 3, met, after.trim()));
    }
    out
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
    fn replaces_a_section_and_keeps_the_rest() {
        let out = set(BODY, "description", "New\nlines.\n\n").unwrap();
        assert_eq!(out, BODY.replace("Old.\n", "New\nlines.\n"));
        // The last section gets no blank line after it.
        let body = "\n## Description\nOld.\n";
        assert_eq!(
            set(body, DESCRIPTION, "New.").unwrap(),
            "\n## Description\nNew.\n"
        );
    }

    #[test]
    fn adds_a_missing_section_in_order() {
        let out = set(BODY, RELEVANT_FILES, "- src/a.rs").unwrap();
        assert_eq!(
            out,
            BODY.replace(
                "## Changelog",
                "## Relevant files\n- src/a.rs\n\n## Changelog"
            )
        );
        assert_eq!(set("", DESCRIPTION, "D").unwrap(), "\n## Description\nD\n");
        let body = "\n## Description\nD\n";
        assert_eq!(
            set(body, ACCEPTANCE_CRITERIA, "- [ ] C").unwrap(),
            "\n## Description\nD\n\n## Acceptance criteria\n- [ ] C\n"
        );
    }

    #[test]
    fn removes_a_blank_section() {
        let out = set(BODY, "Acceptance criteria", "  \n").unwrap();
        let start = BODY.find("## Acceptance").unwrap();
        let end = BODY.find("## Notes").unwrap();
        assert_eq!(out, format!("{}{}", &BODY[..start], &BODY[end..]));
        assert_eq!(set(BODY, RELEVANT_FILES, "").unwrap(), BODY);
    }

    #[test]
    fn refuses_text_that_would_break_the_sections() {
        assert_eq!(
            set(BODY, CHANGELOG, "x"),
            Err(SectionError::Changelog(CHANGELOG))
        );
        assert_eq!(
            set(BODY, "Notes", "x"),
            Err(SectionError::Unknown("Notes".into()))
        );
        assert_eq!(
            set(BODY, DESCRIPTION, "a\n## B"),
            Err(SectionError::Heading)
        );
        assert_eq!(
            set(BODY, DESCRIPTION, "```\n## B"),
            Err(SectionError::UnclosedFence)
        );
        assert!(set(BODY, DESCRIPTION, "### Sub\n```\n## code\n```").is_ok());
    }

    #[test]
    fn reads_and_ticks_criteria() {
        let found = criteria(BODY);
        assert_eq!(
            found
                .iter()
                .map(|c| (c.number, c.met, c.text.as_str()))
                .collect::<Vec<_>>(),
            [(1, false, "One"), (2, true, "Two"), (3, false, "Three")]
        );
        let out = tick(BODY, &[3, 2, 1]).unwrap();
        assert_eq!(
            out,
            BODY.replace("- [ ] One", "- [x] One")
                .replace("* [ ] Three", "* [x] Three")
        );
        assert_eq!(tick(BODY, &[4]), Err(SectionError::NoCriterion(4)));
        assert!(criteria("\n## Description\n- [ ] not a criterion\n").is_empty());
    }
}
