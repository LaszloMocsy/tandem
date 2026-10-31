//! The command line, parsed with clap.

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

use crate::version::Level;

/// Repo tooling for Tandem.
#[derive(Debug, Parser)]
#[command(name = "cargo xtask", bin_name = "cargo xtask")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Read, check and change the app version in Cargo.toml,
    /// desktop/tauri.conf.json and Cargo.lock.
    #[command(args_conflicts_with_subcommands = true)]
    Version(VersionArgs),
}

#[derive(Debug, Args)]
#[command(after_help = "\
Examples:
  cargo xtask version --next minor --create-branch   0.1.0 -> 0.2.0 on a new release/v0.2.0
  cargo xtask version --next patch --dry-run         0.2.0 -> 0.2.1, without writing anything
  cargo xtask version next-beta                      the tag the Beta workflow would build here
  cargo xtask version check pr                       what CI checks on this branch's pull request

The version files hold X.Y.Z only. Betas, X.Y.Z-beta.N, are built from a
release/vX.Y.Z branch by the Beta workflow, which numbers them.")]
#[command(group(ArgGroup::new("target").args(["next", "exact"])))]
pub struct VersionArgs {
    #[command(subcommand)]
    pub command: Option<VersionCommand>,

    #[command(flatten)]
    pub change: Change,
}

/// Flags that change the version.
#[derive(Debug, Args)]
#[group(skip)]
pub struct Change {
    /// Move to the next version at LEVEL
    #[arg(long, value_enum, value_name = "LEVEL")]
    pub next: Option<NextLevel>,

    /// Set this version: X.Y.Z
    #[arg(long, value_name = "VERSION")]
    pub exact: Option<String>,

    /// Lower the version with --exact without asking
    #[arg(long, requires = "exact", conflicts_with = "next")]
    pub yes: bool,

    /// Print the change without writing anything
    #[arg(long, requires = "target")]
    pub dry_run: bool,

    /// Cut release/v<new> from origin/main and change the version there
    #[arg(long, requires = "target")]
    pub create_branch: bool,
}

