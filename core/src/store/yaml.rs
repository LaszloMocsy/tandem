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
    KEYWORDS.contains(&s.to_ascii_lowercase().as_str()) || is_number(s) || is_date(s)
}

/// A number in YAML 1.1 or 1.2: decimal integers and floats, with 1.1's
/// underscores and base-60 parts (`1:30`), binary, octal and hex
/// integers, exponents, and `.inf` and `.nan`. Text such as `7dqv6u9`
/// that merely starts with a digit is a string.
fn is_number(s: &str) -> bool {
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(s, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    let all = |text: &str, digit: fn(char) -> bool| {
        text.chars().any(digit) && text.chars().all(|c| digit(c) || c == '_')
    };
    for (prefix, digit) in [
        ("0b", (|c| matches!(c, '0' | '1')) as fn(char) -> bool),
        ("0o", |c| matches!(c, '0'..='7')),
        ("0x", |c| c.is_ascii_hexdigit()),
    ] {
        if let Some(rest) = unsigned.strip_prefix(prefix) {
            return all(rest, digit);
        }
    }

    let decimal = |c: char| c.is_ascii_digit();
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    if let Some(exponent) = exponent {
        let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if digits.is_empty() || !digits.chars().all(decimal) {
            return false;
        }
    }
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (mantissa, None),
    };
    // YAML 1.1 floats allow more dots after the first, as in `1.2.3`.
    let fraction_ok = fraction.is_none_or(|f| f.chars().all(|c| decimal(c) || "_.".contains(c)));
    let whole_ok = if whole.is_empty() {
        fraction.is_some()
    } else {
        whole
            .split(':')
            .all(|part| !part.is_empty() && part.chars().all(|c| decimal(c) || c == '_'))
    };
    fraction_ok && whole_ok && mantissa.chars().any(decimal)
}

/// A YAML 1.1 timestamp starts with a date such as `2026-10-13`.
fn is_date(s: &str) -> bool {
    let mut parts = s.splitn(3, '-');
    let (Some(year), Some(month), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let day: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let digits = |t: &str, len: std::ops::RangeInclusive<usize>| {
        len.contains(&t.len()) && t.chars().all(|c| c.is_ascii_digit())
    };
    digits(year, 4..=4) && digits(month, 1..=2) && digits(&day, 1..=2)
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
        "7dqv6u9",
        "2345678",
        "22e3456",
        "1e5",
        "1E-5",
        "2e",
        "1_000",
        "0b101",
        "0o17",
        "0x1f",
        "1:30:00",
        "1.2.3",
        ".5",
        "+5",
        ".nan",
        ".Inf",
        "2026-10-13T12:00:00Z",
        "2026-1-3",
        "v1.2",
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
        // Task ids that start with a digit but can't be numbers.
        for id in ["7dqv6u9", "2fa4mqd", "9e2abcd", "3456789x"] {
            assert_eq!(scalar(id), id);
        }
        assert_eq!(scalar("2e"), "2e");
        assert_eq!(scalar("1.2.3a"), "1.2.3a");
    }

    #[test]
    fn quoted_where_needed() {
        assert_eq!(scalar("Fix: token refresh"), "\"Fix: token refresh\"");
        assert_eq!(scalar("true"), "\"true\"");
        assert_eq!(scalar("2345678"), "\"2345678\"");
        for number in [
            "22e3456",
            "1e5",
            "1_000",
            "0b101",
            "0o17",
            "0x1f",
            "1:30",
            "1.2.3",
            ".5",
            "+5",
            ".nan",
            "-.inf",
            "2026-10-13",
            "2026-10-13T12:00:00Z",
        ] {
            assert_eq!(scalar(number), format!("\"{number}\""), "{number}");
        }
        assert_eq!(scalar("say \"hi\""), "say \"hi\"");
        assert_eq!(scalar("\"hi\""), "\"\\\"hi\\\"\"");
        assert_eq!(flow_seq(["a,b"]), "[\"a,b\"]");
    }
}
