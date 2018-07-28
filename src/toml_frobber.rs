use crates::Crate;
use errors::*;
use file;
use std::path::Path;
use toml::value::Table;
use toml::{self, Value};

pub fn frob_toml(dir: &Path, krate: &Crate) -> Result<()> {
    info!("frobbing {}", krate);

    let cargo_toml = dir.join("Cargo.toml");

    let toml_str = file::read_string(&cargo_toml).chain_err(|| "no Cargo.toml?")?;
    let mut toml: Table = toml::from_str(&toml_str)
        .chain_err(|| Error::from(format!("unable to parse {}", cargo_toml.to_string_lossy())))?;

    if frob_table(&mut toml, krate) {
        let toml = Value::Table(toml);
        file::write_string(&cargo_toml, &toml.to_string())?;

        info!("frobbed toml written to {}", cargo_toml.to_string_lossy());
    }

    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(useless_let_if_seq))]
pub fn frob_table(table: &mut Table, krate: &Crate) -> bool {
    let mut changed = false;

    // Frob top-level dependencies
    if frob_dependencies(table, krate) {
        changed = true;
    }

    // Frob target-specific dependencies
    if let Some(&mut Value::Table(ref mut targets)) = table.get_mut("target") {
        for (_, target) in targets.iter_mut() {
            if let Value::Table(ref mut target_table) = *target {
                if frob_dependencies(target_table, krate) {
                    changed = true;
                }
            }
        }
    }

    // Eliminate workspaces
    if table.remove("workspace").is_some() {
        info!("removing workspace from {}", krate);
        changed = true;
    }

    // Eliminate parent workspaces
    if let Some(&mut Value::Table(ref mut package)) = table.get_mut("package") {
        if package.remove("workspace").is_some() {
            info!("removing parent workspace from {}", krate);
            changed = true;
        }
    }

    changed
}

fn frob_dependencies(table: &mut Table, krate: &Crate) -> bool {
    let mut changed = false;

    // Convert path dependencies to registry dependencies
    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(&mut Value::Table(ref mut deps)) = table.get_mut(*section) {
            // Iterate through the "name = { ... }", removing any "path"
            // keys in the dependency definition
            for (dep_name, v) in deps.iter_mut() {
                if let Value::Table(ref mut dep_props) = *v {
                    if dep_props.remove("path").is_some() {
                        info!("removing path from {} in {}", dep_name, krate);
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
    use crates::{Crate, RegistryCrate};

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

            [target."cfg(unix)".dependencies]
            quux = "1.0"
        };

        let result = toml.clone();

        let krate = Crate::Registry(RegistryCrate {
            name: "foo".to_string(),
            version: "1.0".to_string(),
        });
        assert!(!frob_table(toml.as_table_mut().unwrap(), &krate));
        assert_eq!(toml, result);
    }

    #[test]
    fn test_frob_table_changes() {
        let mut toml = toml! {
            [package]
            name = "foo"
            version = "1.0"
            workspace = ".."

            [dependencies]
            bar = { version = "1.0", path = "../bar" }

            [dev-dependencies]
            baz = { version = "1.0", path = "../baz" }

            [target."cfg(unix)".dependencies]
            quux = { version = "1.0", path = "../quux" }

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

            [target."cfg(unix)".dependencies]
            quux = { version = "1.0" }
        };

        let krate = Crate::Registry(RegistryCrate {
            name: "foo".to_string(),
            version: "1.0".to_string(),
        });
        assert!(frob_table(toml.as_table_mut().unwrap(), &krate));
        assert_eq!(toml, result);
    }
}
