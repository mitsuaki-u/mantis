//! Pool caching and selection logic for Uniswap V3

use super::types::{format_address, PoolInfo, UniswapV3Pool, V3FeeTier};
use crate::infrastructure::errors::{Error, Result};
use ethers::types::{Address, U256};
use log::{debug, info, warn};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Pool cache manager for Uniswap V3
pub(super) struct PoolCache {
    pub(super) inner: Arc<RwLock<HashMap<String, Vec<PoolInfo>>>>,
}

impl PoolCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Generate normalized cache key for token pair
    fn cache_key(token_a: &Address, token_b: &Address) -> String {
        // Normalize token order (smaller address first) for consistent cache keys
        if token_a < token_b {
            format!("{}_{}", token_a, token_b)
        } else {
            format!("{}_{}", token_b, token_a)
        }
    }

    /// Convert Alchemy pool data to PoolInfo format
    fn from_alchemy_pool(alchemy_pool: &UniswapV3Pool) -> Result<PoolInfo> {
        // Parse addresses
        let pool_address = Address::from_str(&alchemy_pool.id).map_err(|e| {
            Error::Parse(format!("Invalid pool address {}: {}", alchemy_pool.id, e))
        })?;
        let token0 = Address::from_str(&alchemy_pool.token0.id).map_err(|e| {
            Error::Parse(format!(
                "Invalid token0 address {}: {}",
                alchemy_pool.token0.id, e
            ))
        })?;
        let token1 = Address::from_str(&alchemy_pool.token1.id).map_err(|e| {
            Error::Parse(format!(
                "Invalid token1 address {}: {}",
                alchemy_pool.token1.id, e
            ))
        })?;

        // Parse fee tier
        let fee_tier = match alchemy_pool.fee_tier.parse::<u32>() {
            Ok(100) => V3FeeTier::Low,
            Ok(500) => V3FeeTier::Medium,
            Ok(3000) => V3FeeTier::Standard,
            Ok(10000) => V3FeeTier::High,
            _ => V3FeeTier::Standard, // Default fallback
        };

        // Parse liquidity
        let liquidity = U256::from_dec_str(&alchemy_pool.liquidity)
            .or_else(|_| {
                if alchemy_pool.liquidity.starts_with("0x") {
                    U256::from_str(&alchemy_pool.liquidity)
                        .map_err(|_| ethers::abi::Error::InvalidData)
                } else {
                    Err(ethers::abi::Error::InvalidData)
                }
            })
            .map_err(|e| {
                Error::Parse(format!(
                    "Invalid liquidity '{}' for pool {}: {}",
                    alchemy_pool.liquidity, alchemy_pool.id, e
                ))
            })?;

        // Parse TVL with validation
        let tvl_usd = alchemy_pool
            .tvl_usd
            .parse::<f64>()
            .map_err(|_| {
                Error::Parse(format!(
                    "Unparseable TVL '{}' for pool {}",
                    alchemy_pool.tvl_usd, alchemy_pool.id
                ))
            })
            .and_then(|v| {
                if v.is_nan() || v.is_infinite() {
                    Err(Error::Parse(format!(
                        "Invalid TVL value '{}' for pool {} (NaN or Infinity)",
                        alchemy_pool.tvl_usd, alchemy_pool.id
                    )))
                } else {
                    Ok(v)
                }
            })?;

        // Parse volume with validation
        let volume_24h_usd = alchemy_pool
            .volume_usd
            .parse::<f64>()
            .map_err(|_| {
                Error::Parse(format!(
                    "Unparseable volume '{}' for pool {}",
                    alchemy_pool.volume_usd, alchemy_pool.id
                ))
            })
            .and_then(|v| {
                if v.is_nan() || v.is_infinite() {
                    Err(Error::Parse(format!(
                        "Invalid volume value '{}' for pool {} (NaN or Infinity)",
                        alchemy_pool.volume_usd, alchemy_pool.id
                    )))
                } else {
                    Ok(v)
                }
            })?;

        let sqrt_price = U256::from_dec_str(&alchemy_pool.sqrt_price).ok();
        let tick = alchemy_pool
            .tick
            .as_ref()
            .and_then(|t| t.parse::<i32>().ok());

        Ok(PoolInfo {
            pool_address,
            token0,
            token1,
            fee_tier,
            liquidity,
            tvl_usd,
            volume_24h_usd,
            sqrt_price,
            tick,
        })
    }

    /// Update cache with discovered pools from market data
    pub async fn update(
        &self,
        pools: Vec<crate::application::events::PoolDiscoveryData>,
        source: &str,
    ) {
        info!(
            "📨 Updating pool cache with {} pools from {}",
            pools.len(),
            source
        );

        // Convert event data to UniswapV3Pool format
        let alchemy_pools: Vec<UniswapV3Pool> = pools
            .into_iter()
            .map(|data| UniswapV3Pool {
                id: data.pool_address,
                token0: super::types::V3Token {
                    id: data.token0_address,
                    symbol: data.token0_symbol,
                    name: data.token0_name,
                    decimals: data.token0_decimals,
                },
                token1: super::types::V3Token {
                    id: data.token1_address,
                    symbol: data.token1_symbol,
                    name: data.token1_name,
                    decimals: data.token1_decimals,
                },
                fee_tier: data.fee_tier,
                liquidity: data.liquidity,
                sqrt_price: data.sqrt_price,
                volume_usd: data.volume_24h_usd.clone(),
                tvl_usd: data.tvl_usd,
                tick: data.tick,
            })
            .collect();

        // Group pools by token pair
        let mut pools_by_pair: HashMap<String, Vec<PoolInfo>> = HashMap::new();

        for alchemy_pool in alchemy_pools {
            // Convert to PoolInfo - skip corrupted pools
            let pool_info = match Self::from_alchemy_pool(&alchemy_pool) {
                Ok(info) => info,
                Err(e) => {
                    warn!("⚠️ Skipping pool {}: {}", alchemy_pool.id, e);
                    continue;
                }
            };

            let token0 = pool_info.token0;
            let token1 = pool_info.token1;
            let cache_key = Self::cache_key(&token0, &token1);

            // Add to list of pools for this pair
            pools_by_pair.entry(cache_key).or_default().push(pool_info);
        }

        // Update cache with all pools
        let mut cache = self.inner.write().await;
        let mut total_pools = 0;

        for (cache_key, mut pair_pools) in pools_by_pair {
            // Sort by TVL descending for consistent ordering
            pair_pools.sort_by(|a, b| {
                b.tvl_usd
                    .partial_cmp(&a.tvl_usd)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            debug!(
                "💾 Caching {} pools for pair {} (fee tiers: {:?})",
                pair_pools.len(),
                cache_key,
                pair_pools.iter().map(|p| p.fee_tier).collect::<Vec<_>>()
            );

            total_pools += pair_pools.len();
            cache.insert(cache_key, pair_pools);
        }

        info!(
            "✅ Successfully cached {} pools across {} token pairs from {}",
            total_pools,
            cache.len(),
            source
        );
    }

    /// Get all cached pools for a token pair
    pub async fn get_pools(&self, token_a: Address, token_b: Address) -> Option<Vec<PoolInfo>> {
        let cache = self.inner.read().await;
        let cache_key = Self::cache_key(&token_a, &token_b);
        cache.get(&cache_key).cloned()
    }

    /// Select best pool from candidates using quote comparison
    ///
    /// ## Purpose:
    /// Given multiple pool candidates for the same token pair, determines which pool
    /// will give the best swap output by querying the Uniswap V3 Quoter contract.
    ///
    /// ## Why This Is Needed:
    /// - Uniswap V3 allows multiple pools for the same token pair (different fee tiers)
    /// - Higher fee pools may give worse quotes, but sometimes they have better liquidity
    /// - The only accurate way to know which is best is to query the actual Quoter contract
    /// - TVL alone is not reliable (a high-fee low-TVL pool can beat a low-fee high-TVL pool)
    ///
    /// ## How It Works:
    /// 1. Takes a list of candidate pools (already sorted by TVL descending)
    /// 2. Queries Uniswap Quoter contract for each pool to get actual output amount
    /// 3. Compares all outputs and returns the pool giving maximum output tokens
    /// 4. Skips pools where quotes fail (insufficient liquidity, pool doesn't exist, etc.)
    ///
    /// ## Parameters:
    /// - `pools`: Candidate pools sorted by TVL (higher TVL first)
    /// - `token_in/token_out`: The token pair being traded
    /// - `amount_in`: Input amount for the swap
    /// - `quote_fn`: Async function to query Quoter contract (injected for testability)
    ///
    /// ## Returns:
    /// - `Ok(Some(pool))`: Best pool found with highest output
    /// - `Ok(None)`: No pools provided or all quotes failed
    /// - `Err`: Should not happen (errors are handled per-pool, not propagated)
    pub async fn select_best<F, Fut>(
        &self,
        pools: Vec<PoolInfo>,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        quote_fn: F,
    ) -> Result<Option<PoolInfo>>
    where
        F: Fn(Address, Address, U256, V3FeeTier) -> Fut,
        Fut: std::future::Future<Output = Result<U256>>,
    {
        if pools.is_empty() {
            return Ok(None);
        }

        info!(
            "🔍 Comparing {} pools using Quoter contract for {}/{} (amount: {})",
            pools.len(),
            format_address(token_in),
            format_address(token_out),
            amount_in
        );

        let mut best_pool: Option<PoolInfo> = None;
        let mut best_output = U256::zero();

        // Query each pool using Uniswap's Quoter contract
        // Note: We must query all pools because quote outputs can vary significantly
        // and TVL alone doesn't predict which pool will give the best rate
        for pool in pools {
            match quote_fn(token_in, token_out, amount_in, pool.fee_tier).await {
                Ok(output) => {
                    debug!(
                        "💰 Pool quote: fee={:?}, TVL=${:.2}, output={}",
                        pool.fee_tier, pool.tvl_usd, output
                    );

                    if output > best_output {
                        best_output = output;
                        best_pool = Some(pool);
                    }
                }
                Err(e) => {
                    debug!(
                        "⚠️ Quote failed for pool fee={:?}: {}. Skipping.",
                        pool.fee_tier, e
                    );
                }
            }
        }

        if let Some(ref pool) = best_pool {
            info!(
                "🎯 Best pool selected: fee={:?}, TVL=${:.2}, output={}",
                pool.fee_tier, pool.tvl_usd, best_output
            );
        } else {
            warn!("❌ No valid quotes received from any pool");
        }

        Ok(best_pool)
    }
}
