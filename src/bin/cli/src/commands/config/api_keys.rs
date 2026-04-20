//! API key management commands

use crate::config::Config;
use crate::error::Result;
use colored::*;

/// Handle the SetKey command to update API keys
pub async fn handle_set_key(service: String, key: String) -> Result<()> {
    let mut config = Config::load()?;
    config.set_api_key(&service, key)?;
    println!(
        "{} API key for {} updated successfully",
        "✓".green(),
        service.cyan()
    );
    Ok(())
}
