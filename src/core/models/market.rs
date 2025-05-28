use crate::core::models::news::NewsItem;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetrics {
    // Core identifiers
    pub id: String,
    pub symbol: String,
    pub name: String,

    // Price and market data
    pub price_usd: f64,
    pub price_change_24h: f64,
    pub volume_24h: f64,
    pub market_cap: f64,
    pub market_cap_rank: Option<usize>,

    // Additional metadata
    pub latest_news: Option<NewsItem>,
    pub chain: Option<String>,
    pub last_updated: DateTime<Utc>,
}

#[derive(Debug)]
pub struct MarketOverview {
    pub trending: Vec<TrendingToken>,
    pub gainers: Vec<TokenMetrics>,
    pub losers: Vec<TokenMetrics>,
    pub volume_leaders: Vec<TokenMetrics>,
}

#[derive(Debug)]
pub struct MarketOptions {
    pub limit: usize,
    pub min_market_cap: Option<f64>,
    pub max_market_cap: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendingToken {
    pub id: String,
    pub name: String,
    pub symbol: String,
    pub market_cap_rank: usize,
    pub price_usd: f64,
    pub price_change_24h: f64,
    pub volume_24h: Option<f64>,
}
