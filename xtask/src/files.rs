//! Reading and rewriting the version in the files that hold it. Every function
//! works on file contents, so the edits are testable without a checkout.

use toml_edit::{DocumentMut, value};

/// `[workspace.package] version` from the root `Cargo.toml`.
pub fn cargo_version(cargo_toml: &str) -> Result<String, String> {
    let doc: DocumentMut = cargo_toml
        .parse()
        .map_err(|e| format!("Cargo.toml doesn't parse: {e}"))?;
    doc.get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml has no [workspace.package] version".to_owned())
}

/// The top-level `version` from `tauri.conf.json`.
pub fn tauri_version(tauri_conf: &str) -> Result<String, String> {
    let json: serde_json::Value = serde_json::from_str(tauri_conf)
        .map_err(|e| format!("tauri.conf.json doesn't parse: {e}"))?;
    json.get("version")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "tauri.conf.json has no version".to_owned())
}

/// Name and version of every workspace crate in `Cargo.lock` except `skip`.
/// Workspace crates are the packages with no `source`.
pub fn lock_versions(cargo_lock: &str, skip: &str) -> Result<Vec<(String, String)>, String> {
    let doc: DocumentMut = cargo_lock
        .parse()
        .map_err(|e| format!("Cargo.lock doesn't parse: {e}"))?;
    let packages = doc
        .get("package")
        .and_then(|p| p.as_array_of_tables())
        .ok_or_else(|| "Cargo.lock has no packages".to_owned())?;
    Ok(packages
        .iter()
        .filter(|p| !p.contains_key("source"))
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?;
            let version = p.get("version")?.as_str()?;
            (name != skip).then(|| (name.to_owned(), version.to_owned()))
        })
        .collect())
}

/// `Cargo.toml` with `[workspace.package] version` set to `new`, everything
/// else byte for byte.
pub fn set_cargo_version(cargo_toml: &str, new: &str) -> Result<String, String> {
    let mut doc: DocumentMut = cargo_toml
        .parse()
        .map_err(|e| format!("Cargo.toml doesn't parse: {e}"))?;
    let item = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("package"))
        .and_then(|p| p.get_mut("version"))
        .filter(|v| v.is_str())
        .ok_or_else(|| "Cargo.toml has no [workspace.package] version".to_owned())?;
    let decor = item.as_value().map(|v| v.decor().clone());
    *item = value(new);
    if let (Some(decor), Some(v)) = (decor, item.as_value_mut()) {
        *v.decor_mut() = decor;
    }
    one_line_changed(cargo_toml, doc.to_string(), "Cargo.toml")
}

/// `tauri.conf.json` with its top-level `"version": "<old>"` line set to
/// `new`, everything else byte for byte.
pub fn set_tauri_version(tauri_conf: &str, old: &str, new: &str) -> Result<String, String> {
    let line = format!("  \"version\": \"{old}\",");
    let matches = tauri_conf.lines().filter(|l| *l == line).count();
    if matches != 1 {
        return Err(format!(
            "expected one {line:?} line in tauri.conf.json, found {matches}"
        ));
    }
    let updated = tauri_conf.replacen(&line, &format!("  \"version\": \"{new}\","), 1);
    if tauri_version(&updated)? != new {
        return Err("the version line in tauri.conf.json isn't the top-level version".to_owned());
    }
    one_line_changed(tauri_conf, updated, "tauri.conf.json")
}

/// Returns `after`, or an error unless it differs from `before` in exactly one
/// line, so an edit can't spill over into the rest of the file.
fn one_line_changed(before: &str, after: String, file: &str) -> Result<String, String> {
    let (a, b): (Vec<_>, Vec<_>) = (before.split('\n').collect(), after.split('\n').collect());
    let changed = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    if a.len() != b.len() || changed != 1 {
        return Err(format!(
            "the edit to {file} would change more than the version line"
        ));
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_TOML: &str = r#"[workspace]
resolver = "3"
members = ["core", "xtask"]

[workspace.package]
version = "0.1.0"   # the app version
edition = "2024"

[workspace.dependencies]
core = { path = "core", version = "0.1.0" }
"#;

    const TAURI_CONF: &str = r#"{
  "productName": "Tandem",
  "version": "0.1.0",
  "plugins": {
    "x": {
      "version": "0.1.0",
      "other": 1
    }
  },
  "bundle": { "active": true }
}
"#;

    const CARGO_LOCK: &str = r#"version = 4

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "tandem-board-core"
version = "0.1.0"

[[package]]
name = "tandem-board-mcp"
version = "0.1.0"
dependencies = [
 "serde",
]

[[package]]
name = "xtask"
version = "0.0.0"
"#;

    #[test]
    fn reads_every_version() {
        assert_eq!(cargo_version(CARGO_TOML).unwrap(), "0.1.0");
        assert_eq!(tauri_version(TAURI_CONF).unwrap(), "0.1.0");
        assert_eq!(
            lock_versions(CARGO_LOCK, "xtask").unwrap(),
            [
                ("tandem-board-core".to_owned(), "0.1.0".to_owned()),
                ("tandem-board-mcp".to_owned(), "0.1.0".to_owned()),
            ]
        );
    }

    #[test]
    fn reports_missing_versions() {
        assert!(cargo_version("[workspace]\nmembers = []\n").is_err());
        assert!(cargo_version("[workspace.package]\nversion = 1\n").is_err());
        assert!(tauri_version("{}").is_err());
        assert!(tauri_version("not json").is_err());
    }

    #[test]
    fn cargo_bump_changes_only_the_version() {
        let bumped = set_cargo_version(CARGO_TOML, "0.2.0-alpha.1").unwrap();
        assert_eq!(
            bumped,
            CARGO_TOML.replace(
                "version = \"0.1.0\"   # the app version",
                "version = \"0.2.0-alpha.1\"   # the app version",
            )
        );
    }

    #[test]
    fn tauri_bump_changes_only_the_top_level_version() {
        let bumped = set_tauri_version(TAURI_CONF, "0.1.0", "0.2.0").unwrap();
        assert_eq!(
            bumped,
            TAURI_CONF.replacen("  \"version\": \"0.1.0\",", "  \"version\": \"0.2.0\",", 1)
        );
        assert!(bumped.contains("      \"version\": \"0.1.0\","));
    }

    #[test]
    fn tauri_bump_refuses_an_unexpected_layout() {
        let compact = "{\"version\": \"0.1.0\"}\n";
        assert!(set_tauri_version(compact, "0.1.0", "0.2.0").is_err());
        let twice = "{\n  \"version\": \"0.1.0\",\n  \"version\": \"0.1.0\",\n}\n";
        assert!(set_tauri_version(twice, "0.1.0", "0.2.0").is_err());
        let nested_only = "{\n  \"a\": {\n  \"version\": \"0.1.0\",\n  \"b\": 1 },\n  \"version\": \"0.1.0\"\n}\n";
        assert!(set_tauri_version(nested_only, "0.1.0", "0.2.0").is_err());
    }
}
