//! Parent and dependency graph checks, following "Parents and loops" in the
//! spec. `parent` must form a tree and `depends_on` must not loop.
//!
//! [`Graph::check_change`] runs before every write from the board or MCP and
//! refuses a change that would close a loop or point at a missing parent.
//! [`Graph::validate`] runs after every rescan, because hand edits and merges
//! can bring loops in; it is linear in the number of tasks and edges.

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use crate::store::{Task, TaskId};

/// The parent and dependency edges of every task in a project. Tasks whose
/// file failed to parse belong here too, with no edges, so that they still
/// count as existing parents.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    nodes: HashMap<TaskId, Edges>,
}

#[derive(Debug, Clone, Default)]
struct Edges {
    parent: Option<TaskId>,
    depends_on: Vec<TaskId>,
}

/// Why a proposed change was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphError {
    /// The task would become its own ancestor. `path` runs from the task up
    /// its new parent chain and back to it.
    #[error("the parent chain would loop: {}", Path(.path))]
    ParentCycle { path: Vec<TaskId> },
    /// A dependency chain would lead back to the task. `path` runs from the
    /// task through its dependencies and back to it.
    #[error("the dependency chain would loop: {}", Path(.path))]
    DependencyCycle { path: Vec<TaskId> },
    #[error("the parent `{parent}` does not exist")]
    ParentNotFound { parent: TaskId },
    #[error("the dependency `{dependency}` does not exist")]
    DependencyNotFound { dependency: TaskId },
}

impl GraphError {
    /// The spec's error code for MCP responses.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ParentCycle { .. } | Self::DependencyCycle { .. } => "cycle_detected",
            Self::ParentNotFound { .. } => "parent_not_found",
            Self::DependencyNotFound { .. } => "not_found",
        }
    }
}

/// Why a task gets the invalid badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Reason {
    /// The task is in a parent loop: it is its own ancestor.
    ParentLoop,
    /// The task's parent does not exist.
    ParentNotFound(TaskId),
    /// The task is in a dependency loop.
    DependencyLoop,
}

