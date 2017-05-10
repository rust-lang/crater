use JOB_DIR;
use bmk::Arguable;
use docker::{self, Perm, RustEnv};
use docker::Container;
use errors::*;
use file;
use home;
use model;
use model::Cmd;
use rand;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Copy, Clone, Debug)]
pub struct JobId(pub u64);

#[derive(Serialize, Deserialize, Clone)]
struct Job {
    id: JobId,
    cmd: Cmd,
    kind: JobKind,
    state: JobState,
}

#[derive(Serialize, Deserialize, Clone)]
enum JobKind {
    Docker(Option<Container>),
    Ec2,
}

#[derive(Serialize, Deserialize, Clone)]
enum JobState {
    Created,
    Running,
    Done,
}

use std::fmt::{self, Display, Formatter};

impl Display for JobId {
    fn fmt(&self, f: &mut Formatter) -> ::std::result::Result<(), fmt::Error> {
        let s = format!("{0:}", self.0);
        s.fmt(f)
    }
}

fn job_path(job: JobId) -> PathBuf {
    Path::new(JOB_DIR).join(&format!("{}.json", job))
}

fn write_job(job: &Job) -> Result<()> {
    let job_path = &job_path(job.id);
    fs::create_dir_all(&Path::new(JOB_DIR))?;

    file::write_json(job_path, job)
}

fn read_job(job: JobId) -> Result<Job> {
    let job_path = &job_path(job);
    file::read_json(job_path)
}

pub fn create_local(cmd: Cmd) -> Result<()> {
    log!("create local job: {}", cmd.clone().to_args().join(" "));

    let job = &Job {
        id: JobId(rand::random()),
        cmd: cmd,
        kind: JobKind::Docker(None),
        state: JobState::Created,
    };

    write_job(job)?;

    log!("job {} created in {}", job.id, job_path(job.id).display());

    Ok(())
}

pub fn start(job: JobId) -> Result<()> {
    start_(job, false, false)
}

pub fn run(job: JobId) -> Result<()> {
    start_(job, true, false)?;
    wait(job)
}

pub fn run_again(job: JobId) -> Result<()> {
    start_(job, true, true)?;
    wait(job)
}

fn start_(job: JobId, wait: bool, again: bool) -> Result<()> {

    let job = read_job(job)?;

    match job.state {
        JobState::Created => (),
        JobState::Running => bail!("job {} already running", job.id),
        JobState::Done => {
            if !again {
                bail!("job {} already done", job.id)
            }
        }
    }

    match job.kind {
        JobKind::Docker(None) => {

            let work_dir = env::current_dir()?;
            let cargo_home = home::cargo_home()?;
            let rustup_home = home::rustup_home()?;
            let target_dir = PathBuf::from("./target");

            let self_exe = env::current_exe()?;
            let exe = self_exe
                .strip_prefix(&work_dir)
                .chain_err(|| "self exe prefix")?
                .to_owned();

            log!("job config:");
            log!("work_dir: {}", work_dir.display());
            log!("cargo_home: {}", cargo_home.display());
            log!("rustup_home: {}", rustup_home.display());
            log!("target_dir: {}", target_dir.display());
            log!("self_exe: {}", self_exe.display());
            log!("exe: {}", exe.display());

            let cmd = Cmd::RunCmdForJob(model::Job(job.id));
            let args = model::conv::cmd_to_args(cmd);
            let args = args.iter().map(|s| &**s).collect::<Vec<_>>();
            let exe = format!("{}", exe.display());
            let mut args_ = vec![&*exe];
            args_.extend(args);
            let env = RustEnv {
                args: &*args_,
                privileged: true,
                work_dir: (work_dir, Perm::ReadWrite),
                cargo_home: (cargo_home, Perm::ReadWrite),
                rustup_home: (rustup_home, Perm::ReadWrite),
                target_dir: (target_dir, Perm::ReadWrite),
            };

            let c = docker::create_rust_container(&env)?;
            if wait {
                    docker::run_container(&c)
                } else {
                    docker::start_container(&c)
                }
                .map_err(|e| {
                             let _ = docker::delete_container(&c);
                             e
                         })?;

            let mut job = job;
            job.state = JobState::Running;
            job.kind = JobKind::Docker(Some(c.clone()));
            write_job(&job)?;

            log!("job {} started in container {}", job.id, c);
        }
        JobKind::Docker(Some(c)) => {
            bail!("job {} already started in container {}", job.id, c);
        }
        JobKind::Ec2 => {
            panic!("ec2");
        }
    }

    Ok(())
}

pub fn wait(job: JobId) -> Result<()> {
    let job = read_job(job)?;

    match job.state {
        JobState::Created => bail!("job {} not running", job.id),
        JobState::Running => (),
        JobState::Done => return Ok(()),
    }

    let job_ = job.clone();
    match job.kind {
        JobKind::Docker(Some(c)) => {
            docker::wait_for_container(&c)?;
            let mut job = job_;
            job.state = JobState::Done;
            job.kind = JobKind::Docker(None);
            write_job(&job)?;
            docker::delete_container(&c)?;
        }
        JobKind::Docker(None) => {
            bail!("docker container not started for job {}", job.id);
        }
        JobKind::Ec2 => {
            panic!("ec2");
        }
    }

    Ok(())
}

/// This is the command run inside the job container to execute
/// the originally-requested command that defines the job
pub fn run_cmd_for_job(job: JobId) -> Result<()> {
    use bmk;

    let job = read_job(job)?;

    // FIXME: Instead of reinitializing the run loop here it would be better
    // to just pass the cmd to be run back to the original loop.
    let state = model::state::GlobalState::init();
    let _ = bmk::run(state, job.cmd)?;

    Ok(())
}
