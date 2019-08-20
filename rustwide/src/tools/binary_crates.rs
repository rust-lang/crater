use crate::cmd::{Binary, Command, Runnable};
use crate::tools::{binary_path, Tool, CARGO_INSTALL_UPDATE};
use crate::{Toolchain, Workspace};
use failure::Error;

pub(crate) struct BinaryCrate {
    pub(super) crate_name: &'static str,
    pub(super) binary: &'static str,
    pub(super) cargo_subcommand: Option<&'static str>,
}

impl Runnable for BinaryCrate {
    fn name(&self) -> Binary {
        Binary::ManagedByRustwide(if self.cargo_subcommand.is_some() {
            "cargo".into()
        } else {
            self.binary.into()
        })
    }

    fn prepare_command<'w, 'pl>(&self, mut cmd: Command<'w, 'pl>) -> Command<'w, 'pl> {
        if let Some(subcommand) = self.cargo_subcommand {
            cmd = cmd.args(&[subcommand]);
        }
        cmd
    }
}

impl Tool for BinaryCrate {
    fn name(&self) -> &'static str {
        self.binary
    }

    fn is_installed(&self, workspace: &Workspace) -> Result<bool, Error> {
        let path = binary_path(workspace, self.binary);
        if !path.is_file() {
            return Ok(false);
        }

        Ok(crate::native::is_executable(path)?)
    }

    fn install(&self, workspace: &Workspace) -> Result<(), Error> {
        Command::new(workspace, &Toolchain::MAIN.cargo())
            .args(&["install", self.crate_name])
            .timeout(None)
            .run()?;
        Ok(())
    }

    fn update(&self, workspace: &Workspace) -> Result<(), Error> {
        Command::new(workspace, &CARGO_INSTALL_UPDATE)
            .args(&[self.crate_name])
            .timeout(None)
            .run()?;
        Ok(())
    }
}
