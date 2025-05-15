// Core modules
pub mod core;
pub mod cli;
pub mod domain;
pub mod infra;
pub mod utils;

// Re-export Error type for backward compatibility
pub use core::error::Error;

// Initialize logger with default settings
pub fn init_logger() {
    // For backward compatibility, use default stdout logging
    // This is mainly for integration tests or simple usage
    let _ = utils::logging::init_logger(
        Some("info"),  // Default log level
        false,         // Debug mode off
        None,          // No log file
        "logs",        // Default logs directory
        "default",     // Command name
        None,          // No module filters
    );
}

// Re-exports for backward compatibility
// These help maintain existing code that imports from the old structure
pub use core::config as config;
pub use core::error as error;
pub use core::models as types;

pub use cli::commands;
pub use cli::display;

pub use domain::trading;
pub use domain::dex;
pub use domain::market;
pub use domain::wallet;

pub use infra::api;
pub use infra::db;
pub use infra::db::repositories;
pub use infra::cache;
pub use infra::actors;
pub use infra::collector as data;

// Re-export commonly used types and functions
pub use infra::api::market::get_market_overview;
pub use infra::api::{get_dex_pair, get_dex_stats};
pub use infra::api::news::get_token_news;
pub use infra::api::wallet::get_wallet_info; 