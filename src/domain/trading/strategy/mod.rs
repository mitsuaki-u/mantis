//! Trading Strategy Module
//!
//! This module provides a comprehensive framework for implementing and managing
//! trading strategies. It includes:
//!
//! - Core types and traits for strategy implementation
//! - Base utilities and risk management
//! - Individual strategy implementations (Momentum, RSI)
//! - Strategy factory for creation and registration
//!
//! ## Usage
//!
//! ```rust
//! use crate::domain::trading::strategy::{Strategy, create_strategy};
//!
//! // Create a momentum strategy
//! let strategy = create_strategy("momentum", 5.0, 1_000_000.0, 10.0, None, None, None)?;
//!
//! // Analyze a token for entry signals
//! let should_buy = strategy.analyze_for_entry(&token_metrics);
//! ```

// Core strategy framework
pub mod base;
pub mod core;

// Individual strategy implementations
pub mod momentum;
// pub mod mock; // Temporarily disabled due to compilation issues
pub mod rsi;

// Strategy factory and utilities
pub mod factory;

// Re-export core types for backward compatibility
pub use core::{ExitReason, Position, Signal, Strategy, TradingStrategy};

// Re-export strategy implementations
pub use momentum::MomentumStrategy;
// pub use mock::MockStrategy; // Temporarily disabled
pub use rsi::RSIStrategy;

// Re-export factory functions
pub use factory::{
    create_momentum_strategy_with_weights, create_strategy, get_available_strategies,
    validate_strategy_params,
};

// Re-export the macro
pub use crate::register_strategy;

// Backward compatibility: re-export everything that was previously public
pub use base::{BaseStrategy, RiskManager};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::market::TokenMetrics;

    fn create_test_token() -> TokenMetrics {
        TokenMetrics {
            id: "test-token".to_string(),
            symbol: "TEST".to_string(),
            name: "Test Token".to_string(),
            price_usd: 1.0,
            price_change_24h: 5.0,
            volume_24h: 500_000.0,
            market_cap: 1_000_000.0,
            market_cap_rank: Some(100),
            latest_news: None,
            chain: Some("ethereum".to_string()),
            last_updated: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_strategy_creation() {
        // Test momentum strategy creation
        let momentum = create_strategy("momentum", 5.0, 1_000_000.0, 10.0, None, None, None);
        assert!(momentum.is_ok());
        assert_eq!(momentum.unwrap().name(), "momentum_strategy");

        // Test RSI strategy creation
        let rsi = create_strategy("rsi", 30.0, 100_000.0, 5.0, None, None, None);
        assert!(rsi.is_ok());
        assert_eq!(rsi.unwrap().name(), "rsi_strategy");
    }

    #[test]
    fn test_strategy_analysis() {
        let token = create_test_token();

        // Test momentum strategy
        let momentum = MomentumStrategy::new(5.0, 100_000.0, 10.0);
        let signal = momentum.analyze(&token);
        assert!(matches!(signal, Signal::Hold)); // Should be Hold without enough data

        // Test RSI strategy
        let rsi = RSIStrategy::new(30.0, 100_000.0, 5.0);
        let signal = rsi.analyze(&token);
        assert!(matches!(signal, Signal::Hold)); // Should be Hold without enough data
    }

    #[test]
    fn test_position_calculations() {
        let position = Position::new(
            "test-token".to_string(),
            "test-provider".to_string(),
            1.0,
            100.0,
            chrono::Utc::now(),
        );

        // Test PnL calculations
        assert_eq!(position.calculate_pnl(1.1), 10.0); // 10% gain on 100 tokens
        assert_eq!(position.calculate_pnl_pct(1.1), 10.0); // 10% gain
        assert_eq!(position.calculate_pnl(0.9), -10.0); // 10% loss on 100 tokens
        assert_eq!(position.calculate_pnl_pct(0.9), -10.0); // 10% loss
    }

    #[test]
    fn test_signal_methods() {
        assert!(Signal::Buy.is_buy());
        assert!(Signal::StrongBuy.is_buy());
        assert!(!Signal::Hold.is_buy());

        assert!(Signal::Sell.is_sell());
        assert!(Signal::StrongSell.is_sell());
        assert!(!Signal::Hold.is_sell());

        assert!(Signal::Hold.is_hold());
        assert!(!Signal::Buy.is_hold());
    }

    #[test]
    fn test_exit_reason() {
        let risk_exit = ExitReason::risk_based("Stop loss triggered");
        assert!(risk_exit.is_risk_based);
        assert_eq!(risk_exit.confidence, 1.0);

        let strategy_exit = ExitReason::strategy_based("RSI overbought");
        assert!(!strategy_exit.is_risk_based);
        assert_eq!(strategy_exit.confidence, 0.8);
    }

    #[test]
    fn test_available_strategies() {
        let strategies = get_available_strategies();
        assert!(strategies.contains(&"momentum"));
        assert!(strategies.contains(&"rsi"));
        assert!(strategies.len() >= 2);
    }
}
