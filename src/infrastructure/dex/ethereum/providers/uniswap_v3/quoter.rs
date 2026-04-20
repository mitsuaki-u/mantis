//! Quote operations for Uniswap V3
//!
//! This module handles getting swap quotes from the Uniswap V3 Quoter contract.

use super::abi::load_quoter_abi;
use super::types::{format_address, PoolInfo, V3FeeTier};
use crate::infrastructure::errors::{Error, Result};
use ethers::contract::Contract;
use ethers::types::{Address, U256};
use log::{debug, info};
use std::sync::Arc;

/// Quoter for Uniswap V3 swap quotes
pub(super) struct Quoter {
    provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
    quoter_address: Address,
}

impl Quoter {
    pub fn new(
        provider: Arc<ethers::providers::Provider<ethers::providers::Http>>,
        quoter_address: Address,
    ) -> Self {
        Self {
            provider,
            quoter_address,
        }
    }

    /// Get a quote for exact input single swap using Uniswap V3 Quoter contract
    pub async fn quote_single(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        fee_tier: V3FeeTier,
    ) -> Result<U256> {
        info!(
            "📊 Getting quote: {} -> {} (amount: {}, fee: {:?})",
            format_address(token_in),
            format_address(token_out),
            amount_in,
            fee_tier
        );

        // Create Quoter contract instance
        let quoter_abi = load_quoter_abi()?;
        let quoter_contract = Contract::new(self.quoter_address, quoter_abi, self.provider.clone());

        // Call quoteExactInputSingle
        let quote_result: U256 = quoter_contract
            .method::<_, U256>(
                "quoteExactInputSingle",
                (
                    token_in,
                    token_out,
                    fee_tier as u32,
                    amount_in,
                    U256::zero(), // sqrtPriceLimitX96 (no limit)
                ),
            )
            .map_err(|e| Error::Contract(format!("Failed to prepare quote call: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Quote call failed: {}", e)))?;

        info!(
            "✅ Quote result: {} tokens out for {} tokens in (fee: {:.2}%)",
            quote_result,
            amount_in,
            (fee_tier as u32 as f64) / crate::core::constants::V3_FEE_TIER_DIVISOR * 100.0
        );

        Ok(quote_result)
    }

    /// Get the best quote by trying multiple pools or fee tiers
    ///
    /// ## Strategy:
    /// 1. If pool data is available, queries each pool via Quoter contract and returns best output
    /// 2. If no pools provided, falls back to trying standard fee tiers in likely order
    ///
    /// ## How Pool Selection Works:
    /// - Each Uniswap V3 pool has a different fee tier (0.01%, 0.05%, 0.3%, 1%)
    /// - The same token pair can have multiple pools with different fee tiers
    /// - Different fee tiers give different quote outputs due to varying slippage & fees
    /// - We query all discovered pools and select the one giving maximum output tokens
    ///
    /// ## Fallback Behavior:
    /// If no cached pools are available or all quotes fail:
    /// - Try fee tiers in order: Standard (0.3%) → Medium (0.05%) → High (1%) → Low (0.01%)
    /// - Standard (0.3%) is tried first as it's the most common tier with highest liquidity
    /// - Returns first successful quote, not necessarily the best
    ///
    /// ## Returns:
    /// - Ok((amount_out, fee_tier)): Best quote found and the fee tier used
    /// - Err: All quotes failed across all pools/tiers (no liquidity or pair doesn't exist)
    pub async fn quote_best(
        &self,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        pools: Option<Vec<PoolInfo>>,
    ) -> Result<(U256, V3FeeTier)> {
        info!(
            "🔍 Finding best quote: {} -> {} (amount: {})",
            format_address(token_in),
            format_address(token_out),
            amount_in
        );

        // Try to get quotes from cached pools if available
        if let Some(pool_list) = pools {
            if !pool_list.is_empty() {
                let mut best_output = U256::zero();
                let mut best_fee_tier: Option<V3FeeTier> = None;

                // Get quotes from all cached pools and find the best one
                // We try all pools because different fee tiers can give significantly different outputs
                for pool_info in pool_list {
                    match self
                        .quote_single(token_in, token_out, amount_in, pool_info.fee_tier)
                        .await
                    {
                        Ok(amount_out) => {
                            debug!(
                                "💰 Quote from pool: {} tokens out using {:.2}% fee (TVL: ${:.2})",
                                amount_out,
                                (pool_info.fee_tier as u32 as f64)
                                    / crate::core::constants::V3_FEE_TIER_DIVISOR
                                    * 100.0,
                                pool_info.tvl_usd
                            );

                            if amount_out > best_output {
                                best_output = amount_out;
                                best_fee_tier = Some(pool_info.fee_tier);
                            }
                        }
                        Err(e) => {
                            debug!(
                                "Quote failed for fee tier {:?}: {}. Skipping.",
                                pool_info.fee_tier, e
                            );
                        }
                    }
                }

                // If we found a valid quote, return it
                if let Some(fee_tier) = best_fee_tier {
                    info!(
                        "✅ Best quote: {} tokens out using {:.2}% fee",
                        best_output,
                        (fee_tier as u32 as f64) / crate::core::constants::V3_FEE_TIER_DIVISOR
                            * 100.0
                    );
                    return Ok((best_output, fee_tier));
                }
            }
        } else {
            debug!("No cached pools provided, falling back to standard fee tier search");
        }

        // Fallback: try standard fee tiers in order of likelihood
        let fee_tiers = [
            V3FeeTier::Standard,
            V3FeeTier::Medium,
            V3FeeTier::High,
            V3FeeTier::Low,
        ];

        for fee_tier in fee_tiers.iter() {
            match self
                .quote_single(token_in, token_out, amount_in, *fee_tier)
                .await
            {
                Ok(amount_out) => {
                    info!(
                        "✅ Fallback quote successful: {} tokens out using {:.2}% fee",
                        amount_out,
                        (*fee_tier as u32 as f64) / crate::core::constants::V3_FEE_TIER_DIVISOR
                            * 100.0
                    );
                    return Ok((amount_out, *fee_tier));
                }
                Err(e) => {
                    debug!(
                        "Quote failed for fee tier {:?}: {}. Trying next tier.",
                        fee_tier, e
                    );
                }
            }
        }

        Err(Error::Trading(format!(
            "Unable to get quote for pair {} -> {} across all fee tiers",
            format_address(token_in),
            format_address(token_out)
        )))
    }
}
