//! Which build is running, so a dev build can't pass for a pre-release. The
//! environment is fixed at build time: `cargo tauri dev` is always dev, and
//! release builds read `TANDEM_ENV` (`alpha`, `beta`, or unset for production).

/// The build environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildEnv {
    Dev,
    Alpha,
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
            "" => Some(Self::Production),
            "alpha" => Some(Self::Alpha),
            "beta" => Some(Self::Beta),
            _ => None,
        }
    }

    /// The native window title, which also shows in the Dock and Cmd-Tab.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Dev => "Tandem DEV",
            Self::Alpha => "Tandem ALPHA",
            Self::Beta => "Tandem BETA",
            Self::Production => "Tandem",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BuildEnv;

    #[test]
    fn dev_runs_are_dev_whatever_the_variable_says() {
        assert_eq!(BuildEnv::detect(true, None), Some(BuildEnv::Dev));
        assert_eq!(BuildEnv::detect(true, Some("beta")), Some(BuildEnv::Dev));
    }

    #[test]
    fn release_builds_follow_the_variable() {
        assert_eq!(
            BuildEnv::detect(false, Some("alpha")),
            Some(BuildEnv::Alpha)
        );
        assert_eq!(BuildEnv::detect(false, Some("beta")), Some(BuildEnv::Beta));
        assert_eq!(BuildEnv::detect(false, None), Some(BuildEnv::Production));
        assert_eq!(
            BuildEnv::detect(false, Some("")),
            Some(BuildEnv::Production)
        );
    }

    #[test]
    fn unknown_values_are_refused() {
        assert_eq!(BuildEnv::detect(false, Some("rc")), None);
        assert_eq!(BuildEnv::detect(false, Some("Alpha")), None);
        assert_eq!(BuildEnv::detect(false, Some("production")), None);
    }

    #[test]
    fn titles_name_the_environment_except_in_production() {
        assert_eq!(BuildEnv::Dev.title(), "Tandem DEV");
        assert_eq!(BuildEnv::Alpha.title(), "Tandem ALPHA");
        assert_eq!(BuildEnv::Beta.title(), "Tandem BETA");
        assert_eq!(BuildEnv::Production.title(), "Tandem");
    }
}
