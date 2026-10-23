//! The git facts the app needs, through the git CLI: whether a folder is in
//! a repo, and the repo's common dir, which identifies a project across all
//! its worktrees. The app never writes to a repo.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Why a git command failed.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("could not run git; is it installed and on PATH?")]
    Spawn(#[source] io::Error),
    #[error("`git {command}` failed: {message}")]
    Failed { command: String, message: String },
}

/// Whether `folder` is inside a git work tree. `false` if git can't be run.
pub fn is_repo(folder: &Path) -> bool {
    git(folder, &["rev-parse", "--is-inside-work-tree"])
        .is_ok_and(|out| out.stdout.trim_ascii() == b"true")
}

/// The git common dir of the repo `folder` is in, canonicalized: the same
/// for the main checkout and every linked worktree, such as `/code/app/.git`.
/// `None` outside a repo, or if git can't be run.
pub fn common_dir(folder: &Path) -> Option<PathBuf> {
    let out = git(
        folder,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    let path = Path::new(path.trim_end_matches(['\n', '\r']));
    path.canonicalize().ok()
}

/// `git config user.name` in `folder`, if set.
pub fn user_name(folder: &Path) -> Option<String> {
    let out = git(folder, &["config", "user.name"]).ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// Runs `git -C folder <args>`, failing on a non-zero exit with git's
/// message.
fn git(folder: &Path, args: &[&str]) -> Result<Output, GitError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(args)
        // Set inside git hooks; they would point the command at another
        // repo than `folder`'s.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_PREFIX")
        .env_remove("GIT_COMMON_DIR")
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
