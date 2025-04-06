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

const BINANCE_API_URL: &str = "https://api.binance.com/api/v3";
const BINANCE_WS_URL: &str = "wss://stream.binance.com:9443/ws";

pub struct BinanceProvider {
    api_key: Option<String>,
    api_secret: Option<String>,
    client: Client,
    ws_connection: Arc<RwLock<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    is_connected: Arc<RwLock<bool>>,
    token_symbols: Arc<RwLock<HashMap<String, String>>>, // Maps token_id to binance symbol
}

#[derive(Deserialize)]
struct BinanceTickerResponse {
    symbol: String,
    price: String,
}

#[derive(Deserialize)]
struct Binance24hrResponse {
    symbol: String,
    priceChange: String,
    priceChangePercent: String,
    weightedAvgPrice: String,
    lastPrice: String,
    volume: String,
    quoteVolume: String,
}

#[derive(Serialize)]
struct BinanceWsSubscription {
    method: String,
    params: Vec<String>,
    id: i32,
}

#[derive(Deserialize)]
struct BinanceWsTickerEvent {
    e: String, // Event type
    E: u64,    // Event time
    s: String, // Symbol
    p: String, // Price change
    P: String, // Price change percent
    c: String, // Last price
    Q: String, // Last quantity
    v: String, // Total traded base asset volume
    q: String, // Total traded quote asset volume
}

impl BinanceProvider {
    pub fn new(api_key: Option<String>, api_secret: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|e| {
                error!("Failed to build HTTP client: {}", e);
                Client::new()
            });

