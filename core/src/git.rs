//! Commit review, the one git operation the app runs, and the read-only
//! checks the board needs, through the git CLI. Every command runs in the repo with `git -C`, and paths are pathspecs
//! relative to it.

use std::io;
use std::path::Path;
use std::process::{Command, Output};

/// Why a git command failed.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("could not run git; is it installed and on PATH?")]
    Spawn(#[source] io::Error),
    #[error("`git {command}` failed: {message}")]
    Failed { command: String, message: String },
}

/// Whether `folder` is inside a git work tree, as creating a board there
/// checks before warning you. `false` if git can't be run.
pub fn is_repo(folder: &Path) -> bool {
    git(folder, &["rev-parse", "--is-inside-work-tree"])
        .is_ok_and(|out| out.stdout.trim_ascii() == b"true")
}

/// Whether the latest commit has `path`, so git history keeps a copy of it
/// once the file is deleted. `false` outside a repo or before the first
/// commit.
pub fn is_committed(repo: &Path, path: &str) -> bool {
    // `./` makes the path relative to `repo`, not to the work tree's root.
    git(repo, &["cat-file", "-e", &format!("HEAD:./{path}")]).is_ok()
}

/// Whether `path` has changes git hasn't committed: edited, staged or not
/// yet tracked. Checks that one path with `git status --porcelain`.
pub fn has_changes(repo: &Path, path: &str) -> Result<bool, GitError> {
    let out = git(
        repo,
        &["status", "--porcelain", "--untracked-files=all", "--", path],
    )?;
    Ok(!out.stdout.is_empty())
}

/// Commits `path` alone: `git add`, then `git commit` with the file as
/// pathspec. Anything else that is staged stays staged and out of the
/// commit. Returns the new commit's hash.
pub fn commit_file(repo: &Path, path: &str, message: &str) -> Result<String, GitError> {
    git(repo, &["add", "--", path])?;
    git(
        repo,
        &["commit", "--quiet", "--message", message, "--", path],
    )?;
    let out = git(repo, &["rev-parse", "HEAD"])?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Runs `git -C repo <args>`, failing on a non-zero exit with git's message.
fn git(repo: &Path, args: &[&str]) -> Result<Output, GitError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        // Set inside git hooks; they would point the command at another
        // repo or index than `repo`'s.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_PREFIX")
        .output()
        .map_err(GitError::Spawn)?;
    if out.status.success() {
        return Ok(out);
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let message = [stderr.trim(), stdout.trim()]
        .into_iter()
        .find(|text| !text.is_empty())
        .map_or_else(|| out.status.to_string(), |text| text.replace('\n', " "));
    Err(GitError::Failed {
        command: args.first().copied().unwrap_or_default().to_owned(),
        message,
    })
}
