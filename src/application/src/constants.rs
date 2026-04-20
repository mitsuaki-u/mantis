//! Application layer constants
//!
//! Constants for actor system, task management, and application-level configuration.

// ============================================================================
// From constants::actors - ACTOR TIMEOUTS
// ============================================================================

/// Actor shutdown wait time in milliseconds
pub const ACTOR_SHUTDOWN_WAIT_MS: u64 = 500;

/// Graceful shutdown wait time in seconds
pub const GRACEFUL_SHUTDOWN_SECS: u64 = 2;

/// Actor status query timeout in seconds
pub const ACTOR_QUERY_TIMEOUT_SECS: u64 = 1;

/// Actor metrics query timeout in seconds
pub const ACTOR_METRICS_TIMEOUT_SECS: u64 = 5;

/// Health check interval in seconds (5 minutes)
pub const HEALTH_CHECK_INTERVAL_SECS: u64 = 300;

/// Actor initialization wait time in milliseconds
pub const ACTOR_INIT_WAIT_MS: u64 = 1000;

/// Event routing setup wait time in milliseconds
pub const ROUTING_SETUP_WAIT_MS: u64 = 500;

// ============================================================================
// From constants::actors - CONCURRENCY & PERFORMANCE
// ============================================================================

/// Strategy actor database concurrency limit
pub const STRATEGY_DB_CONCURRENCY: usize = 10;

// ============================================================================
// From constants::actors - TASK TIMEOUTS
// ============================================================================

/// Market data task timeout in seconds
pub const MARKET_TASK_TIMEOUT_SECS: u64 = 10;

/// Execution task timeout in seconds
pub const EXECUTION_TASK_TIMEOUT_SECS: u64 = 5;

/// Health check response timeout in seconds
pub const HEALTH_CHECK_TIMEOUT_SECS: u64 = 5;

/// Timeout for stuck database operations (5 minutes)
pub const STUCK_OPERATION_TIMEOUT_SECS: u64 = 300;

// ============================================================================
// From constants::actors - RETRY LOGIC
// ============================================================================

/// Retry log interval in seconds (to avoid log spam)
pub const RETRY_LOG_INTERVAL_SECS: u64 = 60;

/// Maximum jitter in milliseconds (prevents thundering herd)
pub const MAX_JITTER_MS: u64 = 100;

/// Maximum backoff time for market data retries (seconds)
pub const MARKET_RETRY_MAX_BACKOFF_SECS: u64 = 60;

/// Initial retry delay for failed operations (seconds)
pub const INITIAL_RETRY_DELAY_SECS: u64 = 5;

/// Maximum exponent for exponential backoff (2^6 = 64 seconds)
pub const MAX_BACKOFF_EXPONENT: u32 = 6;

/// Number of consecutive failures before triggering exponential backoff
pub const CONSECUTIVE_FAILURES_BEFORE_BACKOFF: usize = 3;

// ============================================================================
// From constants::actors - STATUS & HEALTH CHECKS
// ============================================================================

/// Supervisor health check interval in seconds
pub const SUPERVISOR_HEALTH_CHECK_INTERVAL_SECS: u64 = 300;

// ============================================================================
// From constants::actors - ERROR THRESHOLDS
// ============================================================================

/// Critical error count threshold for actor health (>10 = critical)
pub const ACTOR_CRITICAL_ERROR_THRESHOLD: usize = 10;

/// Degraded error count threshold for actor health (>5 = degraded)
pub const ACTOR_DEGRADED_ERROR_THRESHOLD: usize = 5;

/// Critical failure count threshold for supervision (>10 = critical)
pub const SUPERVISION_CRITICAL_FAILURE_THRESHOLD: usize = 10;

/// Degraded failure count threshold for supervision (>3 = degraded)
pub const SUPERVISION_DEGRADED_FAILURE_THRESHOLD: usize = 3;

// ============================================================================
// From constants::system - CONFIGURATION
// ============================================================================

/// Application name identifier
pub const APP_NAME: &str = "mantis";

/// Configuration file name
pub const CONFIG_FILENAME: &str = "config.json";

// ============================================================================
// From constants::system - PAPER TRADING SIMULATION
// ============================================================================

/// Default simulated ETH balance for paper trading
pub const DEFAULT_SIMULATED_ETH_BALANCE: f64 = 10.0;

/// Default simulated token balance for paper trading
pub const DEFAULT_SIMULATED_TOKEN_BALANCE: f64 = 1000.0;

/// Default simulated WETH balance for paper trading
pub const DEFAULT_SIMULATED_WETH_BALANCE: f64 = 10.0;

// ============================================================================
// From constants::system - CONVERSIONS
// ============================================================================

/// Conversion factor from decimal to percentage
pub const DECIMAL_TO_PERCENTAGE: f64 = 100.0;

/// Hours to seconds conversion factor
pub const HOURS_TO_SECONDS: u64 = 3600;

/// Days to seconds conversion factor
pub const DAYS_TO_SECONDS: u64 = 86_400;

// ============================================================================
// From constants::system - FILE SYSTEM
// ============================================================================

/// Default directory name for logs
pub const DEFAULT_LOGS_DIRECTORY: &str = "logs";

/// Fallback directory when log directory creation fails
pub const FALLBACK_DIRECTORY: &str = ".";

// ============================================================================
// From constants::network - APPLICATION-LEVEL DEFAULTS
// ============================================================================

/// Default live trading mode (disabled for safety - paper trading is default)
pub const DEFAULT_LIVE_TRADING: bool = false;
