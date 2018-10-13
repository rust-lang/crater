use crates::Crate;
use errors::*;
use std::path::Path;
use toml::value::Table;
use toml::{self, Value};

pub(super) struct TomlFrobber<'a> {
    krate: &'a Crate,
    table: Table,
}

impl<'a> TomlFrobber<'a> {
    pub(super) fn new(krate: &'a Crate, source_dir: &Path) -> Result<Self> {
        let toml_content = ::std::fs::read_to_string(&source_dir.join("Cargo.toml"))
            .chain_err(|| format!("missing Cargo.toml from {}", krate))?;
        let table: Table = toml::from_str(&toml_content).chain_err(|| {
            format!(
                "unable to parse Cargo.toml at {}",
                source_dir.to_string_lossy()
            )
        })?;

        Ok(TomlFrobber { krate, table })
    }

    #[cfg(test)]
    fn new_with_table(krate: &'a Crate, table: Table) -> Self {
        TomlFrobber { krate, table }
    }

    pub(super) fn frob(&mut self) {
        info!("started frobbing {}", self.krate);

        self.remove_workspaces();
        self.remove_unwanted_cargo_features();
        self.remove_dependencies();

        info!("finished frobbing {}", self.krate);
    }

    fn remove_workspaces(&mut self) {
        let krate = self.krate.to_string();

        if self.table.remove("workspace").is_some() {
            info!("removed workspace from {}", krate);
        }

        // Eliminate parent workspaces
        if let Some(&mut Value::Table(ref mut package)) = self.table.get_mut("package") {
            if package.remove("workspace").is_some() {
                info!("removed parent workspace from {}", krate);
            }
        }
    }

    fn remove_unwanted_cargo_features(&mut self) {
        let krate = self.krate.to_string();

        // Remove the unwanted features from the main list
        let mut has_publish_lockfile = false;
        let mut has_default_run = false;
        if let Some(&mut Value::Array(ref mut vec)) = self.table.get_mut("cargo-features") {
            vec.retain(|key| {
                if let Value::String(key) = key {
                    match key.as_str() {
                        "publish-lockfile" => has_publish_lockfile = true,
                        "default-run" => has_default_run = true,
                        _ => return true,
                    }
                }

                false
            });
        }

        // Strip the 'publish-lockfile' key from [package]
        if has_publish_lockfile {
            info!("disabled cargo feature 'publish-lockfile' from {}", krate);
            if let Some(&mut Value::Table(ref mut package)) = self.table.get_mut("package") {
                package.remove("publish-lockfile");
            }
        }

        // Strip the 'default-run' key from [package]
        if has_default_run {
            info!("disabled cargo feature 'default-run' from {}", krate);
            if let Some(&mut Value::Table(ref mut package)) = self.table.get_mut("package") {
                package.remove("default-run");
            }
        }
    }

    fn remove_dependencies(&mut self) {
        let krate = self.krate.to_string();

        Self::remove_dependencies_from_table(&mut self.table, &krate);

        // Frob target-specific dependencies
        if let Some(&mut Value::Table(ref mut targets)) = self.table.get_mut("target") {
            for (_, target) in targets.iter_mut() {
                if let Value::Table(ref mut target_table) = *target {
                    Self::remove_dependencies_from_table(target_table, &krate);
                }
            }
        }
    }

    // This is not a method to avoid borrow checker problems
    fn remove_dependencies_from_table(table: &mut Table, krate: &str) {
        // Convert path dependencies to registry dependencies
        for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(&mut Value::Table(ref mut deps)) = table.get_mut(*section) {
                // Iterate through the "name = { ... }", removing any "path"
                // keys in the dependency definition
                for (dep_name, v) in deps.iter_mut() {
                    if let Value::Table(ref mut dep_props) = *v {
                        if dep_props.remove("path").is_some() {
                            info!("removed path dependency {} from {}", dep_name, krate);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn save(self, output_file: &Path) -> Result<()> {
        let crate_name = self.krate.to_string();
        ::std::fs::write(output_file, Value::Table(self.table).to_string().as_bytes())?;
        info!(
            "frobbed toml for {} written to {}",
            crate_name,
            output_file.to_string_lossy()
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::TomlFrobber;
    use crates::Crate;
    use toml::Value;

    #[test]
    fn test_frob_table_noop() {
        let toml = toml! {
            cargo-features = ["foobar"]

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

        let krate = Crate::Local("build-pass".to_string());
        let mut frobber = TomlFrobber::new_with_table(&krate, toml.as_table().unwrap().clone());
        frobber.frob();

        assert_eq!(Value::Table(frobber.table), result);
    }

    #[test]
    fn test_frob_table_changes() {
        let toml = toml! {
            cargo-features = ["foobar", "publish-lockfile", "default-run"]

            [package]
            name = "foo"
            version = "1.0"
            workspace = ".."
            publish-lockfile = true
            default-run = "foo"

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
            cargo-features = ["foobar"]

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

        let krate = Crate::Local("build-pass".to_string());
        let mut frobber = TomlFrobber::new_with_table(&krate, toml.as_table().unwrap().clone());
        frobber.frob();

        assert_eq!(Value::Table(frobber.table), result);
    }
}
