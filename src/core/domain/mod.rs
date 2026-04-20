pub mod dex;
pub mod market;
pub mod strategy_params;
pub mod token;
pub mod trading;
pub mod wallet;

// Re-export commonly used types
pub use dex::{DexPair, DexStats, DexToken};
pub use market::TokenMetrics;
pub use trading::{ExitReason, Position, Signal};
// Removed: news::NewsItem (module doesn't exist)
pub use wallet::{TokenHolding, TokenTransfer, Transaction, WalletInfo};

// Re-export the new token data model for easier imports
pub use token::TokenData;

// Re-export strategy parameters for clean architecture
pub use strategy_params::StrategyParams;
