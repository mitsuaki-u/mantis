//! Default configuration values and macros for the Mantis application.
//!
//! This module provides a centralized location for all default configuration values,
//! using macros to reduce repetition and ensure consistency.

use crate::application::constants::DEFAULT_LIVE_TRADING;
use crate::application::constants::DEFAULT_SIMULATED_WETH_BALANCE;
use crate::application::constants::STRATEGY_DB_CONCURRENCY;
use crate::core::constants::{
    DEFAULT_BOLLINGER_WEIGHT, DEFAULT_CONFIDENCE_THRESHOLD, DEFAULT_MACD_WEIGHT,
    DEFAULT_MAX_GAS_COST_PCT, DEFAULT_MAX_GAS_COST_USD, DEFAULT_MAX_POSITIONS,
    DEFAULT_MAX_POSITION_SIZE, DEFAULT_MAX_TOTAL_EXPOSURE, DEFAULT_MIN_POSITION_SIZE,
    DEFAULT_MIN_TRADE_SIZE_FOR_GAS, DEFAULT_RSI_WEIGHT, DEFAULT_STRATEGY_TYPE,
    DEFAULT_VOLUME_WEIGHT,
};
use crate::core::constants::{
    DEFAULT_MAX_DAILY_LOSS, DEFAULT_MAX_DRAWDOWN, DEFAULT_MAX_TRADE_RISK_PCT,
    DEFAULT_MIN_ETH_BALANCE, DEFAULT_MIN_PORTFOLIO_RISK_FACTOR_THRESHOLD,
};
use crate::core::constants::{
    DEFAULT_MAX_EXECUTION_PRICE_DEVIATION, DEFAULT_MAX_TOKENS_TO_SCAN, DEFAULT_MIN_LIQUIDITY,
    DEFAULT_MIN_POOL_TRANSACTION_COUNT, DEFAULT_MIN_VOLUME, MAX_PRICE_DISCREPANCY_THRESHOLD,
};
use crate::core::constants::{DEFAULT_STOP_LOSS_PCT, DEFAULT_TAKE_PROFIT_PCT};
use crate::infrastructure::constants::{
    DEFAULT_COLLECTION_INTERVAL_SECS, DEFAULT_HISTORY_DAYS, DEFAULT_MAX_RETRIES,
    DEFAULT_TIMEOUT_SECS,
};
use crate::infrastructure::constants::{DEFAULT_HOST, DEFAULT_MAX_POOL_SIZE, DEFAULT_PORT};
use directories::ProjectDirs;

/// Macro to generate simple default functions that return constant values
macro_rules! simple_default {
    ($fn_name:ident, $return_type:ty, $value:expr) => {
        pub fn $fn_name() -> $return_type {
            $value
        }
    };
}

/// Macro to generate default functions that use constants from other modules
macro_rules! const_default {
    ($fn_name:ident, $return_type:ty, $const:expr) => {
        pub fn $fn_name() -> $return_type {
            $const
        }
    };
}

// ============================================================================
// BOOLEAN DEFAULTS
// ============================================================================

simple_default!(default_true, bool, true);
simple_default!(default_false, bool, false);
const_default!(default_live_trading, bool, DEFAULT_LIVE_TRADING);
simple_default!(default_enable_price_cross_check, bool, true);

// ============================================================================
// STRING DEFAULTS
// ============================================================================

const_default!(default_strategy, String, DEFAULT_STRATEGY_TYPE.to_string());
simple_default!(default_protocol_v3, String, "uniswap_v3".to_string());
simple_default!(
    default_market_data_provider,
    String,
    "dexscreener_solana".to_string()
);
simple_default!(default_primary_rpc_provider, String, "alchemy".to_string());

// Database defaults
const_default!(default_db_host, String, DEFAULT_HOST.to_string());
simple_default!(default_db_user, String, "mantis".to_string());
simple_default!(default_db_name, String, "mantis".to_string());

// API defaults

// ============================================================================
// NUMERIC DEFAULTS - TRADING
// ============================================================================

const_default!(default_threshold, f64, DEFAULT_CONFIDENCE_THRESHOLD);
const_default!(default_min_volume, f64, DEFAULT_MIN_VOLUME);
const_default!(default_min_liquidity, f64, DEFAULT_MIN_LIQUIDITY);
const_default!(
    default_min_pool_transaction_count,
    u32,
    DEFAULT_MIN_POOL_TRANSACTION_COUNT
);
const_default!(default_stop_loss, f64, DEFAULT_STOP_LOSS_PCT);
const_default!(default_take_profit, f64, DEFAULT_TAKE_PROFIT_PCT);
const_default!(default_max_position_size, f64, DEFAULT_MAX_POSITION_SIZE);
const_default!(default_min_position_size, f64, DEFAULT_MIN_POSITION_SIZE);
const_default!(default_max_total_exposure, f64, DEFAULT_MAX_TOTAL_EXPOSURE);
const_default!(
    default_max_tokens_to_scan,
    usize,
    DEFAULT_MAX_TOKENS_TO_SCAN
);

