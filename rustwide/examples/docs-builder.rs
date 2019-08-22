use env_logger;
use rustwide::{cmd::SandboxBuilder, Crate, Toolchain, WorkspaceBuilder};
use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    setup_logs();

    // Create a new workspace in work/
    let workspace =
        WorkspaceBuilder::new(Path::new("/tmp/docs-builder"), "rustwide-examples").init()?;

    // Run the builds on stable
    let toolchain = Toolchain::Dist {
        name: "stable".into(),
    };
    toolchain.install(&workspace)?;

    // Fetch lazy_static from crates.io
    let krate = Crate::crates_io("lazy_static", "1.0.0");
    krate.fetch(&workspace)?;

    // Configure a sandbox with 1GB of RAM and no network access
    let sandbox = SandboxBuilder::new()
        .memory_limit(Some(1024 * 1024 * 1024))
        .enable_networking(false);

    let mut build_dir = workspace.build_dir("docs");
    build_dir.build(&toolchain, &krate, sandbox, |build| {
        build.cargo().args(&["doc", "--no-deps"]).run()?;
        Ok(())
    })?;

    Ok(())
}

fn setup_logs() {
    let mut env = env_logger::Builder::new();
    env.filter_module("rustwide", log::LevelFilter::Info);
    if let Ok(content) = std::env::var("RUST_LOG") {
        env.parse(&content);
    }
    rustwide::logging::init_with(env.build());
}
