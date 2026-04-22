//! Configuration management for the Honeybadger application.
//!
//! This module provides a modular configuration system with the following components:
//! - `defaults`: Default values and constants
//! - `env`: Environment variable loading
//! - `networks`: Network-specific configuration
//! - `api_keys`: API key management
//! - `validation`: Custom validation functions
//! - `io`: File I/O operations

use crate::error::Result;
use serde::{Deserialize, Serialize};
use validator::Validate;

// Import our modular components
pub mod api_keys;
pub mod defaults;
pub mod env;
pub mod io;
pub mod networks;
pub mod validation;

// Re-export commonly used items
pub use api_keys::ApiKeys;
pub use defaults::*;
pub use env::{EnvLoader, FromEnv};
pub use io::{ConfigFileInfo, ConfigIO, APP_NAME, CONFIG_FILENAME};
pub use networks::NetworkConfig;

/// Main configuration structure for the application
#[derive(Debug, Clone, Serialize, Deserialize, Validate, Default)]
pub struct Config {
    /// API keys for various services
    #[validate]
    pub api_keys: ApiKeys,

    /// Trading bot configuration
    #[validate]
    pub trading: TradingConfig,

    /// Database configuration
    #[validate]
    pub database: DatabaseConfig,

    /// API endpoints and configuration
    #[serde(default)]
    #[validate]
    pub api: ApiConfig,

    /// Data collection configuration
    #[validate]
    pub data_collection: DataCollectionConfig,

    /// Logs configuration
    #[validate]
    pub logs: LogsConfig,

    /// RPC provider configuration
    #[serde(default)]
    #[validate]
    pub rpc: RpcConfig,

    /// DEX (Decentralized Exchange) configuration
    #[serde(default)]
    #[validate]
    pub dex: DexConfig,

    /// Cache configuration
    #[serde(default)]
    #[validate]
    pub cache: CacheConfig,

    /// Solana configuration
    #[serde(default)]
    pub solana: SolanaConfig,

    /// Anthropic API key for AI advisor
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
}

