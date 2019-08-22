use crate::cmd::{Command, MountKind, Runnable, SandboxBuilder};
use crate::prepare::Prepare;
use crate::{Crate, Toolchain, Workspace};
use failure::Error;
use std::path::PathBuf;

/// Directory in the [`Workspace`](struct.Workspace.html) where builds can be executed.
///
/// The build directory contains the source code of the crate being built and the target directory
/// used by cargo to store build artifacts. If multiple builds are executed in the same build
/// directory they will share the target directory.
pub struct BuildDirectory {
    workspace: Workspace,
    name: String,
}

impl BuildDirectory {
    pub(crate) fn new(workspace: Workspace, name: &str) -> Self {
        Self {
            workspace,
            name: name.into(),
        }
    }

    /// Run a sandboxed build of the provided crate with the provided toolchain. The closure will
    /// be provided an instance of [`Build`](struct.Build.html) that allows spawning new processes
    /// inside the sandbox.
    ///
    /// All the state will be kept on disk as long as the closure doesn't exit: after that things
    /// might be removed.
    ///
    /// # Example
    ///
    /// ```no_run
    /// let mut build_dir = workspace.build_dir("foo");
    /// build_dir.build(&krate, &toolchain, sandbox, |build| {
    ///     build.cargo().args(&["test", "--all"]).run()?;
    ///     Ok(())
    /// })?;
    /// ```
    pub fn build<R, F: FnOnce(&Build) -> Result<R, Error>>(
        &mut self,
        toolchain: &Toolchain,
        krate: &Crate,
        sandbox: SandboxBuilder,
        f: F,
    ) -> Result<R, Error> {
        let source_dir = self.source_dir();
        if source_dir.exists() {
            std::fs::remove_dir_all(&source_dir)?;
        }

        let mut prepare = Prepare::new(&self.workspace, toolchain, krate, &source_dir);
        prepare.prepare()?;

        std::fs::create_dir_all(self.target_dir())?;
        let res = f(&Build {
            dir: self,
            toolchain,
            sandbox: sandbox.clone(),
        })?;

        std::fs::remove_dir_all(&source_dir)?;
        Ok(res)
    }

    /// Remove all the contents of the build directory, freeing disk space.
    pub fn purge(&mut self) -> Result<(), Error> {
        let build_dir = self.build_dir();
        if build_dir.exists() {
            std::fs::remove_dir_all(build_dir)?;
        }
        Ok(())
    }

    fn build_dir(&self) -> PathBuf {
        self.workspace.builds_dir().join(&self.name)
    }

    fn source_dir(&self) -> PathBuf {
        self.build_dir().join("source")
    }

    fn target_dir(&self) -> PathBuf {
        self.build_dir().join("target")
    }
}

/// API to interact with a running build.
///
/// This is created from [`BuildDirectory::build`](struct.BuildDirectory.html#method.build)
pub struct Build<'b> {
    dir: &'b BuildDirectory,
    toolchain: &'b Toolchain,
    sandbox: SandboxBuilder,
}

impl Build<'_> {
    /// Run a command inside the sandbox.
    ///
    /// Any `cargo` invocation will automatically be configured to use a target directory mounted
    /// outside the sandbox. The crate's source directory will be the working directory for the
    /// command.
    ///
    /// # Example
    ///
    /// ```no_run
    /// let mut build_dir = workspace.build_dir("foo");
    /// build_dir.build(&krate, &toolchain, sandbox, |build| {
    ///     build.cmd("rustfmt").args(&["--check"]).args("src/main.rs").run()?;
    ///     Ok(())
    /// })?;
    /// ```
    pub fn cmd<R: Runnable>(&self, bin: R) -> Command {
        let container_dir = &*crate::cmd::container_dirs::TARGET_DIR;

        Command::new_sandboxed(
            &self.dir.workspace,
            self.sandbox
                .clone()
                .mount(&self.dir.target_dir(), container_dir, MountKind::ReadWrite),
            bin,
        )
        .cd(self.dir.source_dir())
        .env("CARGO_TARGET_DIR", container_dir)
    }

    /// Run `cargo` inside the sandbox, using the toolchain chosen for the build.
    ///
    /// `cargo` will automatically be configured to use a target directory mounted outside the
    /// sandbox. The crate's source directory will be the working directory for the command.
    ///
    /// # Example
    ///
    /// ```no_run
    /// let mut build_dir = workspace.build_dir("foo");
    /// build_dir.build(&krate, &toolchain, sandbox, |build| {
    ///     build.cargo().args(&["test", "--all"]).run()?;
    ///     Ok(())
    /// })?;
    /// ```
    pub fn cargo(&self) -> Command {
        self.cmd(self.toolchain.cargo())
    }

    /// Get the path to the source code on the host machine (outside the sandbox).
    pub fn host_source_dir(&self) -> PathBuf {
        self.dir.source_dir()
    }

    /// Get the path to the target directory on the host machine (outside the sandbox).
    pub fn host_target_dir(&self) -> PathBuf {
        self.dir.target_dir()
    }
}