        Self {
            api_key,
            api_secret,
            client,
            ws_connection: Arc::new(RwLock::new(None)),
            is_connected: Arc::new(RwLock::new(false)),
            token_symbols: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // Helper method to map token IDs to Binance symbols
    async fn update_token_symbols(&self, tokens: &[String]) -> Result<(), Error> {
        let mut symbol_map = self.token_symbols.write().await;
        
        // For simplicity, we'll just map token_id -> token_id + "USDT"
        // In a real implementation, you would use a proper mapping logic or database
        for token in tokens {
            let symbol = format!("{}USDT", token.to_uppercase());
            symbol_map.insert(token.clone(), symbol);
        }
        
        Ok(())
    }
    
    async fn connect_ws(&self, tokens: Vec<String>) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, Error> {
        // Update token symbols mapping
        self.update_token_symbols(&tokens).await?;
        
        // Get symbols for WebSocket subscription
        let symbols = {
            let symbol_map = self.token_symbols.read().await;
            tokens.iter()
                .filter_map(|token| symbol_map.get(token).cloned())
                .collect::<Vec<_>>()
        };
        
        // Create streams for miniTicker
        let streams = symbols.iter()
            .map(|symbol| format!("{}@miniTicker", symbol.to_lowercase()))
            .collect::<Vec<_>>();
            
        debug!("Connecting to Binance WebSocket at {}", BINANCE_WS_URL);
        
        // Connect to WebSocket
        let url = format!("{}", BINANCE_WS_URL);
        let (mut ws_stream, _) = connect_async(&url).await
            .map_err(|e| Error::Network(format!("Failed to connect to Binance WebSocket: {}", e)))?;
            
        // Subscribe to streams
        let subscription = BinanceWsSubscription {
            method: "SUBSCRIBE".to_string(),
            params: streams,
            id: 1,
        };
        
        let subscription_msg = serde_json::to_string(&subscription)
            .map_err(|e| Error::Parse(format!("Failed to serialize subscription message: {}", e)))?;
            
        ws_stream.send(Message::Text(subscription_msg)).await
            .map_err(|e| Error::Network(format!("Failed to send subscription message: {}", e)))?;
            
        info!("Connected to Binance WebSocket and subscribed to {} symbols", symbols.len());
        
        Ok(ws_stream)
    }
    
    async fn process_ws_message(&self, msg: Message, sender: &mpsc::Sender<MarketDataEvent>) -> Result<(), Error> {
        match msg {
            Message::Text(text) => {
                // Parse message
                let json: Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(e) => {
                        error!("Failed to parse Binance WebSocket message: {}", e);
                        return Ok(());
                    }
                };
                
                // Check if it's a ticker event
                if json["e"].as_str() == Some("24hrMiniTicker") {
                    let symbol = match json["s"].as_str() {
                        Some(s) => s,
                        None => {
                            error!("Missing symbol in Binance WebSocket message");
                            return Ok(());
                        }
                    };
                    
                    let price_str = match json["c"].as_str() {
                        Some(p) => p,
                        None => {
                            error!("Missing price in Binance WebSocket message");
                            return Ok(());
                        }
                    };
                    
                    let volume_str = match json["v"].as_str() {
                        Some(v) => v,
                        None => {
                            error!("Missing volume in Binance WebSocket message");
                            return Ok(());
                        }
                    };
                    
                    // Convert to token_id from symbol
                    let token_id = {
                        // Remove USDT suffix
                        let token_symbol = if symbol.ends_with("USDT") {
                            symbol[..symbol.len() - 4].to_string()
                        } else {
                            symbol.to_string()
                        };
                        
                        // In a real implementation, you would look up token_id from a mapping
                        token_symbol.to_lowercase()
                    };
                    
                    // Parse price and volume
                    let price = match price_str.parse::<f64>() {
                        Ok(p) => p,
                        Err(e) => {
                            error!("Failed to parse price for {}: {}", symbol, e);
                            return Ok(());
                        }
                    };
                    
                    let volume = match volume_str.parse::<f64>() {
                        Ok(v) => v,
                        Err(e) => {
                            error!("Failed to parse volume for {}: {}", symbol, e);
                            return Ok(());
                        }
                    };
                    
                    // Create and send price update event
                    let event = MarketDataEvent::PriceUpdate {
                        token_id,
                        price,
                        volume: Some(volume),
                        change_24h: None, // Not available in miniTicker
                        timestamp: Utc::now(),
                    };
                    
                    if let Err(e) = sender.send(event).await {
                        error!("Failed to send market data event: {}", e);
                    }
                }
                
                Ok(())
            },
            Message::Binary(_) => {
                debug!("Received binary message from Binance WebSocket");
                Ok(())
            },
            Message::Ping(data) => {
                debug!("Received ping from Binance WebSocket");
                // For binance, we need to respond with pong
                if let Some(ws_stream) = self.ws_connection.write().await.as_mut() {
                    ws_stream.send(Message::Pong(data)).await
                        .map_err(|e| Error::Network(format!("Failed to send pong: {}", e)))?;
                }
                Ok(())
            },
            Message::Pong(_) => {
                debug!("Received pong from Binance WebSocket");
                Ok(())
            },
            Message::Close(_) => {
                info!("Binance WebSocket connection closed");
                *self.is_connected.write().await = false;
                Err(Error::Network("WebSocket connection closed".to_string()))
            },
            Message::Frame(_) => {
                debug!("Received frame from Binance WebSocket");
                Ok(())
            },
        }
    }
}

