pub mod dex;
// pub mod market; // Removed
pub mod wallet;

// Re-export commonly used display functions
pub use dex::display_dex_pairs;
pub use dex::display_dex_stats;
// pub use market::display_token_metrics; // Removed
// pub use market::display_trending_tokens; // Removed
pub use wallet::display_wallet_info;
