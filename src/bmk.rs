use errors::*;

pub trait Process {
    fn process(self) -> Result<Vec<Self>> where Self: Sized;
}

pub trait Arguable: Sized {
    fn from_args(args: Vec<String>) -> Result<Self>;
    fn to_args(self) -> Vec<String>;
}

pub fn run<C>(cmd: C) -> Result<()>
    where C: Process,
          C: Arguable
{
    let mut cmds = vec![cmd];
    while let Some(cmd) = cmds.pop() {
        // Round trip through command line argument parsing,
        // just for testing purpose.
        let cmd: Vec<String> = cmd.to_args();
        let cmd: C = Arguable::from_args(cmd)
            .chain_err(|| "error round-tripping cmd through args")?;

        let new_cmds = cmd.process()?;

        // Each command execution returns a list of new commands
        // to execute, in order, before considering the original
        // complete.
        cmds.extend(new_cmds.into_iter().rev());
    }

    Ok(())
}
