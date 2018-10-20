use std::net::TcpStream;

pub fn call() {
    // Try to connect to www.rust-lang.org:80
    // If network access is disabled even this should fail
    let _ = TcpStream::connect("www.rust-lang.org:80").unwrap();
}
