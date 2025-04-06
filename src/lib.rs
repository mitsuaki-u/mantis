pub mod api;
pub mod commands;
pub mod display;
pub mod types;
pub mod config;
pub mod trading;
pub mod db;
pub mod data;
pub mod error;
pub mod repositories;
pub mod utils;
pub mod dex;
pub mod actors;

// Re-export Error type
pub use error::Error;

// Initialize logger
pub fn init_logger() {
    env_logger::init();
}

// Re-export commonly used types and functions
pub use api::market::get_market_overview;
pub use api::{get_dex_pair, get_dex_stats};
pub use api::news::get_token_news;
pub use api::wallet::get_wallet_info; 