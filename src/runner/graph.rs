// This module creates a DAG (Directed Acyclic Graph) that contains all the tasks that needs to be
// executed in order to complete the Crater run. Once the DAG is created, a number of worker
// threads are spawned, and each thread picks the first task without dependencies from the DAG and
// marks it as running, removing it when the task is done. The next task then is picked using a
// depth-first search.
//
//                                   +---+ tc1 <---+
//                                   |             |
//          +---+ crate-complete <---+             +---+ prepare
//          |                        |             |
//          |                        +---+ tc2 <---+
// root <---+
//          |                        +---+ tc1 <---+
//          |                        |             |
//          +---+ crate-complete <---+             +---+ prepare
//                                   |             |
//                                   +---+ tc2 <---+

use crate::config::Config;
use crate::crates::Crate;
use crate::experiments::{Experiment, Mode};
use crate::prelude::*;
use crate::results::{TestResult, WriteResults};
use crate::runner::{
    tasks::{Task, TaskStep},
    RunnerState,
};
use failure::AsFail;
use petgraph::{dot::Dot, graph::NodeIndex, stable_graph::StableDiGraph, Direction};
use std::fmt::{self, Debug};
use std::sync::Arc;

enum Node {
    Task { task: Arc<Task>, running: bool },
    CrateCompleted,
    Root,
}

impl Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Node::Task { ref task, running } => {
                if running {
                    write!(f, "running: {:?}", task)?;
                } else {
                    write!(f, "{:?}", task)?;
                }
            }
            Node::CrateCompleted => write!(f, "crate completed")?,
            Node::Root => write!(f, "root")?,
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(super) enum WalkResult {
    Task(NodeIndex, Arc<Task>),
    Blocked,
    NotBlocked,
    Finished,
}

impl WalkResult {
    pub(super) fn is_finished(&self) -> bool {
        if let WalkResult::Finished = self {
            true
        } else {
            false
        }
    }
}

#[derive(Default)]
pub(super) struct TasksGraph {
    graph: StableDiGraph<Node, ()>,
    root: NodeIndex,
}

impl TasksGraph {
    fn new() -> Self {
        let mut graph = StableDiGraph::new();
        let root = graph.add_node(Node::Root);

        TasksGraph { graph, root }
    }

    fn add_task(&mut self, task: Task, deps: &[NodeIndex]) -> NodeIndex {
        self.add_node(
            Node::Task {
                task: Arc::new(task),
                running: false,
            },
            deps,
        )
    }

    fn add_crate(&mut self, deps: &[NodeIndex]) -> NodeIndex {
        let id = self.add_node(Node::CrateCompleted, deps);
        self.graph.add_edge(self.root, id, ());
        id
    }

    fn add_node(&mut self, node: Node, deps: &[NodeIndex]) -> NodeIndex {
        let id = self.graph.add_node(node);

        for dep in deps {
            self.graph.add_edge(id, *dep, ());
        }

        id
    }

    pub(super) fn next_task<DB: WriteResults>(&mut self, ex: &Experiment, db: &DB) -> WalkResult {
        let root = self.root;
        self.walk_graph(root, ex, db)
    }

    fn walk_graph<DB: WriteResults>(
        &mut self,
        node: NodeIndex,
        ex: &Experiment,
        db: &DB,
    ) -> WalkResult {
        log::trace!("{:?}: walked to node {:?}", node, self.graph[node]);
        // Ensure tasks are only executed if needed
        let mut already_executed = false;
        if let Node::Task {
            ref task,
            running: false,
        } = self.graph[node]
        {
            if !task.needs_exec(ex, db) {
                already_executed = true;
            }
        }
        if already_executed {
            self.mark_as_completed(node);
            return WalkResult::NotBlocked;
        }

        // Try to check for the dependencies of this node
        // The list is collected to make the borrowchecker happy
        let mut neighbors = self.graph.neighbors(node).collect::<Vec<_>>();
        log::trace!("{:?}: neighbors: {:?}", node, neighbors);
        let mut blocked = false;
        for neighbor in neighbors.drain(..) {
            match self.walk_graph(neighbor, ex, db) {
                WalkResult::Task(id, task) => return WalkResult::Task(id, task),
                WalkResult::Finished => return WalkResult::Finished,
                WalkResult::Blocked => blocked = true,
                WalkResult::NotBlocked => {}
            }
        }
        // The early return for Blocked is done outside of the loop, allowing other dependent tasks
        // to be checked first: if they contain a non-blocked task that is returned instead
        if blocked {
            log::trace!("{:?}: this is blocked", node);
            return WalkResult::Blocked;
        }

        let mut delete = false;
        let result = match self.graph[node] {
            Node::Task { running: true, .. } => WalkResult::Blocked,
            Node::Task {
                ref task,
                ref mut running,
            } => {
                *running = true;
                WalkResult::Task(node, task.clone())
            }
            Node::CrateCompleted => {
                // All the steps for this crate were completed
                delete = true;
                WalkResult::NotBlocked
            }
            Node::Root => WalkResult::Finished,
        };

        // This is done after the match to avoid borrowck issues
        if delete {
            log::trace!("{:?}: marked as complete", node);
            self.mark_as_completed(node);
        }

        result
    }

