use crate::error::Error;
use crate::types::market::TokenMetrics;
use super::{MarketDataProvider, MarketDataEvent};
use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, WebSocketStream, MaybeTlsStream};
use futures_util::{StreamExt, SinkExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use log::{info, error, debug, warn};
use std::time::Duration;
use chrono::Utc;
use reqwest::Client;

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
    timestamp: u64,
}

#[derive(Deserialize)]
struct CoinCapAsset {
    id: String,
    rank: String,
    symbol: String,
    name: String,
    supply: String,
    #[serde(rename = "maxSupply")]
    max_supply: Option<String>,
    #[serde(rename = "marketCapUsd")]
    market_cap_usd: String,
    #[serde(rename = "volumeUsd24Hr")]
    volume_usd_24h: String,
    #[serde(rename = "priceUsd")]
    price_usd: String,
    #[serde(rename = "changePercent24Hr")]
    change_percent_24h: String,
    #[serde(rename = "vwap24Hr")]
    vwap_24h: Option<String>,
}

impl CoinCapProvider {
    pub fn new(api_key: Option<String>) -> Self {
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

    async fn connect_ws(&self, tokens: Vec<String>) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
        let assets = tokens.join(",");
        let url = format!("{}{}", COINCAP_WS_URL, assets);
        
        debug!("Connecting to CoinCap WebSocket at {}", url);
        
        let (ws_stream, _) = connect_async(url).await
            .map_err(|e| Error::Network(format!("Failed to connect to CoinCap WebSocket: {}", e)))?;
        
        info!("Connected to CoinCap WebSocket");
        
        Ok(ws_stream)
    }
    
    async fn process_ws_message(&self, msg: Message, sender: &mpsc::Sender<MarketDataEvent>) -> Result<(), Error> {
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
                        },
                        Err(e) => {
                            error!("Failed to parse price for {}: {}", token_id, e);
                        }
                    }
                }
                
                Ok(())
            },
            Message::Binary(_) => {
                debug!("Received binary message from CoinCap WebSocket");
                Ok(())
            },
            Message::Ping(_) => {
                debug!("Received ping from CoinCap WebSocket");
                Ok(())
            },
            Message::Pong(_) => {
                debug!("Received pong from CoinCap WebSocket");
                Ok(())
            },
            Message::Close(_) => {
                info!("CoinCap WebSocket connection closed");
                *self.is_connected.write().await = false;
                Err(Error::Network("WebSocket connection closed".to_string()))
            },
            Message::Frame(_) => {
                debug!("Received frame from CoinCap WebSocket");
                Ok(())
            },
        }
    }
}

#[async_trait]
impl MarketDataProvider for CoinCapProvider {
    fn name(&self) -> &str {
        "CoinCap"
    }
    
    async fn get_market_data(&self) -> Result<Vec<TokenMetrics>, Error> {
        let url = format!("{}/assets", COINCAP_API_URL);
        
        let mut request = self.client.get(&url);
        if let Some(api_key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }
        
        let response = request.send().await
            .map_err(|e| Error::Network(format!("Failed to fetch CoinCap market data: {}", e)))?;
            
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "No error message".to_string());
            return Err(Error::Api(format!("CoinCap API error ({}): {}", status, error_text)));
        }
        
        let response_text = response.text().await
            .map_err(|e| Error::Network(format!("Failed to get response text: {}", e)))?;
            
        let coincap_response: CoinCapResponse = serde_json::from_str(&response_text)
            .map_err(|e| Error::Parse(format!("Failed to parse CoinCap response: {}", e)))?;
            
        let tokens = coincap_response.data.into_iter()
            .filter_map(|asset| {
                let price = asset.price_usd.parse::<f64>().ok()?;
                let volume = asset.volume_usd_24h.parse::<f64>().ok()?;
                let market_cap = asset.market_cap_usd.parse::<f64>().ok()?;
                let change_24h = asset.change_percent_24h.parse::<f64>().ok()?;
                
                Some(TokenMetrics {
                    id: asset.id,
                    symbol: asset.symbol,
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
            
        Ok(tokens)
    }
    
    async fn connect_websocket(&self, tokens: Vec<String>, sender: mpsc::Sender<MarketDataEvent>) -> Result<(), Error> {
        // If already connected, disconnect first
        if *self.is_connected.read().await {
            self.disconnect_websocket().await?;
        }
        
        let ws_stream = self.connect_ws(tokens).await?;
        *self.ws_connection.write().await = Some(ws_stream);
        *self.is_connected.write().await = true;
        
        // Clone necessary Arc references
        let is_connected = self.is_connected.clone();
        let ws_connection = self.ws_connection.clone();
        
        // Spawn a task to process WebSocket messages
        tokio::spawn(async move {
            let mut ws_stream_guard = ws_connection.write().await;
            if let Some(ws_stream) = ws_stream_guard.as_mut() {
                while *is_connected.read().await {
                    match ws_stream.next().await {
                        Some(Ok(msg)) => {
                            let provider = CoinCapProvider::new(None);
                            if let Err(e) = provider.process_ws_message(msg, &sender).await {
                                error!("Error processing CoinCap WebSocket message: {}", e);
                                break;
                            }
                        },
                        Some(Err(e)) => {
                            error!("Error receiving CoinCap WebSocket message: {}", e);
                            break;
                        },
                        None => {
                            info!("CoinCap WebSocket stream ended");
                            break;
                        }
                    }
                }
            }
            
            *is_connected.write().await = false;
            debug!("CoinCap WebSocket processing task ended");
        });
        
        Ok(())
    }
    
    async fn disconnect_websocket(&self) -> Result<(), Error> {
        let mut ws_connection = self.ws_connection.write().await;
        if let Some(ws_stream) = ws_connection.as_mut() {
            // Send close message
            let close_msg = Message::Close(None);
            if let Err(e) = ws_stream.send(close_msg).await {
                error!("Error sending close message to CoinCap WebSocket: {}", e);
            }
            
            // Set connected status to false
            *self.is_connected.write().await = false;
            
            // Remove the connection
            *ws_connection = None;
            
            info!("Disconnected from CoinCap WebSocket");
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