// Implement Clone for BinanceProvider
impl Clone for BinanceProvider {
    fn clone(&self) -> Self {
        BinanceProvider {
            api_key: self.api_key.clone(),
            api_secret: self.api_secret.clone(),
            client: Client::builder().timeout(Duration::from_secs(10))
                .build().unwrap_or_else(|_| Client::new()),
            ws_connection: Arc::new(RwLock::new(None)),
            is_connected: Arc::new(RwLock::new(false)),
            token_symbols: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl MarketDataProvider for BinanceProvider {
    fn name(&self) -> &str {
        "Binance"
    }
    
    async fn get_market_data(&self) -> Result<Vec<TokenMetrics>, Error> {
        // First get list of USDT trading pairs
        let url = format!("{}/ticker/price", BINANCE_API_URL);
        
        let mut request = self.client.get(&url);
        if let Some(api_key) = &self.api_key {
            request = request.header("X-MBX-APIKEY", api_key);
        }
        
        let response = request.send().await
            .map_err(|e| Error::Network(format!("Failed to fetch Binance price data: {}", e)))?;
            
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "No error message".to_string());
            return Err(Error::Api(format!("Binance API error ({}): {}", status, error_text)));
        }
        
        let ticker_data: Vec<BinanceTickerResponse> = response.json().await
            .map_err(|e| Error::Parse(format!("Failed to parse Binance ticker response: {}", e)))?;
            
        // Filter for USDT pairs and extract token symbols
        let usdt_pairs: Vec<BinanceTickerResponse> = ticker_data.into_iter()
            .filter(|ticker| ticker.symbol.ends_with("USDT"))
            .collect();
            
        // Get 24hr statistics for these pairs to get volume and price change
        let symbols = usdt_pairs.iter()
            .map(|ticker| ticker.symbol.clone())
            .collect::<Vec<_>>();
            
        let mut token_metrics = Vec::new();
        
        // Fetch 24hr data in batches to avoid rate limits
        for chunk in symbols.chunks(20) {
            let symbols_query = chunk.join(",");
            let url = format!("{}/ticker/24hr?symbols={}", BINANCE_API_URL, symbols_query);
            
            let mut request = self.client.get(&url);
            if let Some(api_key) = &self.api_key {
                request = request.header("X-MBX-APIKEY", api_key);
            }
            
            let response = request.send().await
                .map_err(|e| Error::Network(format!("Failed to fetch Binance 24hr data: {}", e)))?;
                
            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_else(|_| "No error message".to_string());
                return Err(Error::Api(format!("Binance API error ({}): {}", status, error_text)));
            }
            
            let hr24_data: Vec<Binance24hrResponse> = response.json().await
                .map_err(|e| Error::Parse(format!("Failed to parse Binance 24hr response: {}", e)))?;
                
            // Convert to TokenMetrics
            for data in hr24_data {
                let symbol = if data.symbol.ends_with("USDT") {
                    data.symbol[..data.symbol.len() - 4].to_string()
                } else {
                    data.symbol.clone()
                };
                
                let price = data.lastPrice.parse::<f64>().unwrap_or(0.0);
                let volume = data.volume.parse::<f64>().unwrap_or(0.0);
                let price_change = data.priceChangePercent.parse::<f64>().unwrap_or(0.0);
                
                // Create token metrics
                token_metrics.push(TokenMetrics {
                    id: symbol.to_lowercase(),
                    symbol: symbol.clone(),
                    name: symbol.clone(), // In a real implementation, you would get the actual name
                    price_usd: price,
                    volume_24h: volume,
                    market_cap: 0.0, // Not available from Binance
                    price_change_24h: price_change,
                    market_cap_rank: None, // Not available from Binance
                    latest_news: None, // Not available from Binance
                    chain: None, // Would need to be determined separately
                    last_updated: chrono::Utc::now(),
                });
            }
            
            // Be nice to Binance API and add a small delay between batches
            if chunk.len() < symbols.len() {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        
        Ok(token_metrics)
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
                            let provider = BinanceProvider::new(None, None);
                            if let Err(e) = provider.process_ws_message(msg, &sender).await {
                                error!("Error processing Binance WebSocket message: {}", e);
                                break;
                            }
                        },
                        Some(Err(e)) => {
                            error!("Error receiving Binance WebSocket message: {}", e);
                            break;
                        },
                        None => {
                            info!("Binance WebSocket stream ended");
                            break;
                        }
                    }
                }
            }
            
            *is_connected.write().await = false;
            debug!("Binance WebSocket processing task ended");
        });
        
        Ok(())
    }
    
    async fn disconnect_websocket(&self) -> Result<(), Error> {
        let mut ws_connection = self.ws_connection.write().await;
        if let Some(ws_stream) = ws_connection.as_mut() {
            // Send close message
            let close_msg = Message::Close(None);
            if let Err(e) = ws_stream.send(close_msg).await {
                error!("Error sending close message to Binance WebSocket: {}", e);
            }
            
            // Set connected status to false
            *self.is_connected.write().await = false;
            
            // Remove the connection
            *ws_connection = None;
            
            info!("Disconnected from Binance WebSocket");
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