use errors::*;
use file;
use std::path::Path;
use toml::{self, Value};
use toml::value::Table;

pub fn frob_toml(dir: &Path, name: &str, vers: &str, out: &Path) -> Result<()> {
    info!("frobbing {}-{}", name, vers);
    let toml_str = file::read_string(&dir.join("Cargo.toml")).chain_err(|| "no cargo.toml?")?;
    let mut toml: Table = toml::from_str(&toml_str)
        .chain_err(|| Error::from(format!("unable to parse Cargo.toml at {}", dir.display())))?;

    let mut changed = false;

    // Convert path dependencies to registry dependencies
    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        let maybe_deps = toml.get_mut(*section);
        if let Some(&mut Value::Table(ref mut deps)) = maybe_deps {
            // Iterate through the "name = { ... }", removing any "path"
            // keys in the dependency definition
            for (dep_name, v) in deps.iter_mut() {
                if let Value::Table(ref mut dep_props) = *v {
                    if dep_props.contains_key("path") {
                        info!("removing path from {} in {}-{}", dep_name, name, vers);
                    }
                    if dep_props.remove("path").is_some() {
                        changed = true;
                    }
                }
            }
        }
    }

    // Eliminate workspaces
    if toml.remove("workspace").is_some() {
        info!("removing workspace from {}-{}", name, vers);
        changed = true;
    }

    if changed {
        let toml = Value::Table(toml);
        file::write_string(out, &format!("{}", toml))?;

        info!("frobbed toml written to {}", out.display());
    }

    Ok(())
}
