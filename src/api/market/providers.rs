use crate::error::Error;
use crate::types::market::TokenMetrics;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use std::any::Any;

/// Events emitted by market data providers
#[derive(Debug, Clone)]
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
    fn as_any(&self) -> &dyn Any;
    
    /// Clone the provider
    fn clone_box(&self) -> Box<dyn MarketDataProvider>;
} 