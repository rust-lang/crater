use errors::*;

pub trait Process {
    fn process(self) -> Result<Vec<Self>> where Self: Sized;
}

pub fn run<C>(cmd: C) -> Result<()>
    where C: Process
{
    let mut cmds = vec![cmd];
    while let Some(cmd) = cmds.pop() {
        let new_cmds = cmd.process()?;

        // Each command execution returns a list of new commands
        // to execute, in order, before considering the original
        // complete.
        cmds.extend(new_cmds.into_iter().rev());
    }

    Ok(())
}
