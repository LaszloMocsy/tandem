use std::collections::BTreeMap;

use tandem_board_core::graph::{Graph, GraphError, Reason};
use tandem_board_core::store::{ID_ALPHABET, ID_LEN, Task, TaskId};

/// A distinct, valid id for every number.
fn id(mut n: usize) -> TaskId {
    let mut s = [ID_ALPHABET[0]; ID_LEN];
    for c in s.iter_mut().rev() {
        *c = ID_ALPHABET[n % 32];
        n /= 32;
    }
    std::str::from_utf8(&s).unwrap().parse().unwrap()
}

/// A graph from `(task, parent, depends_on)` triples, using numbers as ids.
fn graph(tasks: &[(usize, Option<usize>, &[usize])]) -> Graph {
    let mut graph = Graph::new();
    for &(task, parent, deps) in tasks {
        graph.insert(
            id(task),
            parent.map(id),
            deps.iter().copied().map(id).collect(),
        );
    }
    graph
}

fn invalid(entries: &[(usize, &[Reason])]) -> BTreeMap<TaskId, Vec<Reason>> {
    entries
        .iter()
        .map(|&(task, reasons)| (id(task), reasons.to_vec()))
        .collect()
}

fn ids(path: &[usize]) -> Vec<TaskId> {
    path.iter().copied().map(id).collect()
}

const NONE: &[usize] = &[];

#[test]
fn self_parent() {
    let g = graph(&[(1, Some(1), NONE)]);
    assert_eq!(g.validate(), invalid(&[(1, &[Reason::ParentLoop])]));

    let err = g.check_change(id(1), Some(id(1)), &[]).unwrap_err();
    assert_eq!(err, GraphError::ParentCycle { path: ids(&[1, 1]) });
    assert_eq!(err.code(), "cycle_detected");

    // A new task naming itself as parent is a loop, not a missing parent.
    assert_eq!(
        Graph::new().check_change(id(2), Some(id(2)), &[]),
        Err(GraphError::ParentCycle { path: ids(&[2, 2]) })
    );
}

#[test]
fn three_task_parent_loop() {
    // 1 → 2 → 3 → 1, and 4 is a child of 1.
    let g = graph(&[
        (1, Some(2), NONE),
        (2, Some(3), NONE),
        (3, Some(1), NONE),
        (4, Some(1), NONE),
    ]);
    // Only tasks in the loop are invalid; the child below it is not.
    assert_eq!(
        g.validate(),
        invalid(&[
            (1, &[Reason::ParentLoop]),
            (2, &[Reason::ParentLoop]),
            (3, &[Reason::ParentLoop]),
        ])
    );

    // The same loop proposed through a write.
    let g = graph(&[(1, Some(2), NONE), (2, Some(3), NONE), (3, None, NONE)]);
    assert_eq!(
        g.check_change(id(3), Some(id(1)), &[]),
        Err(GraphError::ParentCycle {
            path: ids(&[3, 1, 2, 3])
        })
    );
    assert_eq!(
        g.check_change(id(3), Some(id(1)), &[])
            .unwrap_err()
            .to_string(),
        format!(
            "the parent chain would loop: {} → {} → {} → {}",
            id(3),
            id(1),
            id(2),
            id(3)
        )
    );
    // Moving the top task under an unrelated one is fine.
    let mut g = g;
    g.insert(id(9), None, vec![]);
    assert_eq!(g.check_change(id(3), Some(id(9)), &[]), Ok(()));
}

#[test]
fn dependency_loop() {
    // 1 needs 2 needs 3 needs 1, and 4 needs 1.
    let g = graph(&[
        (1, None, &[2]),
        (2, None, &[3]),
        (3, None, &[1]),
        (4, None, &[1]),
    ]);
    assert_eq!(
        g.validate(),
        invalid(&[
            (1, &[Reason::DependencyLoop]),
            (2, &[Reason::DependencyLoop]),
            (3, &[Reason::DependencyLoop]),
        ])
    );

    let g = graph(&[(1, None, &[2]), (2, None, &[3]), (3, None, NONE)]);
    let err = g.check_change(id(3), None, &[id(1)]).unwrap_err();
    assert_eq!(
        err,
        GraphError::DependencyCycle {
            path: ids(&[3, 1, 2, 3])
        }
    );
    assert_eq!(err.code(), "cycle_detected");
    assert_eq!(
        g.check_change(id(3), None, &[id(3)]),
        Err(GraphError::DependencyCycle { path: ids(&[3, 3]) })
    );
}

#[test]
fn self_dependency_is_a_loop() {
    let g = graph(&[(1, None, &[1])]);
    assert_eq!(g.validate(), invalid(&[(1, &[Reason::DependencyLoop])]));
}

#[test]
fn diamond_dependencies_are_not_a_loop() {
    // 1 needs 2 and 3, which both need 4.
    let g = graph(&[
        (1, None, &[2, 3]),
        (2, None, &[4]),
        (3, None, &[4]),
        (4, None, NONE),
    ]);
    assert!(g.validate().is_empty());
    assert_eq!(g.check_change(id(5), None, &[id(1), id(4)]), Ok(()));
}

