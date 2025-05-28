use super::core::Strategy;
use super::momentum::MomentumStrategy;
use super::rsi::RSIStrategy;
use crate::core::error::Error;
use crate::domain::trading::indicators::IndicatorWeights;
use log::{info, warn};

/// Helper macro to easily register a new strategy implementation
#[macro_export]
macro_rules! register_strategy {
    ($strategy_name:expr, $strategy_type:ty, $threshold:expr, $min_volume:expr, $stop_loss:expr) => {
        match $strategy_name {
            name if name == <$strategy_type>::strategy_name() => {
                let strategy = <$strategy_type>::new($threshold, $min_volume, $stop_loss);
                Ok(Strategy::new(Box::new(strategy)))
            }
            _ => continue, // Move to next match arm
        }
    };
}

/// Create a strategy instance based on the strategy name and parameters
pub fn create_strategy(
    strategy_name: &str,
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
    min_data_points: Option<usize>,
    risk_tolerance: Option<i32>,
    testing_mode: Option<String>,
) -> Result<Strategy, Error> {
    info!("🏭 Creating strategy: {}", strategy_name);
    info!(
        "📊 Parameters - Threshold: {:.2}%, Min Volume: ${:.2}M, Stop Loss: {:.2}%",
        threshold,
        min_volume / 1_000_000.0,
        stop_loss_pct
    );

    match strategy_name {
        "momentum" => {
            let mut strategy = MomentumStrategy::new(threshold, min_volume, stop_loss_pct);

            // Configure minimum data points if specified
            if let Some(points) = min_data_points {
                strategy = strategy.with_min_data_points(points);
                info!("📈 Set minimum data points: {}", points);
            }

            // Configure risk tolerance if specified
            if let Some(tolerance) = risk_tolerance {
                let risk_level = tolerance as f64 / 10.0; // Convert to 0.0-1.0 scale
                strategy = strategy.with_risk_tolerance(risk_level);
                info!("⚖️ Set risk tolerance: {:.1}", risk_level);
            }

            // Configure trading mode based on testing_mode
            if let Some(mode_str) = testing_mode {
                let trading_mode = match mode_str.as_str() {
                    "fast" => crate::domain::trading::indicators::TradingMode::FastTest,
                    "ultra_fast" => crate::domain::trading::indicators::TradingMode::UltraFast,
                    "mock" => crate::domain::trading::indicators::TradingMode::Mock,
                    "production" => crate::domain::trading::indicators::TradingMode::Production,
                    _ => {
                        warn!("⚠️ Unknown testing mode '{}', using Production", mode_str);
                        crate::domain::trading::indicators::TradingMode::Production
                    }
                };
                strategy = strategy.with_trading_mode(trading_mode);
                info!("🔧 Set trading mode: {:?}", trading_mode);
            }

            Ok(Strategy::new(Box::new(strategy)))
        }
        "rsi" => {
            let mut strategy = RSIStrategy::new(threshold, min_volume, stop_loss_pct);

            // Configure trading mode based on testing_mode
            if let Some(mode_str) = testing_mode {
                let trading_mode = match mode_str.as_str() {
                    "fast" => crate::domain::trading::indicators::TradingMode::FastTest,
                    "ultra_fast" => crate::domain::trading::indicators::TradingMode::UltraFast,
                    "mock" => crate::domain::trading::indicators::TradingMode::Mock,
                    "production" => crate::domain::trading::indicators::TradingMode::Production,
                    _ => {
                        warn!("⚠️ Unknown testing mode '{}', using Production", mode_str);
                        crate::domain::trading::indicators::TradingMode::Production
                    }
                };
                strategy = strategy.with_trading_mode(trading_mode);
                info!("🔧 Set RSI trading mode: {:?}", trading_mode);
            }

            Ok(Strategy::new(Box::new(strategy)))
        }
        _ => {
            let error_msg = format!("Unknown strategy: {}", strategy_name);
            Err(Error::Config(error_msg))
        }
    }
}

/// Create a momentum strategy with custom indicator weights
pub fn create_momentum_strategy_with_weights(
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
    weights: IndicatorWeights,
) -> Result<Strategy, Error> {
    info!("🏭 Creating momentum strategy with custom weights");
    info!(
        "⚖️ Weights - RSI: {:.2}, MACD: {:.2}, Bollinger: {:.2}, Volume: {:.2}",
        weights.rsi, weights.macd, weights.bollinger_bands, weights.volume
    );

    let strategy =
        MomentumStrategy::new(threshold, min_volume, stop_loss_pct).with_indicator_weights(weights);

    Ok(Strategy::new(Box::new(strategy)))
}

/// Get a list of available strategy names
pub fn get_available_strategies() -> Vec<&'static str> {
    vec![
        MomentumStrategy::strategy_name(),
        RSIStrategy::strategy_name(),
    ]
}

/// Validate strategy parameters
pub fn validate_strategy_params(
    strategy_name: &str,
    threshold: f64,
    min_volume: f64,
    stop_loss_pct: f64,
) -> Result<(), Error> {
    // Check if strategy exists
    let available = get_available_strategies();
    if !available.contains(&strategy_name) {
        return Err(Error::Config(format!(
            "Unknown strategy '{}'. Available strategies: {:?}",
            strategy_name, available
        )));
    }

    // Validate threshold
    if threshold <= 0.0 || threshold > 100.0 {
        return Err(Error::Config(format!(
            "Invalid threshold: {:.2}%. Must be between 0.0 and 100.0",
            threshold
        )));
    }

    // Validate minimum volume
    if min_volume < 0.0 {
        return Err(Error::Config(format!(
            "Invalid minimum volume: ${:.2}. Must be non-negative",
            min_volume
        )));
    }

    // Validate stop loss
    if stop_loss_pct <= 0.0 || stop_loss_pct > 100.0 {
        return Err(Error::Config(format!(
            "Invalid stop loss: {:.2}%. Must be between 0.0 and 100.0",
            stop_loss_pct
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_momentum_strategy() {
        let strategy = create_strategy("momentum", 5.0, 1_000_000.0, 10.0, None, None, None);
        assert!(strategy.is_ok());
        assert_eq!(strategy.unwrap().name(), "momentum_strategy");
    }

    #[test]
    fn test_create_rsi_strategy() {
        let strategy = create_strategy("rsi", 30.0, 100_000.0, 5.0, None, None, None);
        assert!(strategy.is_ok());
        assert_eq!(strategy.unwrap().name(), "rsi_strategy");
    }

    #[test]
    fn test_unknown_strategy() {
        let strategy = create_strategy("unknown", 5.0, 1_000_000.0, 10.0, None, None, None);
        assert!(strategy.is_err());
    }

    #[test]
    fn test_validate_params() {
        // Valid params
        assert!(validate_strategy_params("momentum", 5.0, 1_000_000.0, 10.0).is_ok());

        // Invalid threshold
        assert!(validate_strategy_params("momentum", 0.0, 1_000_000.0, 10.0).is_err());
        assert!(validate_strategy_params("momentum", 101.0, 1_000_000.0, 10.0).is_err());

        // Invalid volume
        assert!(validate_strategy_params("momentum", 5.0, -1.0, 10.0).is_err());

        // Invalid stop loss
        assert!(validate_strategy_params("momentum", 5.0, 1_000_000.0, 0.0).is_err());
        assert!(validate_strategy_params("momentum", 5.0, 1_000_000.0, 101.0).is_err());

        // Unknown strategy
        assert!(validate_strategy_params("unknown", 5.0, 1_000_000.0, 10.0).is_err());
    }
}