impl Change {
    /// Whether any flag asks for a change.
    pub fn requested(&self) -> bool {
        self.next.is_some() || self.exact.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum NextLevel {
    /// X.Y.Z -> X+1.0.0
    Major,
    /// X.Y.Z -> X.Y+1.0
    Minor,
    /// X.Y.Z -> X.Y.Z+1
    Patch,
}

impl From<NextLevel> for Level {
    fn from(level: NextLevel) -> Level {
        match level {
            NextLevel::Major => Level::Major,
            NextLevel::Minor => Level::Minor,
            NextLevel::Patch => Level::Patch,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum VersionCommand {
    /// Print the app version
    Current,
    /// Check that the version files agree, and more with a subcommand
    ///
    /// Prints the version when every check passes.
    #[command(args_conflicts_with_subcommands = true)]
    Check(CheckArgs),
    /// Print a release tag's build environment, as TANDEM_ENV takes it
    ///
    /// Prints beta for a beta tag, or production for a stable one.
    Env {
        /// A release tag, such as v0.2.0-beta.1
        tag: String,
    },
    /// Print the tag the release notes for TAG start from
    ///
    /// That's the last stable tag for a stable TAG, so its notes cover the
    /// betas, or the last tag of any kind for a beta. Prints nothing if
    /// there's none.
    PreviousTag {
        /// A release tag, such as v0.2.0
        tag: String,
    },
    /// Print the tag of the next beta built from this branch
    ///
    /// The branch must be release/vX.Y.Z with X.Y.Z in the version files,
    /// higher than main's version, and vX.Y.Z must not be released yet.
    /// Prints vX.Y.Z-beta.N, one past the highest beta of X.Y.Z tagged on
    /// origin. The branch is the one GitHub Actions runs for, or else the
    /// current one.
    NextBeta,
    /// Set the version files to TAG's version, for a release build
    ///
    /// The files must hold X.Y.Z for TAG vX.Y.Z or vX.Y.Z-beta.N. A beta
    /// tag moves them to X.Y.Z-beta.N, so the build carries the beta's
    /// version; a stable tag leaves them as they are. CI runs this in its
    /// checkout before building; don't commit the result.
    Stamp {
        /// A release tag, such as v0.2.0-beta.1
        tag: String,
    },
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[command(subcommand)]
    pub command: Option<CheckCommand>,

    /// Also check that the version is higher than on REV: a branch, tag or
    /// commit
    #[arg(long, value_name = "REV")]
    pub against: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum CheckCommand {
    /// Check a pull request's version, as CI does
    ///
    /// Into main, it must come from release/v<version>, with a version higher
    /// than main's; once merged, only the branch name counts. Into a
    /// release/vX.Y.Z branch, it must keep that branch's version. Without
    /// NUMBER, checks the pull request GitHub Actions runs for, or else this
    /// branch's, found with gh.
    Pr {
        /// The pull request's number
        number: Option<u64>,
    },
    /// Check that a release tag names the version, or a beta of it
    ///
    /// Checks TAG, or else the tag GitHub Actions runs for, or else the
    /// release tag on HEAD. Prints the tag's version.
    Release {
        /// A release tag, such as v0.2.0
        tag: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(["cargo xtask"].iter().chain(args))
    }

    fn version(args: &[&str]) -> VersionArgs {
        let mut all = vec!["version"];
        all.extend(args);
        let Command::Version(version) = parse(&all).unwrap().command;
        version
    }

    #[test]
    fn the_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_changes() {
        let change = version(&["--next", "minor", "--dry-run"]).change;
        assert_eq!(change.next, Some(NextLevel::Minor));
        assert!(change.dry_run && !change.create_branch);

        let change = version(&["--exact", "0.2.0", "--yes", "--create-branch"]).change;
        assert_eq!(change.exact.as_deref(), Some("0.2.0"));
        assert!(change.yes && change.create_branch);

        assert!(!version(&[]).change.requested());
        assert!(version(&[]).command.is_none());
    }

    #[test]
    fn parses_commands() {
        let check = |args: &[&str]| match version(args).command {
            Some(VersionCommand::Check(check)) => check,
            other => panic!("{other:?}"),
        };
        assert!(check(&["check"]).command.is_none());
        assert_eq!(
            check(&["check", "--against", "v0.1.0"]).against.as_deref(),
            Some("v0.1.0")
        );
        assert!(matches!(
            check(&["check", "pr", "12"]).command,
            Some(CheckCommand::Pr { number: Some(12) })
        ));
        assert!(matches!(
            check(&["check", "pr"]).command,
            Some(CheckCommand::Pr { number: None })
        ));
        assert!(matches!(
            check(&["check", "release", "v0.2.0"]).command,
            Some(CheckCommand::Release { tag: Some(_) })
        ));
        assert!(matches!(
            version(&["current"]).command,
            Some(VersionCommand::Current)
        ));
        assert!(matches!(
            version(&["previous-tag", "v0.2.0"]).command,
            Some(VersionCommand::PreviousTag { .. })
        ));
        assert!(matches!(
            version(&["next-beta"]).command,
            Some(VersionCommand::NextBeta)
        ));
        assert!(matches!(
            version(&["stamp", "v0.2.0-beta.1"]).command,
            Some(VersionCommand::Stamp { .. })
        ));
    }

    #[test]
    fn refuses_conflicting_or_incomplete_options() {
        for args in [
            &["version", "--next", "minor", "--exact", "0.2.0"][..],
            &["version", "--next", "minor", "--pre", "alpha"],
            &["version", "--next", "beta"],
            &["version", "--next", "release"],
            &["version", "next-beta", "release/v0.2.0"],
            &["version", "stamp"],
            &["version", "--next", "minor", "--yes"],
            &["version", "--dry-run"],
            &["version", "--create-branch"],
            &["version", "--next", "path"],
            &["version", "--next", "minor", "check"],
            &["version", "check", "pr", "--against", "main"],
            &["version", "check", "pr", "twelve"],
            &["version", "bump", "0.2.0"],
            &["version", "check", "--base", "main"],
            &["version", "check", "--branch", "release/v0.2.0"],
            &["version", "check", "--tag", "v0.2.0"],
        ] {
            assert!(parse(args).is_err(), "accepted {args:?}");
        }
    }
}
