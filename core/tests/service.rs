use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use jiff::tz::TimeZone;
use jiff::{SignedDuration, Timestamp};
use rusqlite::Connection;
use tandem_board_core::db::{Changes, project_db_path, timestamp_text};
use tandem_board_core::model::{Date, Priority, Status, TaskId, Unmet, UnmetCriterion};
use tandem_board_core::service::{
    Actor, Config, Finish, LogKind, NewTask, Saved, Service, ServiceError, TaskEdit, TaskFilter,
    TaskSummary,
};
use tempfile::TempDir;

const START: &str = "2026-10-15T09:00:00Z";

/// A project with a service whose clock only moves when a test moves it.
struct Env {
    data: TempDir,
    now: Arc<Mutex<Timestamp>>,
    service: Service,
}

impl Env {
    fn new() -> Self {
        Self::with_lease(SignedDuration::from_mins(15))
    }

    fn with_lease(lease: SignedDuration) -> Self {
        let data = TempDir::new().unwrap();
        let now = Arc::new(Mutex::new(START.parse::<Timestamp>().unwrap()));
        let service = open(&data, &now, lease);
        Self { data, now, service }
    }

    fn advance(&self, secs: i64) {
        let mut now = self.now.lock().unwrap();
        *now += SignedDuration::from_secs(secs);
    }

    fn db(&self) -> Connection {
        Connection::open(project_db_path(self.data.path(), "test")).unwrap()
    }

    /// The lease holder and expiry, straight from the database.
    fn lease(&self, id: TaskId) -> Option<(String, String)> {
        self.db()
            .query_row(
                "SELECT a.name, l.expires_at FROM leases l
                 JOIN actors a ON a.id = l.actor JOIN tasks t ON t.seq = l.task
                 WHERE t.id = ?1",
                [id.as_str()],
                |row| {
                    let ms: i64 = row.get(1)?;
                    Ok((
                        row.get(0)?,
                        timestamp_text(Timestamp::from_millisecond(ms).unwrap()),
                    ))
                },
            )
            .ok()
    }

    /// The task in the Markdown format, as `get_task` shows it.
    fn markdown(&self, id: TaskId) -> String {
        self.service.get_task(&Actor::User, id).unwrap().markdown
    }

    /// The changelog as `who · event` lines, with any text after a colon.
    fn changelog(&self, id: TaskId) -> Vec<String> {
        let detail = self.service.get_task(&Actor::User, id).unwrap();
        detail
            .content
            .changelog
            .iter()
            .map(|e| match e.text.as_str() {
                "" => format!("{} · {}", e.who, e.event),
                text => format!("{} · {}: {text}", e.who, e.event),
            })
            .collect()
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

    fn count(&self, sql: &str) -> i64 {
        self.db().query_row(sql, [], |row| row.get(0)).unwrap()
    }
}

fn open(data: &TempDir, now: &Arc<Mutex<Timestamp>>, lease: SignedDuration) -> Service {
    let clock = Arc::clone(now);
    let config = Config {
        lease_duration: lease,
        user_name: "sam".into(),
        clock: Arc::new(move || *clock.lock().unwrap()),
        time_zone: TimeZone::UTC,
    };
    Service::open(data.path(), "test", config).unwrap()
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

fn lines(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn agent_tasks_start_in_backlog_and_yours_in_todo() {
    let env = Env::new();
    let saved = env
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
                labels: vec!["auth".into(), "backend".into(), "Auth".into()],
                ..NewTask::default()
            },
        )
        .unwrap();
    let id = saved.id;
    assert_eq!(
        saved,
        Saved {
            id,
            status: Status::Backlog
        }
    );
    // Ids are random, and one YAML would read as a number is quoted.
    let markdown = env
        .markdown(id)
        .replacen(&format!("id: \"{id}\"\n"), &format!("id: {id}\n"), 1);
    assert_eq!(
        markdown,
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
    let summary = env.summary(mine);
    assert_eq!(summary.status, Status::Todo);
    assert_eq!(summary.priority, Priority::Medium);
    assert_eq!(summary.created, Date::constant(2026, 10, 15));
    assert_eq!(env.changelog(mine), ["sam · created"]);
}

#[test]
fn start_task_is_refused_on_a_backlog_task_until_approved() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&a, "Proposed", &[]);
    let err = env.service.start_task(&a, id).unwrap_err();
    assert_eq!(err.code(), "invalid_transition");
    assert_eq!(
        err.to_string(),
        format!("cannot start task `{id}` while it is backlog")
    );
    assert_eq!(env.summary(id).status, Status::Backlog);

