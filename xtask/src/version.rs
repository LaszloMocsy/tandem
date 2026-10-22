//! App versions: `X.Y.Z`, `X.Y.Z-alpha.N` or `X.Y.Z-beta.N`, with no leading
//! zeros. Release tags are the same with a leading `v`.

use std::cmp::Ordering;
use std::fmt;

/// The build environment a version or tag selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Channel {
    Alpha,
    Beta,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Channel::Alpha => "alpha",
            Channel::Beta => "beta",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The pre-release channel and its number; `None` for production.
    pub pre: Option<(Channel, u64)>,
}

impl Version {
    /// Parses `X.Y.Z`, `X.Y.Z-alpha.N` or `X.Y.Z-beta.N`.
    pub fn parse(s: &str) -> Result<Version, String> {
        let unsupported =
            || format!("unsupported version {s:?}: use X.Y.Z, X.Y.Z-alpha.N or X.Y.Z-beta.N");
        let (core, pre) = match s.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (s, None),
        };
        let mut parts = core.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(unsupported());
        };
        let pre = match pre {
            None => None,
            Some(pre) => {
                let (channel, n) = match pre.split_once('.') {
                    Some(("alpha", n)) => (Channel::Alpha, n),
                    Some(("beta", n)) => (Channel::Beta, n),
                    _ => return Err(unsupported()),
                };
                Some((channel, number(n).ok_or_else(unsupported)?))
            }
        };
        Ok(Version {
            major: number(major).ok_or_else(unsupported)?,
            minor: number(minor).ok_or_else(unsupported)?,
            patch: number(patch).ok_or_else(unsupported)?,
            pre,
        })
    }

    /// Parses a release tag: a version with a leading `v`.
    pub fn parse_tag(tag: &str) -> Result<Version, String> {
        tag.strip_prefix('v')
            .and_then(|v| Version::parse(v).ok())
            .ok_or_else(|| {
                format!("unsupported tag {tag:?}: use vX.Y.Z, vX.Y.Z-alpha.N or vX.Y.Z-beta.N")
            })
    }

    pub fn channel(&self) -> Option<Channel> {
        self.pre.map(|(channel, _)| channel)
    }

    /// Orders pre-releases before their production version, alpha before
    /// beta, as SemVer precedence does for these forms.
    fn key(&self) -> (u64, u64, u64, u8, u64) {
        let (rank, n) = match self.pre {
            Some((Channel::Alpha, n)) => (0, n),
            Some((Channel::Beta, n)) => (1, n),
            None => (2, 0),
        };
        (self.major, self.minor, self.patch, rank, n)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some((channel, n)) = self.pre {
            write!(f, "-{}.{n}", channel.name())?;
        }
        Ok(())
    }
}

/// A decimal number with no sign and no leading zeros.
fn number(s: &str) -> Option<u64> {
    let digits = !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if !digits || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn parses_and_prints_every_form() {
        for s in [
            "0.2.0",
            "1.10.3",
            "0.2.0-alpha.1",
            "0.2.0-alpha.12",
            "0.2.0-beta.0",
        ] {
            assert_eq!(v(s).to_string(), s);
        }
        assert_eq!(v("0.2.0").channel(), None);
        assert_eq!(v("0.2.0-alpha.1").channel(), Some(Channel::Alpha));
        assert_eq!(v("0.2.0-beta.0").channel(), Some(Channel::Beta));
    }

    #[test]
    fn rejects_other_versions() {
        for s in [
            "",
            "0.2",
            "0.2.0.1",
            "01.2.0",
            "0.02.0",
            "0.2.00",
            "+1.2.0",
            "0.2.0-rc.1",
            "0.2.0-alpha",
            "0.2.0-alpha.01",
            "0.2.0-beta.1.1",
            "0.2.0-ALPHA.1",
            "0.2.0-alpha.-1",
            "0.2.0-alpha.1-beta.1",
            "0.2.0\n0.2.0",
            " 0.2.0",
            "v0.2.0",
            "99999999999999999999.0.0",
        ] {
            assert!(Version::parse(s).is_err(), "accepted {s:?}");
        }
    }

    #[test]
    fn tags_need_a_leading_v() {
        assert_eq!(
            Version::parse_tag("v0.2.0-beta.3").unwrap(),
            v("0.2.0-beta.3")
        );
        for tag in ["0.2.0", "v", "", "V0.2.0", "vv0.2.0", "v0.2.0-rc.1"] {
            assert!(Version::parse_tag(tag).is_err(), "accepted {tag:?}");
        }
    }

    #[test]
    fn orders_by_semver_precedence() {
        let ordered = [
            "0.1.9",
            "0.2.0-alpha.0",
            "0.2.0-alpha.1",
            "0.2.0-alpha.2",
            "0.2.0-alpha.10",
            "0.2.0-beta.0",
            "0.2.0-beta.1",
            "0.2.0",
            "0.2.1",
            "0.10.0",
            "1.0.0-alpha.0",
            "1.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(v("0.2.0").cmp(&v("0.2.0")), Ordering::Equal);
    }
}
