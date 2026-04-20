//! Safe type conversion utilities for numeric types
//!
//! This module provides safe conversion between Decimal and f64 types with proper
//! error handling to avoid silent data corruption from failed conversions.

use crate::core::errors::Error;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;

/// Convert Decimal to f64 with validation
///
/// Returns an error if the conversion fails (e.g., value too large for f64)
/// Use this instead of `.to_f64().unwrap_or(...)` which silently corrupts data
///
/// # Arguments
/// * `value` - Decimal value to convert
/// * `context` - Description of what this value represents (for error messages)
///
/// # Examples
/// ```
/// let price = Decimal::from_str("123.45").unwrap();
/// let price_f64 = decimal_to_f64(price, "token price")?;
/// ```
pub fn decimal_to_f64(value: Decimal, context: &str) -> Result<f64, Error> {
    value.to_f64().ok_or_else(|| {
        Error::Conversion(format!(
            "Failed to convert {} ({}) from Decimal to f64",
            context, value
        ))
    })
}

/// Convert f64 to Decimal with validation
///
/// Returns an error if the conversion fails (e.g., NaN, Infinity)
/// Use this instead of `.from_f64(...).unwrap_or(...)` which silently corrupts data
///
/// # Arguments
/// * `value` - f64 value to convert
/// * `context` - Description of what this value represents (for error messages)
///
/// # Examples
/// ```
/// let amount = 123.45;
/// let amount_decimal = f64_to_decimal(amount, "USD amount")?;
/// ```
pub fn f64_to_decimal(value: f64, context: &str) -> Result<Decimal, Error> {
    // Check for invalid f64 values first
    if value.is_nan() {
        return Err(Error::Conversion(format!(
            "Cannot convert {} to Decimal: value is NaN",
            context
        )));
    }

    if value.is_infinite() {
        return Err(Error::Conversion(format!(
            "Cannot convert {} to Decimal: value is infinite",
            context
        )));
    }

    Decimal::from_f64(value).ok_or_else(|| {
        Error::Conversion(format!(
            "Failed to convert {} ({}) from f64 to Decimal",
            context, value
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_decimal_to_f64_success() {
        let value = Decimal::from_str("123.45").unwrap();
        let result = decimal_to_f64(value, "test value").unwrap();
        assert_eq!(result, 123.45);
    }

    #[test]
    fn test_f64_to_decimal_success() {
        let result = f64_to_decimal(123.45, "test value").unwrap();
        assert_eq!(result, Decimal::from_str("123.45").unwrap());
    }

    #[test]
    fn test_f64_to_decimal_nan() {
        let result = f64_to_decimal(f64::NAN, "test value");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NaN"));
    }

    #[test]
    fn test_f64_to_decimal_infinity() {
        let result = f64_to_decimal(f64::INFINITY, "test value");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("infinite"));
    }

    #[test]
    fn test_round_trip() {
        let original = 123.45;
        let decimal = f64_to_decimal(original, "test").unwrap();
        let back_to_f64 = decimal_to_f64(decimal, "test").unwrap();
        assert!((original - back_to_f64).abs() < 0.0001);
    }
}