/// Trading bot configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct TradingConfig {
    /// Set to true for live trading (real money, not simulated)
    #[serde(default = "default_live_trading")]
    pub live_trading: bool,

    /// Max position size in USD (must be positive)
    #[serde(default = "default_max_position_size")]
    #[validate(range(min = 0.01, message = "Max position size must be at least $0.01"))]
    pub max_position_size: f64,

    /// Min position size in USD (must be positive)
    #[serde(default = "default_min_position_size")]
    #[validate(range(min = 0.01, message = "Min position size must be at least $0.01"))]
    pub min_position_size: f64,

    /// Max total exposure in USD (must be positive)
    #[serde(default = "default_max_total_exposure")]
    #[validate(range(min = 0.01, message = "Max total exposure must be at least $0.01"))]
    pub max_total_exposure: f64,

    /// Strategy type (momentum, rsi, macd, etc.)
    #[serde(default = "default_strategy")]
    pub strategy: String,

    /// Minimum confidence threshold for strategy signals (0.0-1.0)
    #[serde(default = "default_threshold")]
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "Signal confidence threshold must be between 0.0 and 1.0"
    ))]
    pub signal_confidence_threshold: f64,

    /// Minimum volume required for trading in USD (must be non-negative)
    #[serde(default = "default_min_volume")]
    #[validate(range(min = 0.0, message = "Minimum volume must be non-negative"))]
    pub min_volume: f64,

    /// Minimum liquidity required for trading pairs in USD (must be non-negative)
    #[serde(default = "default_min_liquidity")]
    #[validate(range(min = 0.0, message = "Minimum liquidity must be non-negative"))]
    pub min_liquidity: f64,

    /// Minimum transaction count required for V3 pools
    #[serde(default = "default_min_pool_transaction_count")]
    pub min_pool_transaction_count: u32,

    /// Stop loss percentage (0.0-100.0%)
    #[serde(default = "default_stop_loss")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Stop loss percentage must be between 0.0 and 100.0"
    ))]
    pub stop_loss: f64,

    /// Take profit percentage (0.0-100.0%)
    #[serde(default = "default_take_profit")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Take profit percentage must be between 0.0 and 100.0"
    ))]
    pub take_profit: f64,

    /// Maximum number of positions (must be at least 1)
    #[serde(default = "default_max_positions")]
    #[validate(range(min = 1, message = "Maximum positions must be at least 1"))]
    pub max_positions: usize,

    /// Maximum allowed 24-hour volatility percentage (0.0-100.0%)
    /// Tokens with price changes exceeding this threshold will be skipped
    /// Set to 100.0 to effectively disable volatility checks
    #[serde(default = "default_max_volatility_24h")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Max volatility must be between 0.0 and 100.0 percent"
    ))]
    pub max_volatility_24h: f64,

    /// Default RSI weight for momentum strategy (0.0-1.0)
    #[serde(default = "default_rsi_weight")]
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "RSI weight must be between 0.0 and 1.0"
    ))]
    pub rsi_weight: f64,

    /// Default MACD weight for momentum strategy (0.0-1.0)
    #[serde(default = "default_macd_weight")]
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "MACD weight must be between 0.0 and 1.0"
    ))]
    pub macd_weight: f64,

    /// Default Bollinger Bands weight for momentum strategy (0.0-1.0)
    #[serde(default = "default_bollinger_weight")]
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "Bollinger Bands weight must be between 0.0 and 1.0"
    ))]
    pub bollinger_weight: f64,

    /// Default Volume weight for momentum strategy (0.0-1.0)
    #[serde(default = "default_volume_weight")]
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "Volume weight must be between 0.0 and 1.0"
    ))]
    pub volume_weight: f64,

    /// Indicator profile preset: optimizes indicator periods for your scan interval
    /// Options: "scalping", "day_trading", "swing_trading", "standard"
    /// Recommended: "day_trading" for 60s intervals
    #[serde(default = "default_indicator_profile")]
    pub indicator_profile: String,

    /// Maximum number of tokens to scan for trading opportunities (0 = unlimited, recommended: 100-200)
    #[serde(default = "default_max_tokens_to_scan")]
    pub max_tokens_to_scan: usize,

    /// Maximum daily loss percentage (0.0-100.0%)
    #[serde(default = "default_max_daily_loss_config")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Max daily loss must be between 0.0 and 100.0 percent"
    ))]
    pub max_daily_loss: f64,

    /// Maximum drawdown percentage (0.0-100.0%)
    #[serde(default = "default_max_drawdown_config")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Max drawdown must be between 0.0 and 100.0 percent"
    ))]
    pub max_drawdown: f64,

    /// Maximum single trade risk as percentage of wallet (0.0-100.0%)
    #[serde(default = "default_max_single_trade_risk_percentage_of_wallet")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Max trade risk percentage must be between 0.0 and 100.0"
    ))]
    pub max_trade_risk_pct: f64,

    /// Minimum required ETH balance for trading (must be positive)
    #[serde(default = "default_min_required_eth_balance_for_trading", alias = "min_eth_balance")]
    #[validate(range(min = 0.0, message = "Minimum native token balance must be non-negative"))]
    pub min_native_balance: f64,

    /// List of specific tokens to track (empty means use defaults)
    #[serde(default = "default_tokens_to_track")]
    pub tokens_to_track: Vec<String>,

    #[serde(default = "default_market_data_provider")]
    pub market_data_provider: String,

    // === GAS PROTECTION SETTINGS ===
    /// Maximum USD amount allowed to spend on gas per transaction
    #[serde(default = "default_max_gas_cost_usd")]
    #[validate(range(min = 0.0, message = "Max gas cost USD must be non-negative"))]
    pub max_gas_cost_usd: f64,

    /// Maximum percentage of trade size that can be spent on gas
    #[serde(default = "default_max_gas_cost_percentage")]
    #[validate(range(
        min = 0.0,
        max = 100.0,
        message = "Max gas cost percentage must be between 0.0 and 100.0"
    ))]
    pub max_gas_cost_percentage: f64,

    /// Transaction priority for gas pricing (Low, Medium, Standard, High, Urgent)
    /// Priority determines gas price multiplier: Low=0.9x, Standard=1.0x, High=1.2x, Urgent=1.5x
    #[serde(default = "default_transaction_priority")]
    pub transaction_priority: crate::infrastructure::dex::TransactionPriority,

    // === PRICE VALIDATION SETTINGS ===
    /// Maximum allowed price deviation from signal to execution (0.0-1.0, default: 0.05 = 5%)
    /// If execution price differs from signal price by more than this percentage, trade is rejected
    #[serde(default = "default_max_execution_price_deviation")]
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "Max execution price deviation must be between 0.0 and 1.0"
    ))]
    pub max_execution_price_deviation: f64,

    /// Minimum portfolio risk factor before halting new trades (0.0-1.0, default: 0.3)
    /// When losses reduce the risk factor below this threshold, no new trades are opened
    /// Risk factor of 1.0 = no losses, 0.5 = moderate losses, 0.3 = significant losses
    #[serde(default = "default_min_portfolio_risk_factor")]
    #[validate(range(
        min = 0.0,
        max = 1.0,
        message = "Min portfolio risk factor must be between 0.0 and 1.0"
    ))]
    pub min_portfolio_risk_factor: f64,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct DatabaseConfig {
    /// PostgreSQL database host
    #[serde(default = "default_db_host")]
    pub host: String,

    /// PostgreSQL database port (1-65535)
    #[serde(default = "default_db_port")]
    #[validate(range(
        min = 1,
        max = 65535,
        message = "Database port must be between 1 and 65535"
    ))]
    pub port: u16,

    /// PostgreSQL database user (cannot be empty)
    #[serde(default = "default_db_user")]
    #[validate(length(min = 1, message = "Database user cannot be empty"))]
    pub user: String,

    /// PostgreSQL database password
    #[serde(default = "default_db_password")]
    pub password: Option<String>,

    /// PostgreSQL database name (cannot be empty)
    #[serde(default = "default_db_name")]
    #[validate(length(min = 1, message = "Database name cannot be empty"))]
    pub dbname: String,

    /// Max connection pool size (must be at least 1)
    #[serde(default = "default_db_pool_max_size")]
    #[validate(range(min = 1, message = "Database pool max size must be at least 1"))]
    pub pool_max_size: usize,
}

