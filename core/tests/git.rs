use std::fs;
use std::path::Path;
use std::process::Command;

use tandem_board_core::git::{self, GitError};
use tandem_board_core::service::{Actor, Config, Finish, NewTask, Service};
use tandem_board_core::store::TaskId;
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

/// A repo with one commit of `README.md` and `other.txt`, an identity, and
/// no signing or hooks from your own git config.
fn repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let repo = dir.path();
    run(repo, &["init", "--quiet"]);
    let hooks = repo.join(".git").join("no-hooks");
    for (key, value) in [
        ("user.name", "Sam"),
        ("user.email", "sam@example.com"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", hooks.to_str().unwrap()),
    ] {
        run(repo, &["config", key, value]);
    }
    fs::write(repo.join("README.md"), "readme\n").unwrap();
    fs::write(repo.join("other.txt"), "one\n").unwrap();
    run(repo, &["add", "."]);
    run(repo, &["commit", "--quiet", "-m", "Initial"]);
    fs::create_dir_all(repo.join(".tandem")).unwrap();
    dir
}

const TASK: &str = ".tandem/k3f9x2m.md";

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

#[test]
fn is_repo_is_true_anywhere_inside_a_work_tree() {
    let repo = repo();
    assert!(git::is_repo(repo.path()));
    let sub = repo.path().join("sub");
    fs::create_dir(&sub).unwrap();
    assert!(git::is_repo(&sub));
    assert!(!git::is_repo(&repo.path().join(".git")));

    let plain = TempDir::new().unwrap();
    assert!(!git::is_repo(plain.path()));
    assert!(!git::is_repo(&plain.path().join("missing")));
}

#[test]
fn has_changes_sees_new_edited_and_staged_files() {
    let dir = repo();
    let repo = dir.path();
    assert!(!git::has_changes(repo, TASK).unwrap(), "missing file");

    fs::write(repo.join(TASK), "v1\n").unwrap();
    assert!(git::has_changes(repo, TASK).unwrap(), "untracked");
    run(repo, &["add", TASK]);
    assert!(git::has_changes(repo, TASK).unwrap(), "staged");
    run(repo, &["commit", "--quiet", "-m", "Add task"]);
    assert!(!git::has_changes(repo, TASK).unwrap(), "committed");

    fs::write(repo.join(TASK), "v2\n").unwrap();
    assert!(git::has_changes(repo, TASK).unwrap(), "edited");
    // Other changed files don't count.
    fs::write(repo.join(TASK), "v1\n").unwrap();
    fs::write(repo.join("other.txt"), "two\n").unwrap();
    assert!(!git::has_changes(repo, TASK).unwrap(), "other file edited");
}

#[test]
fn commit_file_commits_only_that_file_and_leaves_the_rest_staged() {
    let dir = repo();
    let repo = dir.path();
    // Staged: an edit and a new file. Unstaged: another edit.
    fs::write(repo.join("other.txt"), "two\n").unwrap();
    fs::write(repo.join("new.txt"), "new\n").unwrap();
    run(repo, &["add", "other.txt", "new.txt"]);
    fs::write(repo.join("README.md"), "edited\n").unwrap();
    // The task file is staged in an older version, then edited again.
    fs::write(repo.join(TASK), "v1\n").unwrap();
    run(repo, &["add", TASK]);
    fs::write(repo.join(TASK), "v2\n").unwrap();

    let message = "Review k3f9x2m: Fix token refresh race\n\nTask: k3f9x2m";
    let hash = git::commit_file(repo, TASK, message).unwrap();

    assert_eq!(hash, run(repo, &["rev-parse", "HEAD"]).trim());
    assert_eq!(
        lines(&run(repo, &["show", "--name-only", "--format=", "HEAD"])),
        [TASK]
    );
    assert_eq!(run(repo, &["show", &format!("HEAD:{TASK}")]), "v2\n");
    assert_eq!(run(repo, &["log", "-1", "--format=%B"]).trim_end(), message);
    assert_eq!(
        run(
            repo,
            &["log", "-1", "--format=%(trailers:key=Task,valueonly)"]
        )
        .trim(),
        "k3f9x2m"
    );
    // Everything else is as it was.
    assert_eq!(
        lines(&run(repo, &["diff", "--cached", "--name-only"])),
        ["new.txt", "other.txt"]
    );
    assert_eq!(lines(&run(repo, &["diff", "--name-only"])), ["README.md"]);
    assert_eq!(run(repo, &["show", ":other.txt"]), "two\n");
    assert!(!git::has_changes(repo, TASK).unwrap());
}

#[test]
fn failures_carry_gits_message() {
    let dir = TempDir::new().unwrap();
    let err = git::has_changes(dir.path(), TASK).unwrap_err();
    assert!(
        matches!(&err, GitError::Failed { command, message }
            if command == "status" && message.contains("not a git repository")),
        "{err}"
    );
}

#[test]
fn commit_review_commits_a_done_tasks_file_alone() {
    let dir = repo();
    let repo = dir.path();
    let data = TempDir::new().unwrap();
    let config = Config {
        user_name: "sam".into(),
        ..Config::default()
    };
    let service = Service::open(data.path(), "test", repo, config).unwrap();
    let user = Actor::User;
    let id: TaskId = service
        .create_task(
            &user,
            NewTask {
                title: "Fix  token\trefresh race".into(),
                ..NewTask::default()
            },
        )
        .unwrap()
        .id;
    let path = format!(".tandem/{id}.md");
    service.start_task(&user, id).unwrap();
    service
        .finish_task(
            &user,
            id,
            Finish {
                summary: "Done.".into(),
                ..Finish::default()
            },
        )
        .unwrap();
    assert_eq!(
        service.commit_review(id).unwrap_err().code(),
        "invalid_transition"
    );

    service.mark_reviewed(id).unwrap();
    assert!(service.has_uncommitted_changes(id).unwrap());
    fs::write(repo.join("other.txt"), "two\n").unwrap();
    run(repo, &["add", "other.txt"]);

    service.commit_review(id).unwrap();
    assert_eq!(
        run(repo, &["log", "-1", "--format=%B"]).trim_end(),
        format!("Review {id}: Fix token refresh race\n\nTask: {id}")
    );
    assert_eq!(
        lines(&run(repo, &["show", "--name-only", "--format=", "HEAD"])),
        [path.as_str()]
    );
    assert_eq!(
        run(repo, &["show", &format!("HEAD:{path}")]),
        fs::read_to_string(repo.join(&path)).unwrap()
    );
    assert_eq!(
        lines(&run(repo, &["diff", "--cached", "--name-only"])),
        ["other.txt"]
    );
    assert!(!service.has_uncommitted_changes(id).unwrap());

    // With nothing left to commit, git refuses.
    assert_eq!(service.commit_review(id).unwrap_err().code(), "git_failed");
}