    // Backlog → Todo: you approve it.
    env.service.approve(id).unwrap();
    assert_eq!(env.changelog(id), ["agent-a · created", "sam · approved"]);
    env.service.start_task(&a, id).unwrap();
    assert_eq!(env.summary(id).status, Status::InProgress);
}

#[test]
fn every_transition_records_exactly_its_changes() {
    let env = Env::new();
    let (a, b) = (agent("agent-a"), agent("agent-b"));
    let id = env.create(&Actor::User, "Task", &["One", "Two"]);

    // Todo → In progress, by an agent: a lease is created.
    env.service.start_task(&a, id).unwrap();
    assert_eq!(env.summary(id).status, Status::InProgress);
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

    // Blocked on and off, and a note: the working log, not the changelog.
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
    assert_eq!(log[2].created_at, "2026-10-15T09:05:00.000Z");
    assert_eq!(env.changelog(id), ["sam · created", "agent-a · started"]);

    // A milestone.
    env.service
        .add_changelog_entry(&a, id, "Halfway.", vec!["src/a.rs".into()])
        .unwrap();
    assert_eq!(
        env.changelog(id).last().unwrap(),
        "agent-a · milestone: Halfway.\nFiles: src/a.rs"
    );

    // In progress → Review: criteria ticked, a finished entry with the
    // summary, lease released.
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
    let markdown = env.markdown(id);
    assert!(markdown.contains("status: finished\n"), "{markdown}");
    assert!(markdown.contains("- [x] One\n- [x] Two\n"), "{markdown}");
    assert!(
        markdown.ends_with(
            "### 2026-10-15 · agent-a · finished\nDid it.\nFiles: src/a.rs, src/b.rs\n\
             Follow-ups: none\n"
        ),
        "{markdown}"
    );
    assert_eq!(env.lease(id), None);

    // Review → In progress: reopen with feedback, for the next agent.
    env.service.reopen(id, "Also handle 403.").unwrap();
    let detail = env.service.get_task(&b, id).unwrap();
    assert_eq!(detail.review_feedback.as_deref(), Some("Also handle 403."));
    assert!(
        detail
            .markdown
            .ends_with("### 2026-10-15 · sam · reopened\nAlso handle 403.\n")
    );
    let last = detail.work_log.last().unwrap();
    assert_eq!(
        (last.agent_id.as_deref(), last.kind, last.message.as_str()),
        (None, LogKind::Feedback, "Also handle 403.")
    );
    assert_eq!(env.lease(id), None);

    // The next agent picks the reopened task up.
    env.service.start_task(&b, id).unwrap();
    assert_eq!(env.lease(id).unwrap().0, "agent-b");

    // In progress → Todo: released with a note, lease removed.
    env.service.release_task(&b, id, "Out of time.").unwrap();
    assert_eq!(env.summary(id).status, Status::Todo);
    assert_eq!(env.lease(id), None);

    // Todo → In progress and on to Review, by you: no lease.
    env.service.start_task(&Actor::User, id).unwrap();
    assert_eq!(env.lease(id), None);
    env.service
        .finish_task(&Actor::User, id, finish("Finished it myself."))
        .unwrap();

    // Review → Done: status and reviewed_at, with a reviewed entry.
    env.advance(86_400);
    env.service.mark_reviewed(id).unwrap();
    let detail = env.service.get_task(&Actor::User, id).unwrap();
    assert_eq!(detail.summary.status, Status::Done);
    assert_eq!(
        detail.content.reviewed_at,
        Some(Date::constant(2026, 10, 16))
    );
    assert!(detail.markdown.contains(
        "status: done\npriority: medium\ncreated: 2026-10-15\nreviewed_at: 2026-10-16\n"
    ));
    assert_eq!(
        env.changelog(id),
        lines(&[
            "sam · created",
            "agent-a · started",
            "agent-a · milestone: Halfway.\nFiles: src/a.rs",
            "agent-a · finished: Did it.\nFiles: src/a.rs, src/b.rs\nFollow-ups: none",
            "sam · reopened: Also handle 403.",
            "agent-b · started",
            "agent-b · released: Out of time.",
            "sam · started",
            "sam · finished: Finished it myself.",
            "sam · reviewed",
        ])
    );

    // Moving out of done clears reviewed_at.
    env.db()
        .execute("UPDATE tasks SET status = 'finished'", [])
        .unwrap();
    env.service.reopen(id, "Not quite.").unwrap();
    assert_eq!(
        env.service
            .get_task(&Actor::User, id)
            .unwrap()
            .content
            .reviewed_at,
        None
    );
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
    let events = env.count("SELECT count(*) FROM events");

    let u = Actor::User;
    let attempts: Vec<(&str, Result<Saved, ServiceError>)> = vec![
        ("approve todo", env.service.approve(todo)),
        ("approve in_progress", env.service.approve(in_progress)),
        ("start backlog (you)", env.service.start_task(&u, backlog)),
        (
            "start in_progress (you)",
            env.service.start_task(&u, in_progress),
        ),
        ("start finished", env.service.start_task(&a, finished)),
        ("start done", env.service.start_task(&a, done)),
        (
            "finish todo",
            env.service.finish_task(&u, todo, finish("x")),
        ),
        (
            "finish done",
            env.service.finish_task(&u, done, finish("x")),
        ),
        ("release todo", env.service.release_task(&u, todo, "")),
        (
            "release finished",
            env.service.release_task(&u, finished, ""),
        ),
        ("review in_progress", env.service.mark_reviewed(in_progress)),
        ("review done", env.service.mark_reviewed(done)),
        ("reopen todo", env.service.reopen(todo, "x")),
        ("reopen done", env.service.reopen(done, "x")),
        (
            "milestone todo",
            env.service.add_changelog_entry(&u, todo, "x", vec![]),
        ),
    ];
    for (name, result) in attempts {
        assert_eq!(code(result), "invalid_transition", "{name}");
    }
    assert_eq!(env.count("SELECT count(*) FROM events"), events);
    let statuses: Vec<Status> = [backlog, todo, in_progress, finished, done]
        .map(|id| env.summary(id).status)
        .to_vec();
    assert_eq!(
        statuses,
        [
            Status::Backlog,
            Status::Todo,
            Status::InProgress,
            Status::Finished,
            Status::Done
        ]
    );
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
    let events = env.count("SELECT count(*) FROM events");
    env.service.start_task(&a, id).unwrap();
    assert_eq!(env.count("SELECT count(*) FROM events"), events);

    // You may always act, from the board.
    env.service
        .log_progress(&Actor::User, id, "Note from you.")
        .unwrap();
    env.service.release_task(&Actor::User, id, "").unwrap();
    assert_eq!(env.changelog(id).last().unwrap(), "sam · released");
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
    assert_eq!(
        env.service.next_task(&a, None, None).unwrap().unwrap().id,
        dep
    );

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
    let db = env.db();
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
    let started: Vec<String> = db
        .prepare(
            "SELECT a.name FROM events e JOIN actors a ON a.id = e.actor WHERE e.kind = 'started'",
        )
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(started, [format!("agent-{}", winners[0])]);
}