/// API endpoints and configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate, Default)]
pub struct ApiConfig {
    // Reserved for future API-specific settings
}

/// Data collection configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct DataCollectionConfig {
    /// Market data collection/scan interval in seconds (must be at least 1)
    #[serde(default = "default_collection_interval")]
    #[validate(range(min = 1, message = "Scan interval must be at least 1 second"))]
    pub scan_interval_secs: u64,

    /// Number of days of historical data to collect (reasonable upper limit)
    #[serde(default = "default_history_days")]
    #[validate(range(min = 1, max = 365, message = "History days must be between 1 and 365"))]
    pub history_days: u64,

    /// Auto-start data collection
    #[serde(default = "default_true")]
    pub auto_start: bool,
}

/// Logs configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct LogsConfig {
    /// Directory for log files (cannot be empty)
    #[serde(default = "default_logs_directory")]
    #[validate(length(min = 1, message = "Logs directory cannot be empty"))]
    pub directory: String,
}

/// DEX (Decentralized Exchange) configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct DexConfig {
    /// Blockchain network
    #[serde(default)]
    pub network: Option<String>,

    /// Protocol for token resolution
    #[serde(default = "default_protocol_v3")]
    pub protocol: String,

    /// Optional custom RPC URL
    #[serde(default)]
    pub custom_rpc_url: Option<String>,

    /// Optional router contract address
    #[serde(default)]
    pub router_address: Option<String>,

    /// Optional WETH contract address
    #[serde(default)]
    pub weth_address: Option<String>,

    /// Optional stablecoin contract address
    #[serde(default)]
    pub stablecoin_address: Option<String>,

    pub wallet: Option<WalletConfig>,

    #[serde(default = "default_paper_simulated_weth_balance")]
    pub paper_simulated_weth_balance: f64,

}

/// Solana network configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SolanaConfig {
    /// Helius or other Solana RPC URL
    pub rpc_url: Option<String>,
    /// Network: "mainnet", "devnet"
    #[serde(default = "default_solana_network")]
    pub network: String,
    /// Path to keypair JSON file for live trading
    pub keypair_path: Option<String>,
}

