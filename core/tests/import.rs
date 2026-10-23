use std::fs;
use std::sync::Arc;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use tandem_board_core::model::{Date, Priority, Status, TaskId};
use tandem_board_core::service::{Actor, Config, Finish, Service, TaskFilter};
use tempfile::TempDir;

fn service(data: &TempDir) -> Service {
    let config = Config {
        user_name: "Sam Lee".into(),
        clock: Arc::new(|| "2026-10-20T12:00:00Z".parse::<Timestamp>().unwrap()),
        time_zone: TimeZone::UTC,
        ..Config::default()
    };
    Service::open(data.path(), "test", config).unwrap()
}

fn id(s: &str) -> TaskId {
    s.parse().unwrap()
}

const PARENT: &str = "---
id: a7rq4ne
title: Auth overhaul
status: todo
created: 2026-10-12
---

## Description
Rework auth.

## Changelog

### 2026-10-12 · Sam Lee · created
";

const CHILD: &str = "---
id: k3f9x2m
title: Fix token refresh race
status: in_progress
priority: high
parent: a7rq4ne
labels: [auth, backend]
depends_on: [p2xmq7d, zzzzzzz]
created: 2026-10-13
---

## Description
Parallel requests that hit a 401 each trigger their own token refresh.

## Notes
Kept with the description.

## Acceptance criteria
- [x] Concurrent requests share one refresh call
- [ ] A test covers two parallel 401 responses

## Relevant files
- src/auth/session.ts

## Changelog

### 2026-10-13 · claude-code · created

### 2026-10-14 · sam lee · approved

### 2026-10-15 · claude-code-7f2a · started
Picked up from todo.
";

const DEP: &str = "---
id: p2xmq7d
title: Shared HTTP client
status: done
created: 2026-10-10
reviewed_at: 2026-10-11
---

## Changelog

### 2026-10-10 · Sam Lee · created

### 2026-10-11 · codex-91ab · finished
Added the client.
Files: src/http.ts
Follow-ups: none
Unmet: 2 (no staging)
";

/// Each other's parent: the loop is broken on import.
const LOOP_A: &str =
    "---\nid: m4vt8ha\ntitle: Loop A\nstatus: backlog\nparent: n5wu9jb\ncreated: 2026-10-10\n---\n";
const LOOP_B: &str =
    "---\nid: n5wu9jb\ntitle: Loop B\nstatus: backlog\nparent: m4vt8ha\ncreated: 2026-10-10\n---\n";

fn folder() -> TempDir {
    let repo = TempDir::new().unwrap();
    let dir = repo.path().join(".tandem");
    fs::create_dir(&dir).unwrap();
    for (name, text) in [
        ("a7rq4ne.md", PARENT),
        ("k3f9x2m.md", CHILD),
        ("p2xmq7d.md", DEP),
        ("m4vt8ha.md", LOOP_A),
        ("n5wu9jb.md", LOOP_B),
        ("q6xv2kc.md", "---\nnot: [valid\n---\n"),
        ("README.md", "# Not a task\n"),
        ("config.toml", "format_version = 1\n"),
    ] {
        fs::write(dir.join(name), text).unwrap();
    }
    repo
}

#[test]
fn imports_a_tandem_folder_with_its_history() {
    let repo = folder();
    let dir = repo.path().join(".tandem");
    let data = TempDir::new().unwrap();
    let service = service(&data);
    assert!(service.is_new());
    let events = service.subscribe();

    let report = service.import_folder(&dir).unwrap();
    let mut imported = report.imported.clone();
    imported.sort();
    assert_eq!(
        imported,
        [
            id("a7rq4ne"),
            id("k3f9x2m"),
            id("m4vt8ha"),
            id("n5wu9jb"),
            id("p2xmq7d")
        ]
    );
    assert_eq!(report.skipped.len(), 3, "{:?}", report.skipped);
    assert!(report.skipped[0].starts_with("q6xv2kc.md: invalid frontmatter"));
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.starts_with("k3f9x2m: dependency left out"))
    );
    assert!(
        report
            .skipped
            .iter()
            .any(|s| s.starts_with("n5wu9jb: parent left out"))
    );
    assert_eq!(events.try_iter().count(), 1);
    // The folder is only read.
    assert_eq!(fs::read_to_string(dir.join("k3f9x2m.md")).unwrap(), CHILD);

    let child = service.get_task(&Actor::User, id("k3f9x2m")).unwrap();
    let summary = &child.summary;
    assert_eq!(summary.status, Status::InProgress);
    assert_eq!(summary.priority, Priority::High);
    assert_eq!(summary.parent, Some(id("a7rq4ne")));
    assert_eq!(summary.labels, ["auth", "backend"]);
    assert_eq!(summary.depends_on, [id("p2xmq7d")]);
    assert_eq!(summary.created, Date::constant(2026, 10, 13));
    assert_eq!(summary.lease, None);
    assert_eq!(
        child.content.description,
        "Parallel requests that hit a 401 each trigger their own token refresh.\n\n\
         ## Notes\nKept with the description."
    );
    assert_eq!(child.content.relevant_files, ["src/auth/session.ts"]);
    assert_eq!(
        child
            .content
            .criteria
            .iter()
            .map(|c| c.met)
            .collect::<Vec<_>>(),
        [true, false]
    );
    let changelog: Vec<(String, String, String)> = child
        .content
        .changelog
        .iter()
        .map(|e| (e.date.to_string(), e.who.clone(), e.event.clone()))
        .collect();
    assert_eq!(
        changelog,
        [
            ("2026-10-13".into(), "claude-code".into(), "created".into()),
            // Your name, in any case, is you.
            ("2026-10-14".into(), "Sam Lee".into(), "approved".into()),
            (
                "2026-10-15".into(),
                "claude-code-7f2a".into(),
                "started".into()
            ),
        ]
    );

    let dep = service.get_task(&Actor::User, id("p2xmq7d")).unwrap();
    assert_eq!(dep.content.reviewed_at, Some(Date::constant(2026, 10, 11)));
    assert_eq!(
        dep.content.changelog[1].text,
        "Added the client.\nFiles: src/http.ts\nFollow-ups: none\nUnmet: 2 (no staging)"
    );
    let parent = service.get_task(&Actor::User, id("a7rq4ne")).unwrap();
    assert_eq!(
        parent.children.iter().map(|c| c.id).collect::<Vec<_>>(),
        [id("k3f9x2m")]
    );

    // The agent that started the task in progress carries on with its id.
    assert!(service.is_known_agent("claude-code-7f2a").unwrap());
    let agent = Actor::agent("claude-code-7f2a");
    assert_eq!(
        service
            .start_task(&Actor::agent("other-1"), id("k3f9x2m"))
            .unwrap_err()
            .code(),
        "claimed_by_other"
    );
    service
        .finish_task(
            &agent,
            id("k3f9x2m"),
            Finish {
                summary: "Done.".into(),
                met: vec![2],
                ..Finish::default()
            },
        )
        .unwrap();

    // Importing again changes nothing.
    let again = service.import_folder(&dir).unwrap();
    assert!(again.imported.is_empty());
    assert_eq!(
        again
            .skipped
            .iter()
            .filter(|s| s.ends_with("a task with this id exists already"))
            .count(),
        5
    );
    assert_eq!(
        service
            .list_tasks(&Actor::User, &TaskFilter::default())
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn a_missing_folder_is_an_error() {
    let data = TempDir::new().unwrap();
    let service = service(&data);
    let missing = data.path().join("nowhere");
    assert_eq!(
        service.import_folder(&missing).unwrap_err().code(),
        "internal"
    );
}
