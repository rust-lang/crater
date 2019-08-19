use crate::prelude::*;
use nix::unistd::{Gid, Uid};
use std::convert::AsRef;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

const EXECUTABLE_BITS: u32 = 0o5;

pub(crate) fn current_user() -> Option<u32> {
    Some(Uid::effective().into())
}

fn current_group() -> u32 {
    Gid::effective().into()
}

fn executable_mode_for(path: &Path) -> Fallible<u32> {
    let metadata = path.metadata()?;

    if metadata.uid() == current_user().unwrap() {
        Ok(EXECUTABLE_BITS << 6)
    } else if metadata.gid() == current_group() {
        Ok(EXECUTABLE_BITS << 3)
    } else {
        Ok(EXECUTABLE_BITS)
    }
}

pub(crate) fn is_executable<P: AsRef<Path>>(path: P) -> Fallible<bool> {
    let path = path.as_ref();

    let expected_mode = executable_mode_for(&path)?;
    Ok(path.metadata()?.mode() & expected_mode == expected_mode)
}

pub(crate) fn make_executable<P: AsRef<Path>>(path: P) -> Fallible<()> {
    let path = path.as_ref();

    // Set the executable and readable bits on the file
    let mut perms = path.metadata()?.permissions();
    let new_mode = perms.mode() | executable_mode_for(&path)?;
    perms.set_mode(new_mode);

    ::std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{current_group, current_user, is_executable, make_executable};
    use nix::unistd::{Gid, Uid};
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_current_user() {
        assert_eq!(current_user(), Some(u32::from(Uid::effective())));
    }

    #[test]
    fn test_current_group() {
        assert_eq!(current_group(), u32::from(Gid::effective()));
    }

    #[test]
    fn test_executables() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test");

        // Create the temp file and make sure it's not executable
        File::create(&path).unwrap();
        assert!(!is_executable(&path).unwrap());

        // And then make it executable
        make_executable(&path).unwrap();
        assert!(is_executable(&path).unwrap());
    }
}
