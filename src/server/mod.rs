mod http;
mod tokens;

use errors::*;
use server::http::Server;
use server::tokens::Tokens;

pub struct Data {
    pub tokens: Tokens,
}

pub fn run() -> Result<()> {
    let tokens = tokens::Tokens::load()?;

    let mut server = Server::new(Data { tokens })?;
    server.run()?;
    Ok(())
}
