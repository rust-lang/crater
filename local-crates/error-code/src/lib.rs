#[cfg(channel_beta)]
pub const STRING: String = String::from("invalid");

#[cfg(channel_beta)]
static X: i32 = 42;
#[cfg(channel_beta)]
const Y: i32 = X;

fn hello() {
    println!("Hello, world!");
}
