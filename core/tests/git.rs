use std::fs;
use std::path::Path;
use std::process::Command;

use tandem_board_core::db::{AppDb, folder_name};
use tandem_board_core::git;
use tempfile::TempDir;

/// Runs git in `repo`, asserting it succeeds, and returns its output.
fn run(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A folder holding a repo `app` with one commit, an identity, and no
/// signing or hooks from your own git config.
fn repo() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().unwrap();
    let repo = dir.path().join("app");
    fs::create_dir(&repo).unwrap();
    run(&repo, &["init", "--quiet"]);
    let hooks = repo.join(".git").join("no-hooks");
    for (key, value) in [
        ("user.name", "Sam"),
        ("user.email", "sam@example.com"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", hooks.to_str().unwrap()),
    ] {
        run(&repo, &["config", key, value]);
    }
    fs::write(repo.join("README.md"), "readme\n").unwrap();
    run(&repo, &["add", "."]);
    run(&repo, &["commit", "--quiet", "-m", "Initial"]);
    (dir, repo)
}

#[test]
fn is_repo_is_true_anywhere_inside_a_work_tree() {
    let (dir, repo) = repo();
    let sub = repo.join("src");
    fs::create_dir(&sub).unwrap();
    assert!(git::is_repo(&repo));
    assert!(git::is_repo(&sub));
    let outside = dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    assert!(!git::is_repo(&outside));
}

#[test]
fn every_worktree_of_a_repo_has_the_same_common_dir() {
    let (dir, repo) = repo();
    let worktree = dir.path().join("app-feature");
    run(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    );
    let sub = worktree.join("docs");
    fs::create_dir(&sub).unwrap();

    let common = git::common_dir(&repo).unwrap();
    assert_eq!(common, repo.join(".git").canonicalize().unwrap());
    assert_eq!(git::common_dir(&worktree), Some(common.clone()));
    assert_eq!(git::common_dir(&sub), Some(common.clone()));
    assert_eq!(folder_name(&common), "app");

    let outside = dir.path().join("notes");
    fs::create_dir(&outside).unwrap();
    assert_eq!(git::common_dir(&outside), None);
}

#[test]
fn two_worktrees_open_the_same_project() {
    let (dir, repo) = repo();
    let worktree = dir.path().join("app-feature");
    run(
        &repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature",
            worktree.to_str().unwrap(),
        ],
    );
    let data = TempDir::new().unwrap();
    let mut app = AppDb::open(data.path()).unwrap();
    let common = git::common_dir(&repo).unwrap();

    let first = app
        .add_project(&common, &repo, &folder_name(&common))
        .unwrap();
    let common = git::common_dir(&worktree).unwrap();
    let second = app
        .add_project(&common, &worktree, &folder_name(&common))
        .unwrap();
    assert_eq!(first.id, "app");
    assert_eq!(second.id, first.id);
    assert_eq!(second.last_path, worktree.canonicalize().unwrap());
    assert_eq!(app.projects().unwrap().len(), 1);
}

#[test]
fn the_user_name_comes_from_git_config() {
    let (dir, repo) = repo();
    assert_eq!(git::user_name(&repo).as_deref(), Some("Sam"));
    run(&repo, &["config", "user.name", ""]);
    // An empty name counts as none, whatever the global config says.
    assert_eq!(git::user_name(&repo), None);
    drop(dir);
}