// Risk management defaults
const_default!(default_max_daily_loss_config, f64, DEFAULT_MAX_DAILY_LOSS);
const_default!(default_max_drawdown_config, f64, DEFAULT_MAX_DRAWDOWN);
const_default!(
    default_max_single_trade_risk_percentage_of_wallet,
    f64,
    DEFAULT_MAX_TRADE_RISK_PCT
);
const_default!(
    default_min_required_eth_balance_for_trading,
    f64,
    DEFAULT_MIN_ETH_BALANCE
);
simple_default!(default_max_volatility_24h, f64, 30.0);

// Price validation defaults
const_default!(
    default_max_price_discrepancy_threshold,
    f64,
    MAX_PRICE_DISCREPANCY_THRESHOLD
);
const_default!(
    default_max_execution_price_deviation,
    f64,
    DEFAULT_MAX_EXECUTION_PRICE_DEVIATION
);

// Gas protection defaults
const_default!(default_max_gas_cost_usd, f64, DEFAULT_MAX_GAS_COST_USD);
const_default!(
    default_max_gas_cost_percentage,
    f64,
    DEFAULT_MAX_GAS_COST_PCT
);
const_default!(
    default_min_trade_size_for_gas,
    f64,
    DEFAULT_MIN_TRADE_SIZE_FOR_GAS
);

// Transaction priority default
pub const fn default_transaction_priority() -> crate::infrastructure::dex::TransactionPriority {
    crate::infrastructure::dex::TransactionPriority::Standard
}

// Paper trading simulation defaults
const_default!(
    default_paper_simulated_weth_balance,
    f64,
    DEFAULT_SIMULATED_WETH_BALANCE
);

// Risk factor defaults
const_default!(
    default_min_portfolio_risk_factor,
    f64,
    DEFAULT_MIN_PORTFOLIO_RISK_FACTOR_THRESHOLD
);

// ============================================================================
// NUMERIC DEFAULTS - INDICATOR WEIGHTS
// ============================================================================

const_default!(default_rsi_weight, f64, DEFAULT_RSI_WEIGHT);
const_default!(default_macd_weight, f64, DEFAULT_MACD_WEIGHT);
const_default!(default_bollinger_weight, f64, DEFAULT_BOLLINGER_WEIGHT);
const_default!(default_volume_weight, f64, DEFAULT_VOLUME_WEIGHT);

pub fn default_indicator_profile() -> String {
    "day_trading".to_string()
}

// ============================================================================
// NUMERIC DEFAULTS - INTEGERS
// ============================================================================

const_default!(default_max_positions, usize, DEFAULT_MAX_POSITIONS);
const_default!(
    default_strategy_db_concurrency,
    usize,
    STRATEGY_DB_CONCURRENCY
);

// Database defaults
const_default!(default_db_port, u16, DEFAULT_PORT);
const_default!(default_db_pool_max_size, usize, DEFAULT_MAX_POOL_SIZE);

// API defaults
const_default!(default_timeout, u64, DEFAULT_TIMEOUT_SECS);
const_default!(default_retries, usize, DEFAULT_MAX_RETRIES);

// Data collection defaults
const_default!(
    default_collection_interval,
    u64,
    DEFAULT_COLLECTION_INTERVAL_SECS
);
const_default!(default_history_days, u64, DEFAULT_HISTORY_DAYS);

// ============================================================================
// COMPLEX DEFAULTS
// ============================================================================

pub fn default_db_password() -> Option<String> {
    None
}

pub fn default_tokens_to_track() -> Vec<String> {
    vec![]
}

/// Get the platform-specific default logs directory
pub fn default_logs_directory() -> String {
    if let Some(proj_dirs) = ProjectDirs::from("", "", "mantis") {
        if let Some(data_dir) = proj_dirs.data_dir().parent() {
            let logs_path = data_dir.join("mantis").join("logs");
            return logs_path.to_string_lossy().to_string();
        }
    }

    // Fallback to current directory if we can't determine the proper path
    "./logs".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boolean_defaults() {
        assert!(default_true());
        assert!(!default_false());
        assert_eq!(default_live_trading(), DEFAULT_LIVE_TRADING);
        assert!(default_enable_price_cross_check());
    }

    #[test]
    fn test_string_defaults() {
        assert_eq!(default_strategy(), DEFAULT_STRATEGY_TYPE);
        assert_eq!(default_db_host(), "localhost");
        assert_eq!(default_primary_rpc_provider(), "alchemy");
    }

    #[test]
    fn test_numeric_defaults() {
        assert_eq!(default_threshold(), DEFAULT_CONFIDENCE_THRESHOLD);
        assert_eq!(default_min_volume(), DEFAULT_MIN_VOLUME);
        assert_eq!(default_max_positions(), DEFAULT_MAX_POSITIONS);
        assert_eq!(default_db_port(), 5432);
    }

    #[test]
    fn test_indicator_weights_sum_to_one() {
        let total = default_rsi_weight()
            + default_macd_weight()
            + default_bollinger_weight()
            + default_volume_weight();
        assert!(
            (total - 1.0).abs() < f64::EPSILON,
            "Indicator weights should sum to 1.0, got {}",
            total
        );
    }

    #[test]
    fn test_complex_defaults() {
        assert_eq!(default_db_password(), None);
        assert_eq!(default_max_tokens_to_scan(), DEFAULT_MAX_TOKENS_TO_SCAN);
        assert_eq!(default_tokens_to_track(), Vec::<String>::new());
        assert!(!default_logs_directory().is_empty());
    }
}
