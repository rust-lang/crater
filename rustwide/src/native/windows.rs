use failure::{bail, Error};
use std::fs::File;
use std::path::Path;
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

fn path_ends_in_exe<P: AsRef<Path>>(path: P) -> Result<bool, Error> {
    path.as_ref()
        .extension()
        .ok_or_else(|| failure::format_err!("Unable to get `Path` extension"))
        .map(|ext| ext == "exe")
}

/// Check that the file exists and has `.exe` as its extension.
pub(crate) fn is_executable<P: AsRef<Path>>(path: P) -> Result<bool, Error> {
    let path = path.as_ref();
    File::open(path)
        .map_err(Into::into)
        .and_then(|_| path_ends_in_exe(path))
}

pub(crate) fn make_executable<P: AsRef<Path>>(path: P) -> Result<(), Error> {
    if is_executable(path)? {
        Ok(())
    } else {
        failure::bail!("Downloaded binaries should be executable by default");
    }
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
