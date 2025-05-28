use crate::core::error::{Error, Result};
use crate::domain::trading::indicators::TradingMode;
use directories::ProjectDirs;
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// Constants for configuration
const APP_NAME: &str = "honeybadger";
const CONFIG_FILENAME: &str = "config.json";

/// Main configuration structure for the application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// API keys for various services
    pub api_keys: ApiKeys,

    /// Trading bot configuration
    pub trading: TradingConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// API endpoints and configuration
    pub api: ApiConfig,

    /// Data collection configuration
    pub data_collection: DataCollectionConfig,

    /// Logs configuration
    pub logs: LogsConfig,

    /// DEX (Decentralized Exchange) configuration
    #[serde(default)]
    pub dex: DexConfig,

    /// Cache configuration
    #[serde(default)]
    pub cache: CacheConfig,
}

/// API keys for various services
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeys {
    pub cryptocompare: Option<String>,
    pub coingecko: Option<String>,
    pub etherscan: Option<String>,
    pub coincap: Option<String>,
    pub infura: Option<String>,
}

/// Trading bot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    /// Set to true for paper trading (simulated, not real money)
    #[serde(default = "default_paper_trading")]
    pub paper_trading: bool,

    /// Max position size in USD
    #[serde(default = "default_max_position_size")]
    pub max_position_size: f64,

    /// Max total exposure in USD
    #[serde(default = "default_max_total_exposure")]
    pub max_total_exposure: f64,

    /// Strategy type (momentum, rsi, macd, etc.)
    #[serde(default = "default_strategy")]
    pub strategy: String,

    /// Confidence threshold for strategy signals (0.0-1.0)
    /// Higher values generate fewer signals. Used as confidence_threshold in StrategyActor.
    #[serde(default = "default_threshold")]
    pub threshold: f64,

    /// Minimum volume required for trading in USD
    #[serde(default = "default_min_volume")]
    pub min_volume: f64,

    /// Stop loss percentage
    #[serde(default = "default_stop_loss")]
    pub stop_loss: f64,

    /// Take profit percentage
    #[serde(default = "default_take_profit")]
    pub take_profit: f64,

    /// Maximum number of positions
    #[serde(default = "default_max_positions")]
    pub max_positions: usize,

    /// Risk tolerance level (0-5)
    /// 0=Conservative, 1=Conservative-Moderate, 2=Moderate,
    /// 3=Moderate-Aggressive, 4=Aggressive, 5=Very Aggressive
    #[serde(default = "default_risk_tolerance")]
    pub risk_tolerance: u8,

    /// Process all available tokens, not just those in the tracking list
    #[serde(default = "default_wide_scan_mode")]
    pub wide_scan_mode: bool,

    /// Trading mode for testing
    #[serde(default = "default_trading_mode")]
    pub trading_mode: TradingMode,

    #[serde(default = "default_max_daily_loss_config")]
    pub max_daily_loss: f64,

    #[serde(default = "default_max_drawdown_config")]
    pub max_drawdown: f64,

    #[serde(default = "default_max_single_trade_risk_percentage_of_wallet")]
    pub max_single_trade_risk_percentage_of_wallet: f64,

    #[serde(default = "default_min_required_eth_balance_for_trading")]
    pub min_required_eth_balance_for_trading: f64,
}

/// Strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Strategy type (momentum, rsi, macd, etc.)
    #[serde(default = "default_strategy")]
    pub strategy_type: String,

    /// Confidence threshold for strategy signals (0.0-1.0)
    /// Higher values generate fewer signals. Same as confidence_threshold in StrategyActor.
    #[serde(default = "default_threshold")]
    pub threshold: f64,

    /// Minimum volume required for trading
    #[serde(default = "default_min_volume")]
    pub min_volume: f64,
}

/// Risk management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// Stop loss percentage
    #[serde(default = "default_stop_loss")]
    pub stop_loss_pct: f64,

    /// Take profit percentage
    #[serde(default = "default_take_profit")]
    pub take_profit_pct: f64,

    /// Maximum positions
    #[serde(default = "default_max_positions")]
    pub max_positions: usize,
}

