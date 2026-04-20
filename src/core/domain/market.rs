// Removed: news::NewsItem import (module doesn't exist)
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetrics {
    // Core identifiers
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,

    // Price and market data
    pub price_usd: f64,
    pub price_change_24h: f64,
    pub volume_24h: f64,

    // Additional metadata
    pub chain: Option<String>,
    pub last_updated: DateTime<Utc>,
}
