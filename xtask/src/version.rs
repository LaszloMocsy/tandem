//! App versions: `X.Y.Z` for a stable release, or `X.Y.Z-beta.N` for the Nth
//! beta of it, with no leading zeros. Release tags are the same with a
//! leading `v`.
//!
//! The version files only ever hold `X.Y.Z`. A beta's version exists only in
//! its tag and in the build made from it, since the MSI and RPM bundlers
//! refuse a version with a `-`.

use std::cmp::Ordering;
use std::fmt;

/// What `cargo xtask version --next` raises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Major,
    Minor,
    Patch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// The beta's number; `None` for a stable release.
    pub beta: Option<u64>,
}

impl Version {
    /// Parses `X.Y.Z` or `X.Y.Z-beta.N`.
    pub fn parse(s: &str) -> Result<Version, String> {
        let unsupported = || format!("unsupported version {s:?}: use X.Y.Z or X.Y.Z-beta.N");
        let (core, beta) = match s.split_once('-') {
            Some((core, pre)) => {
                let n = pre.strip_prefix("beta.").ok_or_else(unsupported)?;
                (core, Some(number(n).ok_or_else(unsupported)?))
            }
            None => (s, None),
        };
        let mut parts = core.split('.');
        let (Some(major), Some(minor), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(unsupported());
        };
        Ok(Version {
            major: number(major).ok_or_else(unsupported)?,
            minor: number(minor).ok_or_else(unsupported)?,
            patch: number(patch).ok_or_else(unsupported)?,
            beta,
        })
    }

    /// Parses a release tag: a version with a leading `v`.
    pub fn parse_tag(tag: &str) -> Result<Version, String> {
        tag.strip_prefix('v')
            .and_then(|v| Version::parse(v).ok())
            .ok_or_else(|| format!("unsupported tag {tag:?}: use vX.Y.Z or vX.Y.Z-beta.N"))
    }

    pub fn is_beta(&self) -> bool {
        self.beta.is_some()
    }

    /// The stable release this version is or leads to: `0.2.0-beta.3` → `0.2.0`.
    pub fn release(self) -> Version {
        Version { beta: None, ..self }
    }

    /// The `N`th beta of this release.
    pub fn with_beta(self, n: u64) -> Version {
        Version {
            beta: Some(n),
            ..self
        }
    }

    /// The build environment for this version's release: `beta` or
    /// `production`, as `TANDEM_ENV` takes it.
    pub fn build_env(&self) -> &'static str {
        if self.is_beta() { "beta" } else { "production" }
    }

    /// The lowest stable version above this one at `level`: `0.1.0` → `0.2.0`
    /// for minor. A beta moves to the release it leads to when that's at
    /// `level`, so `0.2.0-beta.1` → `0.2.0` for minor or patch, but `1.0.0`
    /// for major.
    pub fn next(self, level: Level) -> Result<Version, String> {
        let overflow = || format!("{self} can't go any higher");
        let up = |n: u64| n.checked_add(1).ok_or_else(overflow);
        let stable = |major, minor, patch| Version {
            major,
            minor,
            patch,
            beta: None,
        };
        let (major, minor, patch) = (self.major, self.minor, self.patch);
        let lands = self.is_beta();
        let next = match level {
            Level::Major if lands && minor == 0 && patch == 0 => self.release(),
            Level::Major => stable(up(major)?, 0, 0),
            Level::Minor if lands && patch == 0 => self.release(),
            Level::Minor => stable(major, up(minor)?, 0),
            Level::Patch if lands => self.release(),
            Level::Patch => stable(major, minor, up(patch)?),
        };
        debug_assert!(next > self, "{next} isn't higher than {self}");
        Ok(next)
    }

    /// Orders betas before their stable release, as SemVer precedence does
    /// for these forms.
    fn key(&self) -> (u64, u64, u64, bool, u64) {
        let (stable, n) = (self.beta.is_none(), self.beta.unwrap_or(0));
        (self.major, self.minor, self.patch, stable, n)
    }
}

/// The next beta of `release` given the existing `tags`: one past the
/// highest `v<release>-beta.N`, or the first. Fails if `release` is a beta
/// or is already released. Tags that aren't release tags are ignored.
pub fn next_beta<'a>(
    release: Version,
    tags: impl IntoIterator<Item = &'a str>,
) -> Result<Version, String> {
    if release.is_beta() {
        return Err(format!("{release} is a beta; betas lead to an X.Y.Z"));
    }
    let mut highest = 0;
    for tag in tags {
        let Ok(version) = Version::parse_tag(tag) else {
            continue;
        };
        match version.beta {
            None if version == release => {
                return Err(format!(
                    "v{release} is already released; raise the version for the next betas"
                ));
            }
            Some(n) if version.release() == release => highest = highest.max(n),
            _ => {}
        }
    }
    let n = highest
        .checked_add(1)
        .ok_or_else(|| format!("{release} has no beta numbers left"))?;
    Ok(release.with_beta(n))
}

