use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use rusqlite::Connection;
use tandem_board_core::index::project_db_path;
use tandem_board_core::service::{
    Actor, Config, Finish, LogKind, NewTask, Service, ServiceError, TaskEdit, TaskFilter,
    TaskSummary, Written,
};
use tandem_board_core::store::{Priority, ReadOnly, Status, TaskId, Unmet};
use tempfile::TempDir;

const START: &str = "2026-10-15T09:00:00Z";

/// A project with a service whose clock only moves when a test moves it.
struct Env {
    data: TempDir,
    repo: TempDir,
    now: Arc<Mutex<Timestamp>>,
    service: Service,
}

impl Env {
    fn new() -> Self {
        Self::with_lease(SignedDuration::from_mins(15))
    }

    fn with_lease(lease: SignedDuration) -> Self {
        let data = TempDir::new().unwrap();
        let repo = TempDir::new().unwrap();
        let now = Arc::new(Mutex::new(START.parse::<Timestamp>().unwrap()));
        let service = open(&data, &repo, &now, lease);
        Self {
            data,
            repo,
            now,
            service,
        }
    }

    fn advance(&self, secs: i64) {
        let mut now = self.now.lock().unwrap();
        *now += SignedDuration::from_secs(secs);
    }

    fn path(&self, id: TaskId) -> PathBuf {
        self.repo.path().join(".tandem").join(id.file_name())
    }

    fn file(&self, id: TaskId) -> String {
        fs::read_to_string(self.path(id)).unwrap()
    }

    fn db(&self) -> Connection {
        Connection::open(project_db_path(self.data.path(), "test")).unwrap()
    }

    /// The lease holder and expiry, straight from the database.
    fn lease(&self, id: TaskId) -> Option<(String, String)> {
        self.db()
            .query_row(
                "SELECT agent_id, expires_at FROM leases WHERE task_id = ?1",
                [id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok()
    }

    fn summary(&self, id: TaskId) -> TaskSummary {
        self.service
            .list_tasks(&Actor::User, &TaskFilter::default())
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap()
    }

    /// A task you create, so it starts in todo.
    fn todo(&self, title: &str) -> TaskId {
        self.create(&Actor::User, title, &[])
    }

    fn create(&self, actor: &Actor, title: &str, criteria: &[&str]) -> TaskId {
        self.service
            .create_task(
                actor,
                NewTask {
                    title: title.into(),
                    description: format!("{title}."),
                    acceptance_criteria: criteria.iter().map(|c| c.to_string()).collect(),
                    ..NewTask::default()
                },
            )
            .unwrap()
            .id
    }
}

fn open(
    data: &TempDir,
    repo: &TempDir,
    now: &Arc<Mutex<Timestamp>>,
    lease: SignedDuration,
) -> Service {
    let clock = Arc::clone(now);
    let config = Config {
        lease_duration: lease,
        user_name: "sam".into(),
        clock: Arc::new(move || *clock.lock().unwrap()),
        time_zone: TimeZone::UTC,
    };
    Service::open(data.path(), "test", repo.path(), config).unwrap()
}

fn agent(name: &str) -> Actor {
    Actor::agent(name)
}

fn code<T: std::fmt::Debug>(result: Result<T, ServiceError>) -> &'static str {
    result.unwrap_err().code()
}

fn finish(summary: &str) -> Finish {
    Finish {
        summary: summary.into(),
        ..Finish::default()
    }
}

/// `after` is `before` with `status` changed and `appended` added at the end.
fn assert_appended(before: &str, after: &str, from: &str, to: &str, appended: &str) {
    let expected = before.replacen(&format!("status: {from}\n"), &format!("status: {to}\n"), 1);
    assert_eq!(after, format!("{expected}{appended}"));
}

#[test]
fn agent_tasks_start_in_backlog_and_yours_in_todo() {
    let env = Env::new();
    let written = env
        .service
        .create_task(
            &agent("claude-code-7f2a"),
            NewTask {
                title: "Fix token refresh race".into(),
                description: "Parallel requests that hit a 401 each trigger their own refresh.\n"
                    .into(),
                acceptance_criteria: vec![
                    "Concurrent requests share one refresh call".into(),
                    "A test covers two parallel 401 responses".into(),
                ],
                priority: Some(Priority::High),
                labels: vec!["auth".into(), "backend".into(), "auth".into()],
                ..NewTask::default()
            },
        )
        .unwrap();
    let id = written.id;
    assert_eq!(
        written,
        Written {
            id,
            path: format!(".tandem/{id}.md"),
            status: Status::Backlog,
        }
    );
    // Ids are random, and one YAML would read as a number is quoted.
    let file = env
        .file(id)
        .replacen(&format!("id: \"{id}\"\n"), &format!("id: {id}\n"), 1);
    assert_eq!(
        file,
        format!(
            "---\nid: {id}\ntitle: Fix token refresh race\nstatus: backlog\npriority: high\n\
             labels: [auth, backend]\ncreated: 2026-10-15\n---\n\n\
             ## Description\nParallel requests that hit a 401 each trigger their own refresh.\n\n\
             ## Acceptance criteria\n\
             - [ ] Concurrent requests share one refresh call\n\
             - [ ] A test covers two parallel 401 responses\n\n\
             ## Changelog\n\n### 2026-10-15 · claude-code-7f2a · created\n"
        )
    );

    let mine = env.todo("Mine");
    assert_eq!(env.summary(mine).status, Status::Todo);
    assert!(env.file(mine).contains("### 2026-10-15 · sam · created\n"));
}

#[test]
fn start_task_is_refused_on_a_backlog_task_until_approved() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&a, "Proposed", &[]);
    let before = env.file(id);
    let err = env.service.start_task(&a, id).unwrap_err();
    assert_eq!(err.code(), "invalid_transition");
    assert_eq!(
        err.to_string(),
        format!("cannot start task `{id}` while it is backlog")
    );
    assert_eq!(env.file(id), before);

    // Backlog → Todo: you approve it.
    env.service.approve(id).unwrap();
    let approved = env.file(id);
    assert_appended(
        &before,
        &approved,
        "backlog",
        "todo",
        "\n### 2026-10-15 · sam · approved\n",
    );
    env.service.start_task(&a, id).unwrap();
    assert_eq!(env.summary(id).status, Status::InProgress);
}

