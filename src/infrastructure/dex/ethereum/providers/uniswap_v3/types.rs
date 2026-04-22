//! Type definitions for Uniswap V3 operations

use crate::infrastructure::dex::TransactionPriority;
use ethers::types::{Address, U256};
use serde::{Deserialize, Serialize};

/// Pool data from market data providers (used for pool cache population)
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
    pub volume_usd: String,
    #[serde(rename = "totalValueLockedUSD")]
    pub tvl_usd: String,
    pub tick: Option<String>,
}

/// Token data within a V3 pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V3Token {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub decimals: String,
}

/// Uniswap V3 fee tiers in basis points
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum V3FeeTier {
    Low = 100,       // 0.01%
    Medium = 500,    // 0.05%
    Standard = 3000, // 0.3%
    High = 10000,    // 1%
}

/// Parameters for executing a swap transaction
#[derive(Debug, Clone)]
pub struct SwapParams {
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub amount_out_minimum: U256,
    pub to: Address,
    pub deadline: U256,
    pub priority: TransactionPriority,
    pub gas_limit: Option<u64>,
    pub trade_size_usd: Option<f64>,
}

/// Gas estimation results for swap and wrap execution
#[derive(Debug, Clone)]
pub struct GasEstimate {
    pub gas_limit: U256,
    pub gas_price_wei: U256,
    pub gas_price_gwei: f64,
    pub estimated_cost_eth: f64,
    pub estimated_cost_usd: f64,
}

/// V3 Pool information (enhanced with Alchemy market data)
#[derive(Debug, Clone)]
pub struct PoolInfo {
    pub pool_address: Address,
    pub token0: Address,
    pub token1: Address,
    pub fee_tier: V3FeeTier,
    pub liquidity: U256,
    pub tvl_usd: f64,
    pub volume_24h_usd: f64,
    pub sqrt_price: Option<U256>,
    pub tick: Option<i32>,
}

/// Uniswap V3 ExactInputSingle parameters for SwapRouter contract
#[derive(Debug, Clone)]
pub(crate) struct ExactInputSingleParams {
    pub token_in: Address,
    pub token_out: Address,
    pub fee: u32,
    pub recipient: Address,
    pub deadline: U256,
    pub amount_in: U256,
    pub amount_out_minimum: U256,
    pub sqrt_price_limit_x96: U256,
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Format Ethereum address for logging (first 8 characters)
///
/// Used consistently across Uniswap V3 modules for concise log output
pub(super) fn format_address(addr: Address) -> String {
    format!("{:?}", addr)[0..8].to_string()
}