/// The tag that release notes for `tag` start from: the highest earlier
/// stable tag for a stable release, so its notes cover every beta since the
/// last one, or the highest earlier tag of any kind for a beta. Tags that
/// aren't release tags are ignored.
pub fn previous_tag<'a>(tag: Version, tags: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    tags.into_iter()
        .filter_map(|t| Some((Version::parse_tag(t).ok()?, t)))
        .filter(|(v, _)| *v < tag && (tag.is_beta() || !v.is_beta()))
        .max_by_key(|(v, _)| *v)
        .map(|(_, t)| t)
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
        if let Some(n) = self.beta {
            write!(f, "-beta.{n}")?;
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
            "0.2.0-beta.1",
            "0.2.0-beta.12",
            "0.2.0-beta.0",
        ] {
            assert_eq!(v(s).to_string(), s);
        }
        assert!(!v("0.2.0").is_beta());
        assert!(v("0.2.0-beta.1").is_beta());
        assert_eq!(v("0.2.0-beta.3").release(), v("0.2.0"));
        assert_eq!(v("0.2.0").with_beta(2), v("0.2.0-beta.2"));
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
            "0.2.0-alpha.1",
            "0.2.0-beta",
            "0.2.0-beta.01",
            "0.2.0-beta.1.1",
            "0.2.0-BETA.1",
            "0.2.0-beta.-1",
            "0.2.0-beta.1-beta.1",
            "0.2.0+1",
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
        for tag in ["0.2.0", "v", "", "V0.2.0", "vv0.2.0", "v0.2.0-alpha.1"] {
            assert!(Version::parse_tag(tag).is_err(), "accepted {tag:?}");
        }
    }

    #[test]
    fn betas_build_as_beta_and_releases_as_production() {
        assert_eq!(v("0.2.0-beta.1").build_env(), "beta");
        assert_eq!(v("0.2.0").build_env(), "production");
    }

    #[test]
    fn the_next_beta_follows_the_highest_one_so_far() {
        let next = |release: &str, tags: &[&str]| {
            next_beta(v(release), tags.iter().copied()).map(|v| v.to_string())
        };
        assert_eq!(next("0.2.0", &[]).unwrap(), "0.2.0-beta.1");
        assert_eq!(
            next(
                "0.2.0",
                &["v0.1.0", "v0.2.0-beta.1", "v0.2.0-beta.3", "v0.3.0-beta.7"]
            )
            .unwrap(),
            "0.2.0-beta.4"
        );
        // A beta of another release, or a tag that isn't a release tag,
        // doesn't count.
        assert_eq!(
            next("0.2.0", &["v0.2.1-beta.5", "v0.2.0-alpha.2", "nightly"]).unwrap(),
            "0.2.0-beta.1"
        );
        // Today's v0.1.0-beta.1 carries on as the first beta of 0.1.0.
        assert_eq!(next("0.1.0", &["v0.1.0-beta.1"]).unwrap(), "0.1.0-beta.2");
    }

    #[test]
    fn there_are_no_betas_after_the_release() {
        let err = next_beta(v("0.2.0"), ["v0.2.0-beta.2", "v0.2.0"]).unwrap_err();
        assert!(err.contains("v0.2.0 is already released"), "{err}");
        let err = next_beta(v("0.2.0-beta.1"), []).unwrap_err();
        assert!(err.contains("is a beta"), "{err}");
    }

    #[test]
    fn stable_notes_start_from_the_last_stable_tag() {
        let tags = [
            "v0.1.0",
            "v0.2.0-beta.1",
            "v0.2.0",
            "v0.3.0-beta.1",
            "v0.3.0-beta.2",
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
    fn beta_notes_start_from_the_last_tag_of_any_kind() {
        let tags = ["v0.1.0", "v0.2.0-beta.1", "v0.2.0-beta.2", "v0.2.0"];
        let prev = |tag: &str| previous_tag(Version::parse_tag(tag).unwrap(), tags);
        assert_eq!(prev("v0.2.0-beta.1"), Some("v0.1.0"));
        assert_eq!(prev("v0.2.0-beta.2"), Some("v0.2.0-beta.1"));
        assert_eq!(prev("v0.2.0-beta.3"), Some("v0.2.0-beta.2"));
        assert_eq!(prev("v0.3.0-beta.1"), Some("v0.2.0"));
        assert_eq!(previous_tag(v("0.1.0-beta.1"), tags), None);
    }

    #[test]
    fn next_raises_the_numbers() {
        let next = |s: &str, level| v(s).next(level).unwrap().to_string();
        assert_eq!(next("0.1.0", Level::Major), "1.0.0");
        assert_eq!(next("0.1.0", Level::Minor), "0.2.0");
        assert_eq!(next("0.1.0", Level::Patch), "0.1.1");
        assert_eq!(next("1.2.3", Level::Minor), "1.3.0");
        assert_eq!(next("1.2.3", Level::Major), "2.0.0");
    }

    #[test]
    fn next_from_a_beta_lands_on_its_release_when_it_can() {
        let next = |s: &str, level| v(s).next(level).unwrap().to_string();
        assert_eq!(next("0.1.0-beta.1", Level::Patch), "0.1.0");
        assert_eq!(next("0.1.0-beta.1", Level::Minor), "0.1.0");
        assert_eq!(next("0.1.0-beta.1", Level::Major), "1.0.0");
        assert_eq!(next("0.2.3-beta.1", Level::Patch), "0.2.3");
        assert_eq!(next("0.2.3-beta.1", Level::Minor), "0.3.0");
        assert_eq!(next("1.0.0-beta.4", Level::Major), "1.0.0");
    }

    #[test]
    fn next_refuses_to_overflow() {
        let max = Version {
            major: u64::MAX,
            minor: 0,
            patch: 0,
            beta: None,
        };
        let err = max.next(Level::Major).unwrap_err();
        assert!(err.contains("can't go any higher"), "{err}");
    }

    #[test]
    fn orders_by_semver_precedence() {
        let ordered = [
            "0.1.9",
            "0.2.0-beta.0",
            "0.2.0-beta.1",
            "0.2.0-beta.2",
            "0.2.0-beta.10",
            "0.2.0",
            "0.2.1",
            "0.10.0",
            "1.0.0-beta.1",
            "1.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        assert_eq!(v("0.2.0").cmp(&v("0.2.0")), Ordering::Equal);
    }
}
