use errors::*;
use std::path::Path;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
use winapi::um::winnt::PROCESS_TERMINATE;

pub(crate) fn kill_process(id: u32) -> Result<()> {
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, id);
        if TerminateProcess(handle, 101) == 0 {
            return Err(ErrorKind::KillProcessFailed(format!(
                "TerminateProcess for process {} failed",
                id
            )).into());
        }
        if CloseHandle(handle) == 0 {
            return Err(ErrorKind::KillProcessFailed(format!(
                "CloseHandle for process {} failed",
                id
            )).into());
        }
    }

    Ok(())
}

pub(crate) fn current_user() -> u32 {
    unimplemented!();
}

pub(crate) fn is_executable<P: AsRef<Path>>(_path: P) -> Result<bool> {
    unimplemented!();
}

pub(crate) fn make_executable<P: AsRef<Path>>(_path: P) -> Result<()> {
    unimplemented!();
}
