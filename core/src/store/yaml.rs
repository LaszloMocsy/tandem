//! Writing YAML scalars for frontmatter. Values are written plain when that
//! reads back as the same string in any YAML reader, and double-quoted
//! otherwise.

use std::borrow::Cow;
use std::fmt::Write;

/// A scalar for a `key: value` line.
pub(crate) fn scalar(s: &str) -> Cow<'_, str> {
    if is_plain_safe(s, false) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(double_quoted(s))
    }
}

/// A flow sequence such as `[auth, backend]`.
pub(crate) fn flow_seq<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let items: Vec<Cow<'_, str>> = items
        .into_iter()
        .map(|s| {
            if is_plain_safe(s, true) {
                Cow::Borrowed(s)
            } else {
                Cow::Owned(double_quoted(s))
            }
        })
        .collect();
    format!("[{}]", items.join(", "))
}

fn is_plain_safe(s: &str, in_flow: bool) -> bool {
    let Some(first) = s.chars().next() else {
        return false;
    };
    if first.is_whitespace() || s.ends_with(char::is_whitespace) {
        return false;
    }
    if "-?:,[]{}#&*!|>'\"%@`".contains(first) {
        return false;
    }
    if s.contains(": ") || s.contains(" #") || s.ends_with(':') {
        return false;
    }
    if s.chars()
        .any(|c| c.is_control() || (in_flow && ",[]{}".contains(c)))
    {
        return false;
    }
    !looks_like_non_string(s)
}

/// Whether a plain scalar could be read as a boolean, null, number, date or
/// time by a YAML 1.1 or 1.2 reader.
fn looks_like_non_string(s: &str) -> bool {
    const KEYWORDS: [&str; 10] = [
        "true", "false", "yes", "no", "on", "off", "y", "n", "null", "~",
    ];
    if KEYWORDS.contains(&s.to_ascii_lowercase().as_str()) {
        return true;
    }
    let starts_like_number = s.starts_with(|c: char| c.is_ascii_digit() || "+-.".contains(c));
    starts_like_number
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-._:".contains(c))
}

fn double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                write!(out, "\\u{:04x}", u32::from(c)).expect("writing to a String");
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Doc {
        value: String,
        list: Vec<String>,
    }

    const TRICKY: &[&str] = &[
        "Fix token refresh race",
        "Fix: token refresh",
        "trailing colon:",
        "a # not a comment",
        "#hashtag",
        "- starts like a list",
        "-dash",
        "? question",
        "[bracketed]",
        "{braced}",
        "comma, inside",
        "'single quoted'",
        "\"double quoted\"",
        "back\\slash",
        "tab\there",
        "line\nbreak",
        "bell\u{7}",
        " leading space",
        "trailing space ",
        "true",
        "No",
        "null",
        "~",
        "123",
        "1.5",
        "-3",
        ".inf",
        "0x1F",
        "2026-10-13",
        "12:30",
        "2fa login",
        "100% done",
        "@mention",
        "`code`",
        "*bold*",
        "&anchor",
        "!tag",
        "|pipe",
        ">fold",
        "Ünïcödé · ok",
        "",
    ];

    #[test]
    fn scalars_read_back_unchanged() {
        for s in TRICKY {
            let yaml = format!("value: {}\nlist: {}\n", scalar(s), flow_seq([*s, "x"]));
            let doc: Doc = serde_saphyr::from_str(&yaml)
                .unwrap_or_else(|e| panic!("{s:?} wrote unreadable YAML {yaml:?}: {e}"));
            assert_eq!(doc.value, *s, "scalar for {s:?}: {yaml:?}");
            assert_eq!(doc.list, [*s, "x"], "flow item for {s:?}: {yaml:?}");
        }
    }

    #[test]
    fn plain_where_safe() {
        assert_eq!(scalar("Fix token refresh race"), "Fix token refresh race");
        assert_eq!(scalar("Ünïcödé · ok"), "Ünïcödé · ok");
        assert_eq!(flow_seq(["auth", "backend"]), "[auth, backend]");
        assert_eq!(flow_seq([]), "[]");
    }

    #[test]
    fn quoted_where_needed() {
        assert_eq!(scalar("Fix: token refresh"), "\"Fix: token refresh\"");
        assert_eq!(scalar("true"), "\"true\"");
        assert_eq!(scalar("2345678"), "\"2345678\"");
        assert_eq!(scalar("say \"hi\""), "say \"hi\"");
        assert_eq!(scalar("\"hi\""), "\"\\\"hi\\\"\"");
        assert_eq!(flow_seq(["a,b"]), "[\"a,b\"]");
    }
}
