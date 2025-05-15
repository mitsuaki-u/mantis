pub mod binance;
pub mod coincap;
pub mod coingecko;
pub mod providers;

use crate::core::config::Config;
use crate::core::error::{Error, Result};
use crate::core::models::market::{MarketOptions, MarketOverview, TokenMetrics, TrendingToken};
use crate::core::models::token::{DataProvider, TokenData};
use crate::infra::api::adapters;
use crate::utils::retry::with_retry;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::{debug, info, warn};
pub use providers::{MarketDataEvent, MarketDataProvider};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc::{self, Sender};
use tokio::try_join;

/// Events emitted by market data providers
#[derive(Debug, Clone)]
pub enum MarketProviderEvent {
    Connected,
    Disconnected,
    Data(MarketDataEvent),
    Error(String),
}

/// Trait defining the interface for market data providers.
/// Concrete implementations handle specifics for each exchange/API.
#[async_trait]
pub trait MarketApi: Send + Sync {
    /// Get the name of the API provider.
    fn name(&self) -> &str;

    /// Check if the provider supports real-time WebSocket connections.
    fn supports_websocket(&self) -> bool;

    /// Fetch the latest market data for all relevant tokens (polling method).
    /// Returns TokenMetrics for compatibility, though implementations might use TokenData internally.
    async fn get_market_data(&self) -> Result<Vec<TokenMetrics>>;

    /// Connect to the WebSocket stream for real-time data.
    /// Implementations should handle sending MarketDataEvent messages via the sender.
    async fn connect_websocket(
        &self,
        tokens_to_track: Vec<String>,
        sender: Sender<MarketDataEvent>,
    ) -> Result<()>;

    /// Disconnect from the WebSocket stream.
    async fn disconnect_websocket(&self) -> Result<()>;

    // Add clone_box for creating trait objects
    fn clone_box(&self) -> Box<dyn MarketApi>;
}

// Implement Clone for Box<dyn MarketApi>
impl Clone for Box<dyn MarketApi> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// Factory function to create a specific market data provider based on config.
pub fn create_market_api(config: &Config) -> Box<dyn MarketDataProvider> {
    info!("Selecting market data provider based on available API keys");

    let has_coincap = config.api_keys.coincap.is_some();
    // let has_binance_api = config.api_keys.binance_api_key.is_some(); // Field does not exist on ApiKeys
    // let has_binance_secret = config.api_keys.binance_secret_key.is_some(); // Field does not exist on ApiKeys
    let has_coingecko = config.api_keys.coingecko.is_some();

    debug!("API key availability:");
    debug!("- CoinCap: {}", if has_coincap { "✅" } else { "❌" });
    // debug!(
    //     "- Binance: {}",
    //     if has_binance_api && has_binance_secret {
    //         "✅"
    //     } else {
    //         "❌"
    //     }
    // );
    debug!("- CoinGecko: {}", if has_coingecko { "✅" } else { "❌" });

    // Prioritize providers supporting WebSocket
    if has_coincap {
        info!("Selected CoinCap provider (supports WebSocket)");
        return Box::new(coincap::CoinCapProvider::new(
            config.api_keys.coincap.clone(),
        ));
    }
    // if has_binance_api && has_binance_secret { // Temporarily disable Binance provider selection
    //     info!("Selected Binance provider (supports WebSocket)");
    //     return Box::new(binance::BinanceProvider::new(
    //         config.api_keys.binance_api_key.clone(),
    //         config.api_keys.binance_secret_key.clone(),
    //     ));
    // }
    if has_coingecko {
        info!("Selected CoinGecko provider (polling)");
        return Box::new(coingecko::CoinGeckoProvider::new(
            config.api_keys.coingecko.clone(),
        ));
    }

    warn!("No primary provider keys found, falling back to CoinGecko public API.");
    Box::new(coingecko::CoinGeckoProvider::new(None))
}