#[test]
fn every_transition_writes_exactly_its_changes() {
    let env = Env::new();
    let (a, b) = (agent("agent-a"), agent("agent-b"));
    let id = env.create(&Actor::User, "Task", &["One", "Two"]);
    let created = env.file(id);

    // Todo → In progress, by an agent: a lease is created.
    env.service.start_task(&a, id).unwrap();
    let started = env.file(id);
    assert_appended(
        &created,
        &started,
        "todo",
        "in_progress",
        "\n### 2026-10-15 · agent-a · started\n",
    );
    assert_eq!(
        env.lease(id),
        Some(("agent-a".into(), "2026-10-15T09:15:00.000Z".into()))
    );

    // In progress → In progress: any call by the holder extends the lease.
    env.advance(300);
    env.service.list_tasks(&a, &TaskFilter::default()).unwrap();
    assert_eq!(
        env.lease(id),
        Some(("agent-a".into(), "2026-10-15T09:20:00.000Z".into()))
    );

    // Blocked on and off, and a note: SQLite only.
    env.service
        .set_blocked(&a, id, "Which API version?")
        .unwrap();
    assert_eq!(
        env.summary(id).blocked.as_deref(),
        Some("Which API version?")
    );
    env.service.set_blocked(&a, id, "").unwrap();
    assert_eq!(env.summary(id).blocked, None);
    env.service.log_progress(&a, id, "Read the code.").unwrap();
    assert_eq!(env.file(id), started);
    let log = env.service.get_task(&a, id).unwrap().work_log;
    assert_eq!(
        log.iter()
            .map(|e| (e.agent_id.as_deref(), e.kind, e.message.as_str()))
            .collect::<Vec<_>>(),
        [
            (Some("agent-a"), LogKind::Blocked, "Which API version?"),
            (Some("agent-a"), LogKind::Unblocked, ""),
            (Some("agent-a"), LogKind::Note, "Read the code."),
        ]
    );

    // A milestone changelog entry.
    env.service
        .add_changelog_entry(&a, id, "Halfway.", vec!["src/a.rs".into()])
        .unwrap();
    let milestone = env.file(id);
    assert_eq!(
        milestone,
        format!("{started}\n### 2026-10-15 · agent-a · milestone\nHalfway.\nFiles: src/a.rs\n")
    );

    // In progress → Review: criteria ticked, changelog with summary, lease
    // released.
    env.service
        .finish_task(
            &a,
            id,
            Finish {
                summary: "Did it.".into(),
                files_changed: vec!["src/a.rs".into(), "src/b.rs".into()],
                follow_ups: Some("none".into()),
                met: vec![1, 2],
                unmet: vec![],
            },
        )
        .unwrap();
    let finished = env.file(id);
    assert_appended(
        &milestone
            .replace("- [ ] One", "- [x] One")
            .replace("- [ ] Two", "- [x] Two"),
        &finished,
        "in_progress",
        "finished",
        "\n### 2026-10-15 · agent-a · finished\nDid it.\nFiles: src/a.rs, src/b.rs\n\
         Follow-ups: none\n",
    );
    assert_eq!(env.lease(id), None);

    // Review → In progress: reopen with feedback, stored for the next agent.
    env.service.reopen(id, "Also handle 403.").unwrap();
    let reopened = env.file(id);
    assert_appended(
        &finished,
        &reopened,
        "finished",
        "in_progress",
        "\n### 2026-10-15 · sam · reopened\nAlso handle 403.\n",
    );
    let detail = env.service.get_task(&b, id).unwrap();
    assert_eq!(detail.review_feedback.as_deref(), Some("Also handle 403."));
    assert_eq!(detail.markdown, reopened);
    let last = detail.work_log.last().unwrap();
    assert_eq!(
        (last.agent_id.as_deref(), last.kind),
        (None, LogKind::Feedback)
    );
    assert_eq!(env.lease(id), None);

    // The next agent picks the reopened task up.
    env.service.start_task(&b, id).unwrap();
    let restarted = env.file(id);
    assert_eq!(
        restarted,
        format!("{reopened}\n### 2026-10-15 · agent-b · started\n")
    );
    assert_eq!(env.lease(id).unwrap().0, "agent-b");

    // In progress → Todo: released with a note, lease removed.
    env.service.release_task(&b, id, "Out of time.").unwrap();
    let released = env.file(id);
    assert_appended(
        &restarted,
        &released,
        "in_progress",
        "todo",
        "\n### 2026-10-15 · agent-b · released\nOut of time.\n",
    );
    assert_eq!(env.lease(id), None);

    // Todo → In progress and on to Review, by you: no lease.
    env.service.start_task(&Actor::User, id).unwrap();
    assert_eq!(env.lease(id), None);
    env.service
        .finish_task(&Actor::User, id, finish("Finished it myself."))
        .unwrap();
    let finished = env.file(id);

    // Review → Done: status and reviewed_at only, no changelog entry.
    env.advance(86_400);
    env.service.mark_reviewed(id).unwrap();
    assert_eq!(
        env.file(id),
        finished.replace(
            "status: finished\ncreated: 2026-10-15\n",
            "status: done\ncreated: 2026-10-15\nreviewed_at: 2026-10-16\n"
        )
    );
    assert_eq!(env.summary(id).status, Status::Done);
}

