//! CLI configuration overrides for clean integration with config system

use crate::config::Config;
use log::{debug, error, info};
use serde::Deserialize;
use std::process;
use validator::Validate;

/// CLI overrides that map to config structure for clean merging
#[derive(Default, Deserialize, Validate)]
pub struct CliOverrides {
    // Trading config overrides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_trading: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(min = 0.01, message = "Max position size must be at least $0.01"))]
    pub max_position_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(min = 0.01, message = "Max total exposure must be at least $0.01"))]
    pub max_total_exposure: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "Confidence threshold must be between 0.0 and 1.0"
    ))]
    pub threshold: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(min = 0.0, message = "Minimum volume must be non-negative"))]
    pub min_volume: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(min = 0.0, message = "Minimum liquidity must be non-negative"))]
    pub min_liquidity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Stop loss percentage must be between 0.0 and 100.0"
    ))]
    pub stop_loss: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Take profit percentage must be between 0.0 and 100.0"
    ))]
    pub take_profit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(min = 1, message = "Maximum positions must be at least 1"))]
    pub max_positions: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens_to_scan: Option<usize>,

    // Data collection overrides
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(range(min = 1, message = "Collection interval must be at least 1 second"))]
    pub interval: Option<u64>,

    // API keys overrides (no validation needed - they're just strings)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alchemy_key: Option<String>,

    // Cache overrides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(url(message = "Redis URL must be a valid URL"))]
    pub redis_url: Option<String>,
}

/// Global CLI arguments that should be available across all commands
#[derive(clap::Args)]
pub struct GlobalArgs {
    /// Enable live trading mode (real money - paper trading is the default for safety)
    #[arg(long, global = true, action = clap::ArgAction::SetTrue)]
    pub live: bool,

    /// Market scan interval in seconds
    #[arg(long, global = true)]
    pub scan_interval: Option<u64>,

    /// Maximum position size in USD
    #[arg(long, global = true)]
    pub max_position: Option<f64>,

    /// Maximum total exposure in USD
    #[arg(long, global = true)]
    pub max_exposure: Option<f64>,

    /// Strategy type (momentum, rsi, macd, etc.)
    #[arg(long, global = true)]
    pub strategy: Option<String>,

    /// Confidence threshold for strategy signals (0.0-1.0)
    #[arg(long, global = true)]
    pub confidence_threshold: Option<f64>,

    /// Minimum volume required for trading
    #[arg(long, global = true)]
    pub min_volume: Option<f64>,

    /// Minimum liquidity required for trading pairs in USD
    #[arg(long, global = true)]
    pub min_liquidity: Option<f64>,

    /// Stop loss percentage
    #[arg(long, global = true)]
    pub stop_loss: Option<f64>,

    /// Take profit percentage
    #[arg(long, global = true)]
    pub take_profit: Option<f64>,

    /// Maximum number of positions
    #[arg(long, global = true)]
    pub max_positions: Option<usize>,

    /// Alchemy API key
    #[arg(long, global = true)]
    pub alchemy_key: Option<String>,

    /// Enable Redis cache
    #[arg(long, global = true, action = clap::ArgAction::SetTrue)]
    pub enable_cache: bool,

    /// Redis URL
    #[arg(long, global = true)]
    pub redis_url: Option<String>,

    /// Enable debug logging
    #[arg(short, long, global = true, action = clap::ArgAction::SetTrue)]
    pub debug: bool,

    /// Write logs to a file
    #[arg(long, global = true)]
    pub log_file: Option<String>,

    /// Set log level (error, warn, info, debug, trace)
    #[arg(long, global = true, value_parser = ["error", "warn", "info", "debug", "trace"])]
    pub log_level: Option<String>,

    /// Filter logs by module (comma-separated, e.g., "honeybadger::trading,honeybadger::api")
    #[arg(long, global = true)]
    pub log_modules: Option<String>,

    /// Maximum number of tokens to scan (0 = unlimited, recommended: 100-200)
    #[arg(long, global = true)]
    pub max_tokens_to_scan: Option<usize>,
}

