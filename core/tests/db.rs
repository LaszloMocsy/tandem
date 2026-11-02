use std::fs;

use jiff::Timestamp;
use rusqlite::Connection;
use tandem_board_core::db::backup::{self, backup_dir};
use tandem_board_core::db::{
    AppDb, AppSettings, DbError, ProjectDb, Theme, UpdateChannel, app_db_path, delete_project_db,
    folder_name, project_db_path,
};
use tandem_board_core::features::{Feature, Features};
use tempfile::TempDir;

fn names(conn: &Connection, kind: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type = ?1 AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .unwrap();
    stmt.query_map([kind], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn pragma(conn: &Connection, name: &str) -> i64 {
    conn.pragma_query_value(None, name, |row| row.get(0))
        .unwrap()
}

#[test]
fn a_new_project_database_has_the_whole_schema() {
    let data = TempDir::new().unwrap();
    let db = ProjectDb::open(data.path(), "test").unwrap();
    assert!(db.created());
    let conn = db.connection();
    assert_eq!(
        names(conn, "table"),
        [
            "actors",
            "changes",
            "criteria",
            "events",
            "labels",
            "leases",
            "restoring",
            "task_deps",
            "task_labels",
            "tasks"
        ]
    );
    assert_eq!(
        names(conn, "index"),
        [
            "events_by_task",
            "leases_by_actor",
            "task_deps_by_target",
            "task_labels_by_label",
            "tasks_by_parent"
        ]
    );
    let mode: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    assert_eq!(pragma(conn, "foreign_keys"), 1);
    assert_eq!(pragma(conn, "user_version"), 3);
    assert!(project_db_path(data.path(), "test").ends_with("db/projects/test.db"));
    drop(db);

    // Opening it again runs nothing and creates nothing.
    assert!(!ProjectDb::open(data.path(), "test").unwrap().created());
    assert!(!backup_dir(data.path(), "test").exists());
}

#[test]
fn an_older_project_database_is_backed_up_before_it_is_migrated() {
    let data = TempDir::new().unwrap();
    drop(ProjectDb::open(data.path(), "test").unwrap());
    // Take it back to schema 2, before `restoring`.
    let conn = Connection::open(project_db_path(data.path(), "test")).unwrap();
    conn.execute_batch("DROP TABLE restoring; PRAGMA user_version = 2;")
        .unwrap();
    drop(conn);

    let db = ProjectDb::open(data.path(), "test").unwrap();
    assert!(!db.created());
    assert_eq!(pragma(db.connection(), "user_version"), 3);
    let backups = backup::list(&backup_dir(data.path(), "test")).unwrap();
    assert_eq!(backups.len(), 1);
    let saved = Connection::open(&backups[0].path).unwrap();
    assert_eq!(pragma(&saved, "user_version"), 2);
}

#[test]
fn the_schema_refuses_bad_rows() {
    let data = TempDir::new().unwrap();
    let db = ProjectDb::open(data.path(), "test").unwrap();
    let conn = db.connection();
    conn.execute(
        "INSERT INTO actors (kind, name, first_seen, last_seen) VALUES ('user', 'sam', 0, 0)",
        [],
    )
    .unwrap();
    let insert = |status: &str, parent: Option<i64>| {
        conn.execute(
            "INSERT INTO tasks (id, title, status, parent, created_by, created_at, updated_at)
             VALUES (lower(hex(randomblob(4))), 'T', ?1, ?2, 1, 0, 0)",
            rusqlite::params![status, parent],
        )
    };
    insert("todo", None).unwrap();
    // Unknown statuses, missing parents and wrong types are refused.
    assert!(insert("doing", None).is_err());
    assert!(insert("todo", Some(99)).is_err());
    assert!(
        conn.execute(
            "UPDATE tasks SET created_at = 'yesterday' WHERE seq = 1",
            []
        )
        .is_err()
    );
    assert!(
        conn.execute(
            "UPDATE tasks SET relevant_files = 'not json' WHERE seq = 1",
            []
        )
        .is_err()
    );
    // Events and the rest go with their task.
    conn.execute(
        "INSERT INTO events (task, at, actor, kind) VALUES (1, 0, 1, 'created')",
        [],
    )
    .unwrap();
    conn.execute("DELETE FROM tasks WHERE seq = 1", []).unwrap();
    let events: i64 = conn
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .unwrap();
    assert_eq!(events, 0);
}

#[test]
fn refuses_bad_project_ids_and_newer_schemas() {
    let data = TempDir::new().unwrap();
    for bad in ["../evil", "", "A"] {
        assert!(matches!(
            ProjectDb::open(data.path(), bad),
            Err(DbError::InvalidProjectId(_))
        ));
    }
    drop(ProjectDb::open(data.path(), "test").unwrap());
    let conn = Connection::open(project_db_path(data.path(), "test")).unwrap();
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(matches!(
        ProjectDb::open(data.path(), "test"),
        Err(DbError::NewerSchema {
            version: 99,
            supported: 3,
            ..
        })
    ));
}

#[test]
fn deleting_a_project_database_leaves_other_projects() {
    let data = TempDir::new().unwrap();
    drop(ProjectDb::open(data.path(), "one").unwrap());
    drop(ProjectDb::open(data.path(), "two").unwrap());
    delete_project_db(data.path(), "one").unwrap();
    assert!(!project_db_path(data.path(), "one").exists());
    assert!(project_db_path(data.path(), "two").exists());
    // A missing database is fine.
    delete_project_db(data.path(), "one").unwrap();
}

#[test]
fn the_app_database_lists_projects() {
    let data = TempDir::new().unwrap();
    let parent = TempDir::new().unwrap();
    let repo = parent.path().join("My Repo");
    let other = parent.path().join("other").join("my-repo");
    fs::create_dir(&repo).unwrap();
    fs::create_dir_all(&other).unwrap();

    let mut app = AppDb::open(data.path()).unwrap();
    let conn = Connection::open(app_db_path(data.path())).unwrap();
    assert_eq!(names(&conn, "table"), ["projects", "settings"]);

    let first = app.add_project(&repo, &repo, &folder_name(&repo)).unwrap();
    assert_eq!(
        (first.id.as_str(), first.name.as_str()),
        ("my-repo", "My Repo")
    );
    assert_eq!(first.git_dir, repo.canonicalize().unwrap());
    assert_eq!(first.last_path, first.git_dir);
    // Adding the same project again, by any path, returns it unchanged.
    let again = app
        .add_project(&parent.path().join("other/../My Repo"), &repo, "ignored")
        .unwrap();
    assert_eq!(again, first);
    // Another project with the same slug gets a suffix.
    let second = app.add_project(&other, &other, "my-repo").unwrap();
    assert_eq!(second.id, "my-repo-2");

    let at: Timestamp = "2026-10-15T09:30:00Z".parse().unwrap();
    assert!(app.mark_opened("my-repo-2", at).unwrap());
    assert!(!app.mark_opened("missing", at).unwrap());
    let projects = app.projects().unwrap();
    assert_eq!(
        projects.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["my-repo-2", "my-repo"]
    );
    assert_eq!(
        projects[0].last_opened_at.as_deref(),
        Some("2026-10-15T09:30:00.000Z")
    );

    assert!(app.set_name("my-repo", "Tandem").unwrap());
    assert!(!app.set_name("missing", "Tandem").unwrap());
    drop(app);
    let app = AppDb::open(data.path()).unwrap();
    let renamed = app.project("my-repo").unwrap().unwrap();
    assert_eq!(renamed.name, "Tandem");
    assert!(app.remove_project("my-repo").unwrap());
    assert_eq!(app.project("my-repo").unwrap(), None);
    assert!(repo.is_dir());
}

#[test]
fn app_settings_default_and_persist() {
    let data = TempDir::new().unwrap();
    let app = AppDb::open(data.path()).unwrap();
    assert_eq!(
        app.settings().unwrap(),
        AppSettings {
            mcp_port: None,
            theme: Theme::System,
            update_channel: None,
            check_updates: true,
        }
    );
    app.set_mcp_port(Some(4400)).unwrap();
    app.set_theme(Theme::Dark).unwrap();
    app.set_update_channel(Some(UpdateChannel::Beta)).unwrap();
    app.set_check_updates(false).unwrap();
    drop(app);

    let app = AppDb::open(data.path()).unwrap();
    assert_eq!(
        app.settings().unwrap(),
        AppSettings {
            mcp_port: Some(4400),
            theme: Theme::Dark,
            update_channel: Some(UpdateChannel::Beta),
            check_updates: false,
        }
    );
    app.set_mcp_port(None).unwrap();
    app.set_theme(Theme::System).unwrap();
    app.set_update_channel(None).unwrap();
    app.set_check_updates(true).unwrap();
    assert_eq!(app.settings().unwrap(), AppSettings::default());
    // Defaults are stored as no row at all.
    let conn = Connection::open(app_db_path(data.path())).unwrap();
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM settings", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn unreadable_app_settings_fall_back_to_defaults() {
    let data = TempDir::new().unwrap();
    let app = AppDb::open(data.path()).unwrap();
    let conn = Connection::open(app_db_path(data.path())).unwrap();
    conn.execute_batch(
        "INSERT INTO settings VALUES ('mcp_port', '70000'), ('theme', 'sepia'),
           ('update_channel', 'nightly'), ('check_updates', 'maybe'), ('later', 'x');",
    )
    .unwrap();
    assert_eq!(app.settings().unwrap(), AppSettings::default());
    conn.execute("UPDATE settings SET value = '0' WHERE key = 'mcp_port'", [])
        .unwrap();
    assert_eq!(app.settings().unwrap().mcp_port, None);
}

#[test]
fn features_default_off_and_persist() {
    let data = TempDir::new().unwrap();
    let app = AppDb::open(data.path()).unwrap();
    assert_eq!(app.features().unwrap(), Features::OFF);

    app.set_feature(Feature::Structure, true).unwrap();
    app.set_feature(Feature::Shaping, true).unwrap();
    app.set_feature(Feature::Shaping, false).unwrap();
    drop(app);

    let app = AppDb::open(data.path()).unwrap();
    assert_eq!(
        app.features().unwrap(),
        Features::OFF.with(Feature::Structure, true)
    );
    // Flags are settings rows of their own, holding `true` or `false`, and
    // leave the other settings alone.
    assert_eq!(app.settings().unwrap(), AppSettings::default());
    let conn = Connection::open(app_db_path(data.path())).unwrap();
    let mut stmt = conn
        .prepare("SELECT key, value FROM settings ORDER BY key")
        .unwrap();
    let rows: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        [
            ("feature.shaping".to_owned(), "false".to_owned()),
            ("feature.structure".to_owned(), "true".to_owned()),
        ]
    );

    for feature in Feature::ALL {
        app.set_feature(feature, true).unwrap();
    }
    assert_eq!(
        app.features().unwrap().enabled().count(),
        Feature::ALL.len()
    );
}

#[test]
fn only_true_turns_a_feature_on() {
    let data = TempDir::new().unwrap();
    let app = AppDb::open(data.path()).unwrap();
    let conn = Connection::open(app_db_path(data.path())).unwrap();
    conn.execute_batch(
        "INSERT INTO settings VALUES ('feature.structure', 'TRUE'), ('feature.canvas', '1'),
           ('feature.assistant', 'yes'), ('feature.shaping', ''), ('feature.later', 'true'),
           ('FEATURE.AUTOMATION', 'true'), ('structure', 'true');",
    )
    .unwrap();
    assert_eq!(app.features().unwrap(), Features::OFF);
    conn.execute(
        "UPDATE settings SET value = 'true' WHERE key = 'feature.canvas'",
        [],
    )
    .unwrap();
    assert_eq!(
        app.features().unwrap(),
        Features::OFF.with(Feature::Canvas, true)
    );
}

#[test]
fn update_channels_parse() {
    for channel in [UpdateChannel::Stable, UpdateChannel::Beta] {
        assert_eq!(channel.as_str().parse::<UpdateChannel>(), Ok(channel));
    }
    assert!("alpha".parse::<UpdateChannel>().is_err());
}

#[test]
fn themes_parse() {
    for theme in [Theme::System, Theme::Light, Theme::Dark] {
        assert_eq!(theme.as_str().parse::<Theme>(), Ok(theme));
    }
    assert!("Dark".parse::<Theme>().is_err());
}

#[test]
fn databases_live_in_the_data_directorys_db_folder() {
    let data = TempDir::new().unwrap();
    let db = data.path().join("db");
    assert_eq!(app_db_path(data.path()), db.join("app.db"));
    assert_eq!(
        project_db_path(data.path(), "test"),
        db.join("projects").join("test.db")
    );
    drop(AppDb::open(data.path()).unwrap());
    drop(ProjectDb::open(data.path(), "test").unwrap());
    assert!(app_db_path(data.path()).is_file());
    assert!(project_db_path(data.path(), "test").is_file());
}
