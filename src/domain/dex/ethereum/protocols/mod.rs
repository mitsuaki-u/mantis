use async_trait::async_trait;
use ethers::types::{Address, U256};

use crate::core::error::Result;
use crate::domain::dex::{TransactionDetails, TransactionPriority};

pub mod uniswap_v2;

// Re-export protocol implementations
pub use uniswap_v2::UniswapV2Protocol;

#[derive(Debug, Clone)]
pub struct SwapParams {
    pub token_in: Address,
    pub token_out: Address,
    pub amount_in: U256,
    pub amount_out_min: U256,
    pub to: Address,
    pub deadline: U256,
    pub slippage_tolerance: f64,
    pub priority: TransactionPriority,
}

/// Common trait for all DEX protocols
#[async_trait]
pub trait DexProtocol: Send + Sync {
    /// Get a quote for swapping tokens
    async fn get_quote(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> Result<U256>;

    /// Execute a token swap
    async fn execute_swap(&self, params: SwapParams) -> Result<TransactionDetails>;

    /// Get the pair address for two tokens
    async fn get_pair_address(&self, token_a: Address, token_b: Address) -> Result<Address>;

    /// Get reserves for a trading pair
    async fn get_reserves(&self, pair_address: Address) -> Result<(U256, U256)>;

    /// Get the router contract address
    fn get_router_address(&self) -> Address;

    /// Get the factory contract address
    fn get_factory_address(&self) -> Address;

    /// Get the protocol name
    fn get_protocol_name(&self) -> &'static str;
}

/// Common utilities for protocol implementations
pub struct ProtocolUtils;

impl ProtocolUtils {
    /// Calculate deadline timestamp (current time + 20 minutes)
    pub fn calculate_deadline() -> U256 {
        let now = chrono::Utc::now().timestamp() as u64;
        U256::from(now + 1200) // 20 minutes from now
    }

    /// Calculate minimum amount out based on slippage tolerance
    pub fn calculate_amount_out_min(amount_out: U256, slippage_tolerance: f64) -> U256 {
        let slippage_factor = 1.0 - (slippage_tolerance / 100.0);
        let amount_out_f64 = amount_out.as_u128() as f64;
        let min_amount = amount_out_f64 * slippage_factor;
        U256::from(min_amount as u128)
    }

    /// Create a trading path between two tokens
    pub fn create_path(token_in: Address, token_out: Address) -> Vec<Address> {
        vec![token_in, token_out]
    }

    /// Create a trading path through WETH
    pub fn create_path_through_weth(
        token_in: Address,
        token_out: Address,
        weth_address: Address,
    ) -> Vec<Address> {
        if token_in == weth_address || token_out == weth_address {
            vec![token_in, token_out]
        } else {
            vec![token_in, weth_address, token_out]
        }
    }
}
