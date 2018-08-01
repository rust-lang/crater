use crates::Crate;
use errors::*;
use file;
use std::collections::BTreeMap;
use std::path::Path;
use toml::value::Table;
use toml::{self, Value};
use toolchain::Toolchain;

pub fn frob_toml(dir: &Path, tc: &Toolchain, krate: &Crate) -> Result<()> {
    info!("frobbing {}", krate);

    let cargo_toml = dir.join("Cargo.toml");

    let toml_str = file::read_string(&cargo_toml).chain_err(|| "no Cargo.toml?")?;
    let mut toml: Table = toml::from_str(&toml_str)
        .chain_err(|| Error::from(format!("unable to parse {}", cargo_toml.to_string_lossy())))?;

    if frob_table(&mut toml, tc, krate) {
        let toml = Value::Table(toml);
        file::write_string(&cargo_toml, &toml.to_string())?;

        info!("frobbed toml written to {}", cargo_toml.to_string_lossy());
    }

    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(useless_let_if_seq))]
pub fn frob_table(table: &mut Table, tc: &Toolchain, krate: &Crate) -> bool {
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

    if tc.enable_tmplazy {
        let patch_table = table.entry("patch".to_string())
            .or_insert_with(|| Value::Table(BTreeMap::new()));
        if let Value::Table(ref mut patch) = patch_table {
            let crates_io_table = patch.entry("crates-io".to_string())
                .or_insert_with(|| Value::Table(BTreeMap::new()));
            if let Value::Table(ref mut crates_io) = crates_io_table {
                let mut lazy_static = BTreeMap::new();
                lazy_static.insert("git".to_string(), Value::String("https://github.com/anp/lazy-static.rs".to_string()));
                lazy_static.insert("rev".to_string(), Value::String("0463a90b433d12db6a0e8f2087b2bd9d1afe9c48".to_string()));
                crates_io.insert("lazy_static".to_string(), Value::Table(lazy_static));
                changed = true;
            }
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