/// Database configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// PostgreSQL database host
    #[serde(default = "default_db_host")]
    pub host: String,

    /// PostgreSQL database port
    #[serde(default = "default_db_port")]
    pub port: u16,

    /// PostgreSQL database user
    #[serde(default = "default_db_user")]
    pub user: String,

    /// PostgreSQL database password (consider env var)
    #[serde(default = "default_db_password")]
    pub password: Option<String>,

    /// PostgreSQL database name
    #[serde(default = "default_db_name")]
    pub dbname: String,

    /// Max connection pool size (deadpool)
    #[serde(default = "default_db_pool_max_size")]
    pub pool_max_size: usize,

    /// Enable database query logging
    #[serde(default)]
    pub query_logging: bool,

    /// Batch size for database operations
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// Batch interval in seconds for database operations
    #[serde(default = "default_batch_interval_secs")]
    pub batch_interval_secs: u64,

    /// Maintenance interval in hours for database operations
    #[serde(default = "default_maintenance_interval_hours")]
    pub maintenance_interval_hours: u64,
}

/// API configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Base URL for CoinGecko API
    #[serde(default = "default_coingecko_url")]
    pub coingecko_url: String,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub request_timeout: u64,

    /// Retry attempts for API requests
    #[serde(default = "default_retries")]
    pub max_retries: usize,
}

/// Data collection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataCollectionConfig {
    /// Interval in seconds for data collection
    #[serde(default = "default_collection_interval")]
    pub interval: u64,

    /// Maximum history to maintain (in days)
    #[serde(default = "default_history_days")]
    pub history_days: u64,

    /// Whether to auto-start data collection
    #[serde(default = "default_true")]
    pub auto_start: bool,

    /// Whether to use WebSockets for data collection if available
    #[serde(default = "default_false")]
    pub use_websockets: bool,
}

/// Logs configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsConfig {
    /// Default directory to store log files
    pub directory: String,
}

/// DEX (Decentralized Exchange) configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexConfig {
    /// Default DEX name (uniswap, sushiswap, etc.)
    #[serde(default = "default_dex_name")]
    pub name: String,

    /// DEX version (v2, v3)
    pub version: Option<String>,

    /// Network (ethereum, polygon, etc.)
    pub network: Option<String>,

    /// Optional: Infura API key if using Infura for RPC
    #[serde(default)]
    pub infura_api_key: Option<String>,

    /// Optional: Custom RPC timeout in seconds
    #[serde(default)]
    pub rpc_timeout_seconds: Option<u64>,

    /// Optional: Custom RPC URL if not using Infura
    #[serde(default)]
    pub custom_rpc_url: Option<String>,

    /// Optional: Router contract address for the DEX
    #[serde(default)]
    pub router_address: Option<String>,

    /// Optional: WETH contract address
    #[serde(default)]
    pub weth_address: Option<String>,

    /// Optional: Stablecoin contract address (e.g., USDC)
    #[serde(default)]
    pub stablecoin_address: Option<String>,

    /// Optional: WETH/Stablecoin pair contract address
    #[serde(default)]
    pub weth_stablecoin_pair_address: Option<String>,

    /// Optional: Factory contract address
    #[serde(default)]
    pub factory_address: Option<String>,

    pub wallet: Option<WalletConfig>,

    #[serde(default = "default_paper_simulated_eth_balance")]
    pub paper_simulated_eth_balance: f64,

    #[serde(default = "default_paper_simulated_default_token_balance")]
    pub paper_simulated_default_token_balance: f64,

    #[serde(default = "default_paper_simulated_stablecoin_symbol")]
    pub paper_simulated_stablecoin_symbol: String,

    #[serde(default = "default_paper_simulated_stablecoin_balance")]
    pub paper_simulated_stablecoin_balance: f64,

    #[serde(default = "default_testnet_stablecoin_address")]
    pub testnet_stablecoin_address: String,
}

/// Wallet configuration
#[derive(PartialEq, Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    pub private_key_env: Option<String>,
    pub private_key_file: Option<String>,
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Whether cache is enabled
    #[serde(default)]
    pub enabled: bool,

    /// Redis connection URL
    #[serde(default)]
    pub redis_url: Option<String>,

    /// How often to flush cached data to database (in seconds)
    #[serde(default = "default_cache_flush_interval")]
    pub flush_interval: u64,
}

