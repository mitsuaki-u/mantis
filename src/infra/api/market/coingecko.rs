use crate::core::error::{Error, Result};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

// Import necessary items from parent or sibling modules
use super::providers::{MarketDataEvent, MarketDataProvider};
use crate::core::models::market::TokenMetrics;
use crate::core::models::token::TokenData;
use crate::infra::api::adapters;

const BASE_URL: &str = "https://api.coingecko.com/api/v3";
const RETRY_DELAY: u64 = 30; // Delay in seconds before retrying after rate limit

// Provider Struct Definition
pub struct CoinGeckoProvider {
    api_key: Option<String>,
    client: Client,
}

impl CoinGeckoProvider {
    pub fn new(api_key: Option<String>) -> Self {
        info!("📊 Initializing CoinGecko market data provider");
        if api_key.is_some() {
            info!("   • API Key: configured");
        } else {
            info!("   • API Key: not configured (using public API)");
        }
        Self {
            api_key,
            client: Client::builder()
                .timeout(Duration::from_secs(10)) // Example timeout
                .user_agent("honeybadger-trading-bot/0.1.0") // Example user agent
                .build()
                .unwrap_or_else(|e| {
                    error!("Failed to build HTTP client for CoinGeckoProvider: {}", e);
                    Client::new()
                }),
        }
    }
}

#[async_trait]
impl MarketDataProvider for CoinGeckoProvider {
    fn name(&self) -> &str {
        "CoinGecko"
    }

    async fn get_market_data(
        &self,
        wide_scan: bool,
        _tokens_to_track: &[String],
    ) -> Result<Vec<TokenMetrics>> {
        // Determine the limit based on wide_scan flag
        let limit = if wide_scan {
            info!("CoinGeckoProvider: Wide scan enabled, fetching top 250 tokens.");
            250 // Fetch more tokens for wide scan
        } else {
            info!("CoinGeckoProvider: Wide scan disabled, fetching top 100 tokens.");
            100 // Default limit for standard scan
        };

        // Call the internal function with the determined limit
        let raw_data_value = get_market_data_internal(
            limit,
            &self.client,
            self.api_key.as_ref().map(|s| s.as_str()),
        )
        .await?;

        // Parse the response (filtering happens later in MarketDataActor if needed)
        let token_data_list: Vec<TokenData> = raw_data_value
            .as_array()
            .ok_or_else(|| {
                Error::Parse("CoinGecko market data response was not an array".to_string())
            })?
            .iter()
            .filter_map(|coin_json_value| {
                match adapters::convert_coingecko_to_token_data(coin_json_value) {
                    Ok(td) => Some(td),
                    Err(e) => {
                        warn!("Failed to convert CoinGecko data for a token: {}", e);
                        None
                    }
                }
            })
            .collect();

        let metrics: Vec<TokenMetrics> = token_data_list
            .iter() // Iterate over &TokenData
            .map(TokenMetrics::from) // Equivalent to .map(|td| TokenMetrics::from(td))
            .collect();

        debug!(
            "Successfully fetched and parsed {} token metrics from CoinGecko (limit: {})",
            metrics.len(),
            limit
        );
        Ok(metrics)
    }

    async fn connect_websocket(
        &self,
        _tokens: Vec<String>,
        _sender: mpsc::Sender<MarketDataEvent>,
    ) -> Result<()> {
        warn!("CoinGecko provider does not support WebSocket.");
        Err(Error::NotImplemented(
            "CoinGecko WebSocket not supported".to_string(),
        ))
    }

    async fn disconnect_websocket(&self) -> Result<()> {
        Ok(())
    }

    fn supports_websocket(&self) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_box(&self) -> Box<dyn MarketDataProvider> {
        Box::new(CoinGeckoProvider::new(self.api_key.clone()))
    }
}

// Existing free functions - modified to accept client and api_key if necessary
// Renamed original get_market_data to get_market_data_internal to avoid conflict if it was public
// and used elsewhere directly, assuming provider's get_market_data is the main entry now.
// Or, these free functions could be private helpers (mod visibility) if only used by the provider.

async fn get_market_data_internal(
    limit: usize,
    client: &Client,
    _api_key: Option<&str>,
) -> Result<Value> {
    // Added client and api_key params
    debug!(
        "Fetching market data for top {} tokens from CoinGecko",
        limit
    );

    let url = format!(
        "{}/coins/markets?vs_currency=usd&order=market_cap_desc&per_page={}&page=1&sparkline=false&price_change_percentage=24h",
        BASE_URL, limit
        // Example of how an API key might be used if CoinGecko had a common way (they usually use a header or query param for Pro API)
        // api_key.map_or_else(String::new, |k| format!("&x_cg_demo_api_key={}", k))
    );
    debug!("CoinGecko API URL: {}", url);

    make_request_internal(&url, client).await // Pass client to make_request_internal
}

pub async fn get_trending() -> Result<Value> {
    // Kept as public, might be used by other parts (e.g. overview)
    debug!("Fetching trending tokens from CoinGecko");
    let url = format!("{}/search/trending", BASE_URL);
    // For trending, usually no separate client instance needed per call, but if we want to unify client usage:
    let client = Client::builder()
        .user_agent("honeybadger-trading-bot/0.1.0")
        .build()
        .map_err(|e| Error::Network(format!("Failed to build client for get_trending: {}", e)))?;
    debug!("CoinGecko API URL: {}", url);

    make_request_internal(&url, &client).await
}

// Renamed original make_request to make_request_internal
async fn make_request_internal(url: &str, client: &Client) -> Result<Value> {
    // Added client param
    // Client is now passed in, no need to build it here.
    // let client = Client::builder()
    //     .user_agent("Mozilla/5.0") // Consider a more specific user agent for your bot
    //     .build()?;

    let response = client
        .get(url)
        .header("accept", "application/json")
        // If CoinGecko Pro API key is used, it's often a header:
        // .header("X-Cg-Pro-Api-Key", api_key.unwrap_or_default())
        .send()
        .await?;

    match response.status() {
        reqwest::StatusCode::OK => {
            let data = response.json().await?;
            Ok(data)
        }
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            warn!(
                "CoinGecko rate limit exceeded, waiting {} seconds before retry...",
                RETRY_DELAY
            );
            // Note: Retrying here might be better handled by a global retry mechanism if calls are frequent.
            // For simplicity, keeping local retry sleep.
            sleep(Duration::from_secs(RETRY_DELAY)).await;
            // It might be better to return the RateLimit error and let the caller decide on retry.
            Err(Error::RateLimit(
                "CoinGecko rate limit exceeded".to_string(),
            ))
        }
        status => {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            error!("CoinGecko API error: {} - {}", status, error_text);
            Err(Error::Api(format!(
                "CoinGecko API error: {} - {}",
                status, error_text
            )))
        }
    }
}

// Original public get_market_data function - if it was intended to be callable directly
// without a provider instance, it needs to be kept or its callers updated.
// For now, I've made the provider use `get_market_data_internal`.
// If this public one is still needed, it would create its own client.
pub async fn get_market_data(limit: usize) -> Result<Value> {
    debug!("Fetching market data (public fn) for top {} tokens", limit);
    let client = Client::builder()
        .user_agent("honeybadger-trading-bot/0.1.0")
        .build()
        .map_err(|e| {
            Error::Network(format!("Failed to build client for get_market_data: {}", e))
        })?;
    get_market_data_internal(limit, &client, None).await // Assuming no API key for this public variant
}
