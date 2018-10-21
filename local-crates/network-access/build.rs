extern crate rustc_version;

#[path = "src/network.rs"]
mod network;

use rustc_version::{version_meta, Channel};

fn main() {
    if let Channel::Beta = version_meta().unwrap().channel {
        // On the beta channel connect to the network in tests
        println!("cargo:rustc-cfg=channel_beta");
    } else {
        // On the stable channel connect to the network in build.rs
        network::call();
    }

    // Rebuild the crate only if the build.rs file changes
    println!("cargo:rebuild-if-changed=build.rs");
}