/// Configuration source
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    File,
    Environment,
    CommandLine,
}

/// Command line overrides for configuration
pub struct CommandLineOverrides {
    pub paper: bool,
    pub scan_interval: Option<u64>,
    pub max_position: Option<f64>,
    pub max_exposure: Option<f64>,
    pub strategy: Option<String>,
    pub threshold: Option<f64>,
    pub min_volume: Option<f64>,
    pub stop_loss: Option<f64>,
    pub take_profit: Option<f64>,
    pub max_positions: Option<usize>,
    pub risk_tolerance: Option<u8>,
    pub max_trade_risk_pct_wallet: Option<f64>,
    pub min_eth_balance_trade: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_keys: ApiKeys {
                cryptocompare: None,
                coingecko: None,
                etherscan: None,
                coincap: None,
                infura: None,
            },
            trading: TradingConfig::default(),
            database: DatabaseConfig::default(),
            api: ApiConfig::default(),
            data_collection: DataCollectionConfig::default(),
            logs: LogsConfig::default(),
            dex: DexConfig::default(),
            cache: CacheConfig::default(),
        }
    }
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            paper_trading: default_paper_trading(),
            max_position_size: default_max_position_size(),
            max_total_exposure: default_max_total_exposure(),
            strategy: default_strategy(),
            threshold: default_threshold(),
            min_volume: default_min_volume(),
            stop_loss: default_stop_loss(),
            take_profit: default_take_profit(),
            max_positions: default_max_positions(),
            risk_tolerance: default_risk_tolerance(),
            wide_scan_mode: default_wide_scan_mode(),
            trading_mode: default_trading_mode(),
            max_daily_loss: default_max_daily_loss_config(),
            max_drawdown: default_max_drawdown_config(),
            max_single_trade_risk_percentage_of_wallet:
                default_max_single_trade_risk_percentage_of_wallet(),
            min_required_eth_balance_for_trading: default_min_required_eth_balance_for_trading(),
        }
    }
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            strategy_type: default_strategy(),
            threshold: default_threshold(),
            min_volume: default_min_volume(),
        }
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            stop_loss_pct: default_stop_loss(),
            take_profit_pct: default_take_profit(),
            max_positions: default_max_positions(),
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
            query_logging: false,
            batch_size: default_batch_size(),
            batch_interval_secs: default_batch_interval_secs(),
            maintenance_interval_hours: default_maintenance_interval_hours(),
        }
    }
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            coingecko_url: default_coingecko_url(),
            request_timeout: default_timeout(),
            max_retries: default_retries(),
        }
    }
}

impl Default for DataCollectionConfig {
    fn default() -> Self {
        Self {
            interval: default_collection_interval(),
            history_days: default_history_days(),
            auto_start: default_true(),
            use_websockets: default_false(),
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
            name: default_dex_name(),
            version: Some("v2".to_string()),
            network: Some("goerli".to_string()),
            infura_api_key: None,
            rpc_timeout_seconds: Some(30),
            custom_rpc_url: None,
            router_address: None,
            weth_address: None,
            stablecoin_address: None,
            weth_stablecoin_pair_address: None,
            factory_address: None,
            wallet: None,
            paper_simulated_eth_balance: default_paper_simulated_eth_balance(),
            paper_simulated_default_token_balance: default_paper_simulated_default_token_balance(),
            paper_simulated_stablecoin_symbol: default_paper_simulated_stablecoin_symbol(),
            paper_simulated_stablecoin_balance: default_paper_simulated_stablecoin_balance(),
            testnet_stablecoin_address: default_testnet_stablecoin_address(),
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        CacheConfig {
            enabled: false,
            redis_url: Some("redis://127.0.0.1/".to_string()),
            flush_interval: 60,
        }
    }
}

// Default value functions
fn default_true() -> bool {
    true
}
fn default_strategy() -> String {
    "momentum".to_string()
}
fn default_threshold() -> f64 {
    0.5
}
fn default_min_volume() -> f64 {
    10000.0
} // $10,000 minimum daily volume
fn default_stop_loss() -> f64 {
    5.0
} // 5% stop loss
fn default_take_profit() -> f64 {
    10.0
} // 10% take profit
fn default_max_positions() -> usize {
    5
}
fn default_coingecko_url() -> String {
    "https://api.coingecko.com/api/v3".to_string()
}
fn default_timeout() -> u64 {
    10
} // 10 seconds
fn default_retries() -> usize {
    3
}
fn default_collection_interval() -> u64 {
    300
} // 5 minutes
fn default_history_days() -> u64 {
    30
} // 30 days of history
fn default_logs_directory() -> String {
    let data_dir = ProjectDirs::from("com", "honeybadger", "honeybadger")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let logs_dir = data_dir.join("logs");
    // Create the logs directory if it doesn't exist
    if !logs_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&logs_dir) {
            eprintln!("Warning: Failed to create logs directory: {}", e);
            return ".".to_string(); // Fallback to current directory
        }
    }
    logs_dir.to_string_lossy().to_string()
}
fn default_risk_tolerance() -> u8 {
    2
} // Moderate
fn default_dex_name() -> String {
    "uniswap".to_string()
}

