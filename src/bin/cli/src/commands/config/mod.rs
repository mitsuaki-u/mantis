//! Configuration management commands
//!
//! This module provides a clean, organized structure for all configuration-related
//! CLI commands. Each command type has its own module for better maintainability.

use crate::error::Result;
use clap::Subcommand;

// Re-export command handlers
pub mod api_keys;
pub mod display;
pub mod formatting;
pub mod getters;
pub mod path;
pub mod reset;
pub mod setters;

/// Configuration management commands
#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Set an API key for external services
    ///
    /// Example: mantis config set-key infura YOUR_API_KEY
    SetKey {
        /// Service name (infura, alchemy)
        service: String,
        /// API key (no quotes needed)
        #[arg(value_parser = clap::value_parser!(String), allow_hyphen_values = true)]
        key: String,
    },

    /// Display current configuration with all settings
    ///
    /// Examples:
    ///   mantis config show
    ///   mantis config show --show-secrets (reveal API keys)
    ///   mantis config show --json (JSON output)
    Show {
        /// Show sensitive values like API keys (masked by default)
        #[arg(short, long)]
        show_secrets: bool,

        /// Output as JSON instead of formatted text
        #[arg(short, long)]
        json: bool,
    },

    /// Get a specific configuration value
    ///
    /// Examples:
    ///   mantis config get trading.live_trading
    ///   mantis config get trading.max_position_size
    ///   mantis config get api_keys.infura
    Get {
        /// Configuration key using dot notation (e.g., trading.live_trading)
        key: String,
    },

    /// Set a specific configuration value
    ///
    /// Examples:
    ///   mantis config set trading.live_trading true
    ///   mantis config set trading.max_position_size 1000.0
    ///   mantis config set trading.max_tokens_to_scan 150
    Set {
        /// Configuration key using dot notation (e.g., trading.live_trading)
        key: String,

        /// Value to set (parsed automatically based on field type)
        value: String,
    },

    /// Reset configuration to factory defaults
    ///
    /// Example: mantis config reset --force
    Reset {
        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Show the path to the configuration file
    ///
    /// Example: mantis config path
    Path,
}

/// Main router for configuration commands
pub async fn handle_config_command(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::SetKey { service, key } => api_keys::handle_set_key(service, key).await,

        ConfigCommands::Show { show_secrets, json } => {
            display::handle_show(show_secrets, json).await
        }

        ConfigCommands::Get { key } => getters::handle_get(key).await,

        ConfigCommands::Reset { force } => reset::handle_reset(force).await,

        ConfigCommands::Path => path::handle_path().await,

        ConfigCommands::Set { key, value } => setters::handle_set(key, value).await,
    }
}
