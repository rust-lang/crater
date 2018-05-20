use config::Config;
use crossbeam;
use errors::*;
use ex::{self, ExMode, Experiment};
use file;
use petgraph::{dot::Dot, graph::NodeIndex, stable_graph::StableDiGraph, Direction};
use results::WriteResults;
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tasks::{Task, TaskStep};
use util;

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

enum WalkResult {
    Task(NodeIndex, Arc<Task>),
    Blocked,
    NotBlocked,
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

    pub fn next_task<DB: WriteResults>(
        &mut self,
        ex: &Experiment,
        db: &DB,
    ) -> Option<(NodeIndex, Arc<Task>)> {
        let root = self.root;
        if let WalkResult::Task(id, task) = self.walk_graph(root, ex, db) {
            Some((id, task))
        } else {
            None
        }
    }

    fn walk_graph<DB: WriteResults>(
        &mut self,
        node: NodeIndex,
        ex: &Experiment,
        db: &DB,
    ) -> WalkResult {
        // Ensure tasks are only executed if needed
        if let Node::Task {
            ref task,
            running: false,
        } = self.graph[node]
        {
            if !task.needs_exec(ex, db) {
                return WalkResult::NotBlocked;
            }
        }

        let mut dependencies = 0;

        // Try to check for the dependencies of this node
        // The list is collected to make the borrowchecker happy
        let mut neighbors = self.graph.neighbors(node).collect::<Vec<_>>();
        for neighbor in neighbors.drain(..) {
            match self.walk_graph(neighbor, ex, db) {
                WalkResult::Task(id, task) => return WalkResult::Task(id, task),
                WalkResult::Blocked => dependencies += 1,
                WalkResult::NotBlocked => {}
            }
        }

        if dependencies == 0 {
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
                Node::Root => WalkResult::NotBlocked,
            };

            // This is done after the match to avoid borrowck issues
            if delete {
                self.mark_as_completed(node);
            }

            result
        } else {
            WalkResult::Blocked
        }
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
    ) -> Result<()> {
        let mut children = self
            .graph
            .neighbors_directed(node, Direction::Incoming)
            .collect::<Vec<_>>();
        for child in children.drain(..) {
            self.mark_as_failed(child, ex, db, error)?;
        }

        match self.graph[node] {
            Node::Task { ref task, .. } => task.mark_as_failed(ex, db, error)?,
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
                        ExMode::BuildOnly => TaskStep::BuildOnly {
                            tc: tc.clone(),
                            quiet,
                        },
                        ExMode::BuildAndTest if config.should_skip(krate) => TaskStep::BuildOnly {
                            tc: tc.clone(),
                            quiet,
                        },
                        ExMode::BuildAndTest => TaskStep::BuildAndTest {
                            tc: tc.clone(),
                            quiet,
                        },
                        ExMode::CheckOnly => TaskStep::CheckOnly {
                            tc: tc.clone(),
                            quiet,
                        },
                        ExMode::UnstableFeatures => TaskStep::UnstableFeatures { tc: tc.clone() },
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
    info!("computing the tasks graph...");
    let graph = Mutex::new(build_graph(ex, config));

    info!("preparing the execution...");
    ex::prepare_all_toolchains(ex)?;

    info!("running tasks in {} threads...", threads_count);

    crossbeam::scope(|scope| -> Result<()> {
        let mut threads = Vec::new();

        for i in 0..threads_count {
            let name = format!("worker-{}", i);
            let join = scope.builder().name(name).spawn(|| -> Result<()> {
                // This uses a `loop` instead of a `while let` to avoid locking the graph too much
                loop {
                    let option_task = graph.lock().unwrap().next_task(ex, db);
                    if let Some((id, task)) = option_task {
                        info!("running task: {:?}", task);
                        if let Err(e) = task.run(config, ex, db) {
                            error!("task failed, marking childs as failed too: {:?}", task);
                            util::report_error(&e);
                            graph.lock().unwrap().mark_as_failed(id, ex, db, &e)?;
                        } else {
                            graph.lock().unwrap().mark_as_completed(id);
                        }
                    } else {
                        break;
                    }
                }

                Ok(())
            })?;
            threads.push(join);
        }

        for thread in threads.drain(..) {
            thread.join()?;
        }

        Ok(())
    })?;

    // Only the root node must be present
    let mut g = graph.lock().unwrap();
    assert!(g.next_task(ex, db).is_none());
    assert_eq!(g.graph.neighbors(g.root).count(), 0);

    Ok(())
}

pub fn dump_dot(ex_name: &str, config: &Config, dest: &Path) -> Result<()> {
    let ex = Experiment::load(ex_name)?;

    info!("computing the tasks graph...");
    let graph = build_graph(&ex, config);

    info!("dumping the tasks graph...");
    file::write_string(dest, &format!("{:?}", Dot::new(&graph.graph)))?;

    info!("tasks graph available in {}", dest.to_string_lossy());

    Ok(())
}