fn default_paper_trading() -> bool {
    true
}

fn default_max_position_size() -> f64 {
    1000.0
}

fn default_max_total_exposure() -> f64 {
    5000.0
}

fn default_wide_scan_mode() -> bool {
    false
}

fn default_cache_flush_interval() -> u64 {
    30 // 30 seconds
}

fn default_trading_mode() -> TradingMode {
    TradingMode::Production
}

fn default_db_host() -> String {
    "localhost".to_string()
}
fn default_db_port() -> u16 {
    5432
}
fn default_db_user() -> String {
    "admin".to_string()
} // Common default
fn default_db_password() -> Option<String> {
    None
} // Recommend setting via env or secrets
fn default_db_name() -> String {
    "honeybadger_db".to_string()
}
fn default_db_pool_max_size() -> usize {
    50
} // Align with previous setting
fn default_batch_size() -> usize {
    100
}
fn default_batch_interval_secs() -> u64 {
    300
} // 5 minutes
fn default_maintenance_interval_hours() -> u64 {
    24
} // 24 hours

fn default_max_daily_loss_config() -> f64 {
    1000.0
}

fn default_max_drawdown_config() -> f64 {
    0.2 // Default max drawdown 20%
}

fn default_max_single_trade_risk_percentage_of_wallet() -> f64 {
    0.02 // Default 2% of wallet for a single trade
}

fn default_min_required_eth_balance_for_trading() -> f64 {
    0.01 // Default 0.01 ETH required to trade
}

fn default_paper_simulated_eth_balance() -> f64 {
    10.0 // Default 10 ETH for paper trading
}

fn default_paper_simulated_default_token_balance() -> f64 {
    1000.0 // Default 1000 for other tokens in paper trading
}

fn default_paper_simulated_stablecoin_symbol() -> String {
    "USDC".to_string()
}

fn default_paper_simulated_stablecoin_balance() -> f64 {
    10000.0 // Default 10,000 for paper trading stablecoin
}

fn default_testnet_stablecoin_address() -> String {
    "0x07865c6E87B9F70255377e024ace6630C1Eaa37F".to_string() // USDC on Goerli
}