#[test]
fn invalid_transitions_are_refused_and_change_nothing() {
    let env = Env::new();
    let a = agent("agent-a");
    let backlog = env.create(&a, "Backlog", &[]);
    let todo = env.todo("Todo");
    let in_progress = env.todo("In progress");
    env.service.start_task(&a, in_progress).unwrap();
    let finished = env.todo("Finished");
    env.service.start_task(&Actor::User, finished).unwrap();
    env.service
        .finish_task(&Actor::User, finished, finish("x"))
        .unwrap();
    let done = env.todo("Done");
    env.service.start_task(&Actor::User, done).unwrap();
    env.service
        .finish_task(&Actor::User, done, finish("x"))
        .unwrap();
    env.service.mark_reviewed(done).unwrap();

    let u = Actor::User;
    let attempts: Vec<(&str, TaskId, Result<Written, ServiceError>)> = vec![
        ("approve todo", todo, env.service.approve(todo)),
        (
            "approve in_progress",
            in_progress,
            env.service.approve(in_progress),
        ),
        (
            "start backlog (you)",
            backlog,
            env.service.start_task(&u, backlog),
        ),
        (
            "start in_progress (you)",
            in_progress,
            env.service.start_task(&u, in_progress),
        ),
        (
            "start finished",
            finished,
            env.service.start_task(&a, finished),
        ),
        ("start done", done, env.service.start_task(&a, done)),
        (
            "finish todo",
            todo,
            env.service.finish_task(&u, todo, finish("x")),
        ),
        (
            "finish done",
            done,
            env.service.finish_task(&u, done, finish("x")),
        ),
        ("release todo", todo, env.service.release_task(&u, todo, "")),
        (
            "release finished",
            finished,
            env.service.release_task(&u, finished, ""),
        ),
        (
            "review in_progress",
            in_progress,
            env.service.mark_reviewed(in_progress),
        ),
        ("review done", done, env.service.mark_reviewed(done)),
        ("reopen todo", todo, env.service.reopen(todo, "x")),
        ("reopen done", done, env.service.reopen(done, "x")),
        (
            "milestone todo",
            todo,
            env.service.add_changelog_entry(&u, todo, "x", vec![]),
        ),
    ];
    let files: Vec<String> = [backlog, todo, in_progress, finished, done]
        .map(|id| env.file(id))
        .to_vec();
    for (name, _, result) in attempts {
        assert_eq!(code(result), "invalid_transition", "{name}");
    }
    let after: Vec<String> = [backlog, todo, in_progress, finished, done]
        .map(|id| env.file(id))
        .to_vec();
    assert_eq!(after, files);
}

