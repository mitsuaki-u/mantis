use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::env;
use directories::ProjectDirs;
use log::{info, debug, warn};
use crate::error::{Error, Result};

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
}

/// API keys for various services
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeys {
    pub cryptocompare: Option<String>,
    pub coingecko: Option<String>,
    pub etherscan: Option<String>,
    pub coincap: Option<String>,
}

/// Trading bot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingConfig {
    /// Enable paper trading (simulate trades without real money)
    #[serde(default = "default_true")]
    pub paper_trading: bool,

    /// Scan interval in seconds
    #[serde(default = "default_scan_interval")]
    pub scan_interval: u64,

    /// Maximum position size in USD
    #[serde(default = "default_max_position")]
    pub max_position_size: f64,

    /// Maximum total exposure in USD
    #[serde(default = "default_max_exposure")]
    pub max_total_exposure: f64,

    /// Strategy configuration
    #[serde(default)]
    pub strategy: StrategyConfig,

    /// Risk management configuration
    #[serde(default)]
    pub risk: RiskConfig,
    
    /// Tokens to track for market data
    #[serde(default)]
    pub tokens_to_track: Option<Vec<String>>,

    /// Risk tolerance level (0-5): 0=Conservative, 1=Conservative-Moderate, 
    /// 2=Moderate, 3=Moderate-Aggressive, 4=Aggressive, 5=Very Aggressive
    #[serde(default = "default_risk_tolerance")]
    pub risk_tolerance: u8,
}

/// Strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// Strategy type (momentum, rsi, macd, etc.)
    #[serde(default = "default_strategy")]
    pub strategy_type: String,

    /// Signal threshold
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
    /// Custom database path (if not using default)
    pub custom_path: Option<PathBuf>,

    /// Enable database query logging
    #[serde(default)]
    pub query_logging: bool,
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
    pub name: String,
    
    /// DEX version (v2, v3)
    pub version: Option<String>,
    
    /// Network (ethereum, polygon, etc.)
    pub network: Option<String>,
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
            },
            trading: TradingConfig::default(),
            database: DatabaseConfig::default(),
            api: ApiConfig::default(),
            data_collection: DataCollectionConfig::default(),
            logs: LogsConfig::default(),
            dex: DexConfig::default(),
        }
    }
}

impl Default for TradingConfig {
    fn default() -> Self {
        Self {
            paper_trading: default_true(),
            scan_interval: default_scan_interval(),
            max_position_size: default_max_position(),
            max_total_exposure: default_max_exposure(),
            strategy: StrategyConfig::default(),
            risk: RiskConfig::default(),
            tokens_to_track: None,
            risk_tolerance: default_risk_tolerance(),
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
            custom_path: None,
            query_logging: false,
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
            name: "uniswap".to_string(),
            version: Some("v3".to_string()),
            network: Some("ethereum".to_string()),
        }
    }
}

// Default value functions
fn default_true() -> bool { true }
fn default_scan_interval() -> u64 { 300 } // 5 minutes
fn default_max_position() -> f64 { 100.0 } // $100
fn default_max_exposure() -> f64 { 1000.0 } // $1000
fn default_strategy() -> String { "momentum".to_string() }
fn default_threshold() -> f64 { 0.5 }
fn default_min_volume() -> f64 { 10000.0 } // $10,000 minimum daily volume
fn default_stop_loss() -> f64 { 5.0 } // 5% stop loss
fn default_take_profit() -> f64 { 10.0 } // 10% take profit
fn default_max_positions() -> usize { 5 }
fn default_coingecko_url() -> String { "https://api.coingecko.com/api/v3".to_string() }
fn default_timeout() -> u64 { 10 } // 10 seconds
fn default_retries() -> usize { 3 }
fn default_collection_interval() -> u64 { 300 } // 5 minutes
fn default_history_days() -> u64 { 30 } // 30 days of history
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
fn default_risk_tolerance() -> u8 { 2 } // Moderate

