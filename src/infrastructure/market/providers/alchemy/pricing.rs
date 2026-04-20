//! Price calculation logic for Uniswap V3 pools

use super::graphql;
use super::types::{AlchemyV3Response, UniswapV3Pool};
use crate::core::calculations::uniswap_v3;
use crate::infrastructure::errors::{Error, Result};
use log::{debug, info, warn};
use reqwest::Client;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;
use std::str::FromStr;

/// Calculate token price from V3 pool data and convert to USD
///
/// Uses core Uniswap V3 calculation for sqrt price → price conversion, then converts to USD
pub(super) fn calculate_token_price_from_pool(
    pool: &UniswapV3Pool,
    is_token0: bool,
    network: &str,
    cached_eth_price_usd: &std::sync::RwLock<Option<f64>>,
) -> Result<Decimal> {
    debug!(
        "Pool {}: Calculating price for {} (is_token0={})",
        pool.id,
        if is_token0 {
            &pool.token0.symbol
        } else {
            &pool.token1.symbol
        },
        is_token0
    );

    let token0_decimals = pool
        .token0
        .decimals
        .parse::<i32>()
        .unwrap_or(crate::infrastructure::constants::ERC20_STANDARD_DECIMALS as i32);
    let token1_decimals = pool
        .token1
        .decimals
        .parse::<i32>()
        .unwrap_or(crate::infrastructure::constants::ERC20_STANDARD_DECIMALS as i32);

    let price_in_paired_token_f64 = uniswap_v3::calculate_v3_price_from_sqrt(
        &pool.sqrt_price,
        token0_decimals,
        token1_decimals,
        is_token0,
        &pool.id,
    )?;

    debug!(
        "Pool {}: price_in_paired_token = {}",
        pool.id, price_in_paired_token_f64
    );

    let paired_token = if is_token0 {
        &pool.token1
    } else {
        &pool.token0
    };
    let paired_address = paired_token.id.to_lowercase();

    let price_usd_f64 = convert_to_usd_price(
        price_in_paired_token_f64,
        &paired_address,
        network,
        cached_eth_price_usd,
    )?;

    let price_usd_decimal = Decimal::from_f64(price_usd_f64.abs().max(0.000001))
        .ok_or_else(|| Error::Parse("Failed to convert final USD price to Decimal".to_string()))?;

    debug!("Pool {}: final_price_usd = ${}", pool.id, price_usd_decimal);

    Ok(price_usd_decimal)
}

/// Convert a token price (in terms of another token) to USD
/// Only supports WETH pairs (for trading tokens) and stablecoins (for ETH price cache)
fn convert_to_usd_price(
    price_in_paired_token: f64,
    paired_token_address: &str,
    network: &str,
    cached_eth_price_usd: &std::sync::RwLock<Option<f64>>,
) -> Result<f64> {
    use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;

    let addresses = if network == "ethereum" {
        NetworkAddresses::mainnet()
    } else {
        return Ok(price_in_paired_token);
    };

    let addr = paired_token_address.to_lowercase();

    let usdc_addr = format!("{:?}", addresses.usdc).to_lowercase();
    let usdt_addr = format!("{:?}", addresses.usdt).to_lowercase();
    let dai_addr = format!("{:?}", addresses.dai).to_lowercase();

    if addr == usdc_addr || addr == usdt_addr || addr == dai_addr {
        return Ok(price_in_paired_token);
    }

    let weth_addr = format!("{:?}", addresses.weth).to_lowercase();
    if addr == weth_addr {
        let eth_price_usd = cached_eth_price_usd
            .read()
            .map_err(|e| Error::Internal(format!("Failed to read ETH price cache: {}", e)))?
            .ok_or_else(|| {
                Error::Parse(
                    "ETH price cache not initialized - cannot convert WETH-paired token to USD"
                        .to_string(),
                )
            })?;
        return Ok(price_in_paired_token * eth_price_usd);
    }

    Err(Error::Parse(format!(
        "Cannot convert to USD - paired token {} is neither stablecoin nor WETH (we only trade WETH pairs)",
        paired_token_address
    )))
}

