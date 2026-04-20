use clap::Subcommand;

pub mod config;
pub mod database;
pub mod trading;

// Re-export commands for easier imports
pub use config::{handle_config_command, ConfigCommands};
pub use database::commands::{handle_database_command, DatabaseCommands};
pub use trading::{handle_trading_command, TradingArgs};

#[derive(Subcommand)]
pub enum Commands {
    /// Manage application configuration (API keys, trading parameters, network settings)
    #[command(subcommand)]
    Config(ConfigCommands),

    /// Control trading bot operations (start, stop, view positions, trading history)
    #[command(subcommand)]
    Trading(Box<TradingArgs>),

    /// Database operations (reset, view schema)
    #[command(subcommand)]
    Database(DatabaseCommands),
}
