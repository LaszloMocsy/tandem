//! Repo tooling, run as `cargo xtask <command>` from anywhere in the repo.

mod check;
mod files;
mod version;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs};

use version::Version;

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  version                  print the app version
  version check [--tag T] [--base REF] [--branch B]
                           check that Cargo.toml, desktop/tauri.conf.json and
                           Cargo.lock agree; with --tag, that T is vVERSION;
                           with --base, that VERSION is higher than on git ref
                           REF; with --branch, that B is version/vVERSION
  version bump VERSION     set the version in all three; VERSION is X.Y.Z,
                           X.Y.Z-alpha.N or X.Y.Z-beta.N and must be higher
  version env TAG          print the build environment for a release tag:
                           alpha, beta, or nothing for production";

/// The files that hold the app version, relative to the repo root.
const CARGO_TOML: &str = "Cargo.toml";
const TAURI_CONF: &str = "desktop/tauri.conf.json";
const CARGO_LOCK: &str = "Cargo.lock";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[&str]) -> Result<(), String> {
    match args {
        ["version"] => println!("{}", current_version(&root())?),
        ["version", "check", options @ ..] => {
            let options = check::Options::parse(options)?;
            let root = root();
            let current = current_version(&root)?;
            if let Some(tag) = options.tag {
                check::tag(current, tag)?;
            }
            if let Some(branch) = options.branch {
                check::branch(current, branch)?;
            }
            if let Some(base) = options.base {
                check::higher(current, version_at(&root, base)?, base)?;
            }
            println!("{current}");
        }
        ["version", "bump", new] => bump(&root(), new)?,
        ["version", "env", tag] => {
            let channel = Version::parse_tag(tag)?.channel();
            println!("{}", channel.map_or("", |c| c.name()));
        }
        ["help" | "--help" | "-h"] => println!("{USAGE}"),
        _ => return Err(format!("unknown command\n\n{USAGE}")),
    }
    Ok(())
}

/// The repo root: the workspace this crate belongs to.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the repo root")
        .to_owned()
}

fn read(root: &Path, file: &str) -> Result<String, String> {
    fs::read_to_string(root.join(file)).map_err(|e| format!("can't read {file}: {e}"))
}

/// The app version, after checking that every file holds the same one.
fn current_version(root: &Path) -> Result<Version, String> {
    let cargo = files::cargo_version(&read(root, CARGO_TOML)?)?;
    let tauri = files::tauri_version(&read(root, TAURI_CONF)?)?;
    let mut found = vec![
        (CARGO_TOML.to_owned(), cargo.clone()),
        (TAURI_CONF.to_owned(), tauri),
    ];
    for (name, version) in files::lock_versions(&read(root, CARGO_LOCK)?, env!("CARGO_PKG_NAME"))? {
        found.push((format!("{CARGO_LOCK} ({name})"), version));
    }
    if found.iter().any(|(_, v)| *v != cargo) {
        let list: Vec<_> = found.iter().map(|(f, v)| format!("  {f}: {v}")).collect();
        return Err(format!("the version files disagree:\n{}", list.join("\n")));
    }
    Version::parse(&cargo).map_err(|e| format!("{CARGO_TOML}: {e}"))
}

/// The app version on git ref `rev`, from its Cargo.toml.
fn version_at(root: &Path, rev: &str) -> Result<Version, String> {
    let show = git(root, &["show", &format!("{rev}:{CARGO_TOML}")])?;
    if !show.status.success() {
        let stderr = String::from_utf8_lossy(&show.stderr);
        return Err(format!(
            "can't read {CARGO_TOML} on {rev}: {}",
            stderr.trim()
        ));
    }
    let cargo_toml =
        String::from_utf8(show.stdout).map_err(|_| format!("{CARGO_TOML} on {rev} isn't UTF-8"))?;
    Version::parse(&files::cargo_version(&cargo_toml)?)
        .map_err(|e| format!("{CARGO_TOML} on {rev}: {e}"))
}

fn bump(root: &Path, new: &str) -> Result<(), String> {
    let new = Version::parse(new.strip_prefix('v').unwrap_or(new))?;
    let current = current_version(root)?;
    if new <= current {
        return Err(format!("{new} is not higher than the current {current}"));
    }

    // A clean start keeps the bump's diff to the version lines.
    let dirty = git(
        root,
        &[
            "diff", "--quiet", "HEAD", "--", CARGO_TOML, CARGO_LOCK, TAURI_CONF,
        ],
    )?;
    if !dirty.status.success() {
        return Err(format!(
            "{CARGO_TOML}, {CARGO_LOCK} or {TAURI_CONF} has uncommitted changes"
        ));
    }

    let (old, new_str) = (current.to_string(), new.to_string());
    let cargo_toml = files::set_cargo_version(&read(root, CARGO_TOML)?, &new_str)?;
    let tauri_conf = files::set_tauri_version(&read(root, TAURI_CONF)?, &old, &new_str)?;
    write(root, CARGO_TOML, &cargo_toml)?;
    write(root, TAURI_CONF, &tauri_conf)?;

    // Rewrites only the workspace crates' entries; dependencies stay as they are.
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .args([
            "update",
            "--workspace",
            "--offline",
            "--quiet",
            "--manifest-path",
        ])
        .arg(root.join(CARGO_TOML))
        .status()
        .map_err(|e| format!("can't run cargo: {e}"))?;
    if !status.success() {
        return Err(format!(
            "cargo update --workspace failed; {CARGO_TOML} and {TAURI_CONF} are already bumped"
        ));
    }
    current_version(root)?;

    println!("Bumped {current} -> {new} in {CARGO_TOML}, {TAURI_CONF} and {CARGO_LOCK}.");
    let branch = git(root, &["branch", "--show-current"])?;
    let branch = String::from_utf8_lossy(&branch.stdout);
    let expected = format!("version/v{new}");
    if branch.trim() != expected {
        println!("Note: you're on '{}', not '{expected}'.", branch.trim());
    }
    println!();
    println!("Next:");
    println!("  1. Commit the bump on {expected}.");
    println!("  2. Merge feature branches into {expected}.");
    println!("  3. Open a pull request from {expected} into main.");
    println!("  4. Merging it tags the merge commit v{new} and starts the release.");
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("can't run git: {e}"))
}

/// Writes through a temp file and a rename, so a failed write leaves the file
/// as it was.
fn write(root: &Path, file: &str, contents: &str) -> Result<(), String> {
    let path = root.join(file);
    let tmp = path.with_extension("xtask-tmp");
    fs::write(&tmp, contents)
        .and_then(|()| fs::rename(&tmp, &path))
        .map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("can't write {file}: {e}")
        })
}
