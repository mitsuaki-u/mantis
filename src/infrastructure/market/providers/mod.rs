pub mod dexscreener;
pub mod traits;

use crate::config::Config;
pub use dexscreener::DexScreenerProvider;
use log::debug;
pub use traits::MarketDataProvider;

/// Factory function to create a market data provider based on config.
pub async fn create_market_api(config: &Config) -> Box<dyn MarketDataProvider> {
    let market_data_provider = &config.trading.market_data_provider;
    debug!("Creating market data provider: '{}'", market_data_provider);
    create_dexscreener_provider(config)
}

fn create_dexscreener_provider(config: &Config) -> Box<dyn MarketDataProvider> {
    Box::new(DexScreenerProvider::new(
        config.trading.min_volume,
        config.trading.min_liquidity,
    ))
}
