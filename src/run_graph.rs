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

use config::Config;
use crossbeam_utils::thread::scope;
use errors::*;
use experiments::{Experiment, Mode};
use file;
use petgraph::{dot::Dot, graph::NodeIndex, stable_graph::StableDiGraph, Direction};
use results::{TestResult, WriteResults};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use tasks::{Task, TaskStep};
use utils;

pub enum Node {
    Task { task: Arc<Task>, running: bool },
    CrateCompleted,
    Root,
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Node::Task { ref task, running } => if running {
                write!(f, "running: {:?}", task)?;
            } else {
                write!(f, "{:?}", task)?;
            },
            Node::CrateCompleted => write!(f, "crate completed")?,
            Node::Root => write!(f, "root")?,
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum WalkResult {
    Task(NodeIndex, Arc<Task>),
    Blocked,
    NotBlocked,
    Finished,
}

impl WalkResult {
    pub fn is_finished(&self) -> bool {
        if let WalkResult::Finished = self {
            true
        } else {
            false
        }
    }
}

#[derive(Default)]
pub struct TasksGraph {
    graph: StableDiGraph<Node, ()>,
    root: NodeIndex,
}

impl TasksGraph {
    pub fn new() -> Self {
        let mut graph = StableDiGraph::new();
        let root = graph.add_node(Node::Root);

        TasksGraph { graph, root }
    }

    pub fn add_task(&mut self, task: Task, deps: &[NodeIndex]) -> NodeIndex {
        self.add_node(
            Node::Task {
                task: Arc::new(task),
                running: false,
            },
            deps,
        )
    }

    pub fn add_crate(&mut self, deps: &[NodeIndex]) -> NodeIndex {
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

    pub fn next_task<DB: WriteResults>(&mut self, ex: &Experiment, db: &DB) -> WalkResult {
        let root = self.root;
        self.walk_graph(root, ex, db)
    }

    fn walk_graph<DB: WriteResults>(
        &mut self,
        node: NodeIndex,
        ex: &Experiment,
        db: &DB,
    ) -> WalkResult {
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
            self.mark_as_completed(node);
        }

        result
    }

    pub fn mark_as_completed(&mut self, node: NodeIndex) {
        self.graph.remove_node(node);
    }

    pub fn mark_as_failed<DB: WriteResults>(
        &mut self,
        node: NodeIndex,
        ex: &Experiment,
        db: &DB,
        error: &Error,
        result: TestResult,
    ) -> Result<()> {
        let mut children = self
            .graph
            .neighbors_directed(node, Direction::Incoming)
            .collect::<Vec<_>>();
        for child in children.drain(..) {
            self.mark_as_failed(child, ex, db, error, result)?;
        }

        match self.graph[node] {
            Node::Task { ref task, .. } => task.mark_as_failed(ex, db, error, result)?,
            Node::CrateCompleted | Node::Root => return Ok(()),
        }

        self.mark_as_completed(node);
        Ok(())
    }
}

fn build_graph(ex: &Experiment, config: &Config) -> TasksGraph {
    let mut graph = TasksGraph::new();

    for krate in &ex.crates {
        if config.should_skip(krate) {
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
                        Mode::BuildAndTest if config.should_skip_tests(krate) => {
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
                        Mode::UnstableFeatures => TaskStep::UnstableFeatures { tc: tc.clone() },
                    },
                },
                &[prepare_id],
            );

            builds.push(build_id);
        }

        graph.add_crate(&builds);
    }

    graph
}

pub fn run_ex<DB: WriteResults + Sync>(
    ex: &Experiment,
    db: &DB,
    threads_count: usize,
    config: &Config,
) -> Result<()> {
    let res = run_ex_inner(ex, db, threads_count, config);

    // Remove all the target dirs even if the experiment failed
    let target_dir = &::toolchain::ex_target_dir(&ex.name);
    if target_dir.exists() {
        utils::remove_dir_all(target_dir)?;
    }

    res
}

fn run_ex_inner<DB: WriteResults + Sync>(
    ex: &Experiment,
    db: &DB,
    threads_count: usize,
    config: &Config,
) -> Result<()> {
    info!("computing the tasks graph...");
    let graph = Mutex::new(build_graph(ex, config));

    info!("preparing the execution...");
    for tc in &ex.toolchains {
        tc.prepare()?;
    }

    info!("running tasks in {} threads...", threads_count);

    // An HashMap is used instead of an HashSet because Thread is not Eq+Hash
    let parked_threads: Mutex<HashMap<thread::ThreadId, thread::Thread>> =
        Mutex::new(HashMap::new());

    scope(|scope| -> Result<()> {
        let mut threads = Vec::new();

        for i in 0..threads_count {
            let name = format!("worker-{}", i);
            let join = scope.builder().name(name).spawn(|| -> Result<()> {
                // This uses a `loop` instead of a `while let` to avoid locking the graph too much
                loop {
                    let walk_result = graph.lock().unwrap().next_task(ex, db);
                    match walk_result {
                        WalkResult::Task(id, task) => {
                            info!("running task: {:?}", task);
                            if let Err(e) = task.run(config, ex, db) {
                                error!("task failed, marking childs as failed too: {:?}", task);
                                utils::report_error(&e);

                                let result = if config.is_broken(&task.krate) {
                                    TestResult::BuildFail
                                } else {
                                    TestResult::Error
                                };
                                graph
                                    .lock()
                                    .unwrap()
                                    .mark_as_failed(id, ex, db, &e, result)?;
                            } else {
                                graph.lock().unwrap().mark_as_completed(id);
                            }

                            // Unpark all the threads
                            let mut parked = parked_threads.lock().unwrap();
                            for (_id, thread) in parked.drain() {
                                thread.unpark();
                            }
                        }
                        WalkResult::Blocked => {
                            // Wait until another thread finished before looking for tasks again
                            // If the thread spuriously wake up (parking does not guarantee no
                            // spurious wakeups) it's not a big deal, it will just get parked again
                            {
                                let mut parked_threads = parked_threads.lock().unwrap();
                                let current = thread::current();
                                parked_threads.insert(current.id(), current);
                            }
                            thread::park();
                        }
                        WalkResult::NotBlocked => unreachable!("NotBlocked leaked from the run"),
                        WalkResult::Finished => break,
                    }
                }

                Ok(())
            })?;
            threads.push(join);
        }

        let mut clean_exit = true;
        for thread in threads.drain(..) {
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    ::utils::report_error(&err);
                    clean_exit = false;
                }
                Err(panic) => {
                    ::utils::report_panic(&panic);
                    clean_exit = false;
                }
            }
        }

        if clean_exit {
            Ok(())
        } else {
            Err("some threads returned an error".into())
        }
    })?;

    // Only the root node must be present
    let mut g = graph.lock().unwrap();
    assert!(g.next_task(ex, db).is_finished());
    assert_eq!(g.graph.neighbors(g.root).count(), 0);

    Ok(())
}

pub fn dump_dot(ex: &Experiment, config: &Config, dest: &Path) -> Result<()> {
    info!("computing the tasks graph...");
    let graph = build_graph(&ex, config);

    info!("dumping the tasks graph...");
    file::write_string(dest, &format!("{:?}", Dot::new(&graph.graph)))?;

    info!("tasks graph available in {}", dest.to_string_lossy());

    Ok(())
}
