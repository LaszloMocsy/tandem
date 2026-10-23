//! Line-level Markdown scanning shared by the section and changelog readers:
//! ATX headings and fenced code blocks, following CommonMark closely enough
//! for task bodies.

/// One line of a body.
pub(super) struct Line<'a> {
    /// Byte offset of the line's first character.
    pub start: usize,
    /// Byte offset just past the line's `\n`, or the end of the body.
    pub end: usize,
    /// The line without its `\n`.
    pub text: &'a str,
    /// The heading level, for headings outside fenced code blocks.
    pub heading: Option<usize>,
}

/// Splits `body` (LF line endings) into lines, tracking fenced code blocks
/// so that headings inside them don't count.
pub(super) fn lines(body: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut fence: Option<char> = None;
    let mut offset = 0;
    for raw in body.split_inclusive('\n') {
        let start = offset;
        offset += raw.len();
        let text = raw.strip_suffix('\n').unwrap_or(raw);
        let heading = match (fence, fence_marker(text)) {
            (Some(open), Some(marker)) if marker == open => {
                fence = None;
                None
            }
            (Some(_), _) => None,
            (None, Some(marker)) => {
                fence = Some(marker);
                None
            }
            (None, None) => heading_level(text),
        };
        out.push(Line {
            start,
            end: offset,
            text,
            heading,
        });
    }
    out
}

/// A `## <name>` section: the heading line and everything up to the next
/// heading of level 1 or 2, or the end of the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Section {
    /// Start of the line after the heading.
    pub content: usize,
    /// Start of the next level 1 or 2 heading, or the end of the body.
    pub end: usize,
}

/// The first `## <name>` section in `body`. Names compare ignoring ASCII
/// case, so `## Acceptance Criteria` counts too.
pub(super) fn find_section(body: &str, name: &str) -> Option<Section> {
    let mut found: Option<Section> = None;
    for line in lines(body) {
        match (&mut found, line.heading) {
            (Some(section), Some(level)) if level <= 2 => {
                section.end = line.start;
                return found;
            }
            (None, Some(2)) if heading_text(line.text).eq_ignore_ascii_case(name) => {
                found = Some(Section {
                    content: line.end,
                    end: body.len(),
                });
            }
            _ => {}
        }
    }
    found
}

/// The fence character (backtick or tilde) if the line opens or closes a
/// fenced code block: up to three spaces of indent, then three or more of
/// the same character.
fn fence_marker(line: &str) -> Option<char> {
    let indented = line.trim_start_matches(' ');
    if line.len() - indented.len() > 3 {
        return None;
    }
    ['`', '~']
        .into_iter()
        .find(|&c| indented.chars().take(3).filter(|&x| x == c).count() == 3)
}

/// The level of an ATX heading line (`## Title` is 2), following CommonMark:
/// up to three spaces of indent, one to six `#`, then a space, tab or end of
/// line.
pub(super) fn heading_level(line: &str) -> Option<usize> {
    let indented = line.trim_start_matches(' ');
    if line.len() - indented.len() > 3 {
        return None;
    }
    let level = indented.len() - indented.trim_start_matches('#').len();
    let rest = &indented[level..];
    ((1..=6).contains(&level) && (rest.is_empty() || rest.starts_with([' ', '\t'])))
        .then_some(level)
}

/// A heading's text without the `#` markers, surrounding whitespace or an
/// optional closing run of `#`.
pub(super) fn heading_text(line: &str) -> &str {
    let text = line.trim().trim_start_matches('#').trim();
    let without_closing = text.trim_end_matches('#');
    if without_closing.is_empty() || without_closing.ends_with([' ', '\t']) {
        without_closing.trim_end()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_levels() {
        assert_eq!(heading_level("## Changelog"), Some(2));
        assert_eq!(heading_level("   # Title"), Some(1));
        assert_eq!(heading_level("#"), Some(1));
        assert_eq!(heading_level("    ## indented code"), None);
        assert_eq!(heading_level("#hashtag"), None);
        assert_eq!(heading_level("####### seven"), None);
        assert_eq!(heading_text("## Changelog ##"), "Changelog");
        assert_eq!(heading_text("##  Changelog  "), "Changelog");
    }

    #[test]
    fn finds_sections() {
        let body = "\n## Description\nX\n### Sub\nY\n\n## Notes\nN\n";
        let section = find_section(body, "description").unwrap();
        assert_eq!(&body[..section.content], "\n## Description\n");
        assert_eq!(&body[section.content..section.end], "X\n### Sub\nY\n\n");
        let notes = find_section(body, "Notes").unwrap();
        assert_eq!(&body[notes.content..notes.end], "N\n");
        assert_eq!(find_section(body, "Sub"), None);
        assert_eq!(find_section("```\n## Notes\n```\n", "Notes"), None);
    }
}
