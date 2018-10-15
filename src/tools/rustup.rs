use dirs::{CARGO_HOME, RUSTUP_HOME};
use errors::*;
use native;
use run::{Binary, RunCommand, Runnable};
use std::env::consts::EXE_SUFFIX;
use std::fs::{self, File};
use std::io;
use tempdir::TempDir;
use toolchain::Toolchain;
use toolchain::MAIN_TOOLCHAIN_NAME;
use tools::{binary_path, InstallableTool, RUSTUP};

static RUSTUP_BASE_URL: &str = "https://static.rust-lang.org/rustup/dist";

pub(crate) struct Rustup;

impl Runnable for Rustup {
    fn binary(&self) -> Binary {
        Binary::InstalledByCrater("rustup".into())
    }

    fn prepare_command(&self, cmd: RunCommand) -> RunCommand {
        cmd.local_rustup(true)
    }
}

impl InstallableTool for Rustup {
    fn name(&self) -> &'static str {
        "rustup"
    }

    fn is_installed(&self) -> Result<bool> {
        let path = binary_path("rustup");
        if !path.is_file() {
            return Ok(false);
        }

        Ok(native::is_executable(path)?)
    }

    fn install(&self) -> Result<()> {
        fs::create_dir_all(&*CARGO_HOME)?;
        fs::create_dir_all(&*RUSTUP_HOME)?;

        let url = format!(
            "{}/{}/rustup-init{}",
            RUSTUP_BASE_URL,
            ::HOST_TARGET,
            EXE_SUFFIX
        );
        let mut resp = ::utils::http::get(&url).chain_err(|| "unable to download rustup")?;

        let tempdir = TempDir::new("crater")?;
        let installer = &tempdir.path().join(format!("rustup-init{}", EXE_SUFFIX));
        {
            let mut file = File::create(installer)?;
            io::copy(&mut resp, &mut file)?;
            native::make_executable(installer)?;
        }

        RunCommand::new(installer.to_string_lossy().as_ref())
            .args(&[
                "-y",
                "--no-modify-path",
                "--default-toolchain",
                MAIN_TOOLCHAIN_NAME,
            ]).local_rustup(true)
            .run()
            .chain_err(|| "unable to install rustup")?;

        Ok(())
    }

    fn update(&self) -> Result<()> {
        RunCommand::new(&RUSTUP)
            .args(&["self", "update"])
            .run()
            .chain_err(|| "failed to update rustup")?;
        RunCommand::new(&RUSTUP)
            .args(&["update", MAIN_TOOLCHAIN_NAME])
            .run()
            .chain_err(|| format!("failed to update main toolchain {}", MAIN_TOOLCHAIN_NAME))?;
        Ok(())
    }
}

pub(crate) struct Cargo<'a> {
    pub(in tools) toolchain: &'a Toolchain,
    pub(in tools) unstable_features: bool,
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

    fn prepare_command(&self, mut cmd: RunCommand) -> RunCommand {
        if self.unstable_features {
            cmd = cmd.env("__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS", "nightly");
        }

        cmd.args(&[format!("+{}", self.toolchain.rustup_name())])
            .local_rustup(true)
    }
}
