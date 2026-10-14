use std::fs;
use std::path::{Path, PathBuf};

use tandem_board_core::store::{
    ChangelogEntry, Date, ParseError, Priority, Status, StoreError, Task, TaskId, Unmet,
    create_task, list_task_files, read_task, task_path, unused_id, write_task,
};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn id(s: &str) -> TaskId {
    s.parse().unwrap()
}

fn date(s: &str) -> Date {
    s.parse().unwrap()
}

fn round_trip(text: &str) -> String {
    Task::parse(text).unwrap().to_markdown()
}

fn entry() -> ChangelogEntry {
    ChangelogEntry {
        date: date("2026-10-15"),
        who: "claude-code".into(),
        event: "reopened".into(),
        summary: "Also cover the refresh timeout.".into(),
        files: vec![],
        follow_ups: None,
        unmet: vec![],
    }
}

/// Files in `dir` other than task files, such as leftover temp files.
fn other_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| TaskId::from_file_name(p.file_name().unwrap().to_str().unwrap()).is_none())
        .collect();
    files.sort();
    files
}

// Round trips

#[test]
fn canonical_file_round_trips_byte_for_byte() {
    let text = fixture("canonical.md");
    let task = Task::parse(&text).unwrap();
    assert_eq!(task.id, id("k3f9x2m"));
    assert_eq!(task.title, "Fix token refresh race");
    assert_eq!(task.status, Status::Finished);
    assert_eq!(task.priority, Some(Priority::High));
    assert_eq!(task.parent, Some(id("a7rq4ne")));
    assert_eq!(task.labels, ["auth", "backend"]);
    assert_eq!(task.depends_on, [id("p2xmq7d")]);
    assert_eq!(task.created, date("2026-10-13"));
    assert_eq!(task.reviewed_at, None);
    assert!(task.body.starts_with("\n## Description\n"));
    assert_eq!(task.to_markdown(), text);
}

#[test]
fn unknown_sections_round_trip_byte_for_byte() {
    let text = fixture("unknown_sections.md");
    let task = Task::parse(&text).unwrap();
    assert_eq!(task.title, "Cache: invalidate on logout");
    assert_eq!(task.labels, ["auth", "needs review"]);
    assert_eq!(task.reviewed_at, Some(date("2026-10-14")));
    assert_eq!(task.to_markdown(), text);
}

#[test]
fn missing_optional_fields_round_trip_byte_for_byte() {
    let text = fixture("minimal.md");
    let task = Task::parse(&text).unwrap();
    assert_eq!(task.priority, None);
    assert_eq!(task.parent, None);
    assert!(task.labels.is_empty());
    assert!(task.depends_on.is_empty());
    assert_eq!(task.reviewed_at, None);
    assert_eq!(task.to_markdown(), text);
}

#[test]
fn crlf_input_differs_only_in_line_endings() {
    let text = fixture("crlf.md");
    assert!(
        text.contains("\r\n"),
        "fixture lost its CRLF endings; check .gitattributes"
    );
    let written = round_trip(&text);
    assert!(!written.contains('\r'));
    assert_eq!(written, text.replace("\r\n", "\n"));
}

#[test]
fn hand_edited_frontmatter_is_written_in_canonical_form() {
    assert_eq!(
        round_trip(&fixture("noncanonical.md")),
        fixture("noncanonical.expected.md")
    );
}

#[test]
fn body_gets_exactly_one_trailing_newline() {
    let head = "---\nid: a7rq4ne\ntitle: T\nstatus: todo\ncreated: 2026-10-13\n---\n";
    assert_eq!(
        round_trip(&format!("{head}\nBody")),
        format!("{head}\nBody\n")
    );
    assert_eq!(
        round_trip(&format!("{head}\nBody\n\n\n")),
        format!("{head}\nBody\n")
    );
    assert_eq!(round_trip(head), head);
    assert_eq!(round_trip(head.trim_end()), head);
    assert_eq!(round_trip(&format!("{head}\n\n")), head);
}

#[test]
fn reserved_words_and_punctuation_round_trip() {
    let mut task = Task::parse(&fixture("minimal.md")).unwrap();
    for title in [
        "true",
        "2026",
        "Fix: it",
        "# not a comment",
        "say \"hi\"",
        "line\nbreak",
    ] {
        task.title = title.into();
        task.labels = vec!["a, b".into(), "null".into()];
        let back = Task::parse(&task.to_markdown()).unwrap();
        assert_eq!(back, task, "{title:?}");
    }
}

// Changelog

/// Appends an entry to the parsed fixture and checks that, compared with the
/// fixture as written, the only change is `added` at the end.
fn assert_append_adds_only(fixture_name: &str, entry: &ChangelogEntry, added: &str) {
    let mut task = Task::parse(&fixture(fixture_name)).unwrap();
    let before = task.to_markdown();
    task.append_changelog(entry).unwrap();
    let after = task.to_markdown();

    let (before_lines, after_lines): (Vec<_>, Vec<_>) =
        (before.lines().collect(), after.lines().collect());
    assert_eq!(
        after_lines[..before_lines.len()],
        before_lines[..],
        "{fixture_name}: an existing line changed"
    );
    assert_eq!(&after[before.len()..], added, "{fixture_name}");
}