fn default_false() -> bool {
    false
}

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
        if self.api.coingecko_url.is_empty() {
            self.api.coingecko_url = default_coingecko_url();
        }
    }

    /// Load configuration from multiple sources with priority:
    /// 1. Command line arguments (highest priority)
    /// 2. Environment variables
    /// 3. Config file
    /// 4. Default values (lowest priority)
    pub fn load() -> Result<Self> {
        let config = Config::default();
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let config_path = get_config_path()?;

        // Ensure directory exists
        if let Some(dir) = config_path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| Error::Config(format!("Failed to create config directory: {}", e)))?;
        }

        let contents = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("Failed to serialize config: {}", e)))?;

        fs::write(&config_path, contents)
            .map_err(|e| Error::Config(format!("Failed to write config: {}", e)))?;

        debug!("Configuration saved to {:?}", config_path);
        Ok(())
    }

    /// Set an API key for a specific service
    pub fn set_api_key(&mut self, service: &str, key: String) -> Result<()> {
        match service.to_lowercase().as_str() {
            "cryptocompare" => self.api_keys.cryptocompare = Some(key),
            "coingecko" => self.api_keys.coingecko = Some(key),
            "etherscan" => self.api_keys.etherscan = Some(key),
            "coincap" => self.api_keys.coincap = Some(key),
            "infura" => self.api_keys.infura = Some(key),
            _ => return Err(Error::Config(format!("Unknown service: {}", service))),
        }

        // Save the updated configuration
        self.save()
    }

    /// Apply command-line overrides to the configuration
    pub fn apply_command_line(&mut self, overrides: CommandLineOverrides) {
        if overrides.paper {
            self.trading.paper_trading = true;
        }

        if let Some(interval) = overrides.scan_interval {
            self.data_collection.interval = interval;
        }

        if let Some(position) = overrides.max_position {
            self.trading.max_position_size = position;
        }

        if let Some(exposure) = overrides.max_exposure {
            self.trading.max_total_exposure = exposure;
        }

        if let Some(strategy_type) = overrides.strategy {
            self.trading.strategy = strategy_type;
        }

        if let Some(threshold_value) = overrides.threshold {
            self.trading.threshold = threshold_value;
        }

        if let Some(volume) = overrides.min_volume {
            self.trading.min_volume = volume;
        }

        if let Some(stop_loss_value) = overrides.stop_loss {
            self.trading.stop_loss = stop_loss_value;
        }

        if let Some(take_profit) = overrides.take_profit {
            self.trading.take_profit = take_profit;
        }

        if let Some(max_positions) = overrides.max_positions {
            self.trading.max_positions = max_positions;
        }

        if let Some(risk_level) = overrides.risk_tolerance {
            self.trading.risk_tolerance = risk_level;
        }

        if let Some(pct) = overrides.max_trade_risk_pct_wallet {
            self.trading.max_single_trade_risk_percentage_of_wallet = pct;
        }

        if let Some(balance) = overrides.min_eth_balance_trade {
            self.trading.min_required_eth_balance_for_trading = balance;
        }
    }

    /// Check if paper trading is enabled
    pub fn is_paper_trading(&self) -> bool {
        self.trading.paper_trading
    }

    /// Get the Redis URL from configuration
    pub fn get_redis_url(&self) -> Option<String> {
        // If the cache is enabled and the Redis URL is set, use it
        if self.cache.enabled && self.cache.redis_url.is_some() {
            // Check if the URL is valid
            if self
                .cache
                .redis_url
                .as_ref()
                .unwrap()
                .starts_with("redis://")
            {
                return Some(self.cache.redis_url.as_ref().unwrap().clone());
            } else {
                warn!(
                    "Invalid Redis URL format: {}",
                    self.cache.redis_url.as_ref().unwrap()
                );
                return None;
            }
        }

        // Try fallback to environment variable
        if let Ok(url) = std::env::var("REDIS_URL") {
            if !url.is_empty() {
                debug!("Using Redis URL from environment variable");
                return Some(url);
            }
        }

        // No valid Redis URL found
        None
    }
}

impl DexConfig {
    /// Check if the current network is a testnet
    pub fn is_testnet(&self) -> bool {
        if let Some(network) = &self.network {
            matches!(
                network.to_lowercase().as_str(),
                "goerli" | "sepolia" | "mumbai"
            )
        } else {
            // Default to testnet (goerli) if no network is specified
            true
        }
    }
}

/// Returns the platform-specific path to the configuration file
pub fn get_config_path() -> Result<PathBuf> {
    get_config_path_internal()
}

fn get_config_path_internal() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| Error::Config("Could not determine configuration directory".to_string()))?;

    let app_config_dir = config_dir.join(APP_NAME);

    // Create directory if it doesn't exist
    if !app_config_dir.exists() {
        fs::create_dir_all(&app_config_dir)
            .map_err(|e| Error::Config(format!("Failed to create config directory: {}", e)))?;
    }

    Ok(app_config_dir.join(CONFIG_FILENAME))
}

