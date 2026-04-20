use crate::core::domain::market::TokenMetrics;
use crate::core::errors::{Error, Result};
// Removed: news::NewsItem import (module doesn't exist)
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::convert::{From, TryFrom};

/// Canonical token data model used throughout the application
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenData {
    // Core identifiers
    pub id: String,     // Unique identifier (e.g., "bitcoin")
    pub symbol: String, // Token symbol (e.g., "BTC")
    pub name: String,   // Token name (e.g., "Bitcoin")

    // Price data - using Decimal for financial precision
    pub price_usd: Decimal,        // Current price in USD
    pub price_change_24h: Decimal, // 24-hour price change percentage
    pub volume_24h: Decimal,       // 24-hour trading volume in USD

    // Metadata
    pub decimals: i32, // Token decimals (e.g., 18 for most ERC20, 6 for USDC)
    pub chain: String, // Chain/network (e.g., "ethereum")
    pub address: Option<String>, // Contract address if applicable
    pub latest_news: Option<String>, // Latest news headline about the token
    pub last_updated: Option<DateTime<Utc>>, // Last time data was updated
}

impl TokenData {
    /// Create a new TokenData with minimal required fields
    pub fn new(id: &str, symbol: &str, name: &str, price_usd: Decimal) -> Self {
        Self {
            id: Self::normalize_token_id(id),
            symbol: symbol.to_uppercase(),
            name: name.to_string(),
            price_usd,
            price_change_24h: Decimal::ZERO,
            volume_24h: Decimal::ZERO,
            decimals: 18, // Default to 18 (ERC20 standard)
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
        if id.contains(':') {
            // New format: chain_id:contract_address - use shared utilities
            match crate::core::utils::normalization::parse_token_id(id) {
                Ok((chain_id, contract_address)) => {
                    // Re-create with normalized address (parse_token_id already validates format)
                    format!("{}:{}", chain_id, contract_address.to_lowercase())
                }
                Err(_) => {
                    // Fallback to simple lowercase if parsing fails
                    id.to_lowercase()
                }
            }
        } else {
            // Legacy format: just lowercase the entire string
            id.to_lowercase()
        }
    }

    /// Provides a default list of tokens to track if not specified elsewhere.
    /// These are popular tokens that are likely to be available on both mainnet and testnets.
    pub fn default_tracked_tokens() -> Vec<String> {
        vec![
            "1:0x0000000000000000000000000000000000000000".to_string(), // ETH
            "1:0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2".to_string(), // WETH
            "1:0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(), // USDC
            "1:0xdac17f958d2ee523a2206206994597c13d831ec7".to_string(), // USDT
        ]
    }

    /// Database adapter: Check if this token should be considered "tracked"
    /// This replaces the old is_tracked field from types.rs
    pub fn is_tracked(&self) -> bool {
        // A token is considered tracked if it has recent price data
        self.last_updated.is_some() && !self.price_usd.is_zero()
    }

    /// Database adapter: Check if this token has valid price data
    /// This replaces the old has_price_data field from types.rs  
    pub fn has_price_data(&self) -> bool {
        !self.price_usd.is_zero()
    }

    /// Database adapter: Get optional fields that were required in canonical but optional in old types.rs
    pub fn price_change_24h_optional(&self) -> Option<Decimal> {
        if self.price_change_24h.is_zero() {
            None
        } else {
            Some(self.price_change_24h)
        }
    }

    /// Database adapter: Get optional volume that was required in canonical but optional in old types.rs
    pub fn volume_24h_optional(&self) -> Option<Decimal> {
        if self.volume_24h.is_zero() {
            None
        } else {
            Some(self.volume_24h)
        }
    }

    /// Database adapter: Get optional chain that was required in canonical but optional in old types.rs
    pub fn chain_optional(&self) -> Option<String> {
        if self.chain.is_empty() {
            None
        } else {
            Some(self.chain.clone())
        }
    }
}

// Convert TokenMetrics to TokenData - fails on invalid critical fields
impl TryFrom<&TokenMetrics> for TokenData {
    type Error = Error;

