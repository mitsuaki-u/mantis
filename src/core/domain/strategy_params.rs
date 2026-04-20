//! Strategy parameters for pure core logic
//!
//! This module defines the StrategyParams struct that encapsulates all configuration
//! needed by core trading strategies and validation logic. The CLI layer loads config
//! and converts it to StrategyParams, then the application layer owns and distributes it.
//!
//! Clean Architecture Pattern:
//! - Core: Defines StrategyParams (this file) - pure types, no I/O
//! - CLI: Loads config from files/env, converts Config → StrategyParams
//! - Application: Owns StrategyParams, passes to strategies on every evaluation

use crate::core::constants::*;

/// Strategy evaluation and validation parameters
///
/// This struct contains all parameters needed by core trading strategies and validation logic.
/// It should never load config itself - only receive values from outer layers.
#[derive(Debug, Clone)]
pub struct StrategyParams {
    // ========== Position Sizing ==========
    /// Minimum position size in USD
    pub min_position_size: f64,

    /// Maximum position size in USD
    pub max_position_size: f64,

    /// Maximum total exposure across all positions in USD
    pub max_total_exposure: f64,

    /// Maximum number of concurrent positions
    pub max_positions: usize,

    // ========== Risk Management ==========
    /// Maximum allowed 24-hour volatility percentage (e.g., 30.0 = 30%)
    /// Tokens with price changes exceeding this threshold will be skipped
    pub max_volatility_24h: f64,

    /// Stop loss percentage (e.g., 5.0 = 5%)
    pub stop_loss_pct: f64,

    /// Take profit percentage (e.g., 10.0 = 10%)
    pub take_profit_pct: f64,

    /// Maximum daily loss percentage
    pub max_daily_loss_pct: f64,

    /// Maximum drawdown percentage
    pub max_drawdown_pct: f64,

    /// Maximum single trade risk as percentage of wallet
    pub max_single_trade_risk_pct: f64,

    // ========== Execution & Validation ==========
    /// Maximum allowed price deviation from signal to execution (e.g., 0.05 = 5%)
    pub max_execution_price_deviation: f64,

    /// Enable pre-execution price cross-check validation
    pub enable_price_cross_check: bool,

    /// Maximum allowed price discrepancy between API and blockchain (e.g., 0.05 = 5%)
    pub max_price_discrepancy_threshold: f64,

    /// Minimum token age in months for trading
    pub min_token_age_months: u32,

    // ========== Market Filters ==========
    /// Minimum 24h volume required for trading in USD
    pub min_volume: f64,

    /// Minimum liquidity requirement in USD
    pub min_liquidity: f64,

    /// Signal confidence threshold (0.0-1.0)
    pub signal_confidence_threshold: f64,
}

impl Default for StrategyParams {
    fn default() -> Self {
        Self {
            // Position Sizing - from core constants
            min_position_size: DEFAULT_MIN_POSITION_SIZE,
            max_position_size: DEFAULT_MAX_POSITION_SIZE,
            max_total_exposure: DEFAULT_MAX_TOTAL_EXPOSURE,
            max_positions: DEFAULT_MAX_POSITIONS,

            // Risk Management - from core constants
            max_volatility_24h: 30.0, // Default max 24h volatility
            stop_loss_pct: DEFAULT_STOP_LOSS_PCT,
            take_profit_pct: DEFAULT_TAKE_PROFIT_PCT,
            max_daily_loss_pct: DEFAULT_MAX_DAILY_LOSS,
            max_drawdown_pct: DEFAULT_MAX_DRAWDOWN,
            max_single_trade_risk_pct: DEFAULT_MAX_TRADE_RISK_PCT,

            // Execution & Validation - from core constants
            max_execution_price_deviation: DEFAULT_MAX_EXECUTION_PRICE_DEVIATION,
            enable_price_cross_check: ENABLE_PRICE_CROSS_CHECK,
            max_price_discrepancy_threshold: MAX_PRICE_DISCREPANCY_THRESHOLD,
            min_token_age_months: 1, // Conservative default (not in constants)

            // Market Filters - from core constants
            min_volume: DEFAULT_MIN_VOLUME,
            min_liquidity: DEFAULT_MIN_LIQUIDITY,
            signal_confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
        }
    }
}

