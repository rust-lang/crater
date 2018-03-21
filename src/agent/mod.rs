mod api;

use agent::api::AgentApi;
use errors::*;

pub fn run(url: &str, token: &str) -> Result<()> {
    info!("connecting to crater server {}...", url);

    let agent = AgentApi::new(url, token);
    let config = agent.config()?;

    info!("connected to the crater server!");
    info!("assigned agent name: {}", config.agent_name);

    Ok(())
}