#[test]
fn only_the_holder_or_you_may_act_on_a_task_in_progress() {
    let env = Env::new();
    let (a, b) = (agent("agent-a"), agent("agent-b"));
    let id = env.todo("Task");
    env.service.start_task(&a, id).unwrap();

    let err = env.service.log_progress(&b, id, "hi").unwrap_err();
    assert_eq!(err.code(), "not_holder");
    assert_eq!(
        err.to_string(),
        format!("only the lease holder can do this; task `{id}` is held by `agent-a`")
    );
    assert_eq!(code(env.service.set_blocked(&b, id, "?")), "not_holder");
    assert_eq!(
        code(env.service.add_changelog_entry(&b, id, "x", vec![])),
        "not_holder"
    );
    assert_eq!(
        code(env.service.finish_task(&b, id, finish("x"))),
        "not_holder"
    );
    assert_eq!(code(env.service.release_task(&b, id, "x")), "not_holder");
    assert_eq!(code(env.service.start_task(&b, id)), "claimed_by_other");
    // Starting again as the holder changes nothing.
    let before = env.file(id);
    env.service.start_task(&a, id).unwrap();
    assert_eq!(env.file(id), before);

    // You may always act, from the board.
    env.service
        .log_progress(&Actor::User, id, "Note from you.")
        .unwrap();
    env.service.release_task(&Actor::User, id, "").unwrap();
    assert!(env.file(id).ends_with("### 2026-10-15 · sam · released\n"));
    assert_eq!(env.lease(id), None);
    // No lease, so no agent holds it.
    let err = env.service.log_progress(&a, id, "x").unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("only the lease holder can do this; no agent holds task `{id}`")
    );
}

#[test]
fn start_task_waits_for_every_dependency_to_be_done() {
    let env = Env::new();
    let a = agent("agent-a");
    let dep = env.todo("Dependency");
    let id = env
        .service
        .create_task(
            &Actor::User,
            NewTask {
                title: "Needs dep".into(),
                depends_on: vec![dep],
                ..NewTask::default()
            },
        )
        .unwrap()
        .id;
    let err = env.service.start_task(&a, id).unwrap_err();
    assert_eq!(err.code(), "deps_not_done");
    assert_eq!(
        err.to_string(),
        format!("task `{id}` depends on tasks that are not done: {dep}")
    );
    assert!(env.service.next_task(&a, None, None).unwrap().unwrap().id == dep);

    env.service.start_task(&Actor::User, dep).unwrap();
    env.service
        .finish_task(&Actor::User, dep, finish("x"))
        .unwrap();
    // Finished is not done yet.
    assert_eq!(code(env.service.start_task(&a, id)), "deps_not_done");
    env.service.mark_reviewed(dep).unwrap();
    env.service.start_task(&a, id).unwrap();
}