impl Config {
    /// Load configuration from multiple sources with priority:
    /// 1. Command line arguments (highest priority)
    /// 2. Environment variables
    /// 3. Config file
    /// 4. Default values (lowest priority)
    pub fn load() -> Result<Self> {
        // Start with default configuration
        let mut config = Config::default();
        let mut sources = vec![ConfigSource::Default];

        // Try to load from config file
        match config.load_from_file() {
            Ok(true) => {
                sources.push(ConfigSource::File);
                debug!("Loaded configuration from file");
            },
            Ok(false) => {
                debug!("No configuration file found, using defaults");
            },
            Err(e) => {
                warn!("Error loading config file: {}", e);
            }
        }

        // Override with environment variables
        if config.load_from_env() {
            sources.push(ConfigSource::Environment);
            debug!("Loaded configuration from environment variables");
        }

        // Save the config if it was just created
        if sources.len() == 1 && sources[0] == ConfigSource::Default {
            if let Err(e) = config.save() {
                warn!("Failed to save default configuration: {}", e);
            } else {
                debug!("Saved default configuration to file");
            }
        }

        info!("Configuration loaded from sources: {:?}", sources);
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

        // Trading configuration
        if let Ok(val) = env::var("HONEYBADGER_PAPER_TRADING") {
            if let Ok(b) = val.parse::<bool>() {
                self.trading.paper_trading = b;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_SCAN_INTERVAL") {
            if let Ok(interval) = val.parse::<u64>() {
                self.trading.scan_interval = interval;
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
            self.trading.strategy.strategy_type = val;
            loaded = true;
        }

        if let Ok(val) = env::var("HONEYBADGER_STRATEGY_THRESHOLD") {
            if let Ok(threshold) = val.parse::<f64>() {
                self.trading.strategy.threshold = threshold;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_MIN_VOLUME") {
            if let Ok(volume) = val.parse::<f64>() {
                self.trading.strategy.min_volume = volume;
                loaded = true;
            }
        }

        // Risk configuration
        if let Ok(val) = env::var("HONEYBADGER_STOP_LOSS") {
            if let Ok(stop_loss) = val.parse::<f64>() {
                self.trading.risk.stop_loss_pct = stop_loss;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_TAKE_PROFIT") {
            if let Ok(take_profit) = val.parse::<f64>() {
                self.trading.risk.take_profit_pct = take_profit;
                loaded = true;
            }
        }

        if let Ok(val) = env::var("HONEYBADGER_MAX_POSITIONS") {
            if let Ok(max_positions) = val.parse::<usize>() {
                self.trading.risk.max_positions = max_positions;
                loaded = true;
            }
        }

        // Database configuration
        if let Ok(val) = env::var("HONEYBADGER_DB_PATH") {
            self.database.custom_path = Some(PathBuf::from(val));
            loaded = true;
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
            _ => return Err(Error::Config(format!("Unknown service: {}", service))),
        }
        
        // Save the updated configuration
        self.save()
    }

    /// Get database path, either from custom path or default
    pub fn db_path(&self) -> Result<PathBuf> {
        if let Some(path) = &self.database.custom_path {
            Ok(path.clone())
        } else {
            Database::get_default_db_path()
        }
    }

    /// Apply command-line overrides to the configuration
    pub fn apply_command_line(&mut self, 
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
        risk_tolerance: Option<u8>
    ) {
        if paper {
            self.trading.paper_trading = true;
        }
        
        if let Some(interval) = scan_interval {
            self.trading.scan_interval = interval;
        }
        
        if let Some(position) = max_position {
            self.trading.max_position_size = position;
        }
        
        if let Some(exposure) = max_exposure {
            self.trading.max_total_exposure = exposure;
        }
        
        if let Some(strategy_type) = strategy {
            self.trading.strategy.strategy_type = strategy_type;
        }
        
        if let Some(threshold_value) = threshold {
            self.trading.strategy.threshold = threshold_value;
        }
        
        if let Some(volume) = min_volume {
            self.trading.strategy.min_volume = volume;
        }
        
        if let Some(stop_loss_value) = stop_loss {
            self.trading.risk.stop_loss_pct = stop_loss_value;
        }
        
        if let Some(take_profit) = take_profit {
            self.trading.risk.take_profit_pct = take_profit;
        }
        
        if let Some(max_positions) = max_positions {
            self.trading.risk.max_positions = max_positions;
        }
        
        if let Some(risk_level) = risk_tolerance {
            self.trading.risk_tolerance = risk_level;
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

/// Helper methods for database path 
pub struct Database;

impl Database {
    /// Get the default database path
    pub fn get_default_db_path() -> Result<PathBuf> {
        let mut path = ProjectDirs::from("com", "honeybadger", "honeybadger")
            .ok_or_else(|| Error::Config("Could not determine data directory".to_string()))?
            .data_dir()
            .to_path_buf();
        
        path.push("trading_history.db");
        Ok(path)
    }
} 