//! Token and ETH price queries for Uniswap V3

use crate::infrastructure::dex::ethereum::config::NetworkConfig;
use crate::infrastructure::errors::{Error, Result};
use ethers::{
    abi::{ethabi, Token},
    providers::Middleware,
    types::{Address, Bytes, H256, U256},
    utils::keccak256,
};
use log::{debug, info};
use rust_decimal::prelude::*;
use std::str::FromStr;
use std::sync::Arc;

use super::types::V3FeeTier;

/// Price fetcher for tokens and ETH
pub(super) struct PriceFetcher {
    network: NetworkConfig,
    rpc_provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
}

impl PriceFetcher {
    pub fn new(
        network: NetworkConfig,
        rpc_provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
    ) -> Self {
        Self {
            network,
            rpc_provider,
        }
    }

    /// Get the USD price of a token
    ///
    /// Supports:
    /// - Native currency (ETH/MATIC) - fetched from external API
    /// - Stablecoin (USDC) - returns $1.00
    /// - Other tokens - requires Alchemy market data (not yet implemented)
    pub async fn get_token_price_usd(&self, token_address: &str) -> Result<Decimal> {
        let token_addr = Address::from_str(token_address)
            .map_err(|e| Error::Parse(format!("Invalid token address: {}", e)))?;

        // Special case for native currency (ETH/MATIC)
        if token_address.to_lowercase() == "eth"
            || token_address.to_lowercase() == "ethereum"
            || token_addr == self.network.weth_address
        {
            return self.get_native_price_usd().await;
        }

        // Special case for stablecoin (USDC)
        if token_addr == self.network.stablecoin_address {
            return Ok(Decimal::new(1, 0)); // $1.00
        }

        // Price data must come from Alchemy's market data provider
        // (Direct pool price calculation from sqrt_price_x96 not yet implemented)
        Err(Error::Trading(format!(
            "No price data available for token {} in Alchemy cache. Ensure token is discovered via Alchemy.",
            token_address
        )))
    }

    /// Get the USD price of the native currency (ETH/MATIC)
    ///
    /// Calculates ETH price from WETH/USDC or WETH/USDT Uniswap V3 pools on-chain.
    /// This ensures consistency with market data and eliminates external API dependencies.
    pub async fn get_native_price_usd(&self) -> Result<Decimal> {
        let eth_price = self.fetch_eth_price_from_pool().await?;
        crate::core::utils::f64_to_decimal(eth_price, "eth_price_usd")
    }

    /// Get ETH price estimate as f64 (convenience method for gas calculations)
    pub async fn get_eth_price_f64(&self) -> Result<f64> {
        let price_decimal = self.get_native_price_usd().await.map_err(|e| {
            Error::Dex(format!(
                "Failed to get ETH price for gas calculation: {}. Cannot proceed without accurate price data.",
                e
            ))
        })?;
        price_decimal
            .to_f64()
            .ok_or_else(|| Error::Dex("Failed to convert ETH price to f64".to_string()))
    }

    /// Fetch ETH price from WETH/USDC or WETH/USDT Uniswap V3 pools
    ///
    /// Queries multiple fee tiers (0.3%, 0.05%, 1.0%) and selects the pool with
    /// the best liquidity for most accurate pricing.
    async fn fetch_eth_price_from_pool(&self) -> Result<f64> {
        info!("🔍 Fetching ETH price from Uniswap V3 pools on-chain");

        let weth_address = self.network.weth_address;
        let usdc_address = self.network.stablecoin_address; // USDC

        // Try different fee tiers, starting with the most liquid (0.3%)
        let fee_tiers = [
            V3FeeTier::Standard, // 0.3% - most liquid for WETH/USDC
            V3FeeTier::Medium,   // 0.05% - sometimes used for stablecoins
            V3FeeTier::High,     // 1.0% - less common but exists
        ];

        for fee_tier in &fee_tiers {
            match self
                .try_get_eth_price_from_pool(weth_address, usdc_address, *fee_tier)
                .await
            {
                Ok(price) => {
                    info!(
                        "✅ Got ETH price from WETH/USDC {:.2}% pool: ${:.2}",
                        (*fee_tier as u32 as f64) / 1000000.0 * 100.0,
                        price
                    );
                    return Ok(price);
                }
                Err(e) => {
                    debug!(
                        "Pool with {:.2}% fee not available or failed: {}",
                        (*fee_tier as u32 as f64) / 1000000.0 * 100.0,
                        e
                    );
                }
            }
        }

        Err(Error::Dex(
            "Failed to fetch ETH price from any WETH/USDC pool. No suitable pools found."
                .to_string(),
        ))
    }