#[test]
fn test_default_values_for_new_config() {
    // Create a new config using Config::default()
    let config = Config::default();

    // Assert that critical default values are set as expected
    assert_eq!(config.trading.paper_trading, default_paper_trading());
    assert_eq!(
        config.trading.max_position_size,
        default_max_position_size()
    );
    assert_eq!(
        config.trading.max_total_exposure,
        default_max_total_exposure()
    );
    assert_eq!(config.trading.strategy, default_strategy());
    assert_eq!(config.trading.threshold, default_threshold());
    assert_eq!(config.trading.min_volume, default_min_volume());
    assert_eq!(config.trading.stop_loss, default_stop_loss());
    assert_eq!(config.trading.take_profit, default_take_profit());
    assert_eq!(config.trading.max_positions, default_max_positions());
    assert_eq!(config.trading.risk_tolerance, default_risk_tolerance());
    assert_eq!(config.trading.wide_scan_mode, default_wide_scan_mode());
    assert_eq!(config.trading.trading_mode, default_trading_mode());
    assert_eq!(
        config.trading.max_daily_loss,
        default_max_daily_loss_config()
    );
    assert_eq!(config.trading.max_drawdown, default_max_drawdown_config());
    assert_eq!(
        config.trading.max_single_trade_risk_percentage_of_wallet,
        default_max_single_trade_risk_percentage_of_wallet()
    );
    assert_eq!(
        config.trading.min_required_eth_balance_for_trading,
        default_min_required_eth_balance_for_trading()
    );
    assert_eq!(config.database.host, default_db_host());
    assert_eq!(config.database.port, default_db_port());
    assert_eq!(config.database.user, default_db_user());
    assert_eq!(config.database.dbname, default_db_name());
    assert_eq!(config.database.pool_max_size, default_db_pool_max_size());
    assert_eq!(config.database.query_logging, false);
    assert_eq!(config.database.batch_size, default_batch_size());
    assert_eq!(
        config.database.batch_interval_secs,
        default_batch_interval_secs()
    );
    assert_eq!(
        config.database.maintenance_interval_hours,
        default_maintenance_interval_hours()
    );
    assert_eq!(config.api.coingecko_url, default_coingecko_url());
    assert_eq!(config.api.request_timeout, default_timeout());
    assert_eq!(config.api.max_retries, default_retries());
    assert_eq!(
        config.data_collection.interval,
        default_collection_interval()
    );
    assert_eq!(config.data_collection.history_days, default_history_days());
    assert_eq!(config.data_collection.auto_start, default_true());
    assert_eq!(config.data_collection.use_websockets, default_false());
    assert_eq!(config.logs.directory, default_logs_directory());
    assert_eq!(config.dex.name, default_dex_name());
    assert_eq!(config.dex.version, Some("v2".to_string()));
    assert_eq!(config.dex.network, Some("goerli".to_string()));
    assert_eq!(config.dex.infura_api_key, None);
    assert_eq!(config.dex.rpc_timeout_seconds, Some(30));
    assert_eq!(config.dex.custom_rpc_url, None);
    assert_eq!(config.dex.router_address, None);
    assert_eq!(config.dex.weth_address, None);
    assert_eq!(config.dex.stablecoin_address, None);
    assert_eq!(config.dex.weth_stablecoin_pair_address, None);
    assert_eq!(config.dex.factory_address, None);
    assert_eq!(config.dex.wallet, None);
    assert_eq!(
        config.dex.paper_simulated_eth_balance,
        default_paper_simulated_eth_balance()
    );
    assert_eq!(
        config.dex.paper_simulated_default_token_balance,
        default_paper_simulated_default_token_balance()
    );
    assert_eq!(
        config.dex.paper_simulated_stablecoin_symbol,
        default_paper_simulated_stablecoin_symbol()
    );
    assert_eq!(
        config.dex.paper_simulated_stablecoin_balance,
        default_paper_simulated_stablecoin_balance()
    );
    assert_eq!(
        config.dex.testnet_stablecoin_address,
        default_testnet_stablecoin_address()
    );
    assert_eq!(config.cache.enabled, false);
    assert_eq!(
        config.cache.redis_url,
        Some("redis://127.0.0.1/".to_string())
    );
    assert_eq!(config.cache.flush_interval, default_cache_flush_interval());
}
