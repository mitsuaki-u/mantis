//! Configuration reset commands

use crate::config::Config;
use crate::error::Result;
use colored::*;

/// Handle the Reset command to restore default configuration
pub async fn handle_reset(force: bool) -> Result<()> {
    if !force {
        println!(
            "{} This will reset all configuration to defaults.",
            "Warning:".bright_yellow()
        );
        println!("Run with --force to confirm this action.");
        return Ok(());
    }

    // Create a new default configuration
    let config = Config::default();

    // Save it
    config.save()?;

    println!("{} Configuration reset to defaults", "✓".green());
    Ok(())
}
