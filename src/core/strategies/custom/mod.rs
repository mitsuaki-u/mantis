//! Custom Trading Strategies
//!
//! This module contains concrete implementations of trading strategies.
//! Each strategy implements the `TradingStrategy` trait defined in the parent module.

pub mod momentum;
pub mod rsi;

// Re-export strategies for easier access
pub use momentum::MomentumStrategy;
pub use rsi::RsiStrategy;