fn default_solana_network() -> String {
    "mainnet".to_string()
}

/// Wallet configuration
#[derive(PartialEq, Debug, Clone, Serialize, Deserialize, Validate)]
pub struct WalletConfig {
    pub private_key_env: Option<String>,
    pub private_key_file: Option<String>,
}

impl WalletConfig {
    /// Load a wallet from the configuration
    pub fn load_wallet(&self) -> crate::error::Result<ethers::signers::LocalWallet> {
        use crate::error::Error;

        // Try to load from environment variable first
        if let Some(env_var) = &self.private_key_env {
            if let Ok(private_key) = std::env::var(env_var) {
                return private_key
                    .parse::<ethers::signers::LocalWallet>()
                    .map_err(|e| {
                        Error::Wallet(format!("Invalid private key from env {}: {}", env_var, e))
                    });
            }
        }

        // Try to load from file
        if let Some(file_path) = &self.private_key_file {
            match std::fs::read_to_string(file_path) {
                Ok(private_key) => {
                    let private_key = private_key.trim();
                    return private_key
                        .parse::<ethers::signers::LocalWallet>()
                        .map_err(|e| {
                            Error::Wallet(format!(
                                "Invalid private key from file {}: {}",
                                file_path, e
                            ))
                        });
                }
                Err(e) => {
                    return Err(Error::Wallet(format!(
                        "Failed to read private key file {}: {}",
                        file_path, e
                    )))
                }
            }
        }

        Err(Error::Wallet(
            "No private key configured in wallet config".to_string(),
        ))
    }
}

/// RPC provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct RpcConfig {
    /// Primary RPC provider to use
    #[serde(default = "default_primary_rpc_provider")]
    pub primary_provider: String,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct CacheConfig {
    /// Whether cache is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Redis connection URL
    #[serde(default)]
    pub redis_url: Option<String>,
}

// ============================================================================
// DEFAULT IMPLEMENTATIONS
// ============================================================================

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            live_trading: default_live_trading(),
            max_position_size: default_max_position_size(),
            min_position_size: default_min_position_size(),
            max_total_exposure: default_max_total_exposure(),
            strategy: default_strategy(),
            signal_confidence_threshold: default_threshold(),
            min_volume: default_min_volume(),
            stop_loss: default_stop_loss(),
            take_profit: default_take_profit(),
            max_positions: default_max_positions(),
            max_volatility_24h: default_max_volatility_24h(),
            rsi_weight: default_rsi_weight(),
            macd_weight: default_macd_weight(),
            bollinger_weight: default_bollinger_weight(),
            volume_weight: default_volume_weight(),
            indicator_profile: default_indicator_profile(),
            max_tokens_to_scan: default_max_tokens_to_scan(),
            max_daily_loss: default_max_daily_loss_config(),
            max_drawdown: default_max_drawdown_config(),
            max_trade_risk_pct: default_max_single_trade_risk_percentage_of_wallet(),
            min_native_balance: default_min_required_eth_balance_for_trading(),
            tokens_to_track: default_tokens_to_track(),
            market_data_provider: default_market_data_provider(),
            min_liquidity: default_min_liquidity(),
            min_pool_transaction_count: default_min_pool_transaction_count(),
            max_gas_cost_usd: default_max_gas_cost_usd(),
            max_gas_cost_percentage: default_max_gas_cost_percentage(),
            transaction_priority: default_transaction_priority(),
            max_execution_price_deviation: default_max_execution_price_deviation(),
            min_portfolio_risk_factor: default_min_portfolio_risk_factor(),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            host: default_db_host(),
            port: default_db_port(),
            user: default_db_user(),
            password: default_db_password(),
            dbname: default_db_name(),
            pool_max_size: default_db_pool_max_size(),
        }
    }
}

impl Default for DataCollectionConfig {
    fn default() -> Self {
        Self {
            scan_interval_secs: default_collection_interval(),
            history_days: default_history_days(),
            auto_start: default_true(),
        }
    }
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            directory: default_logs_directory(),
        }
    }
}

