//! `cargo xtask version --next` and `--exact`: changing the app version.

use std::io::{self, BufRead, IsTerminal, Write};

use crate::cli::Change;
use crate::files;
use crate::repo::{CARGO_LOCK, CARGO_TOML, Repo, TAURI_CONF, VERSION_FILES};
use crate::version::Version;

pub fn run(repo: &Repo, change: &Change) -> Result<(), String> {
    // With --create-branch the new branch starts from main, so main's version
    // is the one to move from.
    let main = if change.create_branch {
        Some(repo.fetch("main")?)
    } else {
        None
    };
    let current = repo.version(main.as_deref())?;
    let new = target(current, change)?;
    let branch = format!("version/v{new}");

    if change.create_branch {
        let exists = repo
            .git(&[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ])?
            .status
            .success();
        let on_origin = repo
            .git(&["ls-remote", "--exit-code", "--heads", "origin", &branch])?
            .status
            .success();
        if exists || on_origin {
            return Err(format!(
                "branch {branch} already exists; switch to it instead"
            ));
        }
        let changes = repo.git_stdout(&["status", "--porcelain", "--untracked-files=no"])?;
        if !changes.is_empty() {
            return Err(
                "the working tree has uncommitted changes; commit or stash them first".to_owned(),
            );
        }
    } else {
        repo.version_files_clean()?;
    }

    if change.dry_run {
        println!("{current} -> {new}");
        if change.create_branch {
            println!("Would create {branch} from origin/main.");
        }
        println!("Would change {}.", VERSION_FILES.join(", "));
        return Ok(());
    }

    if let Some(main) = &main {
        repo.git_ok(&["switch", "--quiet", "--create", &branch, main])?;
        println!("Created {branch} from origin/main.");
    }
    write(repo, current, new)?;
    println!("Changed {current} -> {new} in {CARGO_TOML}, {TAURI_CONF} and {CARGO_LOCK}.");

    let on = repo.git_stdout(&["branch", "--show-current"])?;
    if on != branch {
        println!("Note: you're on '{on}', not '{branch}'.");
    }
    println!();
    println!("Next:");
    println!("  1. Commit the change on {branch}.");
    println!("  2. Merge feature branches into {branch}.");
    println!("  3. Open a pull request from {branch} into main.");
    println!("  4. Merging it tags the merge commit v{new} and starts the release.");
    Ok(())
}

/// The version to move to, after asking before a lower one.
fn target(current: Version, change: &Change) -> Result<Version, String> {
    if let Some(level) = change.next {
        return current.next(level.into(), change.pre.map(Into::into));
    }
    let exact = change
        .exact
        .as_deref()
        .expect("clap requires --next or --exact");
    let new = Version::parse(exact.strip_prefix('v').unwrap_or(exact))?;
    if new == current {
        return Err(format!("the version is already {current}"));
    }
    if new < current && !change.yes && !change.dry_run {
        if !io::stdin().is_terminal() {
            return Err(format!(
                "{new} is lower than the current {current}; pass --yes to lower it"
            ));
        }
        if !confirm(&format!(
            "{new} is lower than the current {current}. Lower it?"
        ))? {
            return Err("left the version as it was".to_owned());
        }
    }
    Ok(new)
}

fn confirm(question: &str) -> Result<bool, String> {
    print!("{question} [y/N] ");
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

/// Changes all three version files or none: if any step fails, the files
/// go back to how they were, so a later run still finds them agreeing.
fn write(repo: &Repo, current: Version, new: Version) -> Result<(), String> {
    let (old, new) = (current.to_string(), new.to_string());
    let originals = [
        (CARGO_TOML, repo.read(CARGO_TOML, None)?),
        (TAURI_CONF, repo.read(TAURI_CONF, None)?),
        (CARGO_LOCK, repo.read(CARGO_LOCK, None)?),
    ];
    let cargo_toml = files::set_cargo_version(&originals[0].1, &new)?;
    let tauri_conf = files::set_tauri_version(&originals[1].1, &old, &new)?;

    let result = repo
        .write(CARGO_TOML, &cargo_toml)
        .and_then(|()| repo.write(TAURI_CONF, &tauri_conf))
        .and_then(|()| repo.update_lock())
        .and_then(|()| repo.version(None).map(drop));
    let Err(err) = result else {
        return Ok(());
    };
    let failed: Vec<_> = originals
        .iter()
        .filter_map(|(file, contents)| repo.write(file, contents).err())
        .collect();
    if failed.is_empty() {
        Err(format!(
            "{err}\nLeft {} as they were.",
            VERSION_FILES.join(", ")
        ))
    } else {
        Err(format!(
            "{err}\nCouldn't restore the version files:\n  {}",
            failed.join("\n  ")
        ))
    }
}
