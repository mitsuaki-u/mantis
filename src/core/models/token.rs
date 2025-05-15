use crate::core::error::{Error, Result};
use crate::core::models::market::TokenMetrics;
use crate::types::news::NewsItem;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::convert::{From, TryFrom};

/// Canonical token data model used throughout the application
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenData {
    // Core identifiers
    pub id: String,     // Unique identifier (e.g., "bitcoin")
    pub symbol: String, // Token symbol (e.g., "BTC")
    pub name: String,   // Token name (e.g., "Bitcoin")

    // Price data
    pub price_usd: f64,          // Current price in USD
    pub price_change_24h: f64,   // 24-hour price change percentage
    pub volume_24h: f64,         // 24-hour trading volume in USD
    pub market_cap: Option<f64>, // Market capitalization in USD

    // Metadata
    pub market_cap_rank: Option<u32>,        // Market cap rank
    pub chain: String,                       // Chain/network (e.g., "ethereum")
    pub address: Option<String>,             // Contract address if applicable
    pub latest_news: Option<String>,         // Latest news headline about the token
    pub last_updated: Option<DateTime<Utc>>, // Last time data was updated
}

impl TokenData {
    /// Create a new TokenData with minimal required fields
    pub fn new(id: &str, symbol: &str, name: &str, price_usd: f64) -> Self {
        Self {
            id: Self::normalize_token_id(id),
            symbol: symbol.to_uppercase(),
            name: name.to_string(),
            price_usd,
            price_change_24h: 0.0,
            volume_24h: 0.0,
            market_cap: None,
            market_cap_rank: None,
            chain: String::new(),
            address: None,
            latest_news: None,
            last_updated: Some(Utc::now()),
        }
    }

    /// Makes sure the symbol is uppercase
    pub fn normalize(&mut self) {
        self.symbol = self.symbol.to_uppercase();
        self.id = Self::normalize_token_id(&self.id);
    }

    /// Normalize a token ID to a consistent format
    pub fn normalize_token_id(id: &str) -> String {
        id.to_lowercase()
    }

    /// Provides a default list of tokens to track if not specified elsewhere.
    pub fn default_tracked_tokens() -> Vec<String> {
        vec![
            "bitcoin".to_string(),
            "ethereum".to_string(),
            "solana".to_string(),
            "chainlink".to_string(), // Example additional token
            "polkadot".to_string(),  // Example additional token
        ]
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

// Keep one From<&TokenMetrics> for TokenData
impl From<&TokenMetrics> for TokenData {
    fn from(metrics: &TokenMetrics) -> Self {
        Self {
            id: metrics.id.clone(),
            symbol: metrics.symbol.clone(),
            name: metrics.name.clone(),
            price_usd: metrics.price_usd,
            price_change_24h: metrics.price_change_24h,
            volume_24h: metrics.volume_24h,
            market_cap: Some(metrics.market_cap),
            market_cap_rank: metrics.market_cap_rank.map(|r| r as u32),
            chain: metrics.chain.clone().unwrap_or_default(),
            address: None,
            latest_news: metrics.latest_news.as_ref().map(|n| n.title.clone()),
            last_updated: Some(metrics.last_updated),
        }
    }
}

// Implement From for DEX Token
impl From<&crate::types::dex::Token> for TokenData {
    fn from(dex_token: &crate::types::dex::Token) -> Self {
        Self {
            id: dex_token.address.clone(), // Use address as id for DEX tokens
            symbol: dex_token.symbol.to_uppercase(),
            name: dex_token.name.clone(),
            price_usd: 0.0, // DEX token doesn't have price info by itself
            price_change_24h: 0.0,
            volume_24h: 0.0,
            market_cap: None,
            market_cap_rank: None,
            chain: String::new(),
            address: Some(dex_token.address.clone()),
            latest_news: None,
            last_updated: Some(Utc::now()),
        }
    }
}

// Keep one From<&TokenData> for TokenMetrics
impl From<&TokenData> for TokenMetrics {
    fn from(token_data: &TokenData) -> Self {
        Self {
            id: token_data.id.clone(),
            symbol: token_data.symbol.clone(),
            name: token_data.name.clone(),
            price_usd: token_data.price_usd,
            price_change_24h: token_data.price_change_24h,
            volume_24h: token_data.volume_24h,
            market_cap: token_data.market_cap.unwrap_or(0.0),
            market_cap_rank: token_data.market_cap_rank.map(|r| r as usize),
            chain: Some(token_data.chain.clone()),
            latest_news: token_data.latest_news.as_ref().map(|title| NewsItem {
                title: title.clone(),
                url: String::new(),
                source: String::new(),
                published_at: token_data.last_updated.unwrap_or_else(Utc::now),
                categories: Vec::new(),
            }),
            last_updated: token_data.last_updated.unwrap_or_else(Utc::now),
        }
    }
}

// Add TryFrom<&TrendingToken> for TokenData
impl TryFrom<&crate::core::models::market::TrendingToken> for TokenData {
    type Error = Error;

    fn try_from(trending: &crate::core::models::market::TrendingToken) -> Result<Self> {
        Ok(Self {
            id: trending.id.clone(),
            symbol: trending.symbol.clone(),
            name: trending.name.clone(),
            price_usd: trending.price_usd,
            price_change_24h: trending.price_change_24h,
            volume_24h: trending
                .volume_24h
                .ok_or_else(|| Error::Parse("Missing volume_24h in TrendingToken".to_string()))?,
            market_cap: None, // TrendingToken doesn't have market_cap
            market_cap_rank: Some(trending.market_cap_rank as u32),
            chain: "trending".to_string(), // Reverted back to String as per TokenData definition
            address: None,
            latest_news: None,              // TrendingToken doesn't have news
            last_updated: Some(Utc::now()), // Use current time as last_updated
        })
    }
}