impl Default for DexConfig {
    fn default() -> Self {
        Self {
            network: Some("solana".to_string()),
            protocol: "jupiter".to_string(),
            custom_rpc_url: None,
            router_address: None,
            weth_address: None,
            stablecoin_address: None,
            wallet: None,
            paper_simulated_weth_balance: default_paper_simulated_weth_balance(),
        }
    }
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            primary_provider: default_primary_rpc_provider(),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            redis_url: Some("redis://127.0.0.1/".to_string()),
        }
    }
}

// ============================================================================
// MAIN CONFIG IMPLEMENTATION
// ============================================================================

impl Config {
    /// Initialize optional fields with default values if not set
    pub fn initialize_optional_fields(&mut self) {
        // Initialize logs configuration
        if self.logs.directory.is_empty() {
            self.logs.directory = default_logs_directory();
        }

        // Make sure the logs directory exists
        let logs_dir = std::path::Path::new(&self.logs.directory);
        if !logs_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(logs_dir) {
                eprintln!("Warning: Failed to create logs directory: {}", e);
            }
        }

        // Initialize any required optional fields here
    }

    /// Load configuration from multiple sources with priority:
    /// 1. Command line arguments (highest priority)
    /// 2. Environment variables
    /// 3. Config file
    /// 4. Default values (lowest priority)
    pub fn load() -> Result<Self> {
        let config_path = ConfigIO::get_config_path()?;
        let mut config: Config = ConfigIO::load_from_file(&config_path)?;

        config.initialize_optional_fields();

        // Load API keys from environment variables (override config file values)
        config.api_keys.load_from_env();

        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = ConfigIO::get_config_path()?;
        ConfigIO::save_to_file(self, &config_path)
    }

    /// Set an API key for a specific service
    pub fn set_api_key(&mut self, service: &str, key: String) -> Result<()> {
        self.api_keys.set_api_key(service, key)?;
        self.save()
    }

    /// Check if paper trading is enabled (inverse of live trading)
    pub fn is_paper_trading(&self) -> bool {
        !self.trading.live_trading
    }

    /// Get the Redis URL from configuration
    pub fn get_redis_url(&self) -> Option<String> {
        EnvLoader::get_redis_url(self.cache.redis_url.as_ref(), self.cache.enabled)
    }
}

// ============================================================================
// DEX CONFIG IMPLEMENTATION - Using NetworkConfig
// ============================================================================

impl DexConfig {
    /// Get recommended tokens for the current network
    pub fn get_recommended_tokens(&self) -> Vec<String> {
        NetworkConfig::get_recommended_tokens(self.network.as_ref())
    }

    pub fn stablecoin_address(&self) -> String {
        NetworkConfig::stablecoin_address(self.network.as_ref(), self.stablecoin_address.as_ref())
    }

    /// Get stablecoin address from authoritative sources (async version)
    pub async fn stablecoin_address_from_source(&self) -> Result<String> {
        NetworkConfig::stablecoin_address_from_source(
            self.network.as_ref(),
            self.stablecoin_address.as_ref(),
        )
        .await
    }

    /// Get base token address for trading (WETH on Ethereum, SOL on Solana)
    pub fn base_token_address(&self) -> String {
        match self.network.as_deref() {
            Some("solana") => "So11111111111111111111111111111111111111112".to_string(),
            _ => NetworkConfig::weth_address(self.network.as_ref(), self.weth_address.as_ref()),
        }
    }

    /// Get base token symbol (SOL on Solana, WETH on Ethereum)
    pub fn base_token_symbol(&self) -> &'static str {
        match self.network.as_deref() {
            Some("solana") => "SOL",
            Some("polygon") | Some("matic") => "WMATIC",
            _ => "WETH",
        }
    }
}

// ============================================================================
// PUBLIC FUNCTIONS (Legacy compatibility)
// ============================================================================

/// Returns the platform-specific path to the configuration file
pub fn get_config_path() -> Result<std::path::PathBuf> {
    ConfigIO::get_config_path()
}

// ============================================================================
// STRATEGY PARAMS CONVERSION (Clean Architecture Pattern)
// ============================================================================

