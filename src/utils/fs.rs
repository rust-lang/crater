use crate::prelude::*;
use crate::utils::try_hard_limit;
use std::fs;
use std::path::Path;
use url::percent_encoding::SIMPLE_ENCODE_SET;

url::define_encode_set! {
    /// The set of characters which cannot be used in a [filename on Windows][windows].
    ///
    /// [windows]: https://docs.microsoft.com/en-us/windows/desktop/fileio/naming-a-file#naming-conventions
    pub FILENAME_ENCODE_SET = [SIMPLE_ENCODE_SET] | { '<', '>', ':', '"', '/', '\\', '|', '?', '*' }
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
