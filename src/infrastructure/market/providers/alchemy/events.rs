//! Event publishing for pool discovery

use super::types::UniswapV3Pool;
use crate::events::PoolDiscoveryData;
use crate::infrastructure::errors::Result;
use crate::EventRouter;
use log::{debug, info};
use std::sync::Arc;

/// Convert Alchemy pools to event format for cache population
pub(super) fn convert_pools_to_event_data(pools: &[UniswapV3Pool]) -> Vec<PoolDiscoveryData> {
    pools
        .iter()
        .map(|pool| PoolDiscoveryData {
            pool_address: pool.id.clone(),
            token0_address: pool.token0.id.clone(),
            token0_symbol: pool.token0.symbol.clone(),
            token0_name: pool.token0.name.clone(),
            token0_decimals: pool.token0.decimals.clone(),
            token1_address: pool.token1.id.clone(),
            token1_symbol: pool.token1.symbol.clone(),
            token1_name: pool.token1.name.clone(),
            token1_decimals: pool.token1.decimals.clone(),
            fee_tier: pool.fee_tier.clone(),
            liquidity: pool.liquidity.clone(),
            sqrt_price: pool.sqrt_price.clone(),
            tick: pool.tick.clone(),
            tvl_usd: pool.tvl_usd.clone(),
            volume_24h_usd: pool.volume_usd.clone(), // Already normalized to 24h volume
        })
        .collect()
}

/// Publish pool discovery events if event router is available
pub(super) async fn publish_pool_events(
    pools: &[UniswapV3Pool],
    discovery_mode: &str,
    event_router: &Option<Arc<EventRouter>>,
) -> Result<()> {
    if let Some(ref event_router) = event_router {
        if !pools.is_empty() {
            let pool_data = convert_pools_to_event_data(pools);
            let event = crate::events::Event::Market(crate::events::MarketEvent::PoolsDiscovered {
                pools: pool_data,
                source: "Alchemy Uniswap V3".to_string(),
                discovery_mode: discovery_mode.to_string(),
                timestamp: chrono::Utc::now(),
            });

            debug!(
                "📡 Publishing pool discovery event: {} pools from {}",
                pools.len(),
                discovery_mode
            );
            event_router.publish(event).await?;
            info!(
                "✅ Published {} pool discoveries via EventRouter",
                pools.len()
            );
        }
    } else {
        debug!("📡 No EventRouter configured - skipping pool event publishing");
    }
    Ok(())
}
