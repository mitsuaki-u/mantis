use crate::core::domain::TokenMetrics;
use crate::core::errors::Error;
use log::{debug, info, trace, warn};
use rust_decimal::Decimal;

/// Validate that a price is reasonable (positive and not suspiciously small/large)
pub fn validate_price(price: Decimal, context: &str) -> Result<(), Error> {
    if price.is_sign_negative() || price.is_zero() {
        return Err(Error::InvalidInput(format!(
            "{}: Price is {} (must be positive)",
            context, price
        )));
    }

    // Check for extremely small prices - raised threshold to $0.10 to filter out micro-cap tokens
    // This prevents issues with very low-priced tokens causing calculation problems
    if price < Decimal::new(10, 2) {
        // 0.10
        return Err(Error::InvalidInput(format!(
            "{}: Price {} is below minimum threshold ($0.10), filtering out micro-cap token.",
            context, price
        )));
    }

    // Check for small prices (less than $1.00) but allow them with warning
    // This catches low-priced tokens that might need monitoring
    if price < Decimal::ONE {
        debug!(
            "⚠️ {}: Price {} is low (< $1.00). Monitor for potential volatility issues.",
            context, price
        );
    }

    // Check for suspiciously large prices (more than $200,000)
    if price > Decimal::new(200_000, 0) {
        return Err(Error::InvalidInput(format!(
            "{}: Price {} exceeds maximum threshold ($200,000), likely invalid data.",
            context, price
        )));
    }

    // Warn for high prices but allow them
    if price > Decimal::new(100_000, 0) {
        warn!(
            "⚠️ {}: Price {} is high (> $100,000), verify legitimacy",
            context, price
        );
    }

    Ok(())
}

/// Validate market token data quality
pub fn validate_token_data(token: &TokenMetrics) -> bool {
    // Basic validation checks
    if token.price_usd <= 0.0 {
        trace!(
            "Rejecting token {} due to invalid price: {}",
            token.symbol,
            token.price_usd
        );
        return false;
    }

    if token.volume_24h < 0.0 {
        trace!(
            "Rejecting token {} due to negative volume: {}",
            token.symbol,
            token.volume_24h
        );
        return false;
    }

    // Check for reasonable price ranges
    if token.price_usd > 1_000_000.0 {
        trace!(
            "Rejecting token {} due to suspiciously high price: {}",
            token.symbol,
            token.price_usd
        );
        return false;
    }

    true
}

/// Result of price discrepancy validation
#[derive(Debug, Clone)]
pub struct PriceDiscrepancyResult {
    pub is_valid: bool,
    pub discrepancy_percentage: f64,
    pub reason: Option<String>,
    pub external_price: Decimal,
    pub blockchain_price: Decimal,
}

/// **STATELESS PRICE VALIDATION UTILITIES**
/// These functions provide price validation without maintaining state
///
/// Validate price discrepancy between external API and blockchain prices
pub fn validate_price_discrepancy(
    external_api_price: Decimal,
    blockchain_price: Decimal,
    max_discrepancy_threshold: f64,
    token_id: &str,
    correlation_id: &str,
) -> PriceDiscrepancyResult {
    // Calculate percentage discrepancy: |blockchain - external| / external * 100
    let price_diff = (blockchain_price - external_api_price).abs();
    let discrepancy_percentage = if external_api_price > Decimal::ZERO {
        crate::core::utils::decimal_to_f64(price_diff / external_api_price, "price discrepancy")
            .unwrap_or(f64::MAX)
    } else {
        f64::MAX // Invalid external price
    };

    let is_within_threshold = discrepancy_percentage <= max_discrepancy_threshold;

    if !is_within_threshold {
        warn!(
            "[{}] 🚨 PRICE DISCREPANCY ALERT for {}: External=${:.8}, Blockchain=${:.8}, Discrepancy={:.2}% (threshold: {:.2}%)",
            &correlation_id[..8], token_id, external_api_price, blockchain_price,
            discrepancy_percentage * 100.0, max_discrepancy_threshold * 100.0
        );
    } else {
        info!(
            "[{}] ✅ Price validation passed for {}: External=${:.8}, Blockchain=${:.8}, Discrepancy={:.2}%",
            &correlation_id[..8], token_id, external_api_price, blockchain_price, discrepancy_percentage * 100.0
        );
    }

    PriceDiscrepancyResult {
        is_valid: is_within_threshold,
        discrepancy_percentage,
        reason: if !is_within_threshold {
            Some(format!(
                "Price discrepancy {:.2}% exceeds threshold {:.2}%",
                discrepancy_percentage * 100.0,
                max_discrepancy_threshold * 100.0
            ))
        } else {
            None
        },
        external_price: external_api_price,
        blockchain_price,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_discrepancy_within_threshold() {
        let external_price = Decimal::new(100, 0); // $1.00
        let blockchain_price = Decimal::new(102, 0); // $1.02
        let threshold = 0.05; // 5%

        let result = validate_price_discrepancy(
            external_price,
            blockchain_price,
            threshold,
            "test_token",
            "test-correlation-id-12345",
        );

        assert!(result.is_valid);
        assert!(result.discrepancy_percentage < threshold);
    }

    #[test]
    fn test_price_discrepancy_exceeds_threshold() {
        let external_price = Decimal::new(100, 0); // $1.00
        let blockchain_price = Decimal::new(110, 0); // $1.10
        let threshold = 0.05; // 5%

        let result = validate_price_discrepancy(
            external_price,
            blockchain_price,
            threshold,
            "test_token",
            "test-correlation-id-12345",
        );

        assert!(!result.is_valid);
        assert!(result.discrepancy_percentage > threshold);
    }
}
