//! Tandem's task service: parses and writes task files, indexes them in
//! SQLite and serializes every change. Has no Tauri dependency.

pub mod graph;
pub mod store;
