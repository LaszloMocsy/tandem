//! The rules of `cargo xtask version check`, apart from the git and GitHub
//! lookups in main.rs, so they're testable on their own.

use crate::version::Version;

/// The prefix of the branches that may merge into main.
const RELEASE_BRANCH: &str = "release/";

/// A pull request, as far as the version rules care.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub head: String,
    pub base: String,
    pub merged: bool,
}

/// What a pull request merges into, which decides its rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Main,
    ReleaseBranch,
    Other,
}

impl PullRequest {
    fn target(&self) -> Target {
        if self.base == "main" {
            Target::Main
        } else if release_branch(&self.base).is_some() {
            Target::ReleaseBranch
        } else {
            Target::Other
        }
    }

    /// Whether the rules compare against the base branch's version. A merged
    /// pull request's base already holds its version, so it isn't compared.
    pub fn needs_base_version(&self) -> bool {
        !self.merged && self.target() != Target::Other
    }

    /// Checks the rules for this pull request, given the version on its head
    /// and, when [`needs_base_version`](Self::needs_base_version), on its base:
    ///
    /// - into `main`: the head is `release/v<version>`, and the version is
    ///   higher than main's;
    /// - into a `release/vX.Y.Z` branch: the version is the same as the
    ///   branch's, since feature branches don't change it;
    /// - into anything else: nothing beyond the files agreeing.
    pub fn check(&self, head: Version, base: Option<Version>) -> Result<(), String> {
        match self.target() {
            Target::Main => {
                branch(head, &self.head)?;
                if let Some(base) = base.filter(|_| !self.merged) {
                    higher(head, base, &self.base)?;
                }
            }
            Target::ReleaseBranch => {
                if let Some(base) = base.filter(|_| !self.merged)
                    && head != base
                {
                    return Err(format!(
                        "this branch has version {head}, but {} has {base}; feature branches \
                         don't change the version, so set it back with \
                         `cargo xtask version --exact {base} --yes`",
                        self.base
                    ));
                }
            }
            Target::Other => {}
        }
        Ok(())
    }
}

/// The version a `release/vX.Y.Z` branch names, if it is one. Betas get no
/// branch of their own, so `release/vX.Y.Z-beta.N` isn't one.
fn release_branch(branch: &str) -> Option<Version> {
    Version::parse_tag(branch.strip_prefix(RELEASE_BRANCH)?)
        .ok()
        .filter(|v| !v.is_beta())
}

/// A pull request into main must come from `release/v<version>`.
fn branch(current: Version, branch: &str) -> Result<(), String> {
    let named = release_branch(branch).ok_or_else(|| {
        format!(
            "pull requests into main come from a release/vX.Y.Z branch, not {branch:?}; \
             merge feature branches into a release branch instead"
        )
    })?;
    if named != current {
        return Err(format!(
            "branch {branch} names version {named}, but the app version is {current}; \
             run `cargo xtask version --exact {named}`"
        ));
    }
    Ok(())
}

/// Whether release tag `tag` fits the app version `current`: a stable tag
/// names it, and a beta tag is a beta of it, since the version files hold
/// the release a beta leads to. A beta in the files (from before betas were
/// built from release branches) still fits its own tag.
fn fits(current: Version, tag: Version) -> bool {
    tag == current || tag.release() == current
}

/// A release tag must name the app version, or be a beta of it.
pub fn release(current: Version, tag: &str) -> Result<Version, String> {
    let version = Version::parse_tag(tag)?;
    if !fits(current, version) {
        return Err(format!(
            "tag {tag} doesn't match the app version {current}: it should be v{current} or \
             v{current}-beta.N"
        ));
    }
    Ok(version)
}