/// Fetch market overview data (trending, gainers, losers, volume leaders)
pub async fn get_market_overview(options: MarketOptions) -> Result<MarketOverview> {
    info!("Fetching market overview with limit: {}", options.limit);

    let coingecko_provider = coingecko::CoinGeckoProvider::new(None);

    let (trending_data_res, market_data_res): (Value, Vec<TokenMetrics>) = try_join!(
        coingecko::get_trending(),
        coingecko_provider.get_market_data(false, &[])
    )?;

    let trending_tokens = parse_trending_tokens(&trending_data_res)?;
    let mut tokens = market_data_res;

    tokens.sort_by(|a, b| {
        b.price_change_24h
            .partial_cmp(&a.price_change_24h)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let gainers = tokens.iter().take(options.limit).cloned().collect();
    let losers = tokens.iter().rev().take(options.limit).cloned().collect();

    tokens.sort_by(|a, b| {
        b.volume_24h
            .partial_cmp(&a.volume_24h)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let volume_leaders = tokens.iter().take(options.limit).cloned().collect();

    let filter_by_market_cap = |tokens: Vec<TokenMetrics>| -> Vec<TokenMetrics> {
        tokens
            .into_iter()
            .filter(|token| {
                let passes_min = options
                    .min_market_cap
                    .map(|min| token.market_cap >= min)
                    .unwrap_or(true);
                let passes_max = options
                    .max_market_cap
                    .map(|max| token.market_cap <= max)
                    .unwrap_or(true);
                passes_min && passes_max
            })
            .collect()
    };

    Ok(MarketOverview {
        trending: trending_tokens,
        gainers: filter_by_market_cap(gainers),
        losers: filter_by_market_cap(losers),
        volume_leaders: filter_by_market_cap(volume_leaders),
    })
}

fn parse_trending_tokens(data: &Value) -> Result<Vec<TrendingToken>> {
    debug!(
        "Trending data response: {}",
        serde_json::to_string_pretty(&data).unwrap_or_else(|_| "<invalid json>".to_string())
    );
    let tokens = data["coins"]
        .as_array()
        .ok_or_else(|| {
            Error::Parse("Invalid trending data format: 'coins' array missing".to_string())
        })?
        .iter()
        .filter_map(|coin| {
            let item = &coin["item"];
            let id = item["id"].as_str()?.to_string();
            let symbol = item["symbol"].as_str()?.to_uppercase();
            let name = item["name"].as_str()?.to_string();
            let market_cap_rank = item["market_cap_rank"]
                .as_u64()
                .map(|r| r as usize)
                .unwrap_or(0);
            let price_change = item["data"]["price_change_percentage_24h"]["usd"]
                .as_f64()
                .unwrap_or(0.0);
            let volume_str = item["data"]["total_volume"].as_str().unwrap_or("");
            let volume = volume_str
                .trim_start_matches('$')
                .replace(',', "")
                .parse::<f64>()
                .unwrap_or(0.0);
            let price_usd = item["data"]["price"].as_f64().unwrap_or_else(|| {
                item["data"]["price_btc"]
                    .as_f64()
                    .map(|btc| btc * 50000.0)
                    .unwrap_or(0.0)
            });

            Some(TrendingToken {
                id,
                symbol,
                name,
                market_cap_rank,
                price_change_24h: price_change,
                price_usd,
                volume_24h: Some(volume),
            })
        })
        .collect();
    Ok(tokens)
}

// Parse market data (assumes CoinGecko format)
fn parse_market_data(data: &Value) -> Result<Vec<TokenMetrics>> {
    let tokens_data: Result<Vec<TokenData>> = data
        .as_array()
        .ok_or_else(|| Error::Parse("Market data is not an array".to_string()))?
        .iter()
        .map(|coin_json_value| adapters::convert_coingecko_to_token_data(coin_json_value))
        .collect();

    // Convert Vec<TokenData> to Vec<TokenMetrics>
    let metrics: Vec<TokenMetrics> = tokens_data?
        .into_iter()
        .map(|td| TokenMetrics::from(&td)) // Assumes From<&TokenData> for TokenMetrics exists
        .collect();

    Ok(metrics)
}

// Don't re-export internal API modules
