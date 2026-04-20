use crate::core::domain::market::TokenMetrics;
use crate::infrastructure::errors::Error;
use async_trait::async_trait;
use std::any::Any;

/// Trait for market data providers
#[async_trait]
pub trait MarketDataProvider: Send + Sync + 'static {
    /// Get the name of the provider
    fn name(&self) -> &str;

    /// Get market data for tokens
    /// max_tokens_to_scan: Maximum number of tokens to fetch (0 = unlimited)
    /// tokens_to_track: Specific tokens to track (empty = auto-discovery)
    async fn get_market_data(
        &self,
        max_tokens_to_scan: usize,
        tokens_to_track: &[String],
        network: &str,
    ) -> Result<Vec<TokenMetrics>, Error>;

    /// Convert to Any for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Clone the provider
    fn clone_box(&self) -> Box<dyn MarketDataProvider>;
}