    pub(super) fn mark_as_completed(&mut self, node: NodeIndex) {
        log::debug!("marking node {:?} as complete", self.graph[node]);
        self.graph.remove_node(node);
    }

    pub(super) fn mark_as_failed<DB: WriteResults, F: AsFail>(
        &mut self,
        node: NodeIndex,
        ex: &Experiment,
        db: &DB,
        state: &RunnerState,
        config: &Config,
        error: &F,
        result: TestResult,
    ) -> Fallible<()> {
        let mut children = self
            .graph
            .neighbors_directed(node, Direction::Incoming)
            .collect::<Vec<_>>();
        for child in children.drain(..) {
            // Don't recursively mark a child as failed if this is not the only parent of the child
            let parents = self
                .graph
                .neighbors_directed(child, Direction::Outgoing)
                .count();
            if parents > 1 {
                log::trace!(
                    "{:?}: prevented recursive mark_as_failed as it has other parents",
                    child
                );
                continue;
            }
            self.mark_as_failed(child, ex, db, state, config, error, result)?;
        }

        match self.graph[node] {
            Node::Task { ref task, .. } => {
                log::debug!("marking task {:?} as failed", task);
                task.mark_as_failed(ex, db, state, config, error, result)?
            }
            Node::CrateCompleted | Node::Root => return Ok(()),
        }

        self.mark_as_completed(node);
        Ok(())
    }

    pub(super) fn pending_crates_count(&self) -> usize {
        self.graph.neighbors(self.root).count()
    }

    pub(super) fn generate_dot<'a>(&'a self) -> Dot<&'a StableDiGraph<impl Debug, ()>> {
        Dot::new(&self.graph)
    }
}

pub(super) fn build_graph(ex: &Experiment, crates: &[Crate], config: &Config) -> TasksGraph {
    let mut graph = TasksGraph::new();

    for krate in crates {
        if !ex.ignore_blacklist && config.should_skip(krate) {
            continue;
        }

        let prepare_id = graph.add_task(
            Task {
                krate: krate.clone(),
                step: TaskStep::Prepare,
            },
            &[],
        );

        let quiet = config.is_quiet(krate);
        let mut builds = Vec::new();
        for tc in &ex.toolchains {
            let build_id = graph.add_task(
                Task {
                    krate: krate.clone(),
                    step: match ex.mode {
                        Mode::BuildOnly => TaskStep::BuildOnly {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::BuildAndTest
                            if !ex.ignore_blacklist && config.should_skip_tests(krate) =>
                        {
                            TaskStep::BuildOnly {
                                tc: tc.clone(),
                                quiet,
                            }
                        }
                        Mode::BuildAndTest => TaskStep::BuildAndTest {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::CheckOnly => TaskStep::CheckOnly {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::Clippy => TaskStep::Clippy {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::Rustdoc => TaskStep::Rustdoc {
                            tc: tc.clone(),
                            quiet,
                        },
                        Mode::UnstableFeatures => TaskStep::UnstableFeatures { tc: tc.clone() },
                    },
                },
                &[prepare_id],
            );

            builds.push(build_id);
        }

        let cleanup_id = graph.add_task(
            Task {
                krate: krate.clone(),
                step: TaskStep::Cleanup,
            },
            &builds,
        );

        graph.add_crate(&[cleanup_id]);
    }

    graph
}
