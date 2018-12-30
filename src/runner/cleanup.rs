//! This module performs cleanup of the target directories managed by Crater. While most of the
//! files in there can't be removed without triggering a rebuild, these can:
//!
//! - All the files inside the root of the target dirs, but not in the subdirectories
//! - The `examples` subdirectory
//! - The `incremental` subdirectory

use crate::prelude::*;
use crate::utils;
use std::ffi::OsStr;
use std::path::Path;
use walkdir::WalkDir;

fn clean_target_dir(dir: &Path) -> Fallible<()> {
    remove_top_level_files(dir)?;
    utils::fs::remove_dir_all(&dir.join("examples"))?;
    utils::fs::remove_dir_all(&dir.join("incremental"))?;
    Ok(())
}

pub(super) fn clean_target_dirs(base: &Path) -> Fallible<()> {
    WalkDir::new(base)
        .into_iter()
        .filter_map(|e| e.ok())
        .map(|entry| {
            if entry.path().is_dir() && entry.file_name() == OsStr::new(".fingerprint") {
                if let Some(parent) = entry.path().parent() {
                    clean_target_dir(&parent)?;
                }
            }
            Ok(())
        })
        .collect::<Fallible<()>>()?;
    Ok(())
}

fn remove_top_level_files(dir: &Path) -> Fallible<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::clean_target_dirs;
    use crate::utils::fs::TempDirBuilder;

    #[test]
    fn test_clean_target_dirs() {
        let path = TempDirBuilder::default()
            .dir("root/debug/.fingerprint")
            .file("root/debug/deps/foo", "")
            .file("root/debug/deps/bar", "")
            .file("root/debug/binary1", "")
            .file("root/debug/binary2", "")
            .file("root/debug/examples/foo", "")
            .file("root/debug/incremental/foo", "")
            .build()
            .unwrap();

        clean_target_dirs(path.path()).unwrap();

        let mut top_level_files_exists = false;
        let mut examples_exists = false;
        let mut incremental_exists = false;
        let mut deps_exists = false;
        let mut fingerprint_exists = false;
        for entry in std::fs::read_dir(path.path().join("root/debug")).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                top_level_files_exists = true;
            }
            match entry.path().file_name().unwrap().to_string_lossy().as_ref() {
                "examples" => examples_exists = true,
                "incremental" => incremental_exists = true,
                "deps" => deps_exists = true,
                ".fingerprint" => fingerprint_exists = true,
                _ => {}
            }
        }
        assert!(!top_level_files_exists);
        assert!(!examples_exists);
        assert!(!incremental_exists);
        assert!(deps_exists);
        assert!(fingerprint_exists);
    }
}
