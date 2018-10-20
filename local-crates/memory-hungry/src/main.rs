#[cfg(channel_beta)]
mod allocate;

fn main() {
    println!("Hello world");
}

#[test]
#[cfg(channel_beta)]
fn test_allocate() {
    allocate::allocate();
}
