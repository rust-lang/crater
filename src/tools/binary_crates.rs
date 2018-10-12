use errors::*;
use native;
use run::{Binary, RunCommand, Runnable};
use tools::{binary_path, InstallableTool, CARGO, CARGO_INSTALL_UPDATE};

pub(crate) struct BinaryCrate {
    pub(in tools) crate_name: &'static str,
    pub(in tools) binary: &'static str,
    pub(in tools) cargo_subcommand: Option<&'static str>,
}

impl Runnable for BinaryCrate {
    fn binary(&self) -> Binary {
        if self.cargo_subcommand.is_some() {
            Binary::InstalledByCrater("cargo".into())
        } else {
            Binary::InstalledByCrater(self.binary.into())
        }
    }

    fn prepare_command(&self, mut cmd: RunCommand) -> RunCommand {
        if let Some(subcommand) = self.cargo_subcommand {
            cmd = cmd.args(&[subcommand]);
        }

        cmd.local_rustup(true)
    }
}

impl InstallableTool for BinaryCrate {
    fn name(&self) -> &'static str {
        self.binary
    }

    fn is_installed(&self) -> Result<bool> {
        let path = binary_path(self.binary);
        if !path.is_file() {
            return Ok(false);
        }

        Ok(native::is_executable(path)?)
    }

    fn install(&self) -> Result<()> {
        RunCommand::new(&CARGO)
            .args(&["install", self.crate_name])
            .enable_timeout(false)
            .run()?;
        Ok(())
    }

    fn update(&self) -> Result<()> {
        RunCommand::new(&CARGO_INSTALL_UPDATE)
            .args(&[self.crate_name])
            .enable_timeout(false)
            .run()?;
        Ok(())
    }
}
