use crate::dirs::{CARGO_HOME, RUSTUP_HOME};
use crate::native;
use crate::prelude::*;
use crate::run::{Binary, RunCommand, Runnable};
use crate::toolchain::Toolchain;
use crate::toolchain::MAIN_TOOLCHAIN_NAME;
use crate::tools::{binary_path, InstallableTool, RUSTUP};
use std::env::consts::EXE_SUFFIX;
use std::fs::{self, File};
use std::io;
use tempfile::tempdir;

static RUSTUP_BASE_URL: &str = "https://static.rust-lang.org/rustup/dist";

pub(crate) struct Rustup;

impl Runnable for Rustup {
    fn binary(&self) -> Binary {
        Binary::InstalledByCrater("rustup".into())
    }

    fn prepare_command<'pl>(&self, cmd: RunCommand<'pl>) -> RunCommand<'pl> {
        cmd.local_rustup(true)
    }
}

impl InstallableTool for Rustup {
    fn name(&self) -> &'static str {
        "rustup"
    }

    fn is_installed(&self) -> Fallible<bool> {
        let path = binary_path("rustup");
        if !path.is_file() {
            return Ok(false);
        }

        Ok(native::is_executable(path)?)
    }

    fn install(&self) -> Fallible<()> {
        fs::create_dir_all(&*CARGO_HOME)?;
        fs::create_dir_all(&*RUSTUP_HOME)?;

        let url = format!(
            "{}/{}/rustup-init{}",
            RUSTUP_BASE_URL,
            crate::HOST_TARGET,
            EXE_SUFFIX
        );
        let mut resp =
            crate::utils::http::get_sync(&url).with_context(|_| "unable to download rustup")?;

        let tempdir = tempdir()?;
        let installer = &tempdir.path().join(format!("rustup-init{}", EXE_SUFFIX));
        {
            let mut file = File::create(installer)?;
            io::copy(&mut resp, &mut file)?;
            native::make_executable(installer)?;
        }

        // TODO(rustup.rs#998): Remove `.quiet(true)` once rust-docs is no longer a mandatory
        // component.
        RunCommand::new(installer.to_string_lossy().as_ref())
            .args(&[
                "-y",
                "--no-modify-path",
                "--default-toolchain",
                MAIN_TOOLCHAIN_NAME,
            ])
            .quiet(true)
            .local_rustup(true)
            .run()
            .with_context(|_| "unable to install rustup")?;

        Ok(())
    }

    fn update(&self) -> Fallible<()> {
        RunCommand::new(&RUSTUP)
            .args(&["self", "update"])
            .run()
            .with_context(|_| "failed to update rustup")?;
        RunCommand::new(&RUSTUP)
            .args(&["update", MAIN_TOOLCHAIN_NAME])
            .run()
            .with_context(|_| format!("failed to update main toolchain {}", MAIN_TOOLCHAIN_NAME))?;
        Ok(())
    }
}

pub(crate) struct Cargo<'a> {
    pub(in crate::tools) toolchain: &'a Toolchain,
    pub(in crate::tools) unstable_features: bool,
}

impl<'a> Cargo<'a> {
    pub(crate) fn toolchain<'new>(&self, toolchain: &'new Toolchain) -> Cargo<'new> {
        Cargo {
            toolchain,
            unstable_features: self.unstable_features,
        }
    }

    pub(crate) fn unstable_features(&self, enable: bool) -> Self {
        Cargo {
            toolchain: self.toolchain,
            unstable_features: enable,
        }
    }
}

impl<'a> Runnable for Cargo<'a> {
    fn binary(&self) -> Binary {
        Binary::InstalledByCrater("cargo".into())
    }

    fn prepare_command<'pl>(&self, mut cmd: RunCommand<'pl>) -> RunCommand<'pl> {
        if self.unstable_features {
            cmd = cmd.env("__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS", "nightly");
        }

        cmd.args(&[format!("+{}", self.toolchain.rustup_name())])
            .local_rustup(true)
    }
}
