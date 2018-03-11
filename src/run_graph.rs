use config::Config;
use errors::*;
use ex::{self, Experiment};
use file;
use petgraph::dot::Dot;
use petgraph::graph::{Graph, NodeIndex};
use std::fmt;
use std::mem;
use std::path::Path;
use tasks::{Task, TaskStep};

pub enum Node {
    Task(Task),
    RunningTask,
    CrateCompleted,
    Root,
}

impl fmt::Debug for Node {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Node::Task(ref task) => write!(f, "{:?}", task)?,
            Node::RunningTask => write!(f, "running task")?,
            Node::CrateCompleted => write!(f, "crate completed")?,
            Node::Root => write!(f, "root")?,
        }
        Ok(())
    }
}

enum WalkResult {
    Task(NodeIndex, Task),
    Blocked,
    NotBlocked,
}

pub struct TasksGraph {
    graph: Graph<Node, ()>,
    root: NodeIndex,
}

impl TasksGraph {
    pub fn new() -> Self {
        let mut graph = Graph::new();
        let root = graph.add_node(Node::Root);

        TasksGraph {
            graph,
            root,
        }
    }

    pub fn add_task(&mut self, task: Task, deps: &[NodeIndex]) -> NodeIndex {
        self.add_node(Node::Task(task), deps)
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

    pub fn next_task(&mut self) -> Option<(NodeIndex, Task)> {
        let root = self.root;
        if let WalkResult::Task(id, task) = self.walk_graph(root) {
            Some((id, task))
        } else {
            None
        }
    }

    fn walk_graph(&mut self, node: NodeIndex) -> WalkResult {
        let mut dependencies = 0;

        // Try to check for the dependencies of this node
        // The list is collected to make the borrowchecker happy
        let mut neighbors = self.graph.neighbors(node).collect::<Vec<_>>();
        for neighbor in neighbors.drain(..) {
            match self.walk_graph(neighbor) {
                WalkResult::Task(id, task) => return WalkResult::Task(id, task),
                WalkResult::Blocked => dependencies += 1,
                WalkResult::NotBlocked => {}
            }
        }

        if dependencies == 0 {
            match &self.graph[node] {
                &Node::Task(_) => {
                    let content = mem::replace(&mut self.graph[node], Node::RunningTask);
                    if let Node::Task(task) = content {
                        WalkResult::Task(node, task)
                    } else {
                        unreachable!();
                    }
                }
                &Node::RunningTask => WalkResult::Blocked,
                &Node::CrateCompleted => {
                    // All the steps for this crate were completed
                    self.graph.remove_node(node);
                    WalkResult::NotBlocked
                }
                &Node::Root => WalkResult::NotBlocked,
            }
        } else {
            WalkResult::Blocked
        }
    }

    pub fn mark_as_completed(&mut self, node: NodeIndex) {
        if let Some(Node::RunningTask) = self.graph.remove_node(node) {
            ()
        } else {
            panic!("removing node {:?}, which is not a RunningTask", node);
        }
    }
}

fn build_graph(ex: &Experiment, config: &Config) -> TasksGraph {
    let mut graph = TasksGraph::new();

    for krate in &ex.crates {
        if config.should_skip(krate) {
            continue;
        }

        let prepare_id = graph.add_task(Task {
            krate: krate.clone(),
            step: TaskStep::Prepare,
        }, &[]);

        let quiet = config.is_quiet(krate);
        let mut builds = Vec::new();
        for tc in &ex.toolchains {
            let build_id = graph.add_task(Task {
                krate: krate.clone(),
                step: if config.should_skip_tests(krate) {
                    TaskStep::BuildOnly {
                        tc: tc.clone(),
                        quiet,
                    }
                } else{
                    TaskStep::BuildAndTest {
                        tc: tc.clone(),
                        quiet,
                    }
                },
            }, &[prepare_id]);

            builds.push(build_id);
        }

        graph.add_crate(&builds);
    }

    graph
}

pub fn run_ex(ex_name: &str, config: &Config) -> Result<()> {
    let mut ex = Experiment::load(ex_name)?;

    info!("computing the tasks graph...");
    let mut graph = build_graph(&ex, config);

    info!("preparing the execution...");
    ex::prepare_all_toolchains(&ex)?;

    info!("running tasks...");
    while let Some((id, task)) = graph.next_task() {
        info!("running task: {:?}", task);
        task.run(&mut ex)?;
        graph.mark_as_completed(id);
    }

    // Only the root node must be present
    assert_eq!(graph.graph.node_count(), 1);

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
