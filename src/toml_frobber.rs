use util;
use file;
use errors::*;
use crates;
use lists::Crate;
use std::fs;
use FROB_DIR;
use std::path::{Path, PathBuf};
use toml::{Parser, Value};

pub fn frob_em() -> Result<()> {
    fs::create_dir_all(FROB_DIR)?;

    for (crate_, dir) in crates::crates_and_dirs()? {
        match crate_ {
            Crate::Version(ref name, ref vers) => {
                let r = frob_toml(&dir, name, &vers.to_string());
                if let Err(e) = r {
                    log!("couldn't frob: {}", e);
                    util::report_error(&e);
                }
            }
            _ => ()
        }
    }

    Ok(())
}

pub fn froml_path(name: &str, vers: &str) -> PathBuf {
    Path::new(FROB_DIR).join(format!("{}-{}.Cargo.toml", name, vers))
}

fn frob_toml(dir: &Path, name: &str, vers: &str) -> Result<()> {
    log!("frobbing {}-{}", name, vers);
    let toml_str = file::read_string(&dir.join("Cargo.toml"))
        .chain_err(|| "no cargo.toml?")?;
    let mut parser = Parser::new(&toml_str);
    let mut toml = parser.parse()
        .ok_or(Error::from(format!("unable to parse Cargo.toml at {}", dir.display())))?;

    let mut changed = false;

    // Convert path dependencies to registry dependencies
    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        let maybe_deps = toml.get_mut(*section);
        match maybe_deps {
            Some(&mut Value::Table(ref mut deps)) => {
                // Iterate through the "name = { ... }", removing any "path"
                // keys in the dependency definition
                for (dep_name, v) in deps.iter_mut() {
                    match v {
                        &mut Value::Table(ref mut dep_props) => {
                            if dep_props.contains_key("path") {
                                log!("removing path from {} in {}-{}",
                                     dep_name, name, vers);
                            }
                            if dep_props.remove("path").is_some() {
                                changed = true;
                            }
                        }
                        _ => ()
                    }
                }
            }
            _ => ()
        }
    }

    // Eliminate workspaces
    if toml.remove("workspace").is_some() {
        log!("removing workspace from {}-{}", name, vers);
        changed = true;
    }

    if changed {
        let toml = Value::Table(toml);
        let new_path = froml_path(name, vers);
        file::write_string(&new_path, &format!("{}", toml))?;

        log!("frobbed toml written to {}", new_path.display());
    }

    Ok(())
}
