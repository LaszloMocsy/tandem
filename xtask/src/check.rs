//! The rules of `cargo xtask version check`, apart from the git and GitHub
//! lookups in main.rs, so they're testable on their own.

use crate::version::Version;

/// The prefix of the branches that may merge into main.
const VERSION_BRANCH: &str = "version/";

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
    VersionBranch,
    Other,
}

impl PullRequest {
    fn target(&self) -> Target {
        if self.base == "main" {
            Target::Main
        } else if version_branch(&self.base).is_some() {
            Target::VersionBranch
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
    /// - into `main`: the head is `version/v<version>`, and the version is
    ///   higher than main's;
    /// - into a `version/vX.Y.Z` branch: the version is the same as the
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
            Target::VersionBranch => {
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

/// The version a `version/vX.Y.Z` branch names, if it is one.
fn version_branch(branch: &str) -> Option<Version> {
    Version::parse_tag(branch.strip_prefix(VERSION_BRANCH)?).ok()
}

/// A pull request into main must come from `version/v<version>`.
fn branch(current: Version, branch: &str) -> Result<(), String> {
    let named = version_branch(branch).ok_or_else(|| {
        format!(
            "pull requests into main come from a version/vX.Y.Z branch, not {branch:?}; \
             merge feature branches into a version branch instead"
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

/// A release tag must name the app version.
pub fn release(current: Version, tag: &str) -> Result<(), String> {
    if Version::parse_tag(tag)? != current {
        return Err(format!("tag {tag} doesn't match the app version {current}"));
    }
    Ok(())
}

/// The release tag among `tags` (those on HEAD): the one naming `current`, or
/// else the only release tag there.
pub fn release_tag_on_head<'a>(
    current: Version,
    tags: impl IntoIterator<Item = &'a str>,
) -> Result<&'a str, String> {
    let tags: Vec<&str> = tags
        .into_iter()
        .filter(|t| Version::parse_tag(t).is_ok())
        .collect();
    let matching = tags
        .iter()
        .find(|t| Version::parse_tag(t).ok() == Some(current));
    match (matching, tags.as_slice()) {
        (Some(tag), _) => Ok(tag),
        (None, [tag]) => Ok(tag),
        (None, []) => Err("HEAD has no vX.Y.Z tag; pass the tag to check".to_owned()),
        (None, _) => Err(format!(
            "HEAD has several release tags ({}); pass the one to check",
            tags.join(", ")
        )),
    }
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
        let into_main = pr("version/v0.2.0", "main", false);
        assert!(into_main.needs_base_version());
        assert!(into_main.check(v("0.2.0"), Some(v("0.1.0"))).is_ok());

        let not_higher = pr("version/v0.1.0", "main", false);
        let err = not_higher.check(v("0.1.0"), Some(v("0.1.0"))).unwrap_err();
        assert!(err.contains("isn't higher than 0.1.0 on main"), "{err}");
        let err = into_main.check(v("0.3.0"), Some(v("0.1.0"))).unwrap_err();
        assert!(
            err.contains("names version 0.2.0, but the app version is 0.3.0"),
            "{err}"
        );

        for head in [
            "feat/x",
            "version/0.2.0",
            "version/v0.2",
            "versions/v0.2.0",
            "main",
        ] {
            let err = pr(head, "main", false)
                .check(v("0.2.0"), Some(v("0.1.0")))
                .unwrap_err();
            assert!(err.contains("version/vX.Y.Z branch"), "{head}: {err}");
        }
    }

    #[test]
    fn a_merged_pull_request_into_main_only_checks_the_name() {
        let merged = pr("version/v0.2.0", "main", true);
        assert!(!merged.needs_base_version());
        assert!(merged.check(v("0.2.0"), None).is_ok());
        assert!(merged.check(v("0.2.0"), Some(v("0.2.0"))).is_ok());
        assert!(merged.check(v("0.3.0"), None).is_err());
        assert!(pr("feat/x", "main", true).check(v("0.2.0"), None).is_err());
    }

    #[test]
    fn into_a_version_branch_keeps_its_version() {
        let feature = pr("feat/x", "version/v0.2.0", false);
        assert!(feature.needs_base_version());
        assert!(feature.check(v("0.2.0"), Some(v("0.2.0"))).is_ok());
        let err = feature.check(v("0.3.0"), Some(v("0.2.0"))).unwrap_err();
        assert!(
            err.contains("feature branches don't change the version"),
            "{err}"
        );
        assert!(err.contains("--exact 0.2.0 --yes"), "{err}");
        assert!(!pr("feat/x", "version/v0.2.0", true).needs_base_version());
    }

    #[test]
    fn into_anything_else_only_needs_the_files_to_agree() {
        for base in ["feat/parent", "version/next", "release"] {
            let other = pr("feat/x", base, false);
            assert!(!other.needs_base_version(), "{base}");
            assert!(other.check(v("9.9.9"), None).is_ok(), "{base}");
        }
    }

    #[test]
    fn release_tags_must_name_the_version() {
        assert!(release(v("0.2.0"), "v0.2.0").is_ok());
        assert!(release(v("0.2.0"), "v0.2.0-beta.1").is_err());
        assert!(release(v("0.2.0"), "0.2.0").is_err());
    }

    #[test]
    fn finds_the_release_tag_on_head() {
        assert_eq!(release_tag_on_head(v("0.2.0"), ["v0.2.0"]), Ok("v0.2.0"));
        assert_eq!(
            release_tag_on_head(v("0.2.0"), ["latest", "v0.2.0-beta.1", "v0.2.0"]),
            Ok("v0.2.0")
        );
        // The only release tag is returned, so the check can say it's wrong.
        assert_eq!(
            release_tag_on_head(v("0.2.0"), ["v0.3.0", "latest"]),
            Ok("v0.3.0")
        );
        assert!(
            release_tag_on_head(v("0.2.0"), ["latest"])
                .unwrap_err()
                .contains("no vX.Y.Z tag")
        );
        assert!(
            release_tag_on_head(v("0.2.0"), ["v0.3.0", "v0.4.0"])
                .unwrap_err()
                .contains("several release tags")
        );
    }

    #[test]
    fn against_needs_a_higher_version() {
        assert!(higher(v("0.2.0"), v("0.1.0"), "v0.1.0").is_ok());
        assert!(higher(v("0.2.0-alpha.1"), v("0.1.0"), "main").is_ok());
        let err = higher(v("0.1.0"), v("0.1.0"), "origin/main").unwrap_err();
        assert!(
            err.contains("0.1.0 isn't higher than 0.1.0 on origin/main"),
            "{err}"
        );
    }
}