#[test]
fn changelog_append_only_adds_lines_at_the_end() {
    let added = "\n### 2026-10-15 · claude-code · reopened\nAlso cover the refresh timeout.\n";
    for name in ["canonical.md", "unknown_sections.md", "crlf.md"] {
        assert_append_adds_only(name, &entry(), added);
    }
}

#[test]
fn changelog_append_adds_a_missing_section_at_the_end() {
    assert_append_adds_only(
        "minimal.md",
        &entry(),
        "\n## Changelog\n\n### 2026-10-15 · claude-code · reopened\nAlso cover the refresh timeout.\n",
    );
}

#[test]
fn changelog_entry_with_every_line() {
    let entry = ChangelogEntry {
        event: "finished".into(),
        summary: "Added the timeout test.".into(),
        files: vec![
            "src/auth/session.ts".into(),
            "src/auth/timeout.test.ts".into(),
        ],
        follow_ups: Some("Retry with backoff (new task)".into()),
        unmet: vec![Unmet {
            criterion: 2,
            reason: "the staging API is down".into(),
        }],
        ..entry()
    };
    assert_append_adds_only(
        "canonical.md",
        &entry,
        "\n### 2026-10-15 · claude-code · finished\n\
         Added the timeout test.\n\
         Files: src/auth/session.ts, src/auth/timeout.test.ts\n\
         Follow-ups: Retry with backoff (new task)\n\
         Unmet: 2 (the staging API is down)\n",
    );
}

#[test]
fn appended_entries_survive_a_round_trip() {
    let mut task = Task::parse(&fixture("canonical.md")).unwrap();
    task.append_changelog(&entry()).unwrap();
    let written = task.to_markdown();
    assert_eq!(round_trip(&written), written);
}

// Parse errors

fn parse_err(text: &str) -> ParseError {
    Task::parse(text).expect_err("parsed")
}

#[test]
fn rejects_files_without_frontmatter() {
    assert!(matches!(
        parse_err("# Title\n"),
        ParseError::MissingFrontmatter
    ));
    assert!(matches!(
        parse_err(" ---\n"),
        ParseError::MissingFrontmatter
    ));
    assert!(matches!(
        parse_err("---\nid: a7rq4ne\n"),
        ParseError::UnclosedFrontmatter
    ));
}

#[test]
fn rejects_invalid_frontmatter() {
    let valid = fixture("minimal.md");
    let cases = [
        (
            "unknown key",
            valid.replace("status:", "owner: me\nstatus:"),
        ),
        (
            "duplicate key",
            valid.replace("status:", "status: todo\nstatus:"),
        ),
        (
            "missing title",
            valid.replace("title: Auth hardening\n", ""),
        ),
        (
            "missing created",
            valid.replace("created: 2026-10-13\n", ""),
        ),
        (
            "bad status",
            valid.replace("status: backlog", "status: doing"),
        ),
        (
            "bad priority",
            valid.replace("status:", "priority: urgent\nstatus:"),
        ),
        ("bad id", valid.replace("id: a7rq4ne", "id: A7RQ4NE")),
        (
            "bad parent",
            valid.replace("status:", "parent: nope\nstatus:"),
        ),
        ("bad date", valid.replace("2026-10-13", "13/10/2026")),
        (
            "bad YAML",
            valid.replace("status: backlog", "status: [backlog"),
        ),
    ];
    for (case, text) in cases {
        assert!(
            matches!(parse_err(&text), ParseError::Frontmatter(_)),
            "{case}"
        );
    }
    assert!(matches!(
        parse_err(&valid.replace("Auth hardening", "\"  \"")),
        ParseError::EmptyTitle
    ));
}

#[test]
fn frontmatter_errors_are_one_line_with_file_line_numbers() {
    let text = fixture("minimal.md").replace("status: backlog", "status: doing");
    let message = parse_err(&text).to_string();
    assert!(!message.contains('\n'), "{message}");
    // `status` is on line 4 of the file, counting the opening `---`.
    assert!(message.contains("line 4, column 9"), "{message}");
    assert!(message.contains("unknown variant `doing`"), "{message}");
}

// Files on disk

#[test]
fn created_file_reads_back_and_has_the_written_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let task = Task::parse(&fixture("canonical.md")).unwrap();
    let path = create_task(dir.path(), &task).unwrap();
    assert_eq!(path, dir.path().join("k3f9x2m.md"));
    assert_eq!(fs::read_to_string(&path).unwrap(), fixture("canonical.md"));
    assert_eq!(read_task(&path).unwrap(), task);
    assert!(other_files(dir.path()).is_empty());
}

