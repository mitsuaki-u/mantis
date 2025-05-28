use super::{MarketDataEvent, MarketDataProvider};
use crate::core::error::Error;
use crate::core::models::market::TokenMetrics;
use async_trait::async_trait;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info, warn};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};

const COINCAP_API_URL: &str = "https://api.coincap.io/v2";
const COINCAP_WS_URL: &str = "wss://ws.coincap.io/prices?assets=";

pub struct CoinCapProvider {
    api_key: Option<String>,
    client: Client,
    ws_connection: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    is_connected: Arc<RwLock<bool>>,
}

#[derive(Deserialize)]
struct CoinCapResponse {
    data: Vec<CoinCapAsset>,
    _timestamp: u64,
}

#[derive(Deserialize)]
struct CoinCapAsset {
    id: String,
    _rank: String,
    symbol: String,
    name: String,
    _supply: String,
    #[serde(rename = "maxSupply")]
    _max_supply: Option<String>,
    #[serde(rename = "marketCapUsd")]
    market_cap_usd: String,
    #[serde(rename = "volumeUsd24Hr")]
    volume_usd_24h: String,
    #[serde(rename = "priceUsd")]
    price_usd: String,
    #[serde(rename = "changePercent24Hr")]
    change_percent_24h: String,
    #[serde(rename = "vwap24Hr")]
    _vwap_24h: Option<String>,
}

impl CoinCapProvider {
    pub fn new(api_key: Option<String>) -> Self {
        info!("📊 Initializing CoinCap market data provider");
        if api_key.is_some() {
            info!("   • API Key: configured");
        } else {
            info!("   • API Key: not configured (using public API)");
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|e| {
                error!("Failed to build HTTP client: {}", e);
                Client::new()
            });

        Self {
            api_key,
            client,
            ws_connection: Arc::new(RwLock::new(None)),
            is_connected: Arc::new(RwLock::new(false)),
        }
    }

    async fn connect_ws(
        &self,
        tokens: Vec<String>,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
        let assets = tokens.join(",");
        let url = format!("{}{}", COINCAP_WS_URL, assets);

        info!("🔌 Connecting to CoinCap WebSocket");
        info!("   • URL: {}", url);
        info!("   • Tokens: {}", assets);

        if self.api_key.is_none() {
            error!("❌ No CoinCap API key configured for WebSocket connection");
            return Err(Error::Api(
                "CoinCap WebSocket requires an API key".to_string(),
            ));
        }

        info!(
            "   • API key: {}",
            if self.api_key.is_some() {
                "Configured ✅"
            } else {
                "Missing ❌"
            }
        );

        // Use a custom connect options with timeout
        let connect_options =
            tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(url)
                .map_err(|e| {
                    Error::Network(format!(
                        "Failed to create request for CoinCap WebSocket: {}",
                        e
                    ))
                })?;

        // Add headers for authentication
        let mut request = connect_options;
        if let Some(key) = &self.api_key {
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {}", key)
                    .parse()
                    .map_err(|e| Error::Network(format!("Failed to create auth header: {}", e)))?,
            );
        }

        // Connect with the customized request
        let (ws_stream, response) = match connect_async(request).await {
            Ok((stream, response)) => {
                info!("✅ Connected to CoinCap WebSocket successfully");
                info!("   • Response status: {}", response.status());
                (stream, response)
            }
            Err(e) => {
                error!("❌ Failed to connect to CoinCap WebSocket: {}", e);
                return Err(Error::Network(format!(
                    "Failed to connect to CoinCap WebSocket: {}",
                    e
                )));
            }
        };

        // Check response headers for any warnings or errors
        if let Some(headers) = response.headers().get("Warning") {
            warn!("WebSocket connection warning: {:?}", headers);
        }

        if !response.status().is_success() {
            error!(
                "WebSocket connection returned non-success status: {}",
                response.status()
            );
            return Err(Error::Network(format!(
                "WebSocket connection failed with status: {}",
                response.status()
            )));
        }

        info!("🚀 WebSocket connection established");

