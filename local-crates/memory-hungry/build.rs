extern crate rustc_version;

#[path = "src/allocate.rs"]
mod allocate;

use rustc_version::{version_meta, Channel};

fn main() {
    if let Channel::Beta = version_meta().unwrap().channel {
        // On the beta channel allocate in tests
        println!("cargo:rustc-cfg=channel_beta");
    } else {
        // On the stable channel allocate in build.rs
        allocate::allocate();
    }

    // Rebuild the crate only if the build.rs file changes
    println!("cargo:rebuild-if-changed=build.rs");
}
