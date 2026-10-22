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

/// What `cargo xtask version --next` moves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Major,
    Minor,
    Patch,
    /// The next alpha of the same pre-release.
    Alpha,
    /// The next beta of the same pre-release.
    Beta,
    /// The production version a pre-release leads to.
    Release,
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

    /// The version after this one at `level`. `major`, `minor` and `patch`
    /// raise the numbers and start a pre-release when `pre` is given:
    /// `0.1.0` → `0.2.0`, or `0.2.0-alpha.1` with `pre`. `alpha` and `beta`
    /// continue a pre-release, and `release` ends one: `0.2.0-beta.2` →
    /// `0.2.0`. A result that isn't higher, or is ambiguous, is an error.
    pub fn next(self, level: Level, pre: Option<Channel>) -> Result<Version, String> {
        let overflow = || format!("{self} can't go any higher");
        let core = |major, minor, patch| Version {
            major,
            minor,
            patch,
            pre: pre.map(|channel| (channel, 1)),
        };
        if pre.is_some() && matches!(level, Level::Alpha | Level::Beta | Level::Release) {
            return Err(format!(
                "--pre only goes with --next major, minor or patch, not {}",
                level.name()
            ));
        }
        let next = match level {
            Level::Major => core(self.major.checked_add(1).ok_or_else(overflow)?, 0, 0),
            Level::Minor => core(
                self.major,
                self.minor.checked_add(1).ok_or_else(overflow)?,
                0,
            ),
            Level::Patch => core(
                self.major,
                self.minor,
                self.patch.checked_add(1).ok_or_else(overflow)?,
            ),
            Level::Alpha | Level::Beta => {
                let channel = if level == Level::Alpha {
                    Channel::Alpha
                } else {
                    Channel::Beta
                };
                let Some((current, n)) = self.pre else {
                    return Err(format!(
                        "{self} isn't a pre-release, so the next {0} is ambiguous; use \
                         --next major, minor or patch with --pre {0}",
                        channel.name()
                    ));
                };
                let n = match current.cmp(&channel) {
                    Ordering::Less => 1,
                    Ordering::Equal => n.checked_add(1).ok_or_else(overflow)?,
                    Ordering::Greater => {
                        return Err(format!(
                            "{self} is already past {}; use --next beta or release",
                            channel.name()
                        ));
                    }
                };
                Version {
                    pre: Some((channel, n)),
                    ..self
                }
            }
            Level::Release => {
                if self.pre.is_none() {
                    return Err(format!(
                        "{self} is already a release; use --next major, minor or patch"
                    ));
                }
                Version { pre: None, ..self }
            }
        };
        debug_assert!(next > self, "{next} isn't higher than {self}");
        Ok(next)
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

/// The tag that release notes for `tag` start from: the highest earlier
/// production tag for a production release, so its notes cover every
/// pre-release since the last one, or the highest earlier tag of any kind for
/// a pre-release. Tags that aren't release tags are ignored.
pub fn previous_tag<'a>(tag: Version, tags: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    tags.into_iter()
        .filter_map(|t| Some((Version::parse_tag(t).ok()?, t)))
        .filter(|(v, _)| *v < tag && (tag.pre.is_some() || v.pre.is_none()))
        .max_by_key(|(v, _)| *v)
        .map(|(_, t)| t)
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Major => "major",
            Level::Minor => "minor",
            Level::Patch => "patch",
            Level::Alpha => "alpha",
            Level::Beta => "beta",
            Level::Release => "release",
        }
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
    fn production_notes_start_from_the_last_production_tag() {
        let tags = [
            "v0.1.0",
            "v0.2.0-alpha.1",
            "v0.2.0-beta.1",
            "v0.2.0",
            "v0.3.0-alpha.1",
            "v0.3.0-alpha.2",
            "v0.3.0-beta.1",
            "v0.3.0",
            "v1.0.0",
            "nightly",
            "v0.2.5-rc.1",
        ];
        let prev = |tag: &str| previous_tag(Version::parse_tag(tag).unwrap(), tags);
        assert_eq!(prev("v0.3.0"), Some("v0.2.0"));
        assert_eq!(prev("v0.2.0"), Some("v0.1.0"));
        assert_eq!(prev("v0.1.0"), None);
        // A tag that isn't in the list yet works the same.
        assert_eq!(prev("v0.4.0"), Some("v0.3.0"));
    }

    #[test]
    fn pre_release_notes_start_from_the_last_tag_of_any_kind() {
        let tags = [
            "v0.1.0",
            "v0.2.0-alpha.1",
            "v0.2.0-alpha.2",
            "v0.2.0-beta.1",
            "v0.2.0",
        ];
        let prev = |tag: &str| previous_tag(Version::parse_tag(tag).unwrap(), tags);
        assert_eq!(prev("v0.2.0-alpha.1"), Some("v0.1.0"));
        assert_eq!(prev("v0.2.0-alpha.2"), Some("v0.2.0-alpha.1"));
        assert_eq!(prev("v0.2.0-beta.1"), Some("v0.2.0-alpha.2"));
        assert_eq!(prev("v0.3.0-alpha.1"), Some("v0.2.0"));
        assert_eq!(
            previous_tag(Version::parse("0.1.0-alpha.1").unwrap(), tags),
            None
        );
    }

    #[test]
    fn next_raises_the_numbers() {
        let next = |s: &str, level, pre| v(s).next(level, pre).unwrap().to_string();
        assert_eq!(next("0.1.0", Level::Major, None), "1.0.0");
        assert_eq!(next("0.1.0", Level::Minor, None), "0.2.0");
        assert_eq!(next("0.1.0", Level::Patch, None), "0.1.1");
        assert_eq!(next("1.2.3", Level::Minor, None), "1.3.0");
        assert_eq!(
            next("0.1.0", Level::Minor, Some(Channel::Alpha)),
            "0.2.0-alpha.1"
        );
        assert_eq!(
            next("0.1.0", Level::Major, Some(Channel::Beta)),
            "1.0.0-beta.1"
        );
        // From a pre-release the numbers still go up; `release` ends it instead.
        assert_eq!(next("0.2.0-beta.1", Level::Patch, None), "0.2.1");
        assert_eq!(
            next("0.2.0-beta.1", Level::Minor, Some(Channel::Alpha)),
            "0.3.0-alpha.1"
        );
    }

    #[test]
    fn next_continues_and_ends_pre_releases() {
        let next = |s: &str, level| v(s).next(level, None).unwrap().to_string();
        assert_eq!(next("0.2.0-alpha.1", Level::Alpha), "0.2.0-alpha.2");
        assert_eq!(next("0.2.0-alpha.9", Level::Alpha), "0.2.0-alpha.10");
        assert_eq!(next("0.2.0-alpha.3", Level::Beta), "0.2.0-beta.1");
        assert_eq!(next("0.2.0-beta.1", Level::Beta), "0.2.0-beta.2");
        assert_eq!(next("0.2.0-alpha.2", Level::Release), "0.2.0");
        assert_eq!(next("0.2.0-beta.2", Level::Release), "0.2.0");
    }

    #[test]
    fn next_refuses_ambiguous_or_lower_results() {
        let err = |s: &str, level, pre| v(s).next(level, pre).unwrap_err();
        assert!(err("0.1.0", Level::Alpha, None).contains("ambiguous"));
        assert!(err("0.1.0", Level::Beta, None).contains("ambiguous"));
        assert!(err("0.2.0-beta.2", Level::Alpha, None).contains("already past alpha"));
        assert!(err("0.2.0", Level::Release, None).contains("already a release"));
        assert!(err("0.2.0-alpha.1", Level::Release, Some(Channel::Beta)).contains("--pre"));
        assert!(err("0.2.0-alpha.1", Level::Alpha, Some(Channel::Alpha)).contains("--pre"));
        let max = Version {
            major: u64::MAX,
            minor: 0,
            patch: 0,
            pre: None,
        };
        assert!(
            max.next(Level::Major, None)
                .unwrap_err()
                .contains("can't go any higher")
        );
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
