use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Cache TTL values in seconds - now configurable via DexConfig
// This is a fallback default if config is not available
pub const TOKEN_METADATA_TTL: usize = 86400; // 24 hours (fallback default)

/// Cached token metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: i32,
    pub timestamp: DateTime<Utc>,
}

/// Cached market data structure containing price and volume information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMarketData {
    pub price: f64,
    pub volume: f64,
    pub timestamp: DateTime<Utc>,
}
