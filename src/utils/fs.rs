use crate::prelude::*;
use crate::utils::try_hard_limit;
use std::fs;
use std::path::{Path, PathBuf};
use url::percent_encoding::SIMPLE_ENCODE_SET;
use walkdir::{DirEntry, WalkDir};

url::define_encode_set! {
    /// The set of characters which cannot be used in a [filename on Windows][windows].
    ///
    /// [windows]: https://docs.microsoft.com/en-us/windows/desktop/fileio/naming-a-file#naming-conventions
    pub FILENAME_ENCODE_SET = [SIMPLE_ENCODE_SET] | { '<', '>', ':', '"', '/', '\\', '|', '?', '*' }
}

pub(crate) fn try_canonicalize<P: AsRef<Path>>(path: P) -> PathBuf {
    fs::canonicalize(&path).unwrap_or_else(|_| path.as_ref().to_path_buf())
}

pub(crate) fn remove_dir_all(dir: &Path) -> Fallible<()> {
    try_hard_limit(10, || {
        fs::remove_dir_all(dir)?;
        if dir.exists() {
            bail!("unable to remove directory: {}", dir.to_string_lossy())
        } else {
            Ok(())
        }
    })
}

pub(crate) fn copy_dir(src_dir: &Path, dest_dir: &Path) -> Fallible<()> {
    info!("copying {} to {}", src_dir.display(), dest_dir.display());

    if dest_dir.exists() {
        remove_dir_all(dest_dir)?;
    }
    fs::create_dir_all(dest_dir)
        .with_context(|_| format!("unable to create dest dir: {}", dest_dir.to_string_lossy()))?;

    fn is_hidden(entry: &DirEntry) -> bool {
        entry
            .file_name()
            .to_str()
            .map(|s| s.starts_with('.'))
            .unwrap_or(false)
    }

    let mut partial_dest_dir = PathBuf::from("./");
    let mut depth = 0;
    for entry in WalkDir::new(src_dir)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
    {
        let entry = entry?;
        while entry.depth() <= depth && depth > 0 {
            assert!(partial_dest_dir.pop());
            depth -= 1;
        }
        let path = dest_dir.join(&partial_dest_dir).join(entry.file_name());
        if entry.file_type().is_dir() && entry.depth() > 0 {
            fs::create_dir_all(&path)?;
            assert_eq!(entry.depth(), depth + 1);
            partial_dest_dir.push(entry.file_name());
            depth += 1;
        }
        if entry.file_type().is_file() {
            fs::copy(&entry.path(), path)?;
        }
    }

    Ok(())
}