        Ok(ws_stream)
    }

    async fn process_ws_message(
        &self,
        msg: Message,
        sender: &mpsc::Sender<MarketDataEvent>,
    ) -> Result<(), Error> {
        match msg {
            Message::Text(text) => {
                let prices: HashMap<String, String> = match serde_json::from_str(&text) {
                    Ok(map) => map,
                    Err(e) => {
                        error!("Failed to parse CoinCap WebSocket message: {}", e);
                        return Ok(());
                    }
                };

                for (token_id, price_str) in prices.iter() {
                    match price_str.parse::<f64>() {
                        Ok(price) => {
                            info!("💹 CoinCap price update: {} = ${:.2}", token_id, price);

                            let event = MarketDataEvent::PriceUpdate {
                                token_id: token_id.clone(),
                                price,
                                volume: None,
                                change_24h: None,
                                timestamp: Utc::now(),
                            };

                            if let Err(e) = sender.send(event).await {
                                error!("Failed to send market data event: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse price for {}: {}", token_id, e);
                        }
                    }
                }

                Ok(())
            }
            Message::Close(_) => {
                warn!("⚠️ CoinCap WebSocket connection closed");
                *self.is_connected.write().await = false;
                Err(Error::Network("WebSocket connection closed".to_string()))
            }
            _ => {
                debug!("Received other message type from CoinCap WebSocket");
                Ok(())
            }
        }
    }
}

#[async_trait]
impl MarketDataProvider for CoinCapProvider {
    fn name(&self) -> &str {
        "CoinCap"
    }

