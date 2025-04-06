pub mod coingecko;
pub mod providers;
pub mod coincap;
pub mod binance;

use crate::error::Error;
use crate::types::market::{MarketOptions, MarketOverview, TokenMetrics, TrendingToken};
use crate::types::token::{TokenData, DataProvider};
use crate::api::adapters;
use log::{debug, info};
use tokio::try_join;
use serde_json::Value;
use reqwest::Client;
use serde::Deserialize;
use crate::config::Config;
use std::time::Duration;
use crate::utils::retry::with_retry;
use tokio::sync::mpsc;
use chrono::{DateTime, Utc};
use async_trait::async_trait;

/// Events emitted by market data providers
#[derive(Debug)]
pub enum MarketDataEvent {
    /// Price update for a token
    PriceUpdate {
        /// Token ID
        token_id: String,
        /// Current price in USD
        price: f64,
        /// Trading volume (optional)
        volume: Option<f64>,
        /// 24h price change percentage (optional)
        change_24h: Option<f64>,
        /// Timestamp of the update
        timestamp: DateTime<Utc>,
    },
    /// Volume update for a token
    VolumeUpdate {
        /// Token ID
        token_id: String,
        /// Trading volume
        volume: f64,
        /// Timestamp of the update
        timestamp: DateTime<Utc>,
    },
    /// Error from the data provider
    Error(String),
}

/// Trait for market data providers
#[async_trait]
pub trait MarketDataProvider: Send + Sync + 'static {
    /// Get the name of the provider
    fn name(&self) -> &str;
    
    /// Get market data for tokens
    async fn get_market_data(&self) -> Result<Vec<TokenMetrics>, Error>;
    
    /// Connect to WebSocket for real-time updates
    async fn connect_websocket(&self, tokens: Vec<String>, sender: mpsc::Sender<MarketDataEvent>) -> Result<(), Error>;
    
    /// Disconnect from WebSocket
    async fn disconnect_websocket(&self) -> Result<(), Error>;
    
    /// Check if the provider supports WebSocket
    fn supports_websocket(&self) -> bool;
    
    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
    
    /// Clone the provider
    fn clone_box(&self) -> Box<dyn MarketDataProvider>;
}

#[derive(Clone)]
pub struct MarketApi {
    client: Client,
    is_paper_trading: bool,
}

