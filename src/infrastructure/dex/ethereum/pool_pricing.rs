//! Infrastructure wrapper for Uniswap V3 pool pricing calculations
//!
//! This module provides an infrastructure-layer wrapper around core Uniswap V3 calculations.
//! The actual mathematical logic lives in core::calculations::uniswap_v3.

use crate::core::calculations::uniswap_v3;
use crate::infrastructure::errors::Result;
use log::debug;

/// Calculate ETH price in USD from a WETH/Stablecoin pool
///
/// This is a thin wrapper around the core V3 pricing calculation that adds
/// infrastructure-specific logging for ETH price calculations.
///
/// # Arguments
/// * `sqrt_price_x96` - The sqrtPriceX96 value from the Uniswap V3 pool (as string)
/// * `token0_decimals` - Decimals of token0 (typically WETH or stablecoin)
/// * `token1_decimals` - Decimals of token1 (typically stablecoin or WETH)
/// * `is_weth_token0` - true if WETH is token0, false if WETH is token1
/// * `pool_id` - Pool address for logging purposes
///
/// # Returns
/// ETH price in USD as f64
///
/// # Implementation
/// Delegates to core::calculations::uniswap_v3::calculate_v3_price_from_sqrt()
pub fn calculate_eth_price_from_pool(
    sqrt_price_x96: &str,
    token0_decimals: i32,
    token1_decimals: i32,
    is_weth_token0: bool,
    pool_id: &str,
) -> Result<f64> {
    debug!("Calculating ETH price from pool {}", pool_id);

    // Delegate to core calculation
    let eth_price = uniswap_v3::calculate_v3_price_from_sqrt(
        sqrt_price_x96,
        token0_decimals,
        token1_decimals,
        is_weth_token0,
        pool_id,
    )?;

    debug!("Final ETH price: ${:.2}", eth_price);

    // Stablecoin price is assumed to be $1, so this is already in USD
    Ok(eth_price)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_eth_price_weth_token0() {
        // Test wrapper function with WETH as token0
        // Detailed math tests are in core::calculations::uniswap_v3
        let sqrt_price = "4340000000000000000000000";
        let result = calculate_eth_price_from_pool(sqrt_price, 18, 6, true, "test_pool");

        assert!(result.is_ok());
        let price = result.unwrap();
        assert!(
            price > 1000.0 && price < 10000.0,
            "ETH price {} is outside reasonable bounds",
            price
        );
    }

    #[test]
    fn test_calculate_eth_price_weth_token1() {
        // Test wrapper function with WETH as token1
        let sqrt_price = "1446000000000000000000000000000000";
        let result = calculate_eth_price_from_pool(sqrt_price, 6, 18, false, "test_pool");

        assert!(result.is_ok());
        let price = result.unwrap();
        assert!(
            price > 1000.0 && price < 10000.0,
            "ETH price {} is outside reasonable bounds",
            price
        );
    }

    #[test]
    fn test_invalid_sqrt_price() {
        // Test error handling
        let result = calculate_eth_price_from_pool("invalid", 18, 6, true, "test_pool");
        assert!(result.is_err());
    }
}
