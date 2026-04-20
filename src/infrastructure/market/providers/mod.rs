pub mod alchemy;
pub mod queries;
pub mod traits;

use crate::config::Config;
pub use alchemy::{UniswapV3Pool, V3Token};
use log::debug;
pub use traits::MarketDataProvider;

/// Factory function to create a specific market data provider based on config.
pub async fn create_market_api(config: &Config) -> Box<dyn MarketDataProvider> {
    // Check the configured market data provider
    let market_data_provider = &config.trading.market_data_provider;

    debug!("Creating market data provider: '{}'", market_data_provider);

    match market_data_provider.to_lowercase().as_str() {
        "alchemy_uniswap_v3" => {
            debug!("Selected Alchemy Uniswap V3 method - using Alchemy's enhanced V3 subgraph");
            create_alchemy_uniswap_v3_provider(config).await
        }
        _ => {
            debug!(
                "Unknown market data provider '{}', defaulting to Alchemy Uniswap V3",
                market_data_provider
            );
            create_alchemy_uniswap_v3_provider(config).await
        }
    }
}

/// Create a Uniswap V3 subgraph market data provider
async fn create_alchemy_uniswap_v3_provider(config: &Config) -> Box<dyn MarketDataProvider> {
    let network = config.dex.network.as_deref().unwrap_or("ethereum");

    let subgraph_url = config.dex.subgraph_url.clone().unwrap_or_else(|| {
        panic!(
            "No subgraph_url configured. \
            Get a free Uniswap V3 subgraph endpoint at https://app.satsuma.xyz \
            and set dex.subgraph_url in your config file, or run:\n  \
            mantis config set dex.subgraph_url YOUR_SUBGRAPH_URL"
        )
    });

    let subgraph_api_key = config.dex.subgraph_api_key.clone();

    debug!("Creating Uniswap V3 provider for network: {}", network);

    Box::new(alchemy::AlchemyUniswapV3Provider::new(
        network,
        subgraph_url,
        subgraph_api_key,
        config.trading.clone(),
        crate::infrastructure::constants::DEFAULT_TIMEOUT_SECS,
    ))
}