#[test]
fn create_refuses_an_existing_id() {
    let dir = tempfile::tempdir().unwrap();
    let task = Task::parse(&fixture("minimal.md")).unwrap();
    create_task(dir.path(), &task).unwrap();
    let mut other = task.clone();
    other.title = "Someone else's task".into();
    assert!(matches!(
        create_task(dir.path(), &other),
        Err(StoreError::AlreadyExists { .. })
    ));
    assert_eq!(read_task(&task_path(dir.path(), task.id)).unwrap(), task);
    assert!(other_files(dir.path()).is_empty());
}

#[test]
fn write_replaces_the_file_and_leaves_no_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = Task::parse(&fixture("minimal.md")).unwrap();
    write_task(dir.path(), &task).unwrap();
    task.status = Status::Todo;
    task.append_changelog(&entry()).unwrap();
    let path = write_task(dir.path(), &task).unwrap();
    assert_eq!(read_task(&path).unwrap(), task);
    assert!(other_files(dir.path()).is_empty());
}

#[test]
fn status_change_touches_only_the_status_line() {
    let dir = tempfile::tempdir().unwrap();
    let before = fixture("canonical.md");
    let mut task = Task::parse(&before).unwrap();
    task.status = Status::Done;
    let path = write_task(dir.path(), &task).unwrap();
    let after = fs::read_to_string(path).unwrap();
    let changed: Vec<_> = before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .collect();
    assert_eq!(changed, [("status: finished", "status: done")]);
    assert_eq!(before.lines().count(), after.lines().count());
}

#[test]
fn invalid_file_is_reported_and_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a7rq4ne.md");
    let broken = fixture("minimal.md").replace("status: backlog", "status: [backlog");
    fs::write(&path, &broken).unwrap();

    assert!(matches!(read_task(&path), Err(StoreError::Invalid { .. })));
    let task = Task::parse(&fixture("minimal.md")).unwrap();
    assert!(matches!(
        write_task(dir.path(), &task),
        Err(StoreError::Invalid { .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    assert!(other_files(dir.path()).is_empty());
}

#[test]
fn non_utf8_file_is_invalid_and_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a7rq4ne.md");
    fs::write(&path, b"---\n\xff\n---\n").unwrap();
    assert!(matches!(
        read_task(&path),
        Err(StoreError::Invalid {
            source: ParseError::NotUtf8,
            ..
        })
    ));
    let task = Task::parse(&fixture("minimal.md")).unwrap();
    assert!(write_task(dir.path(), &task).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"---\n\xff\n---\n");
}

#[test]
fn id_must_match_the_file_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b2b2b2b.md");
    fs::write(&path, fixture("minimal.md")).unwrap();
    assert!(matches!(
        read_task(&path),
        Err(StoreError::Invalid {
            source: ParseError::IdMismatch { .. },
            ..
        })
    ));
    assert!(matches!(
        read_task(&dir.path().join("README.md")),
        Err(StoreError::NotTaskFile { .. })
    ));
}

#[test]
fn never_writes_a_task_it_could_not_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut task = Task::parse(&fixture("minimal.md")).unwrap();
    task.title = " ".into();
    assert!(matches!(
        write_task(dir.path(), &task),
        Err(StoreError::Invalid {
            source: ParseError::EmptyTitle,
            ..
        })
    ));
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn listing_reads_only_id_dot_md_files() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    for name in [
        "k3f9x2m.md",
        "a7rq4ne.md",
        "README.md",
        "config.toml",
        "K3F9X2M.md",
        "k3f9x2.md",
        "k3f9x2l.md",
        "k3f9x2m.md.bak",
        ".tmp-abc.tandem",
        "notes.txt",
    ] {
        fs::write(d.join(name), "").unwrap();
    }
    fs::create_dir(d.join("p2xmq7d.md")).unwrap();

    let listed = list_task_files(d).unwrap();
    assert_eq!(
        listed,
        [
            (id("a7rq4ne"), d.join("a7rq4ne.md")),
            (id("k3f9x2m"), d.join("k3f9x2m.md")),
        ]
    );
}

#[test]
fn unused_id_has_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let id = unused_id(dir.path()).unwrap();
    assert!(!task_path(dir.path(), id).exists());
}

#[cfg(unix)]
#[test]
fn writes_keep_normal_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    // A new task file gets the same mode as any other new file here.
    let reference = dir.path().join("reference.txt");
    fs::write(&reference, "").unwrap();

    let task = Task::parse(&fixture("minimal.md")).unwrap();
    let path = create_task(dir.path(), &task).unwrap();
    assert_eq!(mode(&path), mode(&reference));

    // A replaced file keeps its mode, even bits the umask would clear.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o664)).unwrap();
    write_task(dir.path(), &task).unwrap();
    assert_eq!(mode(&path), 0o664);
}
