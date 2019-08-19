use failure::{bail, Error};
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
use winapi::um::winnt::PROCESS_TERMINATE;

pub(crate) fn kill_process(id: u32) -> Result<(), Error> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, id);
        if handle.is_null() {
            bail!("OpenProcess for process {} failed", id);
        }
        if TerminateProcess(handle, 101) == 0 {
            bail!("TerminateProcess for process {} failed", id);
        }
        if CloseHandle(handle) == 0 {
            bail!("CloseHandle for process {} failed", id);
        }
    }

    Ok(())
}

pub(crate) fn current_user() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn test_kill_process() {
        // Try to kill a sleep command
        let mut cmd = Command::new("timeout").args(&["2"]).spawn().unwrap();
        kill_process(cmd.id()).unwrap();

        // Ensure it returns the code passed to `TerminateProcess`
        assert_eq!(cmd.wait().unwrap().code(), Some(101));
    }
}