#[test]
fn one_hundred_concurrent_starts_have_exactly_one_winner() {
    let env = Env::new();
    let id = env.todo("Contended");
    let service = Arc::new(env.service);
    let barrier = Arc::new(Barrier::new(100));
    let handles: Vec<_> = (0..100)
        .map(|n| {
            let service = Arc::clone(&service);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let actor = agent(&format!("agent-{n}"));
                barrier.wait();
                service.start_task(&actor, id).map_err(|e| e.code())
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let winners: Vec<usize> = (0..100).filter(|&n| results[n].is_ok()).collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    assert!(
        results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| *r == Err("claimed_by_other"))
    );
    let file = fs::read_to_string(env.repo.path().join(".tandem").join(id.file_name())).unwrap();
    assert_eq!(file.matches("· started").count(), 1);
    assert!(file.contains(&format!("· agent-{} · started", winners[0])));
}

#[test]
fn concurrent_starts_through_two_service_instances_have_one_winner() {
    // Two processes sharing a project database: only BEGIN IMMEDIATE keeps
    // them apart.
    let env = Env::new();
    let id = env.todo("Contended");
    let other = Arc::new(open(
        &env.data,
        &env.repo,
        &env.now,
        SignedDuration::from_mins(15),
    ));
    let first = Arc::new(env.service);
    let barrier = Arc::new(Barrier::new(100));
    let handles: Vec<_> = (0..100)
        .map(|n| {
            let service = Arc::clone(if n % 2 == 0 { &first } else { &other });
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                service
                    .start_task(&agent(&format!("agent-{n}")), id)
                    .map_err(|e| e.code())
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(
        results.iter().filter(|r| r.is_ok()).count(),
        1,
        "{results:?}"
    );
    assert!(
        results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| *r == Err("claimed_by_other"))
    );
    let file = fs::read_to_string(env.repo.path().join(".tandem").join(id.file_name())).unwrap();
    assert_eq!(file.matches("· started").count(), 1);
}

#[test]
fn a_stale_lease_can_be_taken_over() {
    let env = Env::with_lease(SignedDuration::from_mins(1));
    let (a, b) = (agent("agent-a"), agent("agent-b"));
    let id = env.todo("Task");
    env.service.start_task(&a, id).unwrap();
    assert_eq!(env.lease(id).unwrap().1, "2026-10-15T09:01:00.000Z");

    env.advance(30);
    assert_eq!(code(env.service.start_task(&b, id)), "claimed_by_other");
    // A's call at 50 s extends its lease to 110 s.
    env.advance(20);
    env.service.log_progress(&a, id, "Still here.").unwrap();
    env.advance(50);
    assert_eq!(code(env.service.start_task(&b, id)), "claimed_by_other");
    assert!(env.summary(id).lease.unwrap().live);

    // At 111 s the lease is stale; the card shows it, and B takes over.
    env.advance(11);
    let lease = env.summary(id).lease.unwrap();
    assert_eq!((lease.agent_id.as_str(), lease.live), ("agent-a", false));
    let before = env.file(id);
    env.service.start_task(&b, id).unwrap();
    assert_eq!(
        env.file(id),
        format!(
            "{before}\n### 2026-10-15 · agent-b · started\n\
             Took over from agent-a after its lease expired.\n"
        )
    );
    let lease = env.summary(id).lease.unwrap();
    assert_eq!((lease.agent_id.as_str(), lease.live), ("agent-b", true));
    assert_eq!(
        code(env.service.finish_task(&a, id, finish("x"))),
        "not_holder"
    );
    env.service.finish_task(&b, id, finish("Done.")).unwrap();
}

#[test]
fn finish_ticks_met_criteria_and_requires_a_reason_for_the_rest() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&Actor::User, "Task", &["One", "Two", "Three"]);
    env.service.start_task(&a, id).unwrap();
    let before = env.file(id);

    let attempt = |met: Vec<u32>, unmet: Vec<(u32, &str)>| {
        env.service.finish_task(
            &a,
            id,
            Finish {
                summary: "Done.".into(),
                met,
                unmet: unmet
                    .into_iter()
                    .map(|(criterion, reason)| Unmet {
                        criterion,
                        reason: reason.into(),
                    })
                    .collect(),
                ..Finish::default()
            },
        )
    };
    let err = attempt(vec![1], vec![]).unwrap_err();
    assert_eq!(err.code(), "criteria_unmet");
    assert!(matches!(err, ServiceError::CriteriaUnmet { ref criteria, .. } if criteria == &[2, 3]));
    assert_eq!(code(attempt(vec![1, 3], vec![])), "criteria_unmet");
    assert_eq!(
        code(attempt(vec![4], vec![(2, "x"), (3, "x")])),
        "invalid_argument"
    );
    assert_eq!(
        code(attempt(vec![1, 2], vec![(2, "x"), (3, "x")])),
        "invalid_argument"
    );
    assert_eq!(
        code(attempt(vec![1], vec![(2, "x"), (3, " ")])),
        "invalid_argument"
    );
    assert_eq!(env.file(id), before);
    assert_eq!(env.summary(id).status, Status::InProgress);

    attempt(vec![3, 1], vec![(2, "needs a staging account")]).unwrap();
    assert_appended(
        &before
            .replace("- [ ] One", "- [x] One")
            .replace("- [ ] Three", "- [x] Three"),
        &env.file(id),
        "in_progress",
        "finished",
        "\n### 2026-10-15 · agent-a · finished\nDone.\nUnmet: 2 (needs a staging account)\n",
    );
}

#[test]
fn a_task_without_criteria_always_passes() {
    let env = Env::new();
    let id = env.todo("No criteria");
    env.service.start_task(&agent("a"), id).unwrap();
    env.service
        .finish_task(&agent("a"), id, finish("Done."))
        .unwrap();
}

#[test]
fn create_and_update_refuse_loops_and_missing_tasks() {
    let env = Env::new();
    let u = Actor::User;
    let top = env.todo("Top");
    let child = env
        .service
        .create_task(
            &u,
            NewTask {
                title: "Child".into(),
                parent: Some(top),
                depends_on: vec![top],
                ..NewTask::default()
            },
        )
        .unwrap()
        .id;
    let missing: TaskId = "zzzzzzz".parse().unwrap();
    let create = |parent, depends_on| {
        env.service.create_task(
            &u,
            NewTask {
                title: "New".into(),
                parent,
                depends_on,
                ..NewTask::default()
            },
        )
    };
    assert_eq!(code(create(Some(missing), vec![])), "parent_not_found");
    assert_eq!(code(create(None, vec![missing])), "not_found");

    let edit = |parent, depends_on| {
        env.service.update_task(
            &u,
            top,
            TaskEdit {
                parent,
                depends_on,
                ..TaskEdit::default()
            },
        )
    };
    let before = env.file(top);
    assert_eq!(code(edit(Some(Some(child)), None)), "cycle_detected");
    assert_eq!(code(edit(None, Some(vec![child]))), "cycle_detected");
    assert_eq!(code(edit(Some(Some(top)), None)), "cycle_detected");
    assert_eq!(code(edit(Some(Some(missing)), None)), "parent_not_found");
    assert_eq!(code(edit(None, Some(vec![missing]))), "not_found");
    assert_eq!(env.file(top), before);
    assert_eq!(
        code(env.service.update_task(&u, missing, TaskEdit::default())),
        "not_found"
    );
}

