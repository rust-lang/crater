mod http;

use errors::*;
use server::http::Server;

pub struct Data;

pub fn run() -> Result<()> {
    let mut server = Server::new(Data)?;
    server.run()?;
    Ok(())
}
