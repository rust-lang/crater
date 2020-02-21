// This tests our ICE regression handling.

#[cfg(channel_beta)]
fn main() {
    break rust;
}

#[cfg(not(channel_beta))]
fn main() {
    thisisabuildfailure;
}