#[test]
fn concurrent_starts_through_two_service_instances_have_one_winner() {
    // Two processes sharing a project database: only BEGIN IMMEDIATE keeps
    // them apart.
    let env = Env::new();
    let id = env.todo("Contended");
    let other = Arc::new(open(&env.data, &env.now, SignedDuration::from_mins(15)));
    let db = env.db();
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
    let started: i64 = db
        .query_row(
            "SELECT count(*) FROM events WHERE kind = 'started'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(started, 1);
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
    env.service.start_task(&b, id).unwrap();
    assert_eq!(
        env.changelog(id).last().unwrap(),
        "agent-b · started: Took over from agent-a after its lease expired."
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
    let before = env.markdown(id);

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
    assert_eq!(env.markdown(id), before);

    attempt(vec![3, 1], vec![(2, "needs a staging account")]).unwrap();
    let markdown = env.markdown(id);
    assert!(markdown.contains("- [x] One\n- [ ] Two\n- [x] Three\n"));
    assert!(markdown.ends_with(
        "### 2026-10-15 · agent-a · finished\nDone.\nUnmet: 2 (needs a staging account)\n"
    ));
    assert_eq!(
        env.summary(id).unmet,
        [UnmetCriterion {
            number: 2,
            text: "Two".into(),
            reason: "needs a staging account".into(),
        }]
    );

    // Ticked by you since, the criterion drops off the badge.
    env.service
        .update_task(
            &Actor::User,
            id,
            TaskEdit {
                acceptance_criteria: Some(lines(&["One", "Three"])),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    assert_eq!(env.summary(id).unmet, []);

    // Reopened, the card is no longer in Review and shows no badge.
    env.service.reopen(id, "Try again.").unwrap();
    assert_eq!(env.summary(id).unmet, []);
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
    let tasks = env.count("SELECT count(*) FROM tasks");
    assert_eq!(code(create(Some(missing), vec![])), "parent_not_found");
    assert_eq!(code(create(None, vec![missing])), "not_found");
    assert_eq!(env.count("SELECT count(*) FROM tasks"), tasks);

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
    let before = env.markdown(top);
    assert_eq!(code(edit(Some(Some(child)), None)), "cycle_detected");
    assert_eq!(code(edit(None, Some(vec![child]))), "cycle_detected");
    assert_eq!(code(edit(Some(Some(top)), None)), "cycle_detected");
    assert_eq!(code(edit(Some(Some(missing)), None)), "parent_not_found");
    assert_eq!(code(edit(None, Some(vec![missing]))), "not_found");
    assert_eq!(env.markdown(top), before);
    assert_eq!(
        code(env.service.update_task(&u, missing, TaskEdit::default())),
        "not_found"
    );

    let detail = env.service.get_task(&u, top).unwrap();
    assert_eq!(
        detail.children.iter().map(|c| c.id).collect::<Vec<_>>(),
        [child]
    );
    let detail = env.service.get_task(&u, child).unwrap();
    assert_eq!(detail.parent.map(|p| p.id), Some(top));
    assert_eq!(detail.summary.depends_on, [top]);
}

#[test]
fn update_task_edits_the_spec_and_nothing_else() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&Actor::User, "Task", &["One"]);
    env.service.start_task(&a, id).unwrap();
    let before = env.markdown(id);

    // You may change the spec of a task in progress; it stays in progress.
    env.service
        .update_task(
            &Actor::User,
            id,
            TaskEdit {
                title: Some("Better title".into()),
                description: Some("New description.\n\n## Any heading\nis fine.".into()),
                relevant_files: Some(vec!["src/a.rs".into()]),
                priority: Some(Priority::Low),
                labels: Some(vec!["ui".into()]),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    assert_eq!(
        env.markdown(id),
        before
            .replace(
                "title: Task\nstatus: in_progress\npriority: medium\n",
                "title: Better title\nstatus: in_progress\npriority: low\nlabels: [ui]\n"
            )
            .replace("Task.\n", "New description.\n\n## Any heading\nis fine.\n")
            .replace(
                "\n## Changelog",
                "\n## Relevant files\n- src/a.rs\n\n## Changelog"
            )
    );
    assert_eq!(env.summary(id).status, Status::InProgress);

    // An edit that changes nothing records nothing.
    let events = env.subscribe_count(|| {
        env.service
            .update_task(
                &a,
                id,
                TaskEdit {
                    priority: Some(Priority::Low),
                    labels: Some(vec!["UI".into()]),
                    ..TaskEdit::default()
                },
            )
            .unwrap();
    });
    assert_eq!(events, 0);

    // Multi-line values are refused where one line is expected.
    let edit = TaskEdit {
        title: Some("Two\nlines".into()),
        ..TaskEdit::default()
    };
    assert_eq!(
        code(env.service.update_task(&Actor::User, id, edit)),
        "invalid_argument"
    );
}

impl Env {
    /// How many change notifications `f` sends.
    fn subscribe_count(&self, f: impl FnOnce()) -> usize {
        let events = self.service.subscribe();
        f();
        events.try_iter().count()
    }
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
                    description: format!("About {}.", title.to_lowercase()),
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
    assert_eq!(next(&b, Some("UI")), Some(low));

    let list = |filter: TaskFilter| -> Vec<TaskId> {
        env.service
            .list_tasks(&b, &filter)
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect()
    };
    let ready = list(TaskFilter {
        ready_only: true,
        ..TaskFilter::default()
    });
    assert_eq!(ready.len(), 2);
    let by_title = list(TaskFilter {
        query: Some("MEDIUM".into()),
        ..TaskFilter::default()
    });
    assert_eq!(by_title, [medium]);
    let by_description = list(TaskFilter {
        query: Some("about low".into()),
        ..TaskFilter::default()
    });
    assert_eq!(by_description, [low]);
    assert_eq!(env.summary(high).lease.unwrap().agent_id, "agent-a");
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
    let edited = "agent-b · edited: Changed after approval; back in backlog until approved again.";

    // Backlog: anything goes, and it stays in backlog.
    let proposed = env.create(&b, "Proposed", &["One"]);
    let edit = TaskEdit {
        acceptance_criteria: Some(vec!["Two".into()]),
        ..title("Proposed, better")
    };
    env.service.update_task(&a, proposed, edit).unwrap();
    assert_eq!(env.summary(proposed).status, Status::Backlog);
    assert_eq!(env.changelog(proposed), ["agent-b · created"]);

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
    assert_eq!(env.changelog(id), ["sam · created"]);

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
        let before = env.markdown(id);
        let saved = env.service.update_task(&b, id, edit.clone()).unwrap();
        assert_eq!(saved.status, Status::Backlog, "{edit:?}");
        assert_eq!(env.changelog(id).last().unwrap(), edited, "{edit:?}");
        assert_eq!(code(env.service.start_task(&a, id)), "invalid_transition");
        // You approve it again; your own edits never send it back.
        env.service.approve(id).unwrap();
        env.service
            .update_task(&Actor::User, id, title("Task"))
            .unwrap();
        assert_eq!(env.summary(id).status, Status::Todo);
        assert_ne!(env.markdown(id), before);
    }

    // In progress: only the holder, and only details.
    let id = env.create(&Actor::User, "Started", &["One"]);
    env.service.start_task(&a, id).unwrap();
    assert_eq!(
        code(env.service.update_task(&b, id, details.clone())),
        "not_holder"
    );
    let before = env.markdown(id);
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
    assert_eq!(env.markdown(id), before);
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
fn the_starting_agent_recovers_a_lost_lease() {
    let env = Env::new();
    let (a, b) = (agent("agent-a"), agent("agent-b"));
    let id = env.todo("Task");
    env.service.start_task(&a, id).unwrap();
    env.db().execute("DELETE FROM leases", []).unwrap();

    // Another agent can't use the gap; the agent that started it carries on.
    assert_eq!(code(env.service.start_task(&b, id)), "claimed_by_other");
    assert_eq!(
        code(env.service.log_progress(&b, id, "Mine?")),
        "not_holder"
    );
    env.service.log_progress(&a, id, "Still mine.").unwrap();
    assert_eq!(env.lease(id).unwrap().0, "agent-a");

    // Once the task changes hands, the old agent can't recover it.
    env.service.finish_task(&a, id, finish("Done.")).unwrap();
    env.service.reopen(id, "One more thing.").unwrap();
    assert_eq!(
        code(env.service.log_progress(&a, id, "Back?")),
        "not_holder"
    );
    assert_eq!(env.lease(id), None);
    // The reopened task is open to any agent, including the old one.
    env.service.start_task(&a, id).unwrap();
}

#[test]
fn summaries_are_kept_as_written() {
    // Nothing is parsed back out of a summary, so lines that look like a
    // changelog's own are just text.
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&Actor::User, "Task", &["One"]);
    env.service.start_task(&a, id).unwrap();
    env.service
        .finish_task(
            &a,
            id,
            Finish {
                met: vec![1],
                ..finish("Done.\nUnmet: none\n```")
            },
        )
        .unwrap();
    assert_eq!(env.summary(id).unmet, []);
    assert_eq!(
        env.changelog(id).last().unwrap(),
        "agent-a · finished: Done.\nUnmet: none\n```"
    );
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

#[test]
fn every_change_is_sent_to_subscribers_once() {
    let env = Env::new();
    let a = agent("agent-a");
    let events = env.service.subscribe();
    let next = || events.try_recv().ok();
    let changed = |id: TaskId| {
        Some(Changes {
            changed: [id].into(),
            ..Changes::default()
        })
    };

    let id = env.todo("Task");
    assert_eq!(next(), changed(id));
    assert_eq!(next(), None);

    env.service.start_task(&a, id).unwrap();
    assert_eq!(next(), changed(id));
    // Notes and flags count too.
    env.service.log_progress(&a, id, "Working.").unwrap();
    assert_eq!(next(), changed(id));
    env.service.set_blocked(&a, id, "Question").unwrap();
    assert_eq!(next(), changed(id));

    // Reads and refused calls send nothing.
    env.service.get_task(&a, id).unwrap();
    env.service.list_tasks(&a, &TaskFilter::default()).unwrap();
    assert!(env.service.approve(id).is_err());
    assert_eq!(next(), None);

    // A deletion, a restore and a cleanup.
    env.service.release_task(&a, id, "").unwrap();
    assert_eq!(next(), changed(id));
    env.service.delete_task(id).unwrap();
    let removed = Some(Changes {
        removed: [id].into(),
        ..Changes::default()
    });
    assert_eq!(next(), removed);
    env.service.restore_task(id).unwrap();
    assert_eq!(next(), changed(id));
    env.service.delete_task(id).unwrap();
    assert_eq!(next(), removed);
    env.service.purge_deleted().unwrap();
    assert_eq!(next(), removed);

    // A dropped receiver is simply unsubscribed.
    drop(events);
    env.todo("Another");
}

#[test]
fn issued_agent_ids_are_unique_and_known() {
    let env = Env::new();
    let first = env.service.register_agent(Some("Claude Code")).unwrap();
    let second = env.service.register_agent(Some("Claude Code")).unwrap();
    assert_ne!(first, second);
    for id in [&first, &second] {
        let suffix = id.strip_prefix("claude-code-").unwrap();
        assert_eq!(suffix.len(), 4);
        assert!(suffix.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(env.service.is_known_agent(id).unwrap());
    }
    let anonymous = env.service.register_agent(None).unwrap();
    assert!(anonymous.starts_with("agent-"));
    let odd = env.service.register_agent(Some("日本 tool!!")).unwrap();
    assert!(odd.starts_with("tool-"), "{odd}");
    assert!(!env.service.is_known_agent("claude-code").unwrap());
    // Your own row is never an agent.
    assert!(!env.service.is_known_agent("sam").unwrap());
    // Issued ids pass the service's own checks.
    let id = env.todo("Task");
    env.service.start_task(&agent(&first), id).unwrap();
}

#[test]
fn get_task_reads_the_whole_task() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&Actor::User, "Task", &["One", "Two"]);
    env.service
        .update_task(
            &Actor::User,
            id,
            TaskEdit {
                relevant_files: Some(vec!["src/a.rs".into(), "src/b.rs".into()]),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    env.service.start_task(&a, id).unwrap();
    env.service
        .finish_task(
            &a,
            id,
            Finish {
                summary: "Done.".into(),
                files_changed: vec!["src/a.rs".into()],
                met: vec![1],
                unmet: vec![Unmet {
                    criterion: 2,
                    reason: "later".into(),
                }],
                ..Finish::default()
            },
        )
        .unwrap();

    let content = env.service.get_task(&Actor::User, id).unwrap().content;
    assert_eq!(content.description, "Task.");
    assert_eq!(
        content
            .criteria
            .iter()
            .map(|c| (c.number, c.text.as_str(), c.met))
            .collect::<Vec<_>>(),
        [(1, "One", true), (2, "Two", false)]
    );
    assert_eq!(content.relevant_files, ["src/a.rs", "src/b.rs"]);
    assert_eq!(
        env.changelog(id),
        lines(&[
            "sam · created",
            "agent-a · started",
            "agent-a · finished: Done.\nFiles: src/a.rs\nUnmet: 2 (later)",
        ])
    );
    assert_eq!(content.changelog[0].date, Date::constant(2026, 10, 15));
    assert_eq!(content.reviewed_at, None);
}

#[test]
fn update_task_keeps_the_ticks_of_unchanged_criteria() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.create(&Actor::User, "Task", &["One", "Two", "Two", "Three"]);
    env.service.start_task(&a, id).unwrap();
    env.service
        .finish_task(
            &a,
            id,
            Finish {
                summary: "Done.".into(),
                met: vec![1, 2],
                unmet: vec![
                    Unmet {
                        criterion: 3,
                        reason: "later".into(),
                    },
                    Unmet {
                        criterion: 4,
                        reason: "later".into(),
                    },
                ],
                ..Finish::default()
            },
        )
        .unwrap();
    let criteria = |list: &[&str]| TaskEdit {
        acceptance_criteria: Some(lines(list)),
        ..TaskEdit::default()
    };

    // The same list changes nothing.
    let before = env.markdown(id);
    let sent = env.subscribe_count(|| {
        env.service
            .update_task(&Actor::User, id, criteria(&["One", "Two", "Two", "Three"]))
            .unwrap();
    });
    assert_eq!(sent, 0);

    // Reordered and reworded: each ticked text keeps one tick.
    env.service
        .update_task(
            &Actor::User,
            id,
            criteria(&["Two", " One ", "Two", "Three, reworded", "Four"]),
        )
        .unwrap();
    assert_eq!(
        env.markdown(id),
        before.replace(
            "- [x] One\n- [x] Two\n- [ ] Two\n- [ ] Three\n",
            "- [x] Two\n- [x] One\n- [ ] Two\n- [ ] Three, reworded\n- [ ] Four\n"
        )
    );
}

#[test]
fn list_agents_shows_recent_agents_and_lease_holders() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.todo("Held task");
    env.service.start_task(&a, id).unwrap();
    env.service
        .list_tasks(&agent("agent-b"), &TaskFilter::default())
        .unwrap();

    // Two days on, agent-b drops off, agent-a's lease is stale but still
    // listed, and agent-c has just called.
    env.advance(2 * 24 * 3600);
    env.service
        .list_tasks(&agent("agent-c"), &TaskFilter::default())
        .unwrap();
    let agents = env
        .service
        .list_agents(SignedDuration::from_hours(24))
        .unwrap();
    let found: Vec<String> = agents
        .iter()
        .map(|agent| {
            let tasks: Vec<String> = agent
                .tasks
                .iter()
                .map(|task| format!("{} {} live={}", task.id, task.title, task.live))
                .collect();
            format!(
                "{} {} [{}]",
                agent.agent_id,
                agent.last_seen_at,
                tasks.join(", ")
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            "agent-c 2026-10-17T09:00:00.000Z []".to_owned(),
            format!("agent-a 2026-10-15T09:00:00.000Z [{id} Held task live=false]"),
        ]
    );

    // A live lease shows as live.
    env.service.log_progress(&a, id, "Back.").unwrap();
    let agents = env
        .service
        .list_agents(SignedDuration::from_hours(24))
        .unwrap();
    assert_eq!(agents[0].agent_id, "agent-a");
    assert!(agents[0].tasks[0].live);
}

#[test]
fn a_deleted_task_is_hidden_until_restored() {
    let env = Env::new();
    let a = agent("agent-a");
    let todo = env.todo("Dropped");
    let started = env.todo("Abandoned");
    env.service.start_task(&a, started).unwrap();

    env.advance(60);
    env.service.delete_task(todo).unwrap();
    assert_eq!(code(env.service.get_task(&Actor::User, todo)), "not_found");
    assert_eq!(code(env.service.get_task(&a, todo)), "not_found");
    assert_eq!(code(env.service.delete_task(todo)), "not_found");
    assert_eq!(code(env.service.start_task(&a, todo)), "not_found");
    let deleted = env.service.deleted_tasks().unwrap();
    assert_eq!(
        deleted
            .iter()
            .map(|t| (t.id, t.title.as_str(), t.status, t.deleted_at.as_str()))
            .collect::<Vec<_>>(),
        [(todo, "Dropped", Status::Todo, "2026-10-15T09:01:00.000Z")]
    );

    // Once the agent's lease goes stale, its task can go too, lease and all.
    env.advance(16 * 60);
    env.service.delete_task(started).unwrap();
    assert_eq!(env.lease(started), None);
    assert!(
        env.service
            .list_tasks(&Actor::User, &TaskFilter::default())
            .unwrap()
            .is_empty()
    );
    // Deleted tasks are listed, the most recently deleted first.
    assert_eq!(env.service.deleted_tasks().unwrap()[0].id, started);

    // Restored, it is back where it was, with its history.
    env.service.restore_task(todo).unwrap();
    assert_eq!(env.summary(todo).status, Status::Todo);
    assert_eq!(
        env.changelog(todo),
        ["sam · created", "sam · deleted", "sam · restored"]
    );
    assert_eq!(code(env.service.restore_task(todo)), "not_found");
    assert_eq!(env.service.deleted_tasks().unwrap().len(), 1);
}

#[test]
fn delete_task_waits_for_the_agent_working_on_it() {
    let env = Env::new();
    let a = agent("agent-a");
    let id = env.todo("Busy");
    env.service.start_task(&a, id).unwrap();

    let check = env.service.delete_check(id).unwrap();
    assert_eq!(
        check.lease.as_ref().map(|l| l.agent_id.as_str()),
        Some("agent-a")
    );
    assert!(check.blocked());
    let err = env.service.delete_task(id).unwrap_err();
    assert_eq!(err.code(), "claimed_by_other");
    assert!(err.to_string().contains("agent-a"), "{err}");
    assert_eq!(env.summary(id).status, Status::InProgress);

    env.service.release_task(&a, id, "").unwrap();
    assert!(!env.service.delete_check(id).unwrap().blocked());
    env.service.delete_task(id).unwrap();
}

#[test]
fn delete_task_is_refused_while_other_tasks_link_to_it() {
    let env = Env::new();
    let u = Actor::User;
    let top = env.todo("Top");
    let link = |title: &str, parent, depends_on| {
        env.service
            .create_task(
                &u,
                NewTask {
                    title: title.into(),
                    parent,
                    depends_on,
                    ..NewTask::default()
                },
            )
            .unwrap()
            .id
    };
    let child = link("Child", Some(top), vec![]);
    let after = link("After", None, vec![top]);

    let check = env.service.delete_check(top).unwrap();
    assert_eq!(check.children, [child]);
    assert_eq!(check.dependents, [after]);
    assert!(check.blocked());
    let err = env.service.delete_task(top).unwrap_err();
    assert_eq!(err.code(), "task_referenced");
    let message = err.to_string();
    assert!(message.contains(&format!("parent of {child}")), "{message}");
    assert!(
        message.contains(&format!("dependency of {after}")),
        "{message}"
    );

    // Deleted tasks don't count as links.
    env.service.delete_task(child).unwrap();
    let check = env.service.delete_check(top).unwrap();
    assert_eq!((check.children, check.dependents), (vec![], vec![after]));
    env.service
        .update_task(
            &u,
            after,
            TaskEdit {
                depends_on: Some(vec![]),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    env.service.delete_task(top).unwrap();

    // A task can't come back while its parent is deleted.
    assert_eq!(code(env.service.restore_task(child)), "parent_not_found");
    env.service.restore_task(top).unwrap();
    env.service.restore_task(child).unwrap();
    assert_eq!(env.summary(child).parent, Some(top));
    // Nor can a live task link to a deleted one.
    env.service.delete_task(child).unwrap();
    assert_eq!(
        code(env.service.update_task(
            &u,
            after,
            TaskEdit {
                parent: Some(Some(child)),
                ..TaskEdit::default()
            },
        )),
        "parent_not_found"
    );
}

#[test]
fn cleaning_up_removes_deleted_tasks_for_good() {
    let env = Env::new();
    let a = agent("agent-a");
    let keep = env.todo("Keep");
    let parent = env
        .service
        .create_task(
            &Actor::User,
            NewTask {
                title: "Parent".into(),
                labels: vec!["gone".into(), "kept".into()],
                acceptance_criteria: vec!["One".into()],
                ..NewTask::default()
            },
        )
        .unwrap()
        .id;
    let child = env
        .service
        .create_task(
            &Actor::User,
            NewTask {
                title: "Child".into(),
                parent: Some(parent),
                depends_on: vec![keep],
                labels: vec!["kept".into()],
                ..NewTask::default()
            },
        )
        .unwrap()
        .id;
    env.service
        .update_task(
            &Actor::User,
            keep,
            TaskEdit {
                labels: Some(vec!["kept".into()]),
                ..TaskEdit::default()
            },
        )
        .unwrap();
    env.service.start_task(&a, parent).unwrap();
    env.service.log_progress(&a, parent, "Note.").unwrap();
    env.service.release_task(&a, parent, "").unwrap();
    env.service.delete_task(child).unwrap();
    env.service.delete_task(parent).unwrap();

    let mut removed = env.service.purge_deleted().unwrap();
    removed.sort();
    let mut expected = vec![parent, child];
    expected.sort();
    assert_eq!(removed, expected);
    assert!(env.service.deleted_tasks().unwrap().is_empty());
    assert_eq!(code(env.service.restore_task(child)), "not_found");
    // Every row of theirs is gone; the rest stays.
    assert_eq!(env.count("SELECT count(*) FROM tasks"), 1);
    assert_eq!(env.count("SELECT count(*) FROM criteria"), 0);
    assert_eq!(env.count("SELECT count(*) FROM task_deps"), 0);
    assert_eq!(
        env.count("SELECT count(*) FROM events e JOIN tasks t ON t.seq = e.task"),
        env.count("SELECT count(*) FROM events")
    );
    assert_eq!(env.count("SELECT count(*) FROM labels"), 1);
    assert_eq!(env.summary(keep).labels, ["kept"]);
    // The agents stay known.
    assert!(env.service.is_known_agent("agent-a").unwrap());
    // Nothing deleted, nothing to do.
    assert!(env.service.purge_deleted().unwrap().is_empty());
}

#[test]
fn your_name_follows_the_config() {
    let env = Env::new();
    let id = env.todo("Task");
    drop(env.service);
    let config = Config {
        user_name: "Sam Lee".into(),
        time_zone: TimeZone::UTC,
        ..Config::default()
    };
    let service = Service::open(env.data.path(), "test", config).unwrap();
    assert!(!service.is_new());
    let detail = service.get_task(&Actor::User, id).unwrap();
    assert_eq!(detail.content.changelog[0].who, "Sam Lee");
    let db = Connection::open(project_db_path(env.data.path(), "test")).unwrap();
    let users: i64 = db
        .query_row(
            "SELECT count(*) FROM actors WHERE kind = 'user'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(users, 1);
}
