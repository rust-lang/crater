#[cfg(channel_beta)]
mod network;

fn main() {
    println!("Hello, world!");
}

#[cfg(channel_beta)]
#[test]
fn test_network_access() {
    network::call();
}