/// The release tag among `tags` (those on HEAD): the one that fits
/// `current`, or else the only release tag there.
pub fn release_tag_on_head<'a>(
    current: Version,
    tags: impl IntoIterator<Item = &'a str>,
) -> Result<&'a str, String> {
    let tags: Vec<(&str, Version)> = tags
        .into_iter()
        .filter_map(|t| Some((t, Version::parse_tag(t).ok()?)))
        .collect();
    let matching: Vec<&str> = tags
        .iter()
        .filter(|(_, v)| fits(current, *v))
        .map(|(t, _)| *t)
        .collect();
    let all: Vec<&str> = tags.iter().map(|(t, _)| *t).collect();
    match (matching.as_slice(), all.as_slice()) {
        ([tag], _) | ([], [tag]) => Ok(tag),
        ([], []) => Err("HEAD has no release tag; pass the tag to check".to_owned()),
        (_, _) => Err(format!(
            "HEAD has several release tags ({}); pass the one to check",
            all.join(", ")
        )),
    }
}

/// A beta is built from a `release/vX.Y.Z` branch whose version files hold
/// `X.Y.Z`, higher than `main`'s version. Returns `X.Y.Z`, the release the
/// beta leads to.
pub fn beta_branch(branch: &str, current: Version, main: Version) -> Result<Version, String> {
    let named = release_branch(branch).ok_or_else(|| {
        format!(
            "betas are built from a release/vX.Y.Z branch, not {branch:?}; run the Beta \
             workflow on the release branch"
        )
    })?;
    if named != current {
        return Err(format!(
            "branch {branch} names version {named}, but the app version is {current}; \
             run `cargo xtask version --exact {named}` on it"
        ));
    }
    higher(current, main, "main")?;
    Ok(named)
}

