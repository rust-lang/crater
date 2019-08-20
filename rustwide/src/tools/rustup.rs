use crate::cmd::{Binary, Command, Runnable};
use crate::toolchain::MAIN_TOOLCHAIN_NAME;
use crate::tools::{binary_path, Tool, RUSTUP};
use crate::workspace::Workspace;
use failure::{Error, ResultExt};
use std::env::consts::EXE_SUFFIX;
use std::fs::{self, File};
use std::io;

use tempfile::tempdir;

static RUSTUP_BASE_URL: &str = "https://static.rust-lang.org/rustup/dist";

pub(crate) struct Rustup;

impl Runnable for Rustup {
    fn name(&self) -> Binary {
        Binary::ManagedByRustwide("rustup".into())
    }
}

impl Tool for Rustup {
    fn name(&self) -> &'static str {
        "rustup"
    }

    fn is_installed(&self, workspace: &Workspace) -> Result<bool, Error> {
        let path = binary_path(workspace, "rustup");
        if !path.is_file() {
            return Ok(false);
        }

        Ok(crate::native::is_executable(path)?)
    }

    fn install(&self, workspace: &Workspace) -> Result<(), Error> {
        fs::create_dir_all(workspace.cargo_home())?;
        fs::create_dir_all(workspace.rustup_home())?;

        let url = format!(
            "{}/{}/rustup-init{}",
            RUSTUP_BASE_URL,
            crate::HOST_TARGET,
            EXE_SUFFIX
        );
        let mut resp = workspace
            .http_client()
            .get(&url)
            .send()?
            .error_for_status()?;

        let tempdir = tempdir()?;
        let installer = &tempdir.path().join(format!("rustup-init{}", EXE_SUFFIX));
        {
            let mut file = File::create(installer)?;
            io::copy(&mut resp, &mut file)?;
            crate::native::make_executable(installer)?;
        }

        // TODO(rustup.rs#998): Remove `.no_output_timeout(true)` once rust-docs is no longer a
        // mandatory component.
        Command::new(workspace, installer.to_string_lossy().as_ref())
            .args(&[
                "-y",
                "--no-modify-path",
                "--default-toolchain",
                MAIN_TOOLCHAIN_NAME,
            ])
            .no_output_timeout(None)
            .env("RUSTUP_HOME", workspace.rustup_home())
            .env("CARGO_HOME", workspace.cargo_home())
            .run()
            .with_context(|_| "unable to install rustup")?;

        Ok(())
    }

    fn update(&self, workspace: &Workspace) -> Result<(), Error> {
        Command::new(workspace, &RUSTUP)
            .args(&["self", "update"])
            .run()
            .with_context(|_| "failed to update rustup")?;
        Command::new(workspace, &RUSTUP)
            .args(&["update", MAIN_TOOLCHAIN_NAME])
            .run()
            .with_context(|_| format!("failed to update main toolchain {}", MAIN_TOOLCHAIN_NAME))?;
        Ok(())
    }
}