#[test]
fn missing_parent() {
    let g = graph(&[(1, Some(9), NONE), (2, Some(1), NONE)]);
    assert_eq!(
        g.validate(),
        invalid(&[(1, &[Reason::ParentNotFound(id(9))])])
    );

    let err = g.check_change(id(2), Some(id(8)), &[]).unwrap_err();
    assert_eq!(err, GraphError::ParentNotFound { parent: id(8) });
    assert_eq!(err.code(), "parent_not_found");
    assert_eq!(Reason::ParentNotFound(id(9)).code(), "parent_not_found");
}

#[test]
fn a_parent_whose_file_is_invalid_still_exists() {
    // Tasks whose file failed to parse are in the graph with no edges.
    let mut g = graph(&[(1, Some(2), NONE)]);
    g.insert(id(2), None, vec![]);
    assert!(g.validate().is_empty());
    assert_eq!(g.check_change(id(3), Some(id(2)), &[]), Ok(()));
}

#[test]
fn missing_dependency_is_refused_with_not_found() {
    let g = graph(&[(1, None, NONE)]);
    let err = g.check_change(id(2), None, &[id(1), id(9)]).unwrap_err();
    assert_eq!(err, GraphError::DependencyNotFound { dependency: id(9) });
    assert_eq!(err.code(), "not_found");
    assert_eq!(
        err.to_string(),
        format!("the dependency `{}` does not exist", id(9))
    );
    // A new task depending on itself is a loop, not a missing dependency.
    assert_eq!(
        g.check_change(id(2), None, &[id(2)]),
        Err(GraphError::DependencyCycle { path: ids(&[2, 2]) })
    );
}

#[test]
fn missing_dependencies_already_on_disk_are_not_invalid() {
    // Hand edits can still bring one in; the spec only marks loops and
    // missing parents invalid, and start_task waits for it anyway.
    let g = graph(&[(1, None, &[9])]);
    assert!(g.validate().is_empty());
}

#[test]
fn valid_deep_tree() {
    // A chain 100,000 deep, both as parents and as dependencies, plus a few
    // leaves on every level near the top: deep enough to overflow a
    // recursive walk.
    const DEPTH: usize = 100_000;
    let mut g = Graph::new();
    g.insert(id(0), None, vec![]);
    for n in 1..DEPTH {
        g.insert(id(n), Some(id(n - 1)), vec![id(n - 1)]);
    }
    for n in 0..100 {
        for leaf in 0..3 {
            let leaf = DEPTH + n * 3 + leaf;
            g.insert(id(leaf), Some(id(n)), vec![id(n)]);
        }
    }
    assert!(g.validate().is_empty());

    let deepest = id(DEPTH - 1);
    assert_eq!(
        g.check_change(id(2 * DEPTH), Some(deepest), &[deepest]),
        Ok(())
    );

    // Hanging the root under its deepest descendant closes a loop through
    // every level.
    match g.check_change(id(0), Some(deepest), &[]) {
        Err(GraphError::ParentCycle { path }) => {
            assert_eq!(path.len(), DEPTH + 1);
            assert_eq!((path[0], path[1], path[DEPTH]), (id(0), deepest, id(0)));
        }
        other => panic!("expected a parent cycle, got {other:?}"),
    }
    match g.check_change(id(0), None, &[deepest]) {
        Err(GraphError::DependencyCycle { path }) => assert_eq!(path.len(), DEPTH + 1),
        other => panic!("expected a dependency cycle, got {other:?}"),
    }
}

#[test]
fn a_loop_elsewhere_does_not_block_a_change() {
    // 1 and 2 are each other's parent and dependency; 3 is unrelated.
    let g = graph(&[(1, Some(2), &[2]), (2, Some(1), &[1]), (3, None, NONE)]);
    assert_eq!(g.check_change(id(3), Some(id(1)), &[id(1)]), Ok(()));
    assert_eq!(g.check_change(id(4), Some(id(2)), &[id(2)]), Ok(()));
}

#[test]
fn a_change_that_breaks_a_loop_is_allowed() {
    let g = graph(&[(1, Some(2), &[2]), (2, Some(1), &[1])]);
    assert_eq!(g.check_change(id(2), None, &[]), Ok(()));
}

#[test]
fn a_task_can_have_several_reasons() {
    // 1 has a missing parent and is in a dependency loop with 2.
    let g = graph(&[(1, Some(9), &[2]), (2, None, &[1])]);
    assert_eq!(
        g.validate(),
        invalid(&[
            (1, &[Reason::ParentNotFound(id(9)), Reason::DependencyLoop]),
            (2, &[Reason::DependencyLoop]),
        ])
    );
}

#[test]
fn builds_from_parsed_tasks() {
    let task = |text: &str| Task::parse(text).unwrap();
    let parent = task("---\nid: a7rq4ne\ntitle: P\nstatus: todo\ncreated: 2026-10-13\n---\n");
    let child = task(
        "---\nid: k3f9x2m\ntitle: C\nstatus: todo\nparent: a7rq4ne\n\
         depends_on: [p2xmq7d]\ncreated: 2026-10-13\n---\n",
    );
    let g = Graph::from_tasks([&parent, &child]);
    assert!(g.validate().is_empty());
    assert_eq!(
        g.check_change(parent.id, Some(child.id), &[]),
        Err(GraphError::ParentCycle {
            path: vec![parent.id, child.id, parent.id]
        })
    );
}
