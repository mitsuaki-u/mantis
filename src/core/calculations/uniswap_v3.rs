//! Uniswap V3 mathematical calculations
//!
//! This module contains pure mathematical functions for Uniswap V3 price calculations.
//! These functions have no I/O dependencies and can be used across different layers.

use crate::errors::{Error, Result};
use log::debug;
use rust_decimal::prelude::*;
use rust_decimal::Decimal;

/// Calculate token price from Uniswap V3 pool sqrtPriceX96
///
/// This is a pure mathematical function that converts Uniswap V3's sqrtPriceX96
/// representation into a human-readable price ratio.
///
/// # Arguments
/// * `sqrt_price_x96` - The sqrtPriceX96 value from the Uniswap V3 pool (as string)
/// * `token0_decimals` - Decimals of token0
/// * `token1_decimals` - Decimals of token1
/// * `is_target_token0` - true to get price of token0 in token1, false for token1 in token0
/// * `pool_id` - Pool identifier for logging purposes (optional, can be empty string)
///
/// # Returns
/// Token price as f64
///
/// # Algorithm
/// Uniswap V3 uses sqrtPriceX96 = sqrt(price) * 2^96 where price = token1/token0
/// To get price:
/// 1. Parse sqrtPriceX96 as f64
/// 2. Calculate: (sqrtPriceX96 / 2^96)^2 to get raw price ratio
/// 3. Adjust for decimal differences: price * 10^(token0_decimals - token1_decimals)
/// 4. If is_target_token0=true, return adjusted_price (token1/token0)
/// 5. If is_target_token0=false, return 1/adjusted_price (token0/token1)
///
/// # Example
/// ```ignore
/// // WETH/USDC pool where WETH is token0
/// let price = calculate_v3_price_from_sqrt(
///     "4340000000000000000000000",
///     18,  // WETH decimals
///     6,   // USDC decimals
///     true, // Price WETH in USDC
///     "pool_address"
/// )?;
/// // Returns ~3000.0 (meaning 1 WETH = 3000 USDC)
/// ```
pub fn calculate_v3_price_from_sqrt(
    sqrt_price_x96: &str,
    token0_decimals: i32,
    token1_decimals: i32,
    is_target_token0: bool,
    pool_id: &str,
) -> Result<f64> {
    if !pool_id.is_empty() {
        debug!("🔢 Calculating V3 price from pool {}", pool_id);
    }

    // Parse sqrt price as f64 for normalization (it's a very large integer ~1e29)
    // We use f64 here because rust_decimal can't handle 2^96 as a divisor
    let sqrt_price_x96_f64 = sqrt_price_x96
        .parse::<f64>()
        .map_err(|e| Error::Parse(format!("Invalid sqrt price in pool data: {}", e)))?;

    // V3 price calculation: price = (sqrtPriceX96 / 2^96)^2
    // 2^96 = 79228162514264337593543950336 (approximately 7.92e28)
    // Use f64 for this division since the numbers are huge but the result is small
    let q96 = 2_f64.powf(96.0);
    let sqrt_price_normalized = sqrt_price_x96_f64 / q96;
    let price_ratio_f64 = sqrt_price_normalized * sqrt_price_normalized;

    // Convert to Decimal for decimal adjustment calculations
    let price_ratio = Decimal::try_from(price_ratio_f64).map_err(|e| {
        Error::Parse(format!(
            "Failed to convert price_ratio {} to Decimal: {}",
            price_ratio_f64, e
        ))
    })?;

    // Adjust for decimal differences
    let decimal_diff = token0_decimals - token1_decimals;
    let decimal_adjustment = if decimal_diff >= 0 {
        Decimal::from(10_i64.pow(decimal_diff.unsigned_abs()))
    } else {
        Decimal::ONE / Decimal::from(10_i64.pow(decimal_diff.unsigned_abs()))
    };

    let adjusted_price = price_ratio * decimal_adjustment;

    // Calculate price based on which token we're pricing
    // Note: price_ratio = (sqrtPriceX96 / 2^96)^2 gives us token1/token0
    // So adjusted_price = (token1 amount) / (token0 amount)
    let final_price_decimal = if is_target_token0 {
        // We want price of token0 in terms of token1
        // adjusted_price already gives us token1/token0, which is what we want!
        adjusted_price
    } else {
        // We want price of token1 in terms of token0
        // adjusted_price gives us token1/token0, we need token0/token1, so invert
        Decimal::ONE / adjusted_price
    };

    // Convert to f64 for return
    let final_price = final_price_decimal
        .to_f64()
        .ok_or_else(|| Error::Parse("Failed to convert final price to f64".to_string()))?;

    if !pool_id.is_empty() {
        debug!("✅ Final V3 price: {:.6}", final_price);
    }

    Ok(final_price)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_v3_price_weth_token0() {
        // Test with realistic sqrtPriceX96 for ~$3000 ETH (WETH/USDC where WETH is token0)
        // Formula: For $3000, we need sqrt(3000 / 10^12) * 2^96 ≈ 4.34e24
        let sqrt_price = "4340000000000000000000000";
        let result = calculate_v3_price_from_sqrt(sqrt_price, 18, 6, true, "test_pool");

        assert!(result.is_ok());
        let price = result.unwrap();
        // Should be around $3000 (allow 1000-10000 range for rounding/precision)
        assert!(
            price > 1000.0 && price < 10000.0,
            "Price {} is outside reasonable bounds",
            price
        );
    }

    #[test]
    fn test_calculate_v3_price_weth_token1() {
        // Test where WETH is token1 (USDC/WETH)
        // Formula: For $3000, price_ratio = 10^12 / 3000, sqrt = 18257, sqrtPriceX96 ≈ 1.446e33
        let sqrt_price = "1446000000000000000000000000000000";
        let result = calculate_v3_price_from_sqrt(sqrt_price, 6, 18, false, "test_pool");

        assert!(result.is_ok());
        let price = result.unwrap();
        // Should be around $3000 (allow 1000-10000 range for rounding/precision)
        assert!(
            price > 1000.0 && price < 10000.0,
            "Price {} is outside reasonable bounds",
            price
        );
    }

    #[test]
    fn test_invalid_sqrt_price() {
        let result = calculate_v3_price_from_sqrt("invalid", 18, 6, true, "test_pool");
        assert!(result.is_err());
    }

    #[test]
    fn test_equal_decimals() {
        // Test with tokens that have same decimals (no adjustment needed)
        // Simple case: sqrt(1) * 2^96
        let sqrt_price = "79228162514264337593543950336"; // This is 2^96
        let result = calculate_v3_price_from_sqrt(sqrt_price, 18, 18, true, "");

        assert!(result.is_ok());
        let price = result.unwrap();
        // Price should be 1.0 (no decimal adjustment, sqrt(1)^2 = 1)
        assert!(
            (price - 1.0).abs() < 0.01,
            "Price {} should be close to 1.0",
            price
        );
    }

    #[test]
    fn test_no_logging_when_pool_id_empty() {
        // Ensure function works without logging when pool_id is empty
        let sqrt_price = "4340000000000000000000000";
        let result = calculate_v3_price_from_sqrt(sqrt_price, 18, 6, true, "");
        assert!(result.is_ok());
    }
}
