//! The options and rules of `cargo xtask version check`, apart from the
//! file reading in main.rs, so they're testable on their own.

use crate::version::Version;

/// The prefix of the branches that may merge into main.
const VERSION_BRANCH: &str = "version/";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options<'a> {
    /// A release tag that must name the app version.
    pub tag: Option<&'a str>,
    /// A git ref whose version the app version must be higher than.
    pub base: Option<&'a str>,
    /// A branch that must be `version/v<app version>`.
    pub branch: Option<&'a str>,
}

impl<'a> Options<'a> {
    pub fn parse(args: &[&'a str]) -> Result<Options<'a>, String> {
        let mut options = Options::default();
        let mut args = args.iter();
        while let Some(&flag) = args.next() {
            let slot = match flag {
                "--tag" => &mut options.tag,
                "--base" => &mut options.base,
                "--branch" => &mut options.branch,
                _ => return Err(format!("unknown option {flag}")),
            };
            let Some(&value) = args.next() else {
                return Err(format!("{flag} needs a value"));
            };
            if slot.replace(value).is_some() {
                return Err(format!("{flag} given twice"));
            }
        }
        Ok(options)
    }
}

pub fn tag(current: Version, tag: &str) -> Result<(), String> {
    if Version::parse_tag(tag)? != current {
        return Err(format!("tag {tag} doesn't match the app version {current}"));
    }
    Ok(())
}

pub fn branch(current: Version, branch: &str) -> Result<(), String> {
    let named = branch
        .strip_prefix(VERSION_BRANCH)
        .and_then(|tag| Version::parse_tag(tag).ok())
        .ok_or_else(|| {
            format!(
                "pull requests into main come from a version/vX.Y.Z branch, not {branch:?}; \
                 merge feature branches into a version branch instead"
            )
        })?;
    if named != current {
        return Err(format!(
            "branch {branch} names version {named}, but the app version is {current}; \
             run `cargo xtask version bump {named}`"
        ));
    }
    Ok(())
}

pub fn higher(current: Version, base: Version, base_ref: &str) -> Result<(), String> {
    if current <= base {
        return Err(format!(
            "the app version {current} isn't higher than {base} on {base_ref}; \
             run `cargo xtask version bump <version>`"
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

    #[test]
    fn parses_options_in_any_order() {
        assert_eq!(Options::parse(&[]).unwrap(), Options::default());
        assert_eq!(
            Options::parse(&["--branch", "version/v0.2.0", "--base", "origin/main"]).unwrap(),
            Options {
                tag: None,
                base: Some("origin/main"),
                branch: Some("version/v0.2.0")
            }
        );
        assert_eq!(
            Options::parse(&["--tag", "v0.2.0"]).unwrap().tag,
            Some("v0.2.0")
        );
    }

    #[test]
    fn rejects_bad_options() {
        assert!(Options::parse(&["--base"]).is_err());
        assert!(Options::parse(&["--base", "a", "--base", "b"]).is_err());
        assert!(Options::parse(&["--head", "x"]).is_err());
        assert!(Options::parse(&["origin/main"]).is_err());
    }

    #[test]
    fn tag_must_name_the_app_version() {
        assert!(tag(v("0.2.0"), "v0.2.0").is_ok());
        assert!(tag(v("0.2.0"), "v0.2.0-beta.1").is_err());
        assert!(tag(v("0.2.0"), "0.2.0").is_err());
    }

    #[test]
    fn branch_must_be_the_version_branch() {
        assert!(branch(v("0.2.0"), "version/v0.2.0").is_ok());
        assert!(branch(v("0.2.0-alpha.1"), "version/v0.2.0-alpha.1").is_ok());

        for name in [
            "feat/x",
            "main",
            "version/0.2.0",
            "version/v0.2",
            "versions/v0.2.0",
            "",
        ] {
            let err = branch(v("0.2.0"), name).unwrap_err();
            assert!(err.contains("version/vX.Y.Z branch"), "{name}: {err}");
        }
        let err = branch(v("0.2.0"), "version/v0.3.0").unwrap_err();
        assert!(
            err.contains("names version 0.3.0, but the app version is 0.2.0"),
            "{err}"
        );
    }

    #[test]
    fn version_must_be_higher_than_the_base() {
        assert!(higher(v("0.2.0"), v("0.1.0"), "main").is_ok());
        assert!(higher(v("0.2.0-alpha.1"), v("0.1.0"), "main").is_ok());
        assert!(higher(v("0.2.0"), v("0.2.0-beta.3"), "main").is_ok());

        let err = higher(v("0.1.0"), v("0.1.0"), "origin/main").unwrap_err();
        assert!(
            err.contains("0.1.0 isn't higher than 0.1.0 on origin/main"),
            "{err}"
        );
        assert!(higher(v("0.2.0-beta.1"), v("0.2.0"), "main").is_err());
    }
}
