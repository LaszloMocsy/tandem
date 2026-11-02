//! Which build is running, so a local build can't pass for a release. The
//! environment is fixed at build time: `cargo tauri dev` is always dev, and
//! release builds read `TANDEM_ENV`: `beta` or `production`, which only the
//! release workflow sets, or unset for dev, so a local `cargo tauri build`
//! is a dev build too.

use tandem_board_core::db::UpdateChannel;

/// The build environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildEnv {
    Dev,
    Beta,
    Production,
}

impl BuildEnv {
    /// The environment this binary was built for.
    pub(crate) fn current() -> Self {
        Self::detect(tauri::is_dev(), option_env!("TANDEM_ENV"))
            .expect("build.rs only accepts known TANDEM_ENV values")
    }

    /// The environment for a dev run or a `TANDEM_ENV` value, or `None` for an
    /// unknown value. A dev run wins over the variable.
    pub(crate) fn detect(is_dev: bool, tandem_env: Option<&str>) -> Option<Self> {
        if is_dev {
            return Some(Self::Dev);
        }
        match tandem_env.unwrap_or("") {
            "" => Some(Self::Dev),
            "beta" => Some(Self::Beta),
            "production" => Some(Self::Production),
            _ => None,
        }
    }

    /// The channel this build updates from unless Settings picks another,
    /// or `None` for dev builds, which never update themselves.
    pub(crate) fn update_channel(self) -> Option<UpdateChannel> {
        match self {
            Self::Dev => None,
            Self::Beta => Some(UpdateChannel::Beta),
            Self::Production => Some(UpdateChannel::Stable),
        }
    }

    /// Whether this build has Labs: feature flags for unfinished work.
    /// Production builds have none, and keep every feature off whatever
    /// `app.db` says, so flags turned on in a beta never reach a release.
    pub(crate) fn has_labs(self) -> bool {
        self != Self::Production
    }

    /// The native window title, which also shows in the Dock and Cmd-Tab.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Dev => "Tandem DEV",
            Self::Beta => "Tandem BETA",
            Self::Production => "Tandem",
        }
    }
}

#[cfg(test)]
mod tests {
    use tandem_board_core::db::UpdateChannel;

    use super::BuildEnv;

    #[test]
    fn dev_runs_are_dev_whatever_the_variable_says() {
        assert_eq!(BuildEnv::detect(true, None), Some(BuildEnv::Dev));
        assert_eq!(BuildEnv::detect(true, Some("beta")), Some(BuildEnv::Dev));
    }

    #[test]
    fn release_builds_follow_the_variable() {
        assert_eq!(BuildEnv::detect(false, Some("beta")), Some(BuildEnv::Beta));
        assert_eq!(
            BuildEnv::detect(false, Some("production")),
            Some(BuildEnv::Production)
        );
    }

    #[test]
    fn local_release_builds_are_dev() {
        assert_eq!(BuildEnv::detect(false, None), Some(BuildEnv::Dev));
        assert_eq!(BuildEnv::detect(false, Some("")), Some(BuildEnv::Dev));
    }

    #[test]
    fn every_build_but_production_has_labs() {
        assert!(BuildEnv::Dev.has_labs());
        assert!(BuildEnv::Beta.has_labs());
        assert!(!BuildEnv::Production.has_labs());
    }

    #[test]
    fn unknown_values_are_refused() {
        assert_eq!(BuildEnv::detect(false, Some("rc")), None);
        assert_eq!(BuildEnv::detect(false, Some("alpha")), None);
        assert_eq!(BuildEnv::detect(false, Some("Beta")), None);
    }

    #[test]
    fn only_beta_and_production_builds_update() {
        assert_eq!(BuildEnv::Dev.update_channel(), None);
        assert_eq!(BuildEnv::Beta.update_channel(), Some(UpdateChannel::Beta));
        assert_eq!(
            BuildEnv::Production.update_channel(),
            Some(UpdateChannel::Stable)
        );
    }

    #[test]
    fn titles_name_the_environment_except_in_production() {
        assert_eq!(BuildEnv::Dev.title(), "Tandem DEV");
        assert_eq!(BuildEnv::Beta.title(), "Tandem BETA");
        assert_eq!(BuildEnv::Production.title(), "Tandem");
    }
}
