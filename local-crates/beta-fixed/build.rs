extern crate rustc_version;

use rustc_version::{version_meta, Channel};

fn main() {
    if let Channel::Beta = version_meta().unwrap().channel {
        println!("cargo:rustc-cfg=channel_beta");
    }

    // Rebuild the crate only if the build.rs file changes
    println!("cargo:rebuild-if-changed=build.rs");
}