impl Reason {
    /// The spec's error code for the same problem.
    pub fn code(self) -> &'static str {
        match self {
            Self::ParentLoop | Self::DependencyLoop => "cycle_detected",
            Self::ParentNotFound(_) => "parent_not_found",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParentLoop => f.write_str("in a parent loop"),
            Self::ParentNotFound(parent) => write!(f, "the parent `{parent}` does not exist"),
            Self::DependencyLoop => f.write_str("in a dependency loop"),
        }
    }
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_tasks<'a>(tasks: impl IntoIterator<Item = &'a Task>) -> Self {
        let mut graph = Self::new();
        for task in tasks {
            graph.insert(task.id, task.parent, task.depends_on.clone());
        }
        graph
    }

    /// Adds a task, or replaces its edges if it is already in the graph.
    pub fn insert(&mut self, id: TaskId, parent: Option<TaskId>, depends_on: Vec<TaskId>) {
        self.nodes.insert(id, Edges { parent, depends_on });
    }

    pub fn remove(&mut self, id: TaskId) {
        self.nodes.remove(&id);
    }

    pub fn contains(&self, id: TaskId) -> bool {
        self.nodes.contains_key(&id)
    }

    /// Checks giving task `id` (new or existing) this `parent` and
    /// `depends_on`, in place of its current edges. The parent and every
    /// dependency must exist, and neither may close a loop. Loops elsewhere
    /// in the graph don't block the change.
    pub fn check_change(
        &self,
        id: TaskId,
        parent: Option<TaskId>,
        depends_on: &[TaskId],
    ) -> Result<(), GraphError> {
        if let Some(parent) = parent {
            self.check_parent(id, parent)?;
        }
        self.check_depends_on(id, depends_on)
    }

    fn check_parent(&self, id: TaskId, parent: TaskId) -> Result<(), GraphError> {
        if parent != id && !self.contains(parent) {
            return Err(GraphError::ParentNotFound { parent });
        }
        let mut path = vec![id];
        let mut seen = HashSet::new();
        let mut current = Some(parent);
        while let Some(task) = current {
            path.push(task);
            if task == id {
                return Err(GraphError::ParentCycle { path });
            }
            // A loop above that doesn't include `id` is not this change's doing.
            if !seen.insert(task) {
                break;
            }
            current = self.nodes.get(&task).and_then(|e| e.parent);
        }
        Ok(())
    }

    /// Depth-first search from the new dependencies for a path back to `id`.
    fn check_depends_on(&self, id: TaskId, depends_on: &[TaskId]) -> Result<(), GraphError> {
        // Each reached task, and the task it was reached from.
        let mut came_from: HashMap<TaskId, TaskId> = HashMap::new();
        let mut stack = Vec::new();
        for &dep in depends_on {
            if dep == id {
                return Err(GraphError::DependencyCycle { path: vec![id, id] });
            }
            if !self.contains(dep) {
                return Err(GraphError::DependencyNotFound { dependency: dep });
            }
            if came_from.insert(dep, id).is_none() {
                stack.push(dep);
            }
        }
        while let Some(task) = stack.pop() {
            for &next in self.depends_on(task) {
                if next == id {
                    let mut path = vec![id];
                    let mut back = task;
                    while back != id {
                        path.push(back);
                        back = came_from[&back];
                    }
                    path[1..].reverse();
                    path.push(id);
                    return Err(GraphError::DependencyCycle { path });
                }
                if let Entry::Vacant(e) = came_from.entry(next) {
                    e.insert(task);
                    stack.push(next);
                }
            }
        }
        Ok(())
    }

    fn depends_on(&self, id: TaskId) -> &[TaskId] {
        self.nodes.get(&id).map_or(&[], |e| &e.depends_on)
    }

    /// Every invalid task with its reasons, in one pass that is linear in the
    /// number of tasks and edges. Only tasks *in* a loop are invalid; a task
    /// below a loop, or depending on one, is not. Dependencies on missing
    /// tasks are not flagged here; [`Graph::check_change`] refuses them.
    pub fn validate(&self) -> BTreeMap<TaskId, Vec<Reason>> {
        // Index order doesn't matter: the result is sorted by id anyway.
        let ids: Vec<TaskId> = self.nodes.keys().copied().collect();
        let index: HashMap<TaskId, usize> =
            ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();

        let mut invalid: BTreeMap<TaskId, Vec<Reason>> = BTreeMap::new();
        let mut parents = Vec::with_capacity(ids.len());
        for id in &ids {
            let parent = self.nodes[id].parent;
            match parent.map(|p| (p, index.get(&p))) {
                Some((p, None)) => {
                    invalid
                        .entry(*id)
                        .or_default()
                        .push(Reason::ParentNotFound(p));
                    parents.push(None);
                }
                Some((_, Some(&i))) => parents.push(Some(i)),
                None => parents.push(None),
            }
        }
        for i in parent_loops(&parents) {
            invalid.entry(ids[i]).or_default().push(Reason::ParentLoop);
        }

        let dependencies: Vec<Vec<usize>> = ids
            .iter()
            .map(|id| {
                self.nodes[id]
                    .depends_on
                    .iter()
                    .filter_map(|dep| index.get(dep).copied())
                    .collect()
            })
            .collect();
        for i in dependency_loops(&dependencies) {
            invalid
                .entry(ids[i])
                .or_default()
                .push(Reason::DependencyLoop);
        }
        invalid
    }
}

