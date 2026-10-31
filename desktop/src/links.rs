//! The web pages the app links to. The UI names a link and Rust builds its
//! URL, so the window can't open arbitrary URLs.

use serde::Deserialize;

use crate::build_env::BuildEnv;

const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

/// The landing page, which also serves the update feeds.
pub(crate) const SITE: &str = "https://tandem.laszlomocsy.com";

/// A page the UI can open in the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Link {
    /// The GitHub repository.
    Repository,
    /// The release notes for the running version. A dev build's version may
    /// not be released yet, so it gets the list of releases instead.
    ReleaseNotes,
    /// Every release, where dev builds find newer ones.
    Releases,
    /// The landing page's downloads, for installs that can't update
    /// themselves.
    Download,
}

impl Link {
    pub(crate) fn url(self, env: BuildEnv, version: &str) -> String {
        match (self, env) {
            (Self::Repository, _) => REPOSITORY.to_owned(),
            (Self::ReleaseNotes, BuildEnv::Dev) => format!("{REPOSITORY}/releases"),
            (Self::ReleaseNotes, _) => format!("{REPOSITORY}/releases/tag/v{version}"),
            (Self::Releases, _) => format!("{REPOSITORY}/releases"),
            (Self::Download, _) => format!("{SITE}/#download"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repository_is_the_projects_github_page() {
        assert_eq!(REPOSITORY, "https://github.com/LaszloMocsy/tandem");
        for env in [BuildEnv::Dev, BuildEnv::Beta, BuildEnv::Production] {
            assert_eq!(Link::Repository.url(env, "0.2.0"), REPOSITORY);
        }
    }

    #[test]
    fn releases_lists_every_release() {
        assert_eq!(
            Link::Releases.url(BuildEnv::Dev, "0.2.0"),
            "https://github.com/LaszloMocsy/tandem/releases"
        );
    }

    #[test]
    fn download_is_the_landing_pages_downloads() {
        assert_eq!(
            Link::Download.url(BuildEnv::Production, "0.2.0"),
            "https://tandem.laszlomocsy.com/#download"
        );
    }

    #[test]
    fn release_notes_name_the_running_version() {
        let notes = |env| Link::ReleaseNotes.url(env, "0.2.0-beta.1");
        assert_eq!(
            notes(BuildEnv::Beta),
            "https://github.com/LaszloMocsy/tandem/releases/tag/v0.2.0-beta.1"
        );
        assert_eq!(
            notes(BuildEnv::Production),
            "https://github.com/LaszloMocsy/tandem/releases/tag/v0.2.0-beta.1"
        );
        assert_eq!(
            notes(BuildEnv::Dev),
            "https://github.com/LaszloMocsy/tandem/releases"
        );
    }

    #[test]
    fn the_ui_names_links_in_camel_case() {
        let link = |name: &str| serde_json::from_value::<Link>(serde_json::json!(name));
        assert_eq!(link("repository").unwrap(), Link::Repository);
        assert_eq!(link("releaseNotes").unwrap(), Link::ReleaseNotes);
        assert!(link("https://example.com").is_err());
    }
}
