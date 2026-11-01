//! Tandem's task service: tasks, their history and agent activity in one
//! SQLite database per project, with every change serialized through the
//! service. Has no Tauri dependency.

pub mod db;
pub mod features;
pub mod git;
pub mod graph;
pub mod markdown;
pub mod model;
pub mod service;