/// The version must be higher than `base`, the version on `rev`.
pub fn higher(current: Version, base: Version, rev: &str) -> Result<(), String> {
    if current <= base {
        return Err(format!(
            "the app version {current} isn't higher than {base} on {rev}; \
             run `cargo xtask version --next <level>`"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn pr(head: &str, base: &str, merged: bool) -> PullRequest {
        PullRequest {
            head: head.to_owned(),
            base: base.to_owned(),
            merged,
        }
    }

    #[test]
    fn into_main_needs_a_matching_version_branch_and_a_higher_version() {
        let into_main = pr("release/v0.2.0", "main", false);
        assert!(into_main.needs_base_version());
        assert!(into_main.check(v("0.2.0"), Some(v("0.1.0"))).is_ok());

        let not_higher = pr("release/v0.1.0", "main", false);
        let err = not_higher.check(v("0.1.0"), Some(v("0.1.0"))).unwrap_err();
        assert!(err.contains("isn't higher than 0.1.0 on main"), "{err}");
        let err = into_main.check(v("0.3.0"), Some(v("0.1.0"))).unwrap_err();
        assert!(
            err.contains("names version 0.2.0, but the app version is 0.3.0"),
            "{err}"
        );

        for head in [
            "feat/x",
            "release/0.2.0",
            "release/v0.2",
            "release/v0.2.0-beta.1",
            "versions/v0.2.0",
            "main",
        ] {
            let err = pr(head, "main", false)
                .check(v("0.2.0"), Some(v("0.1.0")))
                .unwrap_err();
            assert!(err.contains("release/vX.Y.Z branch"), "{head}: {err}");
        }
    }

    #[test]
    fn a_merged_pull_request_into_main_only_checks_the_name() {
        let merged = pr("release/v0.2.0", "main", true);
        assert!(!merged.needs_base_version());
        assert!(merged.check(v("0.2.0"), None).is_ok());
        assert!(merged.check(v("0.2.0"), Some(v("0.2.0"))).is_ok());
        assert!(merged.check(v("0.3.0"), None).is_err());
        assert!(pr("feat/x", "main", true).check(v("0.2.0"), None).is_err());
    }

    #[test]
    fn into_a_version_branch_keeps_its_version() {
        let feature = pr("feat/x", "release/v0.2.0", false);
        assert!(feature.needs_base_version());
        assert!(feature.check(v("0.2.0"), Some(v("0.2.0"))).is_ok());
        let err = feature.check(v("0.3.0"), Some(v("0.2.0"))).unwrap_err();
        assert!(
            err.contains("feature branches don't change the version"),
            "{err}"
        );
        assert!(err.contains("--exact 0.2.0 --yes"), "{err}");
        assert!(!pr("feat/x", "release/v0.2.0", true).needs_base_version());
    }

    #[test]
    fn into_anything_else_only_needs_the_files_to_agree() {
        for base in ["feat/parent", "release/next", "release"] {
            let other = pr("feat/x", base, false);
            assert!(!other.needs_base_version(), "{base}");
            assert!(other.check(v("9.9.9"), None).is_ok(), "{base}");
        }
    }

    #[test]
    fn release_tags_must_name_the_version_or_a_beta_of_it() {
        assert_eq!(release(v("0.2.0"), "v0.2.0"), Ok(v("0.2.0")));
        assert_eq!(release(v("0.2.0"), "v0.2.0-beta.3"), Ok(v("0.2.0-beta.3")));
        // A beta still in the files, as on main before this flow, fits its tag.
        assert!(release(v("0.1.0-beta.1"), "v0.1.0-beta.1").is_ok());
        for tag in ["v0.2.1", "v0.2.1-beta.1", "v0.1.0", "0.2.0"] {
            assert!(release(v("0.2.0"), tag).is_err(), "accepted {tag}");
        }
        let err = release(v("0.2.0"), "v0.3.0").unwrap_err();
        assert!(err.contains("v0.2.0 or v0.2.0-beta.N"), "{err}");
    }

    #[test]
    fn finds_the_release_tag_on_head() {
        assert_eq!(release_tag_on_head(v("0.2.0"), ["v0.2.0"]), Ok("v0.2.0"));
        assert_eq!(
            release_tag_on_head(v("0.2.0"), ["latest", "v0.1.0-beta.1", "v0.2.0"]),
            Ok("v0.2.0")
        );
        assert_eq!(
            release_tag_on_head(v("0.2.0"), ["v0.2.0-beta.2", "latest"]),
            Ok("v0.2.0-beta.2")
        );
        // The only release tag is returned, so the check can say it's wrong.
        assert_eq!(
            release_tag_on_head(v("0.2.0"), ["v0.3.0", "latest"]),
            Ok("v0.3.0")
        );
        assert!(
            release_tag_on_head(v("0.2.0"), ["latest"])
                .unwrap_err()
                .contains("no release tag")
        );
        for tags in [
            &["v0.3.0", "v0.4.0"][..],
            &["v0.2.0-beta.1", "v0.2.0-beta.2"],
        ] {
            assert!(
                release_tag_on_head(v("0.2.0"), tags.iter().copied())
                    .unwrap_err()
                    .contains("several release tags"),
                "{tags:?}"
            );
        }
    }

    #[test]
    fn betas_come_from_a_release_branch_above_main() {
        assert_eq!(
            beta_branch("release/v0.2.0", v("0.2.0"), v("0.1.0")),
            Ok(v("0.2.0"))
        );
        // Today's main holds a beta, which the release it led to is above.
        assert_eq!(
            beta_branch("release/v0.1.0", v("0.1.0"), v("0.1.0-beta.1")),
            Ok(v("0.1.0"))
        );
        for branch in ["main", "feat/x", "release/v0.2.0-beta.1", "release/0.2.0"] {
            let err = beta_branch(branch, v("0.2.0"), v("0.1.0")).unwrap_err();
            assert!(err.contains("release/vX.Y.Z branch"), "{branch}: {err}");
        }
        let err = beta_branch("release/v0.2.0", v("0.3.0"), v("0.1.0")).unwrap_err();
        assert!(err.contains("names version 0.2.0"), "{err}");
        let err = beta_branch("release/v0.2.0", v("0.2.0"), v("0.2.0")).unwrap_err();
        assert!(err.contains("isn't higher than 0.2.0 on main"), "{err}");
    }

    #[test]
    fn against_needs_a_higher_version() {
        assert!(higher(v("0.2.0"), v("0.1.0"), "v0.1.0").is_ok());
        assert!(higher(v("0.1.0"), v("0.1.0-beta.1"), "main").is_ok());
        let err = higher(v("0.1.0"), v("0.1.0"), "origin/main").unwrap_err();
        assert!(
            err.contains("0.1.0 isn't higher than 0.1.0 on origin/main"),
            "{err}"
        );
    }
}
