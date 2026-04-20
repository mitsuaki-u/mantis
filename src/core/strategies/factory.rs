use super::custom::{MomentumStrategy, RsiStrategy};
use super::traits::Strategy;
use crate::core::errors::Error;
use crate::core::indicators::IndicatorWeights;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

/// Strategy configuration trait for type-safe factory pattern
pub trait StrategyConfig {
    /// Create the strategy instance from this configuration
    fn create_strategy(self) -> Result<Strategy, Error>;

    /// Get the strategy name for logging and identification
    fn strategy_name(&self) -> &'static str;
}

/// Configuration for Momentum trading strategy
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct MomentumConfig {
    /// Momentum entry threshold (0.0-100.0%)
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Momentum entry threshold must be between 0.0 and 100.0"
    ))]
    pub momentum_entry_threshold: f64,

    /// Minimum volume in USD (must be non-negative)
    #[validate(range(min = 0.0, message = "Minimum volume must be non-negative"))]
    pub min_volume: f64,

    /// Stop loss percentage (0.0-100.0%)
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Stop loss percentage must be between 0.0 and 100.0"
    ))]
    pub stop_loss_pct: f64,

    /// Indicator profile preset: optimizes indicator periods for your trading style
    /// Options: "scalping", "day_trading", "swing_trading", "standard"
    pub indicator_profile: Option<String>,

    /// RSI indicator weight (0.0-1.0)
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "RSI weight must be between 0.0 and 1.0"
    ))]
    pub rsi_weight: Option<f64>,

    /// MACD indicator weight (0.0-1.0)
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "MACD weight must be between 0.0 and 1.0"
    ))]
    pub macd_weight: Option<f64>,

    /// Bollinger Bands indicator weight (0.0-1.0)
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "Bollinger Bands weight must be between 0.0 and 1.0"
    ))]
    pub bollinger_weight: Option<f64>,

    /// Volume indicator weight (0.0-1.0)
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "Volume weight must be between 0.0 and 1.0"
    ))]
    pub volume_weight: Option<f64>,
}

impl MomentumConfig {
    /// Create a new momentum configuration with required parameters
    pub fn new(momentum_entry_threshold: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        Self {
            momentum_entry_threshold,
            min_volume,
            stop_loss_pct,
            indicator_profile: None,
            rsi_weight: None,
            macd_weight: None,
            bollinger_weight: None,
            volume_weight: None,
        }
    }
}

impl StrategyConfig for MomentumConfig {
    fn create_strategy(self) -> Result<Strategy, Error> {
        self.validate().map_err(|e| {
            Error::Config(format!("Invalid momentum strategy configuration: {}", e))
        })?;

        let mut strategy = MomentumStrategy::new(
            self.momentum_entry_threshold,
            self.min_volume,
            self.stop_loss_pct,
        );

        if let Some(ref profile_str) = self.indicator_profile {
            use crate::core::constants::IndicatorProfile;
            match IndicatorProfile::from_string(profile_str) {
                Some(profile) => {
                    strategy.indicator_profile = profile;
                    debug!(
                        "🔧 Set indicator profile: {} (warmup: {} min, periods: {:?})",
                        profile.as_str(),
                        profile.warmup_minutes(),
                        profile.periods()
                    );
                }
                None => {
                    return Err(Error::Config(format!(
                        "Invalid indicator profile: '{}'. Valid options: scalping, day_trading, swing_trading, standard",
                        profile_str
                    )));
                }
            }
        }

        if self.rsi_weight.is_some()
            || self.macd_weight.is_some()
            || self.bollinger_weight.is_some()
            || self.volume_weight.is_some()
        {
            let defaults = IndicatorWeights::default();
            let weights = IndicatorWeights::new(
                self.rsi_weight.unwrap_or(defaults.rsi),
                self.macd_weight.unwrap_or(defaults.macd),
                self.bollinger_weight.unwrap_or(defaults.bollinger_bands),
                self.volume_weight.unwrap_or(defaults.volume),
            );
            strategy.indicator_weights = weights;
            debug!(
                "🔧 Set indicator weights: RSI={:.2}, MACD={:.2}, Bollinger={:.2}, Volume={:.2}",
                weights.rsi, weights.macd, weights.bollinger_bands, weights.volume
            );
        }

        Ok(Strategy::Momentum(strategy))
    }