    /// Try to get ETH price from a specific pool
    async fn try_get_eth_price_from_pool(
        &self,
        weth_address: Address,
        usdc_address: Address,
        fee_tier: V3FeeTier,
    ) -> Result<f64> {
        // Compute pool address using Uniswap V3 CREATE2 logic
        let pool_address = compute_pool_address(
            self.network.factory_address,
            weth_address,
            usdc_address,
            fee_tier,
        )?;

        debug!("Querying pool at {:?} for ETH price", pool_address);

        // Query slot0 from pool to get sqrtPriceX96
        let sqrt_price_x96 = query_pool_sqrt_price(pool_address, self.rpc_provider.clone()).await?;

        // Check if pool is initialized (sqrtPrice > 0)
        if sqrt_price_x96.is_zero() {
            return Err(Error::Dex(
                "Pool not initialized (sqrtPrice is zero)".to_string(),
            ));
        }

        // Determine which token is token0
        let is_weth_token0 = weth_address < usdc_address;

        // Token decimals (WETH=18, USDC=6)
        let (token0_decimals, token1_decimals) = if is_weth_token0 {
            (18, 6) // WETH/USDC
        } else {
            (6, 18) // USDC/WETH
        };

        // Use shared utility to calculate price
        crate::infrastructure::dex::ethereum::pool_pricing::calculate_eth_price_from_pool(
            &sqrt_price_x96.to_string(),
            token0_decimals,
            token1_decimals,
            is_weth_token0,
            &format!("{:?}", pool_address),
        )
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Compute Uniswap V3 pool address using CREATE2
fn compute_pool_address(
    factory: Address,
    token_a: Address,
    token_b: Address,
    fee: V3FeeTier,
) -> Result<Address> {
    // Sort tokens
    let (token0, token1) = if token_a < token_b {
        (token_a, token_b)
    } else {
        (token_b, token_a)
    };

    // Uniswap V3 INIT_CODE_HASH
    let init_code_hash: H256 = "0xe34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54"
        .parse()
        .map_err(|e| Error::Parse(format!("Failed to parse init code hash: {}", e)))?;

    // Encode pool key: keccak256(abi.encode(token0, token1, fee))
    let pool_key_encoded = ethabi::encode(&[
        Token::Address(token0),
        Token::Address(token1),
        Token::Uint(U256::from(fee as u32)),
    ]);
    let pool_key_hash = keccak256(&pool_key_encoded);

    // CREATE2: keccak256(0xff ++ factory ++ poolKeyHash ++ initCodeHash)
    let mut data = Vec::with_capacity(1 + 20 + 32 + 32);
    data.push(0xff);
    data.extend_from_slice(factory.as_bytes());
    data.extend_from_slice(&pool_key_hash);
    data.extend_from_slice(init_code_hash.as_bytes());

    let hash = keccak256(&data);
    let address = Address::from_slice(&hash[12..]);

    Ok(address)
}

/// Query sqrtPriceX96 from a Uniswap V3 pool's slot0
async fn query_pool_sqrt_price<M: Middleware>(
    pool_address: Address,
    provider: Arc<M>,
) -> Result<U256> {
    // slot0() function selector
    let slot0_selector = &keccak256(b"slot0()")[..4];

    // Call pool.slot0()
    let call_data = Bytes::from(slot0_selector.to_vec());
    let call = ethers::types::transaction::eip2718::TypedTransaction::Legacy(
        ethers::types::TransactionRequest {
            to: Some(ethers::types::NameOrAddress::Address(pool_address)),
            data: Some(call_data),
            ..Default::default()
        },
    );

    let result = provider
        .call(&call, None)
        .await
        .map_err(|e| Error::Network(format!("Failed to call pool.slot0(): {}", e)))?;

    // Decode slot0 result: (sqrtPriceX96, tick, observationIndex, observationCardinality, ...)
    // We only need the first return value (sqrtPriceX96)
    if result.len() < 32 {
        return Err(Error::Parse(
            "slot0() returned insufficient data".to_string(),
        ));
    }

    Ok(U256::from_big_endian(&result[0..32]))
}
