//! Parent and dependency graph checks. `parent` must form a tree and
//! `depends_on` must not loop.
//!
//! [`Graph::check_change`] runs before every write from the board or MCP and
//! refuses a change that would close a loop or point at a missing task.
//! Every write goes through it, so the stored graph never has a loop.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::model::TaskId;

/// The parent and dependency edges of every task in a project that isn't
/// deleted.
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
    /// The error code for MCP responses.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ParentCycle { .. } | Self::DependencyCycle { .. } => "cycle_detected",
            Self::ParentNotFound { .. } => "parent_not_found",
            Self::DependencyNotFound { .. } => "not_found",
        }
    }
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
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