impl From<&GlobalArgs> for CliOverrides {
    fn from(cli: &GlobalArgs) -> Self {
        Self {
            // Map CLI flags to config structure
            live_trading: if cli.live { Some(true) } else { None },
            max_position_size: cli.max_position,
            max_total_exposure: cli.max_exposure,
            strategy: cli.strategy.clone(),
            threshold: cli.confidence_threshold,
            min_volume: cli.min_volume,
            min_liquidity: cli.min_liquidity,
            stop_loss: cli.stop_loss,
            take_profit: cli.take_profit,
            max_positions: cli.max_positions,
            max_tokens_to_scan: cli.max_tokens_to_scan,

            interval: cli.scan_interval,

            alchemy_key: cli.alchemy_key.clone(),

            cache_enabled: if cli.enable_cache { Some(true) } else { None },
            redis_url: cli.redis_url.clone(),
        }
    }
}

/// Apply CLI configuration overrides using clean struct merging
pub fn apply_cli_config(config: &mut Config, cli: &GlobalArgs) {
    debug!("🔍 apply_cli_config: Original config values:");
    debug!("   - min_volume: {}", config.trading.min_volume);
    debug!(
        "   - max_position_size: {}",
        config.trading.max_position_size
    );
    debug!(
        "   - max_total_exposure: {}",
        config.trading.max_total_exposure
    );
    debug!("   - stop_loss: {}", config.trading.stop_loss);

    // Convert CLI to overrides struct
    let overrides = CliOverrides::from(cli);

    // Validate CLI overrides before applying them
    if let Err(validation_errors) = overrides.validate() {
        error!("CLI parameter validation failed:");
        for (field, errors) in validation_errors.field_errors() {
            for err in errors {
                if let Some(message) = &err.message {
                    error!("  --{}: {}", field.replace('_', "-"), message);
                } else {
                    error!("  --{}: Invalid value", field.replace('_', "-"));
                }
            }
        }
        process::exit(1);
    }

    // Apply overrides to config
    apply_overrides_to_config(config, &overrides);

    debug!("🔍 apply_cli_config: Final config values after CLI overrides:");
    debug!("   - min_volume: {}", config.trading.min_volume);
    debug!(
        "   - max_position_size: {}",
        config.trading.max_position_size
    );
    debug!(
        "   - max_total_exposure: {}",
        config.trading.max_total_exposure
    );
    debug!("   - stop_loss: {}", config.trading.stop_loss);
}

/// Helper function to apply overrides to config - much cleaner than repetitive if-lets
fn apply_overrides_to_config(config: &mut Config, overrides: &CliOverrides) {
    // Trading config
    if let Some(v) = overrides.live_trading {
        config.trading.live_trading = v;
    }
    if let Some(v) = overrides.max_position_size {
        debug!("🔍 CLI override: max_position_size = {}", v);
        config.trading.max_position_size = v;
    }
    if let Some(v) = overrides.max_total_exposure {
        debug!("🔍 CLI override: max_total_exposure = {}", v);
        config.trading.max_total_exposure = v;
    }
    if let Some(ref v) = overrides.strategy {
        config.trading.strategy = v.clone();
    }
    if let Some(v) = overrides.threshold {
        config.trading.signal_confidence_threshold = v;
    }
    if let Some(v) = overrides.min_volume {
        debug!(
            "🔍 CLI override: min_volume = {} (was {})",
            v, config.trading.min_volume
        );
        config.trading.min_volume = v;
    }
    if let Some(v) = overrides.min_liquidity {
        config.trading.min_liquidity = v;
    }
    if let Some(v) = overrides.stop_loss {
        config.trading.stop_loss = v;
    }
    if let Some(v) = overrides.take_profit {
        config.trading.take_profit = v;
    }
    if let Some(v) = overrides.max_positions {
        config.trading.max_positions = v;
    }
    if let Some(v) = overrides.max_tokens_to_scan {
        config.trading.max_tokens_to_scan = v;
        info!("CLI override: max_tokens_to_scan = {}", v);
    }

    // Data collection config
    if let Some(v) = overrides.interval {
        config.data_collection.scan_interval_secs = v;
    }

    // API keys
    if let Some(ref v) = overrides.alchemy_key {
        config.api_keys.alchemy = Some(v.clone());
    }

    // Cache config
    if let Some(v) = overrides.cache_enabled {
        config.cache.enabled = v;
    }
    if let Some(ref v) = overrides.redis_url {
        config.cache.redis_url = Some(v.clone());
    }
}
