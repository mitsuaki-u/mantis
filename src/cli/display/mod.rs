pub mod market;
pub mod dex;
pub mod wallet;

// Re-export commonly used display functions
pub use market::display_token_metrics;
pub use market::display_trending_tokens;
pub use dex::display_dex_pairs;
pub use dex::display_dex_stats;
pub use wallet::display_wallet_info; 