/// Convert Config to StrategyParams
///
/// This implements the clean architecture pattern where:
/// - CLI layer loads and owns Config (from files/env)
/// - CLI layer converts Config → StrategyParams
/// - Core layer only knows about StrategyParams (no I/O dependencies)
/// - Application layer receives and distributes StrategyParams
impl From<&Config> for crate::core::domain::StrategyParams {
    fn from(config: &Config) -> Self {
        use crate::core::constants::*;

        crate::core::domain::StrategyParams {
            // Position Sizing
            min_position_size: config.trading.min_position_size,
            max_position_size: config.trading.max_position_size,
            max_total_exposure: config.trading.max_total_exposure,
            max_positions: config.trading.max_positions,

            // Risk Management
            max_volatility_24h: config.trading.max_volatility_24h,
            stop_loss_pct: config.trading.stop_loss,
            take_profit_pct: config.trading.take_profit,
            max_daily_loss_pct: config.trading.max_daily_loss, // Field is named 'max_daily_loss'
            max_drawdown_pct: config.trading.max_drawdown,     // Field is named 'max_drawdown'
            max_single_trade_risk_pct: DEFAULT_MAX_TRADE_RISK_PCT, // Not in config, use constant

            // Execution & Validation
            max_execution_price_deviation: config.trading.max_execution_price_deviation,
            enable_price_cross_check: ENABLE_PRICE_CROSS_CHECK, // Not in config, use constant
            max_price_discrepancy_threshold: MAX_PRICE_DISCREPANCY_THRESHOLD, // Not in config, use constant
            min_token_age_months: 1, // Not in config, use default

            // Market Filters
            min_volume: config.trading.min_volume,
            min_liquidity: config.trading.min_liquidity,
            signal_confidence_threshold: config.trading.signal_confidence_threshold,
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_has_sensible_defaults() {
        // Test that config defaults are valid and make sense for trading
        let config = Config::default();

        // Trading defaults should be reasonable
        assert_eq!(config.trading.strategy, "momentum");
        assert!(config.trading.max_position_size > 0.0);
        assert!(config.trading.max_total_exposure > config.trading.max_position_size);
        assert!(config.trading.stop_loss > 0.0 && config.trading.stop_loss < 100.0);
        assert!(config.trading.take_profit > 0.0);
        assert!(
            config.trading.max_volatility_24h >= 0.0 && config.trading.max_volatility_24h <= 100.0
        );

        // Database defaults should be reasonable
        assert_eq!(config.database.host, "localhost");
        assert_eq!(config.database.port, 5432);
        assert!(config.database.pool_max_size > 0);

        // API config exists (reserved for future settings)

        // Data collection defaults should be reasonable
        assert!(config.data_collection.scan_interval_secs > 0);

        // Cache defaults should be reasonable
        assert!(!config.cache.enabled); // Disabled by default
    }

    #[test]
    fn test_config_load_and_save() {
        let config = Config::default();

        // Test that we can create a config and it has reasonable defaults
        assert!(!config.api_keys.has_any_keys());
        assert_eq!(config.trading.strategy, "momentum");
        assert_eq!(config.database.host, "localhost");

        // Test the modular functionality
        assert!(!config.dex.get_recommended_tokens().is_empty());
    }

    #[test]
    fn test_api_keys_integration() {
        let mut config = Config::default();

        // Test API key setting and validation
        config.api_keys.infura = Some("test_key_1234567890".to_string());
        assert!(config.api_keys.validate_keys().is_ok());

        // Test the modular API key functionality
        assert_eq!(
            config.api_keys.get_api_key("infura"),
            Some(&"test_key_1234567890".to_string())
        );
        assert!(config.api_keys.has_any_keys()); // Should have keys now

        // Test invalid service
        let result = config
            .api_keys
            .set_api_key("invalid_service", "key".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_network_config_integration() {
        let config = Config::default();

        // Test that DexConfig properly uses NetworkConfig
        let tokens = config.dex.get_recommended_tokens();
        assert!(tokens.contains(&"ethereum".to_string()));
        assert!(tokens.contains(&"bitcoin".to_string()));
    }
}