/// Update the cached ETH price from WETH/USDC or WETH/USDT pools
pub(super) async fn update_eth_price_cache(
    pools: &[UniswapV3Pool],
    network: &str,
    cached_eth_price_usd: &std::sync::RwLock<Option<f64>>,
    client: &Client,
    subgraph_url: &str,
    api_key: Option<&str>,
) -> Result<()> {
    use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;

    if network != "ethereum" {
        debug!("Skipping ETH price cache update for non-ethereum network");
        return Ok(());
    }

    let addresses = NetworkAddresses::mainnet();
    let weth_addr = format!("{:?}", addresses.weth).to_lowercase();
    let usdc_addr = format!("{:?}", addresses.usdc).to_lowercase();
    let usdt_addr = format!("{:?}", addresses.usdt).to_lowercase();

    let mut best_pool: Option<(&UniswapV3Pool, bool)> = None;
    let mut best_tvl = Decimal::ZERO;

    for pool in pools {
        let token0_addr = pool.token0.id.to_lowercase();
        let token1_addr = pool.token1.id.to_lowercase();

        let is_weth_usdc_pool = (token0_addr == weth_addr && token1_addr == usdc_addr)
            || (token1_addr == weth_addr && token0_addr == usdc_addr);

        let is_weth_usdt_pool = (token0_addr == weth_addr && token1_addr == usdt_addr)
            || (token1_addr == weth_addr && token0_addr == usdt_addr);

        if is_weth_usdc_pool || is_weth_usdt_pool {
            if let Ok(tvl) = Decimal::from_str(&pool.tvl_usd) {
                if tvl > best_tvl {
                    best_tvl = tvl;
                    let is_weth_token0 = token0_addr == weth_addr;
                    best_pool = Some((pool, is_weth_token0));
                }
            }
        }
    }

    if let Some((pool, is_weth_token0)) = best_pool {
        match calculate_token_price_from_stablecoin_pool(pool, is_weth_token0) {
            Ok(eth_price) => {
                debug!(
                    "Updated ETH price cache: ${:.2} (from pool {} with TVL ${})",
                    eth_price, pool.id, best_tvl
                );

                *cached_eth_price_usd.write().map_err(|e| {
                    Error::Internal(format!("Failed to write ETH price cache: {}", e))
                })? = Some(eth_price);

                return Ok(());
            }
            Err(e) => {
                warn!("Failed to calculate ETH price from pool {}: {}", pool.id, e);
            }
        }
    }

    warn!("⚠️ Could not find suitable WETH/USDC or WETH/USDT pool in current pool set");

    info!("🔍 Fetching WETH/stablecoin pools specifically for ETH price cache...");
    match fetch_weth_stablecoin_pools(client, subgraph_url, api_key).await {
        Ok(weth_pools) if !weth_pools.is_empty() => {
            for pool in &weth_pools {
                let token0_addr = pool.token0.id.to_lowercase();
                let token1_addr = pool.token1.id.to_lowercase();

                let is_weth_pool = token0_addr == weth_addr || token1_addr == weth_addr;
                let is_stablecoin_pool = token0_addr == usdc_addr
                    || token1_addr == usdc_addr
                    || token0_addr == usdt_addr
                    || token1_addr == usdt_addr;

                if is_weth_pool && is_stablecoin_pool {
                    let is_weth_token0 = token0_addr == weth_addr;
                    if let Ok(eth_price) =
                        calculate_token_price_from_stablecoin_pool(pool, is_weth_token0)
                    {
                        debug!(
                            "Updated ETH price cache: ${:.2} (from specifically fetched pool {})",
                            eth_price, pool.id
                        );

                        *cached_eth_price_usd.write().map_err(|e| {
                            Error::Internal(format!("Failed to write ETH price cache: {}", e))
                        })? = Some(eth_price);

                        return Ok(());
                    }
                }
            }
        }
        Ok(_) => warn!("No WETH/stablecoin pools found in specific fetch"),
        Err(e) => warn!("Failed to fetch WETH/stablecoin pools: {}", e),
    }

    Err(Error::Internal(
        "Failed to initialize ETH price cache - no WETH/stablecoin pools available".to_string(),
    ))
}

/// Fetch WETH/stablecoin pools specifically for ETH price calculation
async fn fetch_weth_stablecoin_pools(client: &Client, subgraph_url: &str, api_key: Option<&str>) -> Result<Vec<UniswapV3Pool>> {
    use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;

    let addresses = NetworkAddresses::mainnet();
    let weth_addr = format!("{:?}", addresses.weth);
    let usdc_addr = format!("{:?}", addresses.usdc);
    let usdt_addr = format!("{:?}", addresses.usdt);

    let query = format!(
        r#"{{
            pools(
                first: 10,
                where: {{
                    or: [
                        {{ token0: "{}", token1: "{}" }},
                        {{ token0: "{}", token1: "{}" }},
                        {{ token0: "{}", token1: "{}" }},
                        {{ token0: "{}", token1: "{}" }}
                    ]
                }},
                orderBy: totalValueLockedUSD,
                orderDirection: desc
            ) {{
                id
                token0 {{ id symbol name decimals }}
                token1 {{ id symbol name decimals }}
                feeTier
                liquidity
                sqrtPrice
                volumeUSD
                totalValueLockedUSD
                tick
            }}
        }}"#,
        weth_addr,
        usdc_addr, // WETH/USDC
        usdc_addr,
        weth_addr, // USDC/WETH
        weth_addr,
        usdt_addr, // WETH/USDT
        usdt_addr,
        weth_addr // USDT/WETH
    );

    let response = graphql::execute_v3_query(client, subgraph_url, api_key, &query).await?;

    let pools_data: AlchemyV3Response = serde_json::from_value(response).map_err(|e| {
        Error::Parse(format!(
            "Failed to parse WETH/stablecoin pools response: {}",
            e
        ))
    })?;

    Ok(pools_data.data.pools)
}

/// Calculate token price from a Token/Stablecoin pool (used for WETH/USDC price calculation)
fn calculate_token_price_from_stablecoin_pool(
    pool: &UniswapV3Pool,
    is_target_token_token0: bool,
) -> Result<f64> {
    let token0_decimals = pool.token0.decimals.parse::<i32>().unwrap_or(18);
    let token1_decimals = pool.token1.decimals.parse::<i32>().unwrap_or(18);

    crate::infrastructure::dex::ethereum::pool_pricing::calculate_eth_price_from_pool(
        &pool.sqrt_price,
        token0_decimals,
        token1_decimals,
        is_target_token_token0,
        &pool.id,
    )
}
