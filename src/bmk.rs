use errors::*;

pub trait Process {
    fn process(self) -> Result<()> where Self: Sized;
}

pub fn run<C>(cmd: C) -> Result<()>
    where C: Process
{
    cmd.process()
}
