use file;
use docker::Container;
use errors::*;
use model::Cmd;
use bmk::Arguable;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct Job {
    id: u64,
    cmd: Cmd,
    kind: JobKind
}

#[derive(Serialize, Deserialize)]
enum JobKind {
    Docker(DockerJob),
    Ec2(Ec2Job),
}

#[derive(Serialize, Deserialize)]
struct DockerJob {
    id: Container,
    state: JobState,
}

#[derive(Serialize, Deserialize)]
struct Ec2Job;

#[derive(Serialize, Deserialize)]
enum JobState {
    Created,
    Running,
    Done
}

pub fn create_local(cmd: Cmd) -> Result<()> {
    log!("create local job: {}", cmd.to_args().join(" "));

    panic!("");
}

pub fn run_local(cmd: Cmd) -> Result<()> {
    panic!("")
}

pub fn wait_local(cmd: Cmd) -> Result<()> {
    panic!("")
}
