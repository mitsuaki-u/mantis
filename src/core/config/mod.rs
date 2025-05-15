use crate::core::error::{Error, Result};
use crate::domain::trading::indicators::TradingMode;
use directories::ProjectDirs;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::env;
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
    pub wallet: Option<WalletConfig>,
    #[serde(default)]
    pub testnet: bool,
}

/// Wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
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
            version: None,
            network: Some("goerli".to_string()),
            wallet: None,
            testnet: false,
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            redis_url: None,
            flush_interval: default_cache_flush_interval(),
        }
    }
}

// Default value functions
fn default_true() -> bool {
    true
}
fn default_scan_interval() -> u64 {
    300
} // 5 minutes
fn default_max_position() -> f64 {
    100.0
} // $100
fn default_max_exposure() -> f64 {
    1000.0
} // $1000
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

fn default_redis_url() -> String {
    "redis://127.0.0.1:6379".to_string()
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
    2000.0
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
        let config_path = get_config_path_internal().unwrap_or_else(|e_path| {
            warn!("Could not determine standard config path ({}). Trying current directory for {}.
                    Please ensure your config file is in the expected location or set HONEYBADGER_CONFIG_PATH.", e_path, CONFIG_FILENAME);
            PathBuf::from(CONFIG_FILENAME)
        });

        info!("Attempting to load configuration from: {:?}", config_path); // Logging the path

        let mut config = if config_path.exists() {
            match fs::read_to_string(&config_path) {
                Ok(contents) => {
                    match serde_json::from_str::<Config>(&contents) {
                        Ok(loaded_config) => {
                            info!("Successfully loaded configuration from {:?}.", config_path);
                            loaded_config
                        }
                        Err(e) => {
                            warn!(
                                "Failed to parse configuration file at {:?}: {}. Using default configuration.",
                                config_path,
                                e
                            );
                            Config::default() // Fallback to default if parsing fails
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to read configuration file at {:?}: {}. Using default configuration.",
                        config_path,
                        e
                    );
                    Config::default() // Fallback to default if reading fails
                }
            }
        } else {
            info!(
                "Configuration file not found at {:?}. Using default configuration.",
                config_path
            );
            Config::default() // Use default if file doesn't exist
        };

        // Initialize any fields that might need it after loading defaults or from file
        config.initialize_optional_fields();

        // Override with environment variables as a final step
        // This allows for more flexible configuration in different environments
        let _env_applied = config.load_from_env();
        // TODO: Consider logging if env vars actually overrode something for better traceability

        debug!("Final configuration after load: {:#?}", config);
        Ok(config)
    }

    /// Load configuration from file
    /// Returns Ok(true) if file was loaded, Ok(false) if file doesn't exist
    fn load_from_file(&mut self) -> Result<bool> {
        let config_path = get_config_path()?;

        if !config_path.exists() {
            return Ok(false);
        }

        let contents = fs::read_to_string(&config_path)
            .map_err(|e| Error::Config(format!("Failed to read config: {}", e)))?;

        *self = serde_json::from_str(&contents)
            .map_err(|e| Error::Config(format!("Failed to parse config: {}", e)))?;

        Ok(true)
    }

    /// Load configuration from environment variables
    /// Returns true if any values were loaded from environment
    fn load_from_env(&mut self) -> bool {
        let mut loaded = false;

        // API Keys
        if let Ok(key) = env::var("HONEYBADGER_COINGECKO_KEY") {
            self.api_keys.coingecko = Some(key);
            loaded = true;
        }

        if let Ok(key) = env::var("HONEYBADGER_CRYPTOCOMPARE_KEY") {
            self.api_keys.cryptocompare = Some(key);
            loaded = true;
        }

        if let Ok(key) = env::var("HONEYBADGER_ETHERSCAN_KEY") {
            self.api_keys.etherscan = Some(key);
            loaded = true;
        }

        if let Ok(key) = env::var("HONEYBADGER_COINCAP_KEY") {
            self.api_keys.coincap = Some(key);
            loaded = true;
        }

        if let Ok(key) = env::var("HONEYBADGER_INFURA_KEY") {
            self.api_keys.infura = Some(key);
            loaded = true;
        }

        // Trading configuration
        if let Ok(val) = env::var("HONEYBADGER_PAPER_TRADING") {
            if let Ok(b) = val.parse::<bool>() {
                self.trading.paper_trading = b;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_SCAN_INTERVAL") {
            if let Ok(interval) = val.parse::<u64>() {
                self.data_collection.interval = interval;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_MAX_POSITION") {
            if let Ok(size) = val.parse::<f64>() {
                self.trading.max_position_size = size;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_MAX_EXPOSURE") {
            if let Ok(exposure) = val.parse::<f64>() {
                self.trading.max_total_exposure = exposure;
                loaded = true;
            }
        }

        // Strategy configuration
        if let Ok(val) = env::var("HONEYBADGER_STRATEGY") {
            self.trading.strategy = val;
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_STRATEGY_THRESHOLD") {
            if let Ok(threshold) = val.parse::<f64>() {
                self.trading.threshold = threshold;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_MIN_VOLUME") {
            if let Ok(volume) = val.parse::<f64>() {
                self.trading.min_volume = volume;
                loaded = true;
            }
        }

        // Risk configuration
        if let Ok(val) = env::var("HONEYBADGER_STOP_LOSS") {
            if let Ok(stop_loss) = val.parse::<f64>() {
                self.trading.stop_loss = stop_loss;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_TAKE_PROFIT") {
            if let Ok(take_profit) = val.parse::<f64>() {
                self.trading.take_profit = take_profit;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_MAX_POSITIONS") {
            if let Ok(max_positions) = val.parse::<usize>() {
                self.trading.max_positions = max_positions;
                loaded = true;
            }
        }

        // Database configuration
        if let Ok(val) = env::var("HONEYBADGER_DB_HOST") {
            self.database.host = val;
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DB_PORT") {
            if let Ok(port) = val.parse::<u16>() {
                self.database.port = port;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_DB_USER") {
            self.database.user = val;
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DB_PASSWORD") {
            self.database.password = Some(val);
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DB_NAME") {
            self.database.dbname = val;
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DB_POOL_MAX_SIZE") {
            if let Ok(pool_max_size) = val.parse::<usize>() {
                self.database.pool_max_size = pool_max_size;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_DB_LOGGING") {
            if let Ok(logging) = val.parse::<bool>() {
                self.database.query_logging = logging;
                loaded = true;
            }
        }

        // API configuration
        if let Ok(val) = env::var("HONEYBADGER_COINGECKO_URL") {
            self.api.coingecko_url = val;
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_API_TIMEOUT") {
            if let Ok(timeout) = val.parse::<u64>() {
                self.api.request_timeout = timeout;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_API_RETRIES") {
            if let Ok(retries) = val.parse::<usize>() {
                self.api.max_retries = retries;
                loaded = true;
            }
        }

        // Data collection configuration
        if let Ok(val) = env::var("HONEYBADGER_COLLECTION_INTERVAL") {
            if let Ok(interval) = val.parse::<u64>() {
                self.data_collection.interval = interval;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_HISTORY_DAYS") {
            if let Ok(days) = val.parse::<u64>() {
                self.data_collection.history_days = days;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_AUTO_COLLECT") {
            if let Ok(auto_start) = val.parse::<bool>() {
                self.data_collection.auto_start = auto_start;
                loaded = true;
            }
        }

        // Logs configuration
        if let Ok(val) = env::var("HONEYBADGER_LOGS_DIR") {
            self.logs.directory = val;
            loaded = true;
        }

        // DEX configuration
        if let Ok(val) = env::var("HONEYBADGER_DEX_NAME") {
            self.dex.name = val;
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DEX_VERSION") {
            self.dex.version = Some(val);
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DEX_NETWORK") {
            self.dex.network = Some(val);
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DEX_WALLET_PRIVATE_KEY_ENV") {
            self.dex.wallet = Some(WalletConfig {
                private_key_env: Some(val),
                private_key_file: None,
            });
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DEX_WALLET_PRIVATE_KEY_FILE") {
            self.dex.wallet = Some(WalletConfig {
                private_key_env: None,
                private_key_file: Some(val),
            });
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_DEX_TESTNET") {
            if let Ok(testnet) = val.parse::<bool>() {
                self.dex.testnet = testnet;
                loaded = true;
            }
        }

        // Cache configuration
        if let Ok(val) = env::var("HONEYBADGER_CACHE_ENABLED") {
            if let Ok(enabled) = val.parse::<bool>() {
                self.cache.enabled = enabled;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_REDIS_URL") {
            self.cache.redis_url = Some(val);
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_CACHE_FLUSH_INTERVAL") {
            if let Ok(interval) = val.parse::<u64>() {
                self.cache.flush_interval = interval;
                loaded = true;
            }
        }

        loaded
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
    pub fn apply_command_line(
        &mut self,
        paper: bool,
        scan_interval: Option<u64>,
        max_position: Option<f64>,
        max_exposure: Option<f64>,
        strategy: Option<String>,
        threshold: Option<f64>,
        min_volume: Option<f64>,
        stop_loss: Option<f64>,
        take_profit: Option<f64>,
        max_positions: Option<usize>,
        risk_tolerance: Option<u8>,
    ) {
        if paper {
            self.trading.paper_trading = true;
        }

        if let Some(interval) = scan_interval {
            self.data_collection.interval = interval;
        }

        if let Some(position) = max_position {
            self.trading.max_position_size = position;
        }

        if let Some(exposure) = max_exposure {
            self.trading.max_total_exposure = exposure;
        }

        if let Some(strategy_type) = strategy {
            self.trading.strategy = strategy_type;
        }

        if let Some(threshold_value) = threshold {
            self.trading.threshold = threshold_value;
        }

        if let Some(volume) = min_volume {
            self.trading.min_volume = volume;
        }

        if let Some(stop_loss_value) = stop_loss {
            self.trading.stop_loss = stop_loss_value;
        }

        if let Some(take_profit) = take_profit {
            self.trading.take_profit = take_profit;
        }

        if let Some(max_positions) = max_positions {
            self.trading.max_positions = max_positions;
        }

        if let Some(risk_level) = risk_tolerance {
            self.trading.risk_tolerance = risk_level;
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