    fn strategy_name(&self) -> &'static str {
        "momentum"
    }
}

/// Configuration for RSI trading strategy  
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
#[validate(schema(function = "validate_rsi_thresholds", skip_on_field_errors = false))]
pub struct RsiConfig {
    /// RSI oversold threshold (0.0-100.0, typically 20-30)
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "RSI oversold threshold must be between 0.0 and 100.0"
    ))]
    pub oversold_threshold: f64,

    /// RSI overbought threshold (0.0-100.0, typically 70-80)
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "RSI overbought threshold must be between 0.0 and 100.0"
    ))]
    pub overbought_threshold: f64,

    /// Minimum volume in USD (must be non-negative)
    #[validate(range(min = 0.0, message = "Minimum volume must be non-negative"))]
    pub min_volume: f64,

    /// Stop loss percentage (0.0-100.0%)
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Stop loss percentage must be between 0.0 and 100.0"
    ))]
    pub stop_loss_pct: f64,
}

/// Validate that oversold threshold is less than overbought threshold
fn validate_rsi_thresholds(config: &RsiConfig) -> Result<(), ValidationError> {
    if config.oversold_threshold >= config.overbought_threshold {
        return Err(ValidationError::new(
            "RSI oversold threshold must be less than overbought threshold",
        ));
    }
    Ok(())
}

impl RsiConfig {
    /// Create a new RSI configuration with required parameters
    pub fn new(oversold: f64, overbought: f64, min_volume: f64, stop_loss_pct: f64) -> Self {
        Self {
            oversold_threshold: oversold,
            overbought_threshold: overbought,
            min_volume,
            stop_loss_pct,
        }
    }
}

impl StrategyConfig for RsiConfig {
    fn create_strategy(self) -> Result<Strategy, Error> {
        self.validate()
            .map_err(|e| Error::Config(format!("Invalid RSI strategy configuration: {}", e)))?;

        info!(
            "🏗️ Creating RSI strategy - Oversold: {:.1}, Overbought: {:.1}",
            self.oversold_threshold, self.overbought_threshold
        );

        let strategy = RsiStrategy::new(
            self.oversold_threshold,
            self.overbought_threshold,
            self.min_volume,
            self.stop_loss_pct,
        );

        Ok(Strategy::Rsi(strategy))
    }

    fn strategy_name(&self) -> &'static str {
        "rsi"
    }
}

/// Generic strategy factory using strategy-specific configurations
pub fn create_strategy<T: StrategyConfig>(config: T) -> Result<Strategy, Error> {
    debug!("Creating {} strategy", config.strategy_name());
    config.create_strategy()
}

/// Helper macro to easily register a new strategy implementation
#[macro_export]
macro_rules! register_strategy {
    ($strategy_name:expr, $strategy_type:ty, $momentum_entry_threshold:expr, $min_volume:expr, $stop_loss:expr) => {
        match $strategy_name {
            name if name == <$strategy_type>::strategy_name() => {
                let strategy =
                    <$strategy_type>::new($momentum_entry_threshold, $min_volume, $stop_loss);
                Ok(Strategy::Momentum(strategy)) // Note: This macro assumes momentum strategy
            }
            _ => continue, // Move to next match arm
        }
    };
}

/// Get a list of available strategy names
pub fn get_available_strategies() -> Vec<&'static str> {
    vec![
        MomentumStrategy::strategy_name(),
        RsiStrategy::strategy_name(),
    ]
}