#[test]
fn update_task_edits_the_spec_and_nothing_else() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&Actor::User, "Task", &["One"]);
    env.service.start_task(&a, id).unwrap();
    let before = env.file(id);

    // You may change the spec of a task in progress; it stays in progress.
    env.service
        .update_task(
            &Actor::User,
            id,
            TaskEdit {
                title: Some("Better title".into()),
                description: Some("New description.".into()),
                relevant_files: Some(vec!["src/a.rs".into()]),
                priority: Some(Priority::Low),
                labels: Some(vec!["ui".into()]),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    assert_eq!(
        env.file(id),
        before
            .replace(
                "title: Task\nstatus: in_progress\n",
                "title: Better title\nstatus: in_progress\npriority: low\nlabels: [ui]\n"
            )
            .replace("Task.\n", "New description.\n")
            .replace(
                "\n## Changelog",
                "\n## Relevant files\n- src/a.rs\n\n## Changelog"
            )
    );
    assert_eq!(env.summary(id).status, Status::InProgress);

    // An edit that changes nothing doesn't write.
    let mtime = fs::metadata(env.path(id)).unwrap().modified().unwrap();
    env.service
        .update_task(
            &a,
            id,
            TaskEdit {
                priority: Some(Priority::Low),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    assert_eq!(
        fs::metadata(env.path(id)).unwrap().modified().unwrap(),
        mtime
    );

    // Text that would break the sections is refused.
    let edit = TaskEdit {
        description: Some("## Sneaky heading".into()),
        ..TaskEdit::default()
    };
    assert_eq!(
        code(env.service.update_task(&a, id, edit)),
        "invalid_argument"
    );
}

#[test]
fn an_invalid_file_is_refused_and_never_written() {
    let env = Env::new();
    let id = env.todo("Task");
    let broken = env.file(id).replace("status: todo", "status: [");
    fs::write(env.path(id), &broken).unwrap();
    let err = env.service.start_task(&agent("a"), id).unwrap_err();
    assert_eq!(err.code(), "invalid_file");
    assert_eq!(code(env.service.approve(id)), "invalid_file");
    assert_eq!(env.file(id), broken);

    let missing: TaskId = "zzzzzzz".parse().unwrap();
    assert_eq!(
        code(env.service.start_task(&agent("a"), missing)),
        "not_found"
    );
    assert_eq!(
        code(env.service.get_task(&Actor::User, missing)),
        "not_found"
    );
    assert_eq!(
        code(env.service.log_progress(&Actor::User, missing, "x")),
        "not_found"
    );
}

#[test]
fn a_read_only_project_refuses_file_writes() {
    let data = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    fs::create_dir(repo.path().join(".tandem")).unwrap();
    fs::write(
        repo.path().join(".tandem/config.toml"),
        "format_version = 2\n",
    )
    .unwrap();
    let service = Service::open(data.path(), "test", repo.path(), Config::default()).unwrap();
    assert_eq!(
        service.read_only(),
        Some(&ReadOnly::NewerFormat { version: 2 })
    );
    let err = service
        .create_task(
            &Actor::User,
            NewTask {
                title: "x".into(),
                ..NewTask::default()
            },
        )
        .unwrap_err();
    assert_eq!(err.code(), "read_only");
    assert!(
        service
            .list_tasks(&Actor::User, &TaskFilter::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn next_task_picks_the_highest_priority_ready_task() {
    let env = Env::new();
    let (a, b) = (agent("agent-a"), agent("agent-b"));
    let create = |title: &str, priority, labels: Vec<&str>| {
        env.service
            .create_task(
                &Actor::User,
                NewTask {
                    title: title.into(),
                    priority,
                    labels: labels.into_iter().map(String::from).collect(),
                    ..NewTask::default()
                },
            )
            .unwrap()
            .id
    };
    let low = create("Low", Some(Priority::Low), vec!["ui"]);
    let medium = create("Medium", None, vec![]);
    let high = create("High", Some(Priority::High), vec![]);
    env.create(&a, "Backlog", &[]);

    let next = |actor: &Actor, label: Option<&str>| {
        env.service
            .next_task(actor, label, None)
            .unwrap()
            .map(|t| t.id)
    };
    assert_eq!(next(&a, None), Some(high));
    env.service.start_task(&a, high).unwrap();
    assert_eq!(next(&b, None), Some(medium));
    assert_eq!(next(&b, Some("ui")), Some(low));

    let ready = env
        .service
        .list_tasks(
            &b,
            &TaskFilter {
                ready_only: true,
                ..TaskFilter::default()
            },
        )
        .unwrap();
    assert_eq!(ready.iter().map(|t| t.id).collect::<Vec<_>>().len(), 2);
    let found = env
        .service
        .list_tasks(
            &b,
            &TaskFilter {
                query: Some("MEDIUM".into()),
                ..TaskFilter::default()
            },
        )
        .unwrap();
    assert_eq!(found.iter().map(|t| t.id).collect::<Vec<_>>(), [medium]);
    assert_eq!(env.summary(high).lease.unwrap().agent_id, "agent-a");
}

#[cfg(unix)]
#[test]
fn a_symlinked_tandem_folder_refuses_file_writes() {
    let data = TempDir::new().unwrap();
    let repo = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), repo.path().join(".tandem")).unwrap();
    let service = Service::open(data.path(), "test", repo.path(), Config::default()).unwrap();
    assert_eq!(service.read_only(), Some(&ReadOnly::SymlinkedFolder));
    let err = service
        .create_task(
            &Actor::User,
            NewTask {
                title: "x".into(),
                ..NewTask::default()
            },
        )
        .unwrap_err();
    assert_eq!(err.code(), "read_only");
    assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}

#[test]
fn agents_edit_backlog_freely_and_only_details_after_approval() {
    let env = Env::new();
    let (a, b) = (agent("agent-a"), agent("agent-b"));
    let title = |t: &str| TaskEdit {
        title: Some(t.into()),
        ..TaskEdit::default()
    };
    let details = TaskEdit {
        relevant_files: Some(vec!["src/a.rs".into()]),
        labels: Some(vec!["ui".into()]),
        priority: Some(Priority::High),
        ..TaskEdit::default()
    };

    // Backlog: anything goes, and it stays in backlog.
    let proposed = env.create(&b, "Proposed", &["One"]);
    let edit = TaskEdit {
        acceptance_criteria: Some(vec!["Two".into()]),
        ..title("Proposed, better")
    };
    env.service.update_task(&a, proposed, edit).unwrap();
    assert_eq!(env.summary(proposed).status, Status::Backlog);
    assert!(!env.file(proposed).contains("· edited"));

    // Todo: details keep it approved.
    let dep = env.todo("Dependency");
    let id = env
        .service
        .create_task(
            &Actor::User,
            NewTask {
                title: "Task".into(),
                description: "Do it.".into(),
                acceptance_criteria: vec!["One".into()],
                depends_on: vec![dep],
                ..NewTask::default()
            },
        )
        .unwrap()
        .id;
    env.service.update_task(&b, id, details.clone()).unwrap();
    assert_eq!(env.summary(id).status, Status::Todo);
    assert!(!env.file(id).contains("· edited"));

    // Any other change sends it back to backlog for approval. Dropping a
    // dependency can't be used to start early.
    for edit in [
        title("Something else"),
        TaskEdit {
            description: Some("Do something else.".into()),
            ..TaskEdit::default()
        },
        TaskEdit {
            acceptance_criteria: Some(vec![]),
            ..TaskEdit::default()
        },
        TaskEdit {
            depends_on: Some(vec![]),
            ..TaskEdit::default()
        },
        TaskEdit {
            parent: Some(Some(dep)),
            ..TaskEdit::default()
        },
    ] {
        let before = env.file(id);
        let written = env.service.update_task(&b, id, edit.clone()).unwrap();
        assert_eq!(written.status, Status::Backlog, "{edit:?}");
        assert!(
            env.file(id).ends_with(
                "\n### 2026-10-15 · agent-b · edited\n\
                 Changed after approval; back in backlog until approved again.\n"
            ),
            "{edit:?}"
        );
        assert_eq!(code(env.service.start_task(&a, id)), "invalid_transition");
        // You approve it again; your own edits never send it back.
        env.service.approve(id).unwrap();
        env.service
            .update_task(&Actor::User, id, title("Task"))
            .unwrap();
        assert_eq!(env.summary(id).status, Status::Todo);
        assert_ne!(env.file(id), before);
    }

    // In progress: only the holder, and only details.
    let id = env.create(&Actor::User, "Started", &["One"]);
    env.service.start_task(&a, id).unwrap();
    assert_eq!(
        code(env.service.update_task(&b, id, details.clone())),
        "not_holder"
    );
    let before = env.file(id);
    for edit in [
        title("Mine now"),
        TaskEdit {
            acceptance_criteria: Some(vec![]),
            ..TaskEdit::default()
        },
        TaskEdit {
            depends_on: Some(vec![dep]),
            ..TaskEdit::default()
        },
    ] {
        let err = env.service.update_task(&a, id, edit.clone()).unwrap_err();
        assert_eq!(err.code(), "invalid_transition", "{edit:?}");
    }
    assert_eq!(env.file(id), before);
    env.service.update_task(&a, id, details.clone()).unwrap();
    assert_eq!(env.summary(id).status, Status::InProgress);
    env.service
        .update_task(&Actor::User, id, title("Yours to change"))
        .unwrap();

    // Finished and done tasks are closed to agents.
    env.service
        .finish_task(
            &a,
            id,
            Finish {
                met: vec![1],
                ..finish("Done.")
            },
        )
        .unwrap();
    assert_eq!(
        code(env.service.update_task(&a, id, details.clone())),
        "invalid_transition"
    );
    env.service.mark_reviewed(id).unwrap();
    assert_eq!(
        code(env.service.update_task(&b, id, details)),
        "invalid_transition"
    );
    env.service
        .update_task(&Actor::User, id, title("Yours"))
        .unwrap();
}

#[test]
fn agents_cannot_take_a_task_you_started() {
    let env = Env::new();
    let id = env.todo("Mine");
    env.service.start_task(&Actor::User, id).unwrap();
    let err = env.service.start_task(&agent("agent-a"), id).unwrap_err();
    assert_eq!(err.code(), "claimed_by_other");
    assert_eq!(err.to_string(), format!("task `{id}` is held by `sam`"));
}

#[test]
fn an_agent_resumes_its_own_task_after_a_rebuild() {
    let env = Env::new();
    let (a, b) = (agent("agent-a"), agent("agent-b"));
    let id = env.todo("Task");
    env.service.start_task(&a, id).unwrap();
    // A rebuilt database has no leases.
    env.db().execute("DELETE FROM leases", []).unwrap();

    assert_eq!(code(env.service.start_task(&b, id)), "claimed_by_other");
    env.service.start_task(&a, id).unwrap();
    assert_eq!(env.lease(id).unwrap().0, "agent-a");
}

#[test]
fn summaries_cannot_fake_entry_lines_or_open_a_fence() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&Actor::User, "Task", &["One"]);
    env.service.start_task(&a, id).unwrap();
    let before = env.file(id);
    for summary in [
        "Done.\nUnmet: none",
        "Done.\n```\nrest of the file",
        "Files: x",
    ] {
        let attempt = env.service.finish_task(
            &a,
            id,
            Finish {
                met: vec![1],
                ..finish(summary)
            },
        );
        assert_eq!(code(attempt), "invalid_argument", "{summary:?}");
        let milestone = env.service.add_changelog_entry(&a, id, summary, vec![]);
        assert_eq!(code(milestone), "invalid_argument", "{summary:?}");
        let release = env.service.release_task(&a, id, summary);
        assert_eq!(code(release), "invalid_argument", "{summary:?}");
    }
    assert_eq!(env.file(id), before);
}

#[test]
fn agent_ids_cannot_pass_for_you() {
    let env = Env::new();
    let id = env.todo("Task");
    for name in [
        "sam",
        "SAM",
        "Sam",
        "sam\u{200b}",
        "s\u{0430}m",
        "sam ",
        "a · b",
        "",
        &"a".repeat(65),
    ] {
        let err = env.service.start_task(&agent(name), id).unwrap_err();
        assert_eq!(err.code(), "invalid_argument", "{name:?}");
    }
    assert_eq!(env.summary(id).status, Status::Todo);
    env.service
        .start_task(&agent("claude_code.7f2a-1"), id)
        .unwrap();
}

#[cfg(unix)]
#[test]
fn a_symlinked_task_file_is_never_followed() {
    let env = Env::new();
    let outside = TempDir::new().unwrap();
    let id = env.todo("Real");
    // Replace the task with a link to a file outside the repo.
    let target = outside.path().join("secret.md");
    fs::write(&target, env.file(id).replace("Real", "Secret")).unwrap();
    fs::remove_file(env.path(id)).unwrap();
    std::os::unix::fs::symlink(&target, env.path(id)).unwrap();
    let secret = fs::read_to_string(&target).unwrap();

    assert_eq!(code(env.service.get_task(&Actor::User, id)), "not_found");
    let err = env.service.start_task(&agent("a"), id).unwrap_err();
    assert_eq!(err.code(), "invalid_file");
    assert!(!err.to_string().contains("Secret"));
    let edit = TaskEdit {
        title: Some("x".into()),
        ..TaskEdit::default()
    };
    assert_eq!(
        code(env.service.update_task(&Actor::User, id, edit)),
        "invalid_file"
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), secret);
}

#[cfg(unix)]
#[test]
fn a_tandem_folder_swapped_for_a_symlink_is_not_written() {
    let env = Env::new();
    let id = env.todo("Task");
    let outside = TempDir::new().unwrap();
    let moved = outside.path().join("tasks");
    let dir = env.repo.path().join(".tandem");
    fs::rename(&dir, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &dir).unwrap();
    let before = fs::read_to_string(moved.join(id.file_name())).unwrap();
    let files = fs::read_dir(&moved).unwrap().count();

    assert_eq!(code(env.service.approve(id)), "read_only");
    assert_eq!(code(env.service.start_task(&Actor::User, id)), "read_only");
    let new = NewTask {
        title: "x".into(),
        ..NewTask::default()
    };
    assert_eq!(
        code(env.service.create_task(&Actor::User, new)),
        "read_only"
    );
    assert_eq!(fs::read_dir(&moved).unwrap().count(), files);
    assert_eq!(
        fs::read_to_string(moved.join(id.file_name())).unwrap(),
        before
    );
}
