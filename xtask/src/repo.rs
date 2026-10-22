//! The repo: reading and writing the version files, and running git and gh.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs};

use crate::files;
use crate::version::Version;

/// The files that hold the app version, relative to the repo root.
pub const CARGO_TOML: &str = "Cargo.toml";
pub const TAURI_CONF: &str = "desktop/tauri.conf.json";
pub const CARGO_LOCK: &str = "Cargo.lock";
pub const VERSION_FILES: [&str; 3] = [CARGO_TOML, TAURI_CONF, CARGO_LOCK];

pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    /// The repo this crate lives in.
    pub fn here() -> Repo {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask lives in the repo root")
            .to_owned();
        Repo { root }
    }

    /// `file` in the working tree, or on git rev `rev`.
    pub fn read(&self, file: &str, rev: Option<&str>) -> Result<String, String> {
        let Some(rev) = rev else {
            return fs::read_to_string(self.root.join(file))
                .map_err(|e| format!("can't read {file}: {e}"));
        };
        let show = self.git(&["show", &format!("{rev}:{file}")])?;
        if !show.status.success() {
            let stderr = String::from_utf8_lossy(&show.stderr);
            return Err(format!("can't read {file} on {rev}: {}", stderr.trim()));
        }
        String::from_utf8(show.stdout).map_err(|_| format!("{file} on {rev} isn't UTF-8"))
    }

    /// The app version in the working tree, or on git rev `rev`, after
    /// checking that every version file holds the same one.
    pub fn version(&self, rev: Option<&str>) -> Result<Version, String> {
        let cargo = files::cargo_version(&self.read(CARGO_TOML, rev)?)?;
        let tauri = files::tauri_version(&self.read(TAURI_CONF, rev)?)?;
        let lock = self.read(CARGO_LOCK, rev)?;
        let mut found = vec![
            (CARGO_TOML.to_owned(), cargo.clone()),
            (TAURI_CONF.to_owned(), tauri),
        ];
        for (name, version) in files::lock_versions(&lock, env!("CARGO_PKG_NAME"))? {
            found.push((format!("{CARGO_LOCK} ({name})"), version));
        }
        if found.iter().any(|(_, v)| *v != cargo) {
            let at = rev.map_or(String::new(), |rev| format!(" on {rev}"));
            let list: Vec<_> = found.iter().map(|(f, v)| format!("  {f}: {v}")).collect();
            return Err(format!(
                "the version files disagree{at}:\n{}",
                list.join("\n")
            ));
        }
        Version::parse(&cargo).map_err(|e| format!("{CARGO_TOML}: {e}"))
    }

    /// The version in `Cargo.toml` on git rev `rev`, without checking the
    /// other files.
    pub fn cargo_version_at(&self, rev: &str) -> Result<Version, String> {
        Version::parse(&files::cargo_version(&self.read(CARGO_TOML, Some(rev))?)?)
            .map_err(|e| format!("{CARGO_TOML} on {rev}: {e}"))
    }

    /// Fetches `refspec` from origin and returns the commit it points at. A
    /// shallow clone, such as a CI checkout, stays shallow.
    pub fn fetch(&self, refspec: &str) -> Result<String, String> {
        let shallow = self.git_stdout(&["rev-parse", "--is-shallow-repository"])? == "true";
        let mut args = vec!["fetch", "--quiet", "--no-tags"];
        if shallow {
            args.push("--depth=1");
        }
        args.extend(["origin", refspec]);
        self.git_ok(&args)?;
        self.git_stdout(&["rev-parse", "FETCH_HEAD"])
    }

    /// Fails unless the version files match HEAD, so a change touches only
    /// the version lines.
    pub fn version_files_clean(&self) -> Result<(), String> {
        let mut args = vec!["diff", "--quiet", "HEAD", "--"];
        args.extend(VERSION_FILES);
        if !self.git(&args)?.status.success() {
            return Err(format!(
                "{} has uncommitted changes; commit or discard them first",
                VERSION_FILES.join(", ")
            ));
        }
        Ok(())
    }

    /// Writes through a temp file and a rename, so a failed write leaves the
    /// file as it was.
    pub fn write(&self, file: &str, contents: &str) -> Result<(), String> {
        let path = self.root.join(file);
        let tmp = path.with_extension("xtask-tmp");
        fs::write(&tmp, contents)
            .and_then(|()| fs::rename(&tmp, &path))
            .map_err(|e| {
                let _ = fs::remove_file(&tmp);
                format!("can't write {file}: {e}")
            })
    }

    /// Rewrites the workspace crates' entries in Cargo.lock; dependencies stay
    /// as they are.
    pub fn update_lock(&self) -> Result<(), String> {
        let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
        let status = Command::new(cargo)
            .args([
                "update",
                "--workspace",
                "--offline",
                "--quiet",
                "--manifest-path",
            ])
            .arg(self.root.join(CARGO_TOML))
            .status()
            .map_err(|e| format!("can't run cargo: {e}"))?;
        if !status.success() {
            return Err("cargo update --workspace failed".to_owned());
        }
        Ok(())
    }

    pub fn git(&self, args: &[&str]) -> Result<Output, String> {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .output()
            .map_err(|e| format!("can't run git: {e}"))
    }

    /// Runs git and fails with its error output if it fails.
    pub fn git_ok(&self, args: &[&str]) -> Result<Output, String> {
        let output = self.git(args)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git {} failed: {}", args.join(" "), stderr.trim()));
        }
        Ok(output)
    }

    /// Runs git and returns its trimmed output.
    pub fn git_stdout(&self, args: &[&str]) -> Result<String, String> {
        let output = self.git_ok(args)?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Runs gh in the repo and returns its output.
    pub fn gh(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("gh")
            .current_dir(&self.root)
            .args(args)
            .output()
            .map_err(|e| format!("can't run gh, which looks up pull requests: {e}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh {} failed: {}", args.join(" "), stderr.trim()));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}