impl StrategyParams {
    /// Create a new StrategyParams with custom values
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder pattern: Set minimum position size
    pub fn with_min_position_size(mut self, size: f64) -> Self {
        self.min_position_size = size;
        self
    }

    /// Builder pattern: Set maximum position size
    pub fn with_max_position_size(mut self, size: f64) -> Self {
        self.max_position_size = size;
        self
    }

    /// Builder pattern: Set maximum 24-hour volatility
    pub fn with_max_volatility_24h(mut self, volatility: f64) -> Self {
        self.max_volatility_24h = volatility.clamp(0.0, 100.0);
        self
    }

    /// Builder pattern: Set stop loss percentage
    pub fn with_stop_loss(mut self, pct: f64) -> Self {
        self.stop_loss_pct = pct;
        self
    }

    /// Builder pattern: Set maximum positions
    pub fn with_max_positions(mut self, max: usize) -> Self {
        self.max_positions = max;
        self
    }

    /// Validate that parameters are within reasonable bounds
    pub fn validate(&self) -> Result<(), String> {
        if self.min_position_size <= 0.0 {
            return Err("min_position_size must be positive".to_string());
        }

        if self.max_position_size < self.min_position_size {
            return Err("max_position_size must be >= min_position_size".to_string());
        }

        if self.max_volatility_24h < 0.0 || self.max_volatility_24h > 100.0 {
            return Err("max_volatility_24h must be between 0 and 100".to_string());
        }

        if self.stop_loss_pct < 0.0 || self.stop_loss_pct > 100.0 {
            return Err("stop_loss_pct must be between 0 and 100".to_string());
        }

        if self.take_profit_pct < 0.0 || self.take_profit_pct > 1000.0 {
            return Err("take_profit_pct must be between 0 and 1000".to_string());
        }

        if self.signal_confidence_threshold < 0.0 || self.signal_confidence_threshold > 1.0 {
            return Err("signal_confidence_threshold must be between 0.0 and 1.0".to_string());
        }

        if self.max_execution_price_deviation < 0.0 || self.max_execution_price_deviation > 1.0 {
            return Err("max_execution_price_deviation must be between 0.0 and 1.0".to_string());
        }

        if self.max_price_discrepancy_threshold < 0.0 || self.max_price_discrepancy_threshold > 1.0
        {
            return Err("max_price_discrepancy_threshold must be between 0.0 and 1.0".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_params_are_valid() {
        let params = StrategyParams::default();
        assert!(params.validate().is_ok());
    }

    #[test]
    fn test_builder_pattern() {
        let params = StrategyParams::new()
            .with_min_position_size(50.0)
            .with_max_position_size(500.0)
            .with_max_volatility_24h(40.0)
            .with_stop_loss(7.5)
            .with_max_positions(10);

        assert_eq!(params.min_position_size, 50.0);
        assert_eq!(params.max_position_size, 500.0);
        assert_eq!(params.max_volatility_24h, 40.0);
        assert_eq!(params.stop_loss_pct, 7.5);
        assert_eq!(params.max_positions, 10);
        assert!(params.validate().is_ok());
    }

    #[test]
    fn test_validation_min_greater_than_max() {
        let params = StrategyParams {
            min_position_size: 200.0,
            max_position_size: 100.0,
            ..Default::default()
        };

        assert!(params.validate().is_err());
    }

    #[test]
    fn test_validation_invalid_volatility() {
        let params = StrategyParams {
            max_volatility_24h: 150.0, // Should be 0-100
            ..Default::default()
        };

        assert!(params.validate().is_err());
    }
}