    fn try_from(metrics: &TokenMetrics) -> Result<Self> {
        // Critical field: price_usd must be valid
        let price_usd = crate::core::utils::f64_to_decimal(metrics.price_usd, "price_usd")
            .map_err(|e| {
                Error::InvalidInput(format!(
                    "Invalid price_usd {} for token {}: {}",
                    metrics.price_usd, metrics.id, e
                ))
            })?;

        // Critical field: volume_24h affects liquidity filtering
        let volume_24h = crate::core::utils::f64_to_decimal(metrics.volume_24h, "volume_24h")
            .map_err(|e| {
                Error::InvalidInput(format!(
                    "Invalid volume_24h {} for token {}: {}",
                    metrics.volume_24h, metrics.id, e
                ))
            })?;

        // Non-critical: price_change_24h can default to zero (informational only)
        let price_change_24h =
            crate::core::utils::f64_to_decimal(metrics.price_change_24h, "price_change_24h")
                .unwrap_or_else(|e| {
                    log::warn!(
                        "Invalid price_change_24h {} for token {}: {} - using 0.0",
                        metrics.price_change_24h,
                        metrics.id,
                        e
                    );
                    Decimal::ZERO
                });

        Ok(Self {
            id: metrics.id.clone(),
            symbol: metrics.symbol.clone(),
            name: metrics.name.clone(),
            price_usd,
            price_change_24h,
            volume_24h,
            decimals: metrics.decimals as i32,
            chain: metrics.chain.clone().unwrap_or_default(),
            address: None,
            latest_news: None,
            last_updated: Some(metrics.last_updated),
        })
    }
}

// Implement From for DEX Token
impl From<&crate::core::domain::dex::DexToken> for TokenData {
    fn from(dex_token: &crate::core::domain::dex::DexToken) -> Self {
        // Use shared utility to create token ID (defaults to Ethereum mainnet)
        let token_id = crate::core::utils::normalization::create_token_id(1, &dex_token.address)
            .unwrap_or_else(|_| format!("1:{}", dex_token.address.to_lowercase()));

        Self {
            id: token_id,
            symbol: dex_token.symbol.to_uppercase(),
            name: dex_token.name.clone(),
            price_usd: Decimal::ZERO, // DEX token doesn't have price info by itself
            price_change_24h: Decimal::ZERO,
            volume_24h: Decimal::ZERO,
            decimals: 18, // Default - DEX Token doesn't include decimals
            chain: String::new(),
            address: Some(dex_token.address.clone()),
            latest_news: None,
            last_updated: Some(Utc::now()),
        }
    }
}

// Convert TokenData to TokenMetrics - fails on invalid critical fields
impl TryFrom<&TokenData> for TokenMetrics {
    type Error = Error;

    fn try_from(token_data: &TokenData) -> Result<Self> {
        // Critical field: price_usd must be valid
        let price_usd = crate::core::utils::decimal_to_f64(token_data.price_usd, "price_usd")
            .map_err(|e| {
                Error::InvalidInput(format!(
                    "Invalid price_usd {} for token {}: {}",
                    token_data.price_usd, token_data.id, e
                ))
            })?;

        // Critical field: volume_24h affects strategy evaluation
        let volume_24h = crate::core::utils::decimal_to_f64(token_data.volume_24h, "volume_24h")
            .map_err(|e| {
                Error::InvalidInput(format!(
                    "Invalid volume_24h {} for token {}: {}",
                    token_data.volume_24h, token_data.id, e
                ))
            })?;

        // Non-critical: price_change_24h can default to zero (informational only)
        let price_change_24h =
            crate::core::utils::decimal_to_f64(token_data.price_change_24h, "price_change_24h")
                .unwrap_or_else(|e| {
                    log::warn!(
                        "Invalid price_change_24h {} for token {}: {} - using 0.0",
                        token_data.price_change_24h,
                        token_data.id,
                        e
                    );
                    0.0
                });

        Ok(Self {
            id: token_data.id.clone(),
            symbol: token_data.symbol.clone(),
            name: token_data.name.clone(),
            decimals: 18, // Default to 18 decimals (ERC20 standard)
            price_usd,
            price_change_24h,
            volume_24h,
            chain: Some(token_data.chain.clone()),
            last_updated: token_data.last_updated.unwrap_or_else(Utc::now),
        })
    }
}
