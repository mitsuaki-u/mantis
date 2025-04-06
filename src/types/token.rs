use std::convert::From;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Canonical token data model used throughout the application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    // Core identifiers
    pub id: String,              // Unique identifier (e.g., "bitcoin")
    pub symbol: String,          // Token symbol (e.g., "BTC")
    pub name: String,            // Token name (e.g., "Bitcoin")
    
    // Price data
    pub price_usd: f64,          // Current price in USD
    pub price_change_24h: f64,   // 24-hour price change percentage
    pub volume_24h: f64,         // 24-hour trading volume in USD
    pub market_cap: Option<f64>, // Market capitalization in USD
    
    // Metadata
    pub market_cap_rank: Option<usize>,     // Market cap rank
    pub chain: Option<String>,              // Chain/network (e.g., "ethereum")
    pub address: Option<String>,            // Contract address if applicable
    pub latest_news: Option<String>,        // Latest news headline about the token
    pub last_updated: Option<DateTime<Utc>>,// Last time data was updated
}

impl TokenData {
    /// Create a new TokenData with minimal required fields
    pub fn new(id: &str, symbol: &str, name: &str, price_usd: f64) -> Self {
        Self {
            id: id.to_string(),
            // Always store symbols in uppercase for consistency
            symbol: symbol.to_uppercase(),
            name: name.to_string(),
            price_usd,
            price_change_24h: 0.0,
            volume_24h: 0.0,
            market_cap: None,
            market_cap_rank: None,
            chain: None,
            address: None,
            latest_news: None,
            last_updated: Some(Utc::now()),
        }
    }
    
    /// Makes sure the symbol is uppercase
    pub fn normalize(&mut self) {
        self.symbol = self.symbol.to_uppercase();
    }
}

/// Data source types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataProvider {
    CoinGecko,
    Database,
    Custom,
}

/// Trait for converting from various data sources to the canonical TokenData
pub trait TokenDataAdapter<T> {
    fn to_token_data(&self, provider: DataProvider) -> TokenData;
}

// Implement From for market TokenMetrics
impl From<&crate::types::market::TokenMetrics> for TokenData {
    fn from(metrics: &crate::types::market::TokenMetrics) -> Self {
        Self {
            id: metrics.id.clone(),
            symbol: metrics.symbol.to_uppercase(),
            name: metrics.name.clone(),
            price_usd: metrics.price_usd,
            price_change_24h: metrics.price_change_24h,
            volume_24h: metrics.volume_24h,
            market_cap: Some(metrics.market_cap),
            market_cap_rank: metrics.market_cap_rank,
            chain: metrics.chain.clone(),
            address: None,
            latest_news: metrics.latest_news.as_ref().map(|news| news.title.clone()),
            last_updated: Some(metrics.last_updated),
        }
    }
}

// Implement From for DB TokenMetrics
impl From<&crate::db::TokenMetrics> for TokenData {
    fn from(db_metrics: &crate::db::TokenMetrics) -> Self {
        Self {
            id: db_metrics.id.clone(),
            symbol: db_metrics.symbol.to_uppercase(),
            name: db_metrics.name.clone(),
            price_usd: db_metrics.price_usd,
            price_change_24h: db_metrics.price_change_24h,
            volume_24h: db_metrics.volume_24h,
            market_cap: None,  // DB doesn't store this
            market_cap_rank: None,
            chain: None,
            address: None,
            latest_news: None,
            last_updated: Some(Utc::now()),
        }
    }
}

// Implement From for DEX Token
impl From<&crate::types::dex::Token> for TokenData {
    fn from(dex_token: &crate::types::dex::Token) -> Self {
        Self {
            id: dex_token.address.clone(),  // Use address as id for DEX tokens
            symbol: dex_token.symbol.to_uppercase(),
            name: dex_token.name.clone(),
            price_usd: 0.0,  // DEX token doesn't have price info by itself
            price_change_24h: 0.0,
            volume_24h: 0.0,
            market_cap: None,
            market_cap_rank: None,
            chain: None,
            address: Some(dex_token.address.clone()),
            latest_news: None,
            last_updated: Some(Utc::now()),
        }
    }
}

// Implement conversion to market TokenMetrics
impl From<&TokenData> for crate::types::market::TokenMetrics {
    fn from(token_data: &TokenData) -> Self {
        let latest_news = token_data.latest_news.as_ref().map(|title| {
            crate::types::news::NewsItem {
                title: title.clone(),
                url: String::new(), // Default empty URL since we only store title
                source: "Unknown".to_string(),
                published_at: Utc::now(),
                categories: Vec::new(),
            }
        });

        Self {
            id: token_data.id.clone(),
            symbol: token_data.symbol.clone(),
            name: token_data.name.clone(),
            price_usd: token_data.price_usd,
            price_change_24h: token_data.price_change_24h,
            volume_24h: token_data.volume_24h,
            market_cap: token_data.market_cap.unwrap_or(0.0),
            market_cap_rank: token_data.market_cap_rank,
            latest_news,
            chain: token_data.chain.clone(),
            last_updated: token_data.last_updated.unwrap_or_else(Utc::now),
        }
    }
}

// Implement conversion to db TokenMetrics
impl From<&TokenData> for crate::db::TokenMetrics {
    fn from(token_data: &TokenData) -> Self {
        Self {
            id: token_data.id.clone(),
            symbol: token_data.symbol.clone(),
            name: token_data.name.clone(),
            price_usd: token_data.price_usd,
            price_change_24h: token_data.price_change_24h,
            volume_24h: token_data.volume_24h,
        }
    }
} 