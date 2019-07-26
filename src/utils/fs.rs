use crate::prelude::*;
use crate::utils::try_hard_limit;
use std::fs;
use std::path::{self, Path, PathBuf};
use url::percent_encoding::SIMPLE_ENCODE_SET;
use walkdir::{DirEntry, WalkDir};

url::define_encode_set! {
    /// The set of characters which cannot be used in a [filename on Windows][windows].
    ///
    /// [windows]: https://docs.microsoft.com/en-us/windows/desktop/fileio/naming-a-file#naming-conventions
    pub FILENAME_ENCODE_SET = [SIMPLE_ENCODE_SET] | { '<', '>', ':', '"', '/', '\\', '|', '?', '*' }
}

/// If a prefix uses the extended-length syntax (`\\?\`), return the equivalent version without it.
///
/// Returns `None` if `prefix.kind().is_verbatim()` is `false`.
fn strip_verbatim_from_prefix(prefix: &path::PrefixComponent<'_>) -> Option<PathBuf> {
    let ret = match prefix.kind() {
        path::Prefix::Verbatim(s) => Path::new(s).to_owned(),

        path::Prefix::VerbatimDisk(drive) => [format!(r"{}:\", drive as char)].iter().collect(),

        path::Prefix::VerbatimUNC(_, _) => unimplemented!(),

        _ => return None,
    };

    Some(ret)
}

pub(crate) fn try_canonicalize<P: AsRef<Path>>(path: P) -> PathBuf {
    let mut p = fs::canonicalize(&path).unwrap_or_else(|_| path.as_ref().to_path_buf());

    // `fs::canonicalize` returns an extended-length path on Windows. Such paths not supported by
    // many programs, including rustup. We strip the `\\?\` prefix of the canonicalized path, but
    // this changes the meaning of some path components, and imposes a length of around 260
    // characters.
    if cfg!(windows) {
        // A conservative estimate for the maximum length of a path on Windows.
        //
        // The additional 12 byte restriction is applied when creating directories. It ensures that
        // files can always be created inside that directory without exceeding the path limit.
        const MAX_PATH_LEN: usize = 260 - 12;

        let mut components = p.components();
        let first_component = components.next().unwrap();

        if let path::Component::Prefix(prefix) = first_component {
            if let Some(mut modified_path) = strip_verbatim_from_prefix(&prefix) {
                modified_path.push(components.as_path());
                p = modified_path;
            }
        }

        if p.as_os_str().len() >= MAX_PATH_LEN {
            warn!(
                "Canonicalized path is too long for Windows: {:?}",
                p.as_os_str(),
            );
        }
    }

    p
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

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;

    use std::path::Path;

    #[test]
    fn strip_verbatim() {
        let suite = vec![
            (r"C:\Users\carl", None),
            (r"\Users\carl", None),
            (r"\\?\C:\Users\carl", Some(r"C:\")),
            (r"\\?\Users\carl", Some(r"Users")),
        ];

        for (input, output) in suite {
            let p = Path::new(input);
            let first_component = p.components().next().unwrap();

            if let path::Component::Prefix(prefix) = &first_component {
                let stripped = strip_verbatim_from_prefix(&prefix);
                assert_eq!(stripped.as_ref().map(|p| p.to_str().unwrap()), output);
            } else {
                assert!(output.is_none());
            }
        }
    }
}