    async fn get_market_data(
        &self,
        _wide_scan: bool,
        _tokens_to_track: &[String],
    ) -> Result<Vec<TokenMetrics>, Error> {
        let url = format!("{}/assets", COINCAP_API_URL);

        info!("📊 Fetching market data from CoinCap API (always fetches all assets)");
        info!("   • URL: {}", url);

        let mut request = self.client.get(&url);
        if let Some(api_key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = request
            .send()
            .await
            .map_err(|e| Error::Network(format!("Failed to fetch CoinCap market data: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "No error message".to_string());
            return Err(Error::Api(format!(
                "CoinCap API error ({}): {}",
                status, error_text
            )));
        }

        let response_text = response
            .text()
            .await
            .map_err(|e| Error::Network(format!("Failed to get response text: {}", e)))?;

        let coincap_response: CoinCapResponse = serde_json::from_str(&response_text)
            .map_err(|e| Error::Parse(format!("Failed to parse CoinCap response: {}", e)))?;

        let tokens: Vec<TokenMetrics> = coincap_response
            .data
            .into_iter()
            .filter_map(|asset| {
                let price = asset.price_usd.parse::<f64>().ok()?;
                let volume = asset.volume_usd_24h.parse::<f64>().ok()?;
                let market_cap = asset.market_cap_usd.parse::<f64>().ok()?;
                let change_24h = asset.change_percent_24h.parse::<f64>().ok()?;

                Some(TokenMetrics {
                    id: asset.id.clone(),
                    symbol: asset.symbol.clone(),
                    name: asset.name,
                    price_usd: price,
                    volume_24h: volume,
                    market_cap,
                    price_change_24h: change_24h,
                    market_cap_rank: None,
                    latest_news: None,
                    chain: None,
                    last_updated: Utc::now(),
                })
            })
            .collect();

        info!(
            "✅ Successfully fetched market data for {} tokens from CoinCap",
            tokens.len()
        );

        Ok(tokens)
    }

    async fn connect_websocket(
        &self,
        tokens: Vec<String>,
        sender: mpsc::Sender<MarketDataEvent>,
    ) -> Result<(), Error> {
        // If already connected, disconnect first
        if *self.is_connected.read().await {
            info!("🔄 Disconnecting existing CoinCap WebSocket before new connection");
            self.disconnect_websocket().await?;
        }

        if tokens.is_empty() {
            return Err(Error::InvalidInput(
                "No tokens provided for WebSocket connection".to_string(),
            ));
        }

        // Validate API key is set - this will also be checked in connect_ws but we check here first
        if self.api_key.is_none() {
            return Err(Error::Api(
                "CoinCap WebSocket connection failed: No API key provided".to_string(),
            ));
        }

        info!("🔌 Initializing CoinCap WebSocket connection");
        info!("   • Tokens to track: {}", tokens.join(", "));
        info!(
            "   • API key status: {}",
            if self.api_key.is_some() {
                "Configured ✅"
            } else {
                "Missing ❌"
            }
        );

        // Connect to WebSocket with detailed error handling
        let ws_stream = match self.connect_ws(tokens).await {
            Ok(stream) => {
                info!("✅ CoinCap WebSocket connected successfully!");
                stream
            }
            Err(e) => {
                error!("❌ Failed to establish CoinCap WebSocket connection: {}", e);
                error!("   This might be due to an invalid API key or network issues");
                return Err(e);
            }
        };

        *self.ws_connection.write().await = Some(ws_stream);
        *self.is_connected.write().await = true;

        // Clone necessary Arc references
        let is_connected = self.is_connected.clone();
        let ws_connection = self.ws_connection.clone();

        info!("🚀 Starting CoinCap WebSocket message processing task");

        // Spawn a task to process WebSocket messages
        tokio::spawn(async move {
            let mut ws_stream_guard = ws_connection.write().await;
            if let Some(ws_stream) = ws_stream_guard.as_mut() {
                info!("📊 CoinCap WebSocket message processing started");

                while *is_connected.read().await {
                    // Check global shutdown flag
                    if crate::domain::trading::execution::bot::is_forced_shutdown() {
                        info!("CoinCap WebSocket: Global shutdown detected, exiting");
                        break;
                    }

                    match ws_stream.next().await {
                        Some(Ok(msg)) => {
                            debug!(
                                "📝 Received WebSocket message: {}",
                                if msg.is_text() {
                                    format!(
                                        "text message ({} bytes)",
                                        msg.to_text().map(|t| t.len()).unwrap_or(0)
                                    )
                                } else if msg.is_binary() {
                                    "binary message".to_string()
                                } else if msg.is_ping() {
                                    "ping".to_string()
                                } else if msg.is_pong() {
                                    "pong".to_string()
                                } else if msg.is_close() {
                                    "close".to_string()
                                } else {
                                    "unknown message type".to_string()
                                }
                            );

                            let provider = CoinCapProvider::new(None);
                            if let Err(e) = provider.process_ws_message(msg, &sender).await {
                                error!("Error processing CoinCap WebSocket message: {}", e);
                                // Don't break on processing errors, try the next message
                            }
                        }
                        Some(Err(e)) => {
                            error!("Error receiving CoinCap WebSocket message: {}", e);
                            error!("WebSocket connection will be terminated and reconnected");
                            break;
                        }
                        None => {
                            info!("CoinCap WebSocket stream ended");
                            break;
                        }
                    }
                }
            } else {
                error!("❌ WebSocket connection was None, cannot process messages");
            }

            *is_connected.write().await = false;
            info!("⏹️ CoinCap WebSocket processing task ended");
        });

        Ok(())
    }

    async fn disconnect_websocket(&self) -> Result<(), Error> {
        let mut ws_connection = self.ws_connection.write().await;
        if let Some(ws_stream) = ws_connection.as_mut() {
            info!("🔌 Disconnecting from CoinCap WebSocket");

            // Send close message
            let close_msg = Message::Close(None);
            if let Err(e) = ws_stream.send(close_msg).await {
                error!("Error sending close message to CoinCap WebSocket: {}", e);
            }

            // Set connected status to false
            *self.is_connected.write().await = false;

            // Remove the connection
            *ws_connection = None;

            info!("✅ Successfully disconnected from CoinCap WebSocket");
        }

        Ok(())
    }

    fn supports_websocket(&self) -> bool {
        true
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_box(&self) -> Box<dyn MarketDataProvider> {
        Box::new(self.clone())
    }
}

// Implement Clone for CoinCapProvider
impl Clone for CoinCapProvider {
    fn clone(&self) -> Self {
        Self::new(self.api_key.clone())
    }
}
