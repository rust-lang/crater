use crate::native;
use crate::prelude::*;
use crate::tools::{binary_path, InstallableTool, CARGO, CARGO_INSTALL_UPDATE};
use rustwide::cmd::{Binary, Command, Runnable};
use rustwide::Workspace;

pub(crate) struct BinaryCrate {
    pub(in crate::tools) crate_name: &'static str,
    pub(in crate::tools) binary: &'static str,
    pub(in crate::tools) cargo_subcommand: Option<&'static str>,
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

impl InstallableTool for BinaryCrate {
    fn name(&self) -> &'static str {
        self.binary
    }

    fn is_installed(&self) -> Fallible<bool> {
        let path = binary_path(self.binary);
        if !path.is_file() {
            return Ok(false);
        }

        Ok(native::is_executable(path)?)
    }

    fn install(&self, workspace: &Workspace) -> Fallible<()> {
        Command::new(workspace, &CARGO)
            .args(&["install", self.crate_name])
            .timeout(None)
            .run()?;
        Ok(())
    }

    fn update(&self, workspace: &Workspace) -> Fallible<()> {
        Command::new(workspace, &CARGO_INSTALL_UPDATE)
            .args(&[self.crate_name])
            .timeout(None)
            .run()?;
        Ok(())
    }
}
