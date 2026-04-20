//! Infrastructure layer constants
//!
//! Constants for external integrations: database, network, cache, and retry logic.

// ============================================================================
// From constants::network - HTTP & TIMEOUTS
// ============================================================================

/// Default HTTP request timeout in seconds
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Default maximum retry attempts
pub const DEFAULT_MAX_RETRIES: usize = 3;

/// Default data collection interval in seconds (1 minute)
/// Optimized for momentum strategy which requires ~36 data points for indicators
/// At 60s intervals: 36 min warmup, indicators see 26-36 min of price action
pub const DEFAULT_COLLECTION_INTERVAL_SECS: u64 = 60;

/// Default history days for data collection
pub const DEFAULT_HISTORY_DAYS: u64 = 30;

// ============================================================================
// From constants::network - RPC SETTINGS
// ============================================================================

/// RPC poll interval in seconds
pub const RPC_POLL_INTERVAL_SECS: u64 = 30;

// ============================================================================
// From constants::network - BLOCKCHAIN TRANSACTIONS
// ============================================================================

/// Default transaction timeout in minutes (increased for network congestion)
pub const DEFAULT_TX_TIMEOUT_MINUTES: u64 = 10;

/// Default swap transaction deadline in seconds (10 minutes)
pub const DEFAULT_SWAP_DEADLINE_SECS: u64 = 600;

/// Required block confirmations for transaction finality
pub const REQUIRED_BLOCK_CONFIRMATIONS: u64 = 12;

/// Standard gas limit for ETH transfers (Ethereum protocol standard)
pub const ETHEREUM_STANDARD_GAS_LIMIT: u64 = 21000;

/// Estimated gas limit for ETH <-> Token swaps (simpler path)
pub const ETH_TOKEN_SWAP_GAS_ESTIMATE: u64 = 150_000;

/// Estimated gas limit for Token <-> Token swaps (complex path with intermediary)
pub const TOKEN_TOKEN_SWAP_GAS_ESTIMATE: u64 = 200_000;

/// Estimated gas limit for ETH -> WETH wrapping (simple deposit to WETH contract)
pub const WETH_WRAP_GAS_ESTIMATE: u64 = 30_000;

/// Gas efficiency normalization multiplier
pub const GAS_EFFICIENCY_MULTIPLIER: f64 = 1_000_000.0;

// ============================================================================
// From constants::network - TOKEN STANDARDS
// ============================================================================

/// Standard ERC-20 token decimal places
pub const ERC20_STANDARD_DECIMALS: u8 = 18;

/// Wei per Gwei conversion factor (1 Gwei = 1e9 Wei)
pub const WEI_PER_GWEI: f64 = 1_000_000_000.0;

/// Wei per ETH conversion factor (1 ETH = 1e18 Wei)
pub const WEI_PER_ETH: f64 = 1_000_000_000_000_000_000.0;

// ============================================================================
// From constants::database - CONNECTION POOL
// ============================================================================

/// Default maximum number of database connections in pool
pub const DEFAULT_MAX_POOL_SIZE: usize = 50;

/// Default database host
pub const DEFAULT_HOST: &str = "localhost";

/// Default PostgreSQL port
pub const DEFAULT_PORT: u16 = 5432;

/// Default database user
pub const DEFAULT_USER: &str = "admin";

/// Default database name
pub const DEFAULT_DATABASE_NAME: &str = "mantis_db";

// ============================================================================
// From constants::database - BATCH PROCESSING
// ============================================================================

/// Default batch size for database operations
pub const DEFAULT_BATCH_SIZE: usize = 100;

/// Default batch interval in seconds
pub const DEFAULT_BATCH_INTERVAL_SECS: u64 = 5;

/// Maximum batch size limit
pub const MAX_BATCH_SIZE: usize = 1000;

// ============================================================================
// From constants::database - QUEUE & RETRY
// ============================================================================

/// Maximum retry attempts for failed operations
pub const MAX_RETRY_ATTEMPTS: usize = 5;

/// Metadata TTL in seconds (24 hours)
pub const METADATA_TTL_SECS: u64 = 86400;

/// Completed operation TTL in seconds (1 hour)
pub const COMPLETED_TTL_SECS: u64 = 3600;

/// Token failure TTL in seconds (1 hour)
pub const TOKEN_FAILURE_TTL_SECS: u64 = 3600;

/// Token failure threshold before abandoning
pub const TOKEN_FAILURE_THRESHOLD: usize = 3;

/// Maximum backoff time in seconds (5 minutes)
pub const MAX_BACKOFF_SECS: u64 = 300;

// ============================================================================
// From constants::database - CACHE SETTINGS
// ============================================================================

/// How often to flush cached data to database (in seconds) - system default
pub const CACHE_FLUSH_INTERVAL_SECS: u64 = 60;

/// Token cache TTL in seconds - system default
pub const TOKEN_CACHE_TTL_SECS: u64 = 3600;

/// Default token cache TTL for user configuration (24 hours)
pub const DEFAULT_TOKEN_CACHE_TTL_SECS: u64 = 86400;

/// Default cache flush interval for user configuration (5 minutes)
pub const DEFAULT_CACHE_FLUSH_INTERVAL_SECS: u64 = 300;

// ============================================================================
// From constants::database - QUERY LIMITS
// ============================================================================

/// Default limit for closed positions query
pub const DEFAULT_CLOSED_POSITIONS_LIMIT: i64 = 20;
