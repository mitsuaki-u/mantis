//! Path configuration command - shows config file location

use crate::error::Result;

/// Handle the Path command to show configuration file location
pub async fn handle_path() -> Result<()> {
    let path = crate::config::get_config_path()?;
    println!("Configuration file: {}", path.display());
    Ok(())
}
