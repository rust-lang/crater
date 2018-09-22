use errors::*;
use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

pub(crate) mod hex;
pub mod size;

#[macro_use]
macro_rules! string_enum {
    (pub enum $name:ident { $($item:ident => $str:expr,)* }) => {
        #[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Copy, Clone)]
        pub enum $name {
            $($item,)*
        }

        impl ::std::str::FromStr for $name {
            type Err = ::errors::Error;

            fn from_str(s: &str) -> ::errors::Result<$name> {
                Ok(match s {
                    $($str => $name::$item,)*
                    s => bail!("invalid {}: {}", stringify!($name), s),
                })
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                write!(f, "{}", self.to_str())
            }
        }

        impl $name {
            pub fn to_str(&self) -> &'static str {
                match *self {
                    $($name::$item => $str,)*
                }
            }

            pub fn possible_values() -> &'static [&'static str] {
                &[$($str,)*]
            }
        }
    }
}

pub fn try_hard<F, R>(f: F) -> Result<R>
where
    F: Fn() -> Result<R>,
{
    try_hard_limit(1000, f)
}

pub fn try_hard_limit<F, R>(ms: usize, f: F) -> Result<R>
where
    F: Fn() -> Result<R>,
{
    let mut r;
    for i in 1..3 {
        r = f();
        if r.is_ok() {
            return r;
        } else if let Err(ref e) = r {
            error!("{}", e);
        };
        info!("retrying in {}ms", i * ms);
        thread::sleep(Duration::from_millis((i * ms) as u64));
    }

    f()
}

pub fn remove_dir_all(dir: &Path) -> Result<()> {
    try_hard_limit(10, || {
        fs::remove_dir_all(dir)?;
        if dir.exists() {
            bail!("unable to remove directory");
        } else {
            Ok(())
        }
    })
}

pub fn report_error(e: &Error) {
    error!("{}", e);

    for e in e.iter().skip(1) {
        error!("caused by: {}", e)
    }

    if let Some(backtrace) = e.backtrace() {
        error!("{:?}", backtrace);
    }
}

pub fn report_panic(e: &Any) {
    if let Some(e) = e.downcast_ref::<String>() {
        error!("panicked: {}", e)
    } else if let Some(e) = e.downcast_ref::<&'static str>() {
        error!("panicked: {}", e)
    } else {
        error!("panicked")
    }
}

pub fn copy_dir(src_dir: &Path, dest_dir: &Path) -> Result<()> {
    use walkdir::*;

    info!("copying {} to {}", src_dir.display(), dest_dir.display());

    if dest_dir.exists() {
        remove_dir_all(dest_dir).chain_err(|| "unable to remove test dir")?;
    }
    fs::create_dir_all(dest_dir).chain_err(|| "unable to create test dir")?;

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
        let entry = entry.chain_err(|| "walk dir")?;
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

pub fn split_quoted(input: &str) -> Result<Vec<String>> {
    let mut segments = Vec::new();
    let mut buffer = String::new();

    let mut is_quoted = false;
    let mut is_escaped = false;
    for chr in input.chars() {
        match chr {
            // Always add escaped chars
            _ if is_escaped => {
                buffer.push(chr);
                is_escaped = false;
            }
            // When a \ is encountered, push the next char
            '\\' => is_escaped = true,
            // When a " is encountered, toggle quoting
            '"' => is_quoted = !is_quoted,
            // Split with spaces only if we're not inside a quote
            ' ' | '\t' if !is_quoted => {
                segments.push(buffer);
                buffer = String::new();
            }
            // Otherwise push the char
            _ => buffer.push(chr),
        }
    }

    if is_quoted {
        bail!("unbalanced quotes");
    } else {
        segments.push(buffer);
        Ok(segments)
    }
}

#[cfg(test)]
mod tests {
    use super::split_quoted;

    #[test]
    fn test_split_quoted() {
        macro_rules! test_split_quoted {
            ($($input:expr => [$($segment:expr),*],)*) => {
                $(
                    assert_eq!(split_quoted($input).unwrap(), vec![$($segment.to_string()),*]);
                )*
            }
        }

        // Valid syntaxes
        test_split_quoted! {
            "" => [""],
            "     " => ["", "", "", "", "", ""],
            "a b  c de " => ["a", "b", "", "c", "de", ""],
            "a \\\" b" => ["a", "\"", "b"],
            "a\\ b c" => ["a b", "c"],
            "a \"b c \\\" d\" e" => ["a", "b c \" d", "e"],
            "a b=\"c d e\" f" => ["a", "b=c d e", "f"],
        };

        // Unbalanced quotes
        assert!(split_quoted("a b \" c").is_err());
    }
}
