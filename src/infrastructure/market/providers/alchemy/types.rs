//! Data structures for Alchemy Uniswap V3 provider

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Uniswap V3 pool day data (24h metrics)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolDayData {
    pub pool: PoolReference,
    #[serde(rename = "volumeUSD")]
    pub volume_usd: String,
    #[serde(rename = "tvlUSD")]
    pub tvl_usd: String,
    pub date: i64,
}

/// Pool reference in poolDayData
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolReference {
    pub id: String,
}

/// Response for poolDayDatas query
#[derive(Debug, Deserialize)]
pub(super) struct PoolDayDataResponse {
    pub(super) data: PoolDayDataData,
}

#[derive(Debug, Deserialize)]
pub(super) struct PoolDayDataData {
    #[serde(rename = "poolDayDatas")]
    pub(super) pool_day_datas: Vec<PoolDayData>,
}

/// Uniswap V3 pool data from Alchemy subgraph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniswapV3Pool {
    pub id: String,
    pub token0: V3Token,
    pub token1: V3Token,
    #[serde(rename = "feeTier")]
    pub fee_tier: String,
    pub liquidity: String,
    #[serde(rename = "sqrtPrice")]
    pub sqrt_price: String,
    #[serde(rename = "volumeUSD")]
    pub volume_usd: String, // Initially all-time cumulative, enriched to 24h via enrich_pools_with_24h_volume()
    #[serde(rename = "totalValueLockedUSD")]
    pub tvl_usd: String,
    pub tick: Option<String>,
}

/// V3 token data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Token {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub decimals: String,
}

/// Alchemy Uniswap V3 subgraph response
#[derive(Debug, Deserialize)]
pub(super) struct AlchemyV3Response {
    pub(super) data: AlchemyV3Data,
}

#[derive(Debug, Deserialize)]
pub(super) struct AlchemyV3Data {
    pub(super) pools: Vec<UniswapV3Pool>,
}

/// Token price data aggregated from multiple V3 pools
#[derive(Debug, Clone)]
pub(super) struct TokenPriceData {
    pub(super) address: String,
    pub(super) symbol: String,
    pub(super) name: String,
    pub(super) decimals: u8,
    pub(super) price_usd: Decimal,
    pub(super) volume_24h: Decimal,
    pub(super) liquidity_usd: Decimal,
    pub(super) pool_count: usize,
    pub(super) best_fee_tier: u32,
}
