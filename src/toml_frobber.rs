use errors::*;
use file;
use std::path::Path;
use toml::value::Table;
use toml::{self, Value};

pub fn frob_toml(dir: &Path, name: &str, vers: &str, out: &Path) -> Result<()> {
    info!("frobbing {}-{}", name, vers);
    let toml_str = file::read_string(&dir.join("Cargo.toml")).chain_err(|| "no cargo.toml?")?;
    let mut toml: Table = toml::from_str(&toml_str)
        .chain_err(|| Error::from(format!("unable to parse Cargo.toml at {}", dir.display())))?;

    if frob_table(&mut toml, name, vers) {
        let toml = Value::Table(toml);
        file::write_string(out, &format!("{}", toml))?;

        info!("frobbed toml written to {}", out.display());
    }

    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(useless_let_if_seq))]
pub fn frob_table(table: &mut Table, name: &str, vers: &str) -> bool {
    let mut changed = false;

    // Frob top-level dependencies
    if frob_dependencies(table, name, vers) {
        changed = true;
    }

    // Eliminate workspaces
    if table.remove("workspace").is_some() {
        info!("removing workspace from {}-{}", name, vers);
        changed = true;
    }

    changed
}

fn frob_dependencies(table: &mut Table, name: &str, vers: &str) -> bool {
    let mut changed = false;

    // Convert path dependencies to registry dependencies
    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(&mut Value::Table(ref mut deps)) = table.get_mut(*section) {
            // Iterate through the "name = { ... }", removing any "path"
            // keys in the dependency definition
            for (dep_name, v) in deps.iter_mut() {
                if let Value::Table(ref mut dep_props) = *v {
                    if dep_props.remove("path").is_some() {
                        info!("removing path from {} in {}-{}", dep_name, name, vers);
                        changed = true;
                    }
                }
            }
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::frob_table;

    #[test]
    fn test_frob_table_noop() {
        let mut toml = toml! {
            [package]
            name = "foo"
            version = "1.0"

            [dependencies]
            bar = "1.0"

            [dev-dependencies]
            baz = "1.0"
        };

        let result = toml.clone();

        assert!(!frob_table(toml.as_table_mut().unwrap(), "foo", "1.0"));
        assert_eq!(toml, result);
    }

    #[test]
    fn test_frob_table_changes() {
        let mut toml = toml! {
            [package]
            name = "foo"
            version = "1.0"

            [dependencies]
            bar = { version = "1.0", path = "../bar" }

            [dev-dependencies]
            baz = { version = "1.0", path = "../baz" }

            [workspace]
            members = []
        };

        let result = toml! {
            [package]
            name = "foo"
            version = "1.0"

            [dependencies]
            bar = { version = "1.0" }

            [dev-dependencies]
            baz = { version = "1.0" }
        };

        assert!(frob_table(toml.as_table_mut().unwrap(), "foo", "1.0"));
        assert_eq!(toml, result);
    }
}
