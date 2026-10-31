//! Repo tooling, run as `cargo xtask <command>` from anywhere in the repo.
//! `cargo xtask version` prints what it can do.

mod change;
mod check;
mod cli;
mod files;
mod repo;
mod version;

use std::env;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};

use crate::check::PullRequest;
use crate::cli::{CheckArgs, CheckCommand, Cli, Command, VersionCommand};
use crate::repo::Repo;
use crate::version::Version;

fn main() -> ExitCode {
    let Command::Version(args) = Cli::parse().command;
    let repo = Repo::here();
    let result = match args.command {
        None if args.change.requested() => change::run(&repo, &args.change),
        None => print_version_help(),
        Some(VersionCommand::Current) => repo.version(None).map(|v| println!("{v}")),
        Some(VersionCommand::Check(check)) => run_check(&repo, check),
        Some(VersionCommand::Env { tag }) => {
            Version::parse_tag(&tag).map(|tag| println!("{}", tag.build_env()))
        }
        Some(VersionCommand::PreviousTag { tag }) => previous_tag(&repo, &tag),
        Some(VersionCommand::NextBeta) => next_beta(&repo),
        Some(VersionCommand::Stamp { tag }) => stamp(&repo, &tag),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn print_version_help() -> Result<(), String> {
    let mut cli = Cli::command();
    cli.build();
    let version = cli
        .find_subcommand_mut("version")
        .expect("the version command exists");
    version.print_help().map_err(|e| e.to_string())
}

/// Runs a check and prints the version it checked, for scripts to use.
fn run_check(repo: &Repo, args: CheckArgs) -> Result<(), String> {
    let current = match args.command {
        None => {
            let current = repo.version(None)?;
            if let Some(rev) = &args.against {
                check::higher(current, repo.cargo_version_at(rev)?, rev)?;
            }
            current
        }
        Some(CheckCommand::Pr { number }) => check_pr(repo, number)?,
        Some(CheckCommand::Release { tag }) => {
            let current = repo.version(None)?;
            let tag = match tag.or_else(actions_tag) {
                Some(tag) => tag,
                None => {
                    let tags = repo.git_stdout(&["tag", "--points-at", "HEAD", "--list", "v*"])?;
                    check::release_tag_on_head(current, tags.lines())?.to_owned()
                }
            };
            check::release(current, &tag)?
        }
    };
    println!("{current}");
    Ok(())
}

/// Checks pull request `number`, or else the one GitHub Actions runs for, or
/// else the current branch's, and returns the version on its head.
fn check_pr(repo: &Repo, number: Option<u64>) -> Result<Version, String> {
    let (pr, head) = match number {
        Some(number) => {
            let pr = gh_pull_request(repo, Some(number))?;
            let head = repo.fetch(&format!("pull/{number}/head"))?;
            (pr, Some(head))
        }
        None => match actions_pull_request()? {
            Some(pr) => (pr, None),
            None => (gh_pull_request(repo, None)?, None),
        },
    };
    let version = repo.version(head.as_deref())?;
    let base = if pr.needs_base_version() {
        // A full ref, so a branch name can't pass for a git option.
        let base = repo.fetch(&format!("refs/heads/{}", pr.base))?;
        Some(repo.cargo_version_at(&base)?)
    } else {
        None
    };
    pr.check(version, base)?;
    eprintln!(
        "{} -> {}{}: version {version} is fine",
        pr.head,
        pr.base,
        if pr.merged { " (merged)" } else { "" }
    );
    Ok(version)
}

/// The pull request GitHub Actions runs for, from its environment.
fn actions_pull_request() -> Result<Option<PullRequest>, String> {
    let event = env::var("GITHUB_EVENT_NAME").unwrap_or_default();
    if env::var("GITHUB_ACTIONS").as_deref() != Ok("true") || !event.starts_with("pull_request") {
        return Ok(None);
    }
    let var = |name: &str| env::var(name).map_err(|_| format!("{name} isn't set"));
    let payload = std::fs::read_to_string(var("GITHUB_EVENT_PATH")?)
        .map_err(|e| format!("can't read the event payload: {e}"))?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| format!("the event payload isn't JSON: {e}"))?;
    Ok(Some(PullRequest {
        head: var("GITHUB_HEAD_REF")?,
        base: var("GITHUB_BASE_REF")?,
        merged: payload["pull_request"]["merged"].as_bool().unwrap_or(false),
    }))
}

/// Pull request `number`, or the current branch's, through gh.
fn gh_pull_request(repo: &Repo, number: Option<u64>) -> Result<PullRequest, String> {
    let number = number.map(|n| n.to_string());
    let mut args = vec!["pr", "view"];
    args.extend(number.as_deref());
    args.extend(["--json", "headRefName,baseRefName,state"]);
    let json = repo.gh(&args).map_err(|e| match &number {
        Some(_) => e,
        None => format!("{e}\nopen a pull request for this branch, or pass its number"),
    })?;
    let pr: serde_json::Value =
        serde_json::from_str(&json).map_err(|e| format!("gh returned bad JSON: {e}"))?;
    let field = |name: &str| {
        pr[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("gh didn't return {name}"))
    };
    Ok(PullRequest {
        head: field("headRefName")?,
        base: field("baseRefName")?,
        merged: field("state")? == "MERGED",
    })
}

/// The tag GitHub Actions runs for, if it runs for one.
fn actions_tag() -> Option<String> {
    actions_ref("tag")
}

/// The name of the ref GitHub Actions runs for, if it's of `kind`: `tag` or
/// `branch`.
fn actions_ref(kind: &str) -> Option<String> {
    let actions = env::var("GITHUB_ACTIONS").as_deref() == Ok("true");
    let matches = env::var("GITHUB_REF_TYPE").as_deref() == Ok(kind);
    if actions && matches {
        env::var("GITHUB_REF_NAME").ok()
    } else {
        None
    }
}

/// Prints the tag of the next beta from this release branch.
fn next_beta(repo: &Repo) -> Result<(), String> {
    let branch = match actions_ref("branch") {
        Some(branch) => branch,
        None => repo.git_stdout(&["branch", "--show-current"])?,
    };
    let current = repo.version(None)?;
    let main = repo.fetch("refs/heads/main")?;
    let release = check::beta_branch(&branch, current, repo.cargo_version_at(&main)?)?;
    let tags = repo.remote_tags(&format!("v{release}*"))?;
    let beta = version::next_beta(release, tags.iter().map(String::as_str))?;
    eprintln!("{branch}: the next beta of {release} is {beta}");
    println!("v{beta}");
    Ok(())
}

/// Sets the version files to `tag`'s version for a release build.
fn stamp(repo: &Repo, tag: &str) -> Result<(), String> {
    let current = repo.version(None)?;
    let version = check::release(current, tag)?;
    if version == current {
        eprintln!("The version files already hold {current}.");
        return Ok(());
    }
    repo.version_files_clean()?;
    change::write(repo, current, version)?;
    eprintln!("Set {current} -> {version} for this build; don't commit it.");
    Ok(())
}

fn previous_tag(repo: &Repo, tag: &str) -> Result<(), String> {
    let tag = Version::parse_tag(tag)?;
    let tags = repo.git_stdout(&["tag", "--list", "v*"])?;
    println!("{}", version::previous_tag(tag, tags.lines()).unwrap_or(""));
    Ok(())
}