/// The tasks on a parent loop. Each task has at most one parent, so walking
/// up from every unvisited task finds each loop once; every task is walked
/// over at most once in total.
fn parent_loops(parents: &[Option<usize>]) -> Vec<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum State {
        New,
        OnWalk,
        Done,
    }
    let mut state = vec![State::New; parents.len()];
    let mut in_loop = Vec::new();
    let mut walk = Vec::new();
    for start in 0..parents.len() {
        let mut current = Some(start);
        while let Some(task) = current {
            match state[task] {
                State::New => {
                    state[task] = State::OnWalk;
                    walk.push(task);
                    current = parents[task];
                }
                State::OnWalk => {
                    let loop_start = walk.iter().position(|&t| t == task).expect("on this walk");
                    in_loop.extend_from_slice(&walk[loop_start..]);
                    break;
                }
                State::Done => break,
            }
        }
        for task in walk.drain(..) {
            state[task] = State::Done;
        }
    }
    in_loop
}

/// The tasks on a dependency loop: those in a strongly connected component
/// with more than one task, or that depend on themselves.
fn dependency_loops(edges: &[Vec<usize>]) -> Vec<usize> {
    let mut tarjan = Tarjan::new(edges);
    for root in 0..edges.len() {
        if tarjan.order[root] == Tarjan::UNSEEN {
            tarjan.search(root);
        }
    }
    tarjan.in_loop
}

/// Tarjan's strongly connected components algorithm, iterative so that deep
/// dependency chains can't overflow the stack.
struct Tarjan<'a> {
    edges: &'a [Vec<usize>],
    /// The order each task was first reached in, or `UNSEEN`.
    order: Vec<usize>,
    /// The lowest order reachable from each task within its component.
    low: Vec<usize>,
    on_stack: Vec<bool>,
    stack: Vec<usize>,
    /// The depth-first path: each task and the index of its next edge.
    path: Vec<(usize, usize)>,
    next_order: usize,
    in_loop: Vec<usize>,
}

impl<'a> Tarjan<'a> {
    const UNSEEN: usize = usize::MAX;

    fn new(edges: &'a [Vec<usize>]) -> Self {
        let n = edges.len();
        Self {
            edges,
            order: vec![Self::UNSEEN; n],
            low: vec![0; n],
            on_stack: vec![false; n],
            stack: Vec::new(),
            path: Vec::new(),
            next_order: 0,
            in_loop: Vec::new(),
        }
    }

    fn visit(&mut self, task: usize) {
        self.order[task] = self.next_order;
        self.low[task] = self.next_order;
        self.next_order += 1;
        self.stack.push(task);
        self.on_stack[task] = true;
        self.path.push((task, 0));
    }

    fn search(&mut self, root: usize) {
        self.visit(root);
        while let Some(&(task, edge)) = self.path.last() {
            if let Some(&next) = self.edges[task].get(edge) {
                self.path.last_mut().expect("path is not empty").1 += 1;
                if self.order[next] == Self::UNSEEN {
                    self.visit(next);
                } else if self.on_stack[next] {
                    self.low[task] = self.low[task].min(self.order[next]);
                }
                continue;
            }
            self.path.pop();
            if let Some(&(caller, _)) = self.path.last() {
                self.low[caller] = self.low[caller].min(self.low[task]);
            }
            if self.low[task] == self.order[task] {
                self.pop_component(task);
            }
        }
    }

    /// Pops the component rooted at `root` off the stack, recording its
    /// tasks if they form a loop.
    fn pop_component(&mut self, root: usize) {
        let start = self
            .stack
            .iter()
            .rposition(|&t| t == root)
            .expect("component root is on the stack");
        let component = self.stack.split_off(start);
        for &member in &component {
            self.on_stack[member] = false;
        }
        if component.len() > 1 || self.edges[root].contains(&root) {
            self.in_loop.extend(component);
        }
    }
}

/// Formats a path as `a → b → a`.
struct Path<'a>(&'a [TaskId]);

impl fmt::Display for Path<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, id) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" → ")?;
            }
            write!(f, "{id}")?;
        }
        Ok(())
    }
}