impl MarketApi {
    pub fn new(is_paper_trading: bool) -> Self {
        // Get API key from config
        let api_key = Config::load()
            .ok()
            .and_then(|config| config.api_keys.coingecko.clone());  // Fix: use api_keys.coingecko
        
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    "User-Agent",
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36"
                        .parse()
                        .unwrap(),
                );
                // Add API key if available from config
                if let Some(key) = &api_key {
                    headers.insert(
                        "x-cg-demo-api-key",
                        key.parse().unwrap(),
                    );
                }
                headers
            })
            .build()
            .unwrap_or_else(|e| {
                log::error!("Failed to create HTTP client: {}", e);
                Client::new()
            });
            
        Self {
            client,
            is_paper_trading
        }
    }

    /// Create a MarketApi instance with the default provider based on available API keys
    pub fn new_with_default_provider(config: &crate::config::Config) -> Self {
        // Use paper trading mode based on config
        let is_paper_trading = config.trading.paper_trading;
        
        // For now, just use the regular constructor
        // This could be enhanced to select different providers based on API key availability
        Self::new(is_paper_trading)
    }

    /// Create a specific market data provider based on name
    pub fn create_provider(&self, provider_name: &str, config: &Config) -> Box<dyn MarketDataProvider> {
        match provider_name.to_lowercase().as_str() {
            "coincap" => {
                info!("Creating CoinCap market data provider");
                Box::new(coincap::CoinCapProvider::new(config.api_keys.coincap.clone()))
            },
            "binance" => {
                info!("Creating Binance market data provider");
                Box::new(binance::BinanceProvider::new(
                    config.api_keys.coingecko.clone(), 
                    None
                ))
            },
            _ => {
                info!("Creating CoinGecko market data provider (default)");
                Box::new(self.clone())  // Default to CoinGecko (this instance)
            }
        }
    }
    
    /// Get configured provider based on available API keys
    pub fn get_configured_provider(&self, config: &Config) -> Box<dyn MarketDataProvider> {
        // Prioritize providers that support WebSockets
        if config.api_keys.coincap.is_some() {
            self.create_provider("coincap", config)
        } else if config.api_keys.coingecko.is_some() {
            self.create_provider("coingecko", config)
        } else {
            // Default to using the current instance as CoinGecko provider
            Box::new(self.clone())
        }
    }

    pub async fn get_market_data(&self) -> Result<Vec<TokenMetrics>, Error> {
        if self.is_paper_trading {
            // For paper trading, use real market data
            with_retry(
                "fetch_market_data",
                || async { self.get_production_data().await },
                3,  // Max 3 retries 
                Duration::from_secs(2) // Start with 2 second backoff
            ).await
        } else {
            // For real trading, same implementation but could be different in future
            with_retry(
                "fetch_market_data",
                || async { self.get_production_data().await },
                3,  // Max 3 retries
                Duration::from_secs(2) // Start with 2 second backoff
            ).await
        }
    }

    /// Gets token data using the canonical TokenData model
    pub async fn get_token_data(&self) -> Result<Vec<TokenData>, Error> {
        if self.is_paper_trading {
            self.get_coingecko_token_data().await
        } else {
            self.get_production_token_data().await
        }
    }

    async fn get_coingecko_data(&self) -> Result<Vec<TokenMetrics>, Error> {
        // First get data using the canonical TokenData model
        let token_data = self.get_coingecko_token_data().await?;
        
        // Convert to TokenMetrics for backward compatibility
        let metrics: Vec<TokenMetrics> = token_data
            .iter()
            .map(|data| TokenMetrics::from(data))
            .collect();
            
        Ok(metrics)
    }

    async fn get_coingecko_token_data(&self) -> Result<Vec<TokenData>, Error> {
        let url = "https://api.coingecko.com/api/v3/coins/markets";
        let params = [
            ("vs_currency", "usd"),
            ("order", "volume_desc"),
            ("per_page", "100"),
            ("page", "1"),
            ("sparkline", "false"),
            ("price_change_percentage", "24h")
        ];

        // Add a longer delay between requests (2 seconds)
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let response = self.client
            .get(url)
            .query(&params)
            .send()
            .await
            .map_err(|e| Error::Network(format!("Failed to send request: {}", e)))?;

        // Check for rate limiting
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            // Get retry-after header or default to 60 seconds
            let retry_after = response.headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(Error::RateLimit(format!(
                "Rate limited by CoinGecko. Try again in {} seconds", 
                retry_after
            )));
        }

        // Check response status
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_else(|_| "No error message".to_string());
            return Err(Error::Api(format!(
                "CoinGecko API error ({}): {}", 
                status, 
                text.lines().next().unwrap_or("Unknown error")  // Only show first line of error
            )));
        }

        // Parse the response with better error handling
        let response_text = response.text().await
            .map_err(|e| Error::Network(format!("Failed to get response text: {}", e)))?;

        let json_data: Vec<Value> = serde_json::from_str(&response_text)
            .map_err(|e| Error::Parse(format!("Failed to parse market data: {}", e)))?;

        // Use the adapters to convert the JSON data to our canonical TokenData model
        let tokens = adapters::batch_convert_to_token_data(&json_data, DataProvider::CoinGecko);

        // Add debug logging for tokens
        debug!("Loaded {} tokens from CoinGecko API", tokens.len());
        
        if !tokens.is_empty() {
            debug!("Sample tokens: {}", 
                tokens.iter()
                    .take(5)
                    .map(|token| format!("{}({})", token.symbol, token.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        if tokens.is_empty() {
            Err(Error::Api("CoinGecko API returned empty coin list".to_string()))
        } else {
            Ok(tokens)
        }
    }

    async fn get_production_data(&self) -> Result<Vec<TokenMetrics>, Error> {
        // Convert from TokenData to TokenMetrics
        let token_data = self.get_production_token_data().await?;
        let metrics: Vec<TokenMetrics> = token_data
            .iter()
            .map(|data| TokenMetrics::from(data))
            .collect();
            
        Ok(metrics)
    }
    
    async fn get_production_token_data(&self) -> Result<Vec<TokenData>, Error> {
        // For now, use the same implementation as paper trading
        // In a real production environment, you might use different API providers
        self.get_coingecko_token_data().await
    }
}

// Implement MarketDataProvider trait for MarketApi
#[async_trait]
impl MarketDataProvider for MarketApi {
    fn name(&self) -> &str {
        "CoinGecko"  // Since we're using CoinGecko API by default
    }
    
    async fn get_market_data(&self) -> Result<Vec<TokenMetrics>, Error> {
        self.get_market_data().await
    }
    
    async fn connect_websocket(&self, tokens: Vec<String>, sender: mpsc::Sender<MarketDataEvent>) -> Result<(), Error> {
        // CoinGecko API doesn't support WebSockets in the free tier
        // This is a placeholder implementation
        Err(Error::Api("WebSocket not supported by this provider".to_string()))
    }
    
    async fn disconnect_websocket(&self) -> Result<(), Error> {
        // Since we don't support WebSockets, this is a no-op
        Ok(())
    }
    
    fn supports_websocket(&self) -> bool {
        false  // CoinGecko API doesn't support WebSockets in the free tier
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn clone_box(&self) -> Box<dyn MarketDataProvider> {
        Box::new(self.clone())
    }
}

#[derive(Deserialize)]
struct CoinGeckoMarket {
    id: String,          // This is CoinGecko's unique identifier
    symbol: String,
    name: String,
    #[serde(rename = "current_price")]
    current_price: f64,
    #[serde(rename = "price_change_percentage_24h")]
    price_change_percentage_24h: Option<f64>,
    #[serde(rename = "total_volume")]
    total_volume: f64,
    market_cap: f64,
}

pub async fn get_market_overview(options: MarketOptions) -> Result<MarketOverview, Error> {
    info!("Fetching market overview with limit: {}", options.limit);
    
    let (trending_data, market_data) = try_join!(
        coingecko::get_trending(),
        coingecko::get_market_data(options.limit),
    )?;

    // Parse trending tokens
    let trending = parse_trending_tokens(&trending_data)?;

    // Parse market data and sort into categories
    let mut tokens = parse_market_data(&market_data)?;
    
    // Sort by 24h price change for gainers/losers
    tokens.sort_by(|a, b| b.price_change_24h.partial_cmp(&a.price_change_24h).unwrap());
    
    // Sort by volume for volume leaders
    let mut volume_leaders = tokens.clone();
    volume_leaders.sort_by(|a, b| b.volume_24h.partial_cmp(&a.volume_24h).unwrap());

    // Apply market cap filters if specified
    let tokens: Vec<TokenMetrics> = tokens
        .into_iter()
        .filter(|token| {
            let passes_min = options.min_market_cap
                .map(|min| token.market_cap >= min)
                .unwrap_or(true);
            let passes_max = options.max_market_cap
                .map(|max| token.market_cap <= max)
                .unwrap_or(true);
            passes_min && passes_max
        })
        .collect();

    Ok(MarketOverview {
        trending,
        gainers: tokens.iter().take(options.limit).cloned().collect(),
        losers: tokens.iter().rev().take(options.limit).cloned().collect(),
        volume_leaders: volume_leaders.into_iter().take(options.limit).collect(),
    })
}

fn parse_trending_tokens(data: &Value) -> Result<Vec<TrendingToken>, Error> {
    debug!("Trending data response: {}", serde_json::to_string_pretty(&data).unwrap());

    let tokens = data["coins"]
        .as_array()
        .ok_or_else(|| Error::Parse("Invalid trending data format".to_string()))?
        .iter()
        .filter_map(|coin| {
            let item = &coin["item"];
            debug!("Processing trending token: {}", serde_json::to_string_pretty(item).unwrap());
            
            let price_change = item["data"]["price_change_percentage_24h"]["usd"]
                .as_f64()
                .unwrap_or_default();
            
            // Debug volume data
            debug!("Volume data for {}: raw_volume={}, volume_btc={}", 
                item["symbol"],
                item["data"]["total_volume"].as_str().unwrap_or("none"),
                item["data"]["total_volume_btc"].as_f64().unwrap_or_default()
            );

            let volume = if let Some(vol_str) = item["data"]["total_volume"].as_str() {
                // Remove "$" and "," from string and parse
                vol_str.trim_start_matches('$')
                    .replace(',', "")
                    .parse::<f64>()
                    .ok()
            } else {
                None
            };

            debug!("Parsed volume for {}: {:?}", item["symbol"], volume);

            Some(TrendingToken {
                id: item["id"].as_str()?.to_string(),
                name: item["name"].as_str()?.to_string(),
                symbol: item["symbol"].as_str()?.to_string(),
                market_cap_rank: item["market_cap_rank"].as_u64()? as usize,
                price_usd: item["price_btc"].as_f64()? * 30000.0,
                price_change_24h: price_change,
                volume_24h: volume,  // Use parsed volume
            })
        })
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        Err(Error::Parse("No trending tokens found".to_string()))
    } else {
        Ok(tokens)
    }
}

fn parse_market_data(data: &Value) -> Result<Vec<TokenMetrics>, Error> {
    let tokens = data.as_array()
        .ok_or_else(|| Error::Parse("Invalid market data format".to_string()))?
        .iter()
        .filter_map(|token| {
            Some(TokenMetrics {
                id: token["id"].as_str()?.to_string(),
                name: token["name"].as_str()?.to_string(),
                symbol: token["symbol"].as_str()?.to_string(),
                price_usd: token["current_price"].as_f64()?,
                price_change_24h: token["price_change_percentage_24h"].as_f64()?,
                volume_24h: token["total_volume"].as_f64()?,
                market_cap: token["market_cap"].as_f64()?,
                market_cap_rank: token["market_cap_rank"].as_u64().map(|r| r as usize),
                latest_news: None,
                chain: None,
                last_updated: Utc::now(),
            })
        })
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        Err(Error::Parse("No market data found".to_string()))
    } else {
        Ok(tokens)
    }
}

// Don't re-export internal API modules
 