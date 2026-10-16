//! Tandem's task service: parses and writes task files, indexes them in
//! SQLite and serializes every change. Has no Tauri dependency.

pub mod git;
pub mod graph;
pub mod index;
pub mod service;
pub mod store;
pub mod watcher;
