use failure::Error;
use nix::{
    sys::signal::{kill, Signal},
    unistd::{Pid, Uid},
};

pub(crate) fn kill_process(id: u32) -> Result<(), Error> {
    kill(Pid::from_raw(id as i32), Signal::SIGKILL)?;
    Ok(())
}

pub(crate) fn current_user() -> Option<u32> {
    Some(Uid::effective().into())
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::Command;

    #[test]
    fn test_kill_process() {
        // Try to kill a sleep command
        let mut cmd = Command::new("sleep").args(&["2"]).spawn().unwrap();
        super::kill_process(cmd.id()).unwrap();

        // Ensure it was killed with SIGKILL
        assert_eq!(cmd.wait().unwrap().signal(), Some(9));
    }

    #[test]
    fn test_current_user() {
        assert_eq!(super::current_user(), Some(u32::from(Uid::effective())));
    }
}
