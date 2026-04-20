use crate::core::errors::Error;
use log::{debug, warn};
use rust_decimal::prelude::*;
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

/// Safely format a price value for display, handling very small and very large numbers
pub fn format_price_safe(price: Decimal) -> String {
    if price.is_zero() {
        return "$0.00".to_string();
    }

    let abs_price = price.abs();

    // Handle very small numbers (less than 0.0001)
    if abs_price < Decimal::new(1, 4) && !abs_price.is_zero() {
        if abs_price < Decimal::new(1, 6) {
            // Use scientific notation for extremely small numbers
            match price.to_f64() {
                Some(f64_price) => format!("${:.2e}", f64_price),
                None => format!("$<INVALID:{}>", price),
            }
        } else {
            // Use 6 decimal places for small numbers, trim trailing zeros
            let formatted = format!("{:.6}", price);
            let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
            format!("${}", trimmed)
        }
    }
    // Handle very large numbers (greater than 999,999)
    else if abs_price >= Decimal::new(1_000_000, 0) {
        if abs_price >= Decimal::new(1_000_000_000, 0) {
            format!("${:.2}B", price / Decimal::new(1_000_000_000, 0))
        } else {
            format!("${:.2}M", price / Decimal::new(1_000_000, 0))
        }
    }
    // Handle large numbers (greater than 9999)
    else if abs_price >= Decimal::new(10_000, 0) {
        format!("${:.2}", price)
    }
    // Handle medium numbers (greater than 1)
    else if abs_price >= Decimal::ONE {
        let formatted = format!("{:.4}", price);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        format!("${}", trimmed)
    }
    // Handle small numbers (less than 1)
    else {
        let formatted = format!("{:.6}", price);
        let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
        format!("${}", trimmed)
    }
}

/// Safely format ROI percentage, capping extremely large values
pub fn format_roi_safe(roi_percentage: Decimal) -> String {
    let abs_roi = roi_percentage.abs();

    // Cap extremely large ROI values to prevent display issues
    if abs_roi > Decimal::new(1_000_000, 0) {
        if roi_percentage > Decimal::ZERO {
            ">1,000,000%".to_string()
        } else {
            "<-1,000,000%".to_string()
        }
    } else if abs_roi > Decimal::new(10_000, 0) {
        // Use scientific notation for very large percentages
        match roi_percentage.to_f64() {
            Some(f64_roi) => format!("{:.2e}%", f64_roi),
            None => format!("<INVALID:{}>%", roi_percentage),
        }
    } else if abs_roi >= Decimal::new(100, 0) {
        // Use 1 decimal place for large percentages
        format!("{:.1}%", roi_percentage)
    } else if abs_roi >= Decimal::ONE {
        // Use 2 decimal places for medium percentages
        format!("{:.2}%", roi_percentage)
    } else {
        // Use 3 decimal places for small percentages
        format!("{:.3}%", roi_percentage)
    }
}

/// Safely format position size, handling very large quantities
pub fn format_size_safe(size: Decimal) -> String {
    if size.is_zero() {
        return "0".to_string();
    }

    let abs_size = size.abs();

    // Handle very large quantities
    if abs_size >= Decimal::new(1_000_000, 0) {
        if abs_size >= Decimal::new(1_000_000_000, 0) {
            format!("{:.2}B", size / Decimal::new(1_000_000_000, 0))
        } else {
            format!("{:.2}M", size / Decimal::new(1_000_000, 0))
        }
    } else if abs_size >= Decimal::new(10_000, 0) {
        format!("{:.2}", size)
    } else if abs_size >= Decimal::ONE {
        // Cap at 6 decimals, trim trailing zeros
        let formatted = format!("{:.6}", size);
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        // Cap at 6 decimals for very small quantities too, trim trailing zeros
        let formatted = format!("{:.6}", size);
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn test_format_price_safe() {
        // Test zero
        assert_eq!(format_price_safe(Decimal::ZERO), "$0.00");

        // Test very small numbers
        assert_eq!(format_price_safe(Decimal::new(1, 7)), "$1.00e-7");
        assert_eq!(format_price_safe(Decimal::new(1, 5)), "$0.00001");

        // Test normal numbers
        assert_eq!(format_price_safe(Decimal::ONE), "$1");
        assert_eq!(format_price_safe(Decimal::new(1234567, 6)), "$1.2345");
        assert_eq!(format_price_safe(Decimal::new(5, 1)), "$0.5");

        // Test large numbers
        assert_eq!(format_price_safe(Decimal::new(15000, 0)), "$15000.00");
        assert_eq!(format_price_safe(Decimal::new(1500000, 0)), "$1.50M");
        assert_eq!(format_price_safe(Decimal::new(1500000000, 0)), "$1.50B");
    }

    #[test]
    fn test_format_roi_safe() {
        // Test normal percentages
        assert_eq!(format_roi_safe(Decimal::new(55, 1)), "5.50%");
        assert_eq!(format_roi_safe(Decimal::new(150, 0)), "150.0%");
        assert_eq!(format_roi_safe(Decimal::new(5, 1)), "0.500%");

        // Test very large percentages
        assert_eq!(format_roi_safe(Decimal::new(15000, 0)), "1.50e4%");
        assert_eq!(format_roi_safe(Decimal::new(2000000, 0)), ">1,000,000%");
        assert_eq!(format_roi_safe(Decimal::new(-2000000, 0)), "<-1,000,000%");
    }

    #[test]
    fn test_format_size_safe() {
        // Test zero
        assert_eq!(format_size_safe(Decimal::ZERO), "0");

        // Test normal sizes
        assert_eq!(format_size_safe(Decimal::ONE), "1");
        assert_eq!(format_size_safe(Decimal::new(1234567, 6)), "1.234567");
        assert_eq!(format_size_safe(Decimal::new(5, 1)), "0.5");

        // Test large sizes
        assert_eq!(format_size_safe(Decimal::new(15000, 0)), "15000.00");
        assert_eq!(format_size_safe(Decimal::new(1500000, 0)), "1.50M");
        assert_eq!(format_size_safe(Decimal::new(1500000000, 0)), "1.50B");
    }

    #[test]
    fn test_validate_price() {
        // Test valid prices
        assert!(validate_price(Decimal::ONE, "test").is_ok());
        assert!(validate_price(Decimal::new(50, 0), "test").is_ok());
        assert!(validate_price(Decimal::new(104_000, 0), "test").is_ok()); // Bitcoin range

        // Test invalid prices
        assert!(validate_price(Decimal::ZERO, "test").is_err());
        assert!(validate_price(Decimal::new(-10, 0), "test").is_err());

        // Test small prices - below $0.10 threshold
        assert!(validate_price(Decimal::new(5, 2), "test").is_err()); // $0.05
        assert!(validate_price(Decimal::new(1, 3), "test").is_err()); // $0.001

        // Test edge case - minimum valid price
        assert!(validate_price(Decimal::new(10, 2), "test").is_ok()); // $0.10

        // Test high prices - above $200k threshold
        assert!(validate_price(Decimal::new(250_000, 0), "test").is_err()); // $250k
        assert!(validate_price(Decimal::new(199_999, 0), "test").is_ok()); // Just under ceiling
    }
}
