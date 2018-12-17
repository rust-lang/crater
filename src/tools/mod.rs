mod binary_crates;
mod rustup;

use crate::dirs::CARGO_HOME;
use crate::prelude::*;
use crate::toolchain::MAIN_TOOLCHAIN;
use crate::tools::binary_crates::BinaryCrate;
use crate::tools::rustup::{Cargo, Rustup};
use std::env::consts::EXE_SUFFIX;
use std::path::PathBuf;

pub(crate) static RUSTUP: Rustup = Rustup;

pub(crate) static CARGO: Cargo = Cargo {
    toolchain: &MAIN_TOOLCHAIN,
    unstable_features: false,
};

pub(crate) static CARGO_INSTALL_UPDATE: BinaryCrate = BinaryCrate {
    crate_name: "cargo-update",
    binary: "cargo-install-update",
    cargo_subcommand: Some("install-update"),
};

pub(crate) static RUSTUP_TOOLCHAIN_INSTALL_MASTER: BinaryCrate = BinaryCrate {
    crate_name: "rustup-toolchain-install-master",
    binary: "rustup-toolchain-install-master",
    cargo_subcommand: None,
};

static INSTALLABLE_TOOLS: &[&InstallableTool] = &[
    &RUSTUP,
    &CARGO_INSTALL_UPDATE,
    &RUSTUP_TOOLCHAIN_INSTALL_MASTER,
];

fn binary_path(name: &str) -> PathBuf {
    PathBuf::from(CARGO_HOME.as_str())
        .join("bin")
        .join(format!("{}{}", name, EXE_SUFFIX))
}

trait InstallableTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_installed(&self) -> Fallible<bool>;
    fn install(&self) -> Fallible<()>;
    fn update(&self) -> Fallible<()>;
}

pub(crate) fn install() -> Fallible<()> {
    for tool in INSTALLABLE_TOOLS {
        if tool.is_installed()? {
            info!("tool {} is installed, trying to update it", tool.name());
            tool.update()?;
        } else {
            info!("tool {} is missing, installing it", tool.name());
            tool.install()?;

            if !tool.is_installed()? {
                bail!("tool {} is still missing after install", tool.name());
            }
        }
    }

    Ok(())
}
