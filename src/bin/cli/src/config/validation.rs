//! Custom validation logic for configuration values.
//!
//! This module provides custom validation functions that can be used
//! with the validator crate for complex validation rules.

use validator::ValidationError;

/// Custom validation function for indicator weights
/// Ensures all weights are non-negative and sum to approximately 1.0
pub fn validate_indicator_weights(
    rsi_weight: f64,
    macd_weight: f64,
    bollinger_weight: f64,
    volume_weight: f64,
) -> Result<(), ValidationError> {
    let weights = [rsi_weight, macd_weight, bollinger_weight, volume_weight];

    // Check all weights are non-negative
    for &weight in weights.iter() {
        if weight < 0.0 {
            return Err(ValidationError::new("Weight cannot be negative"));
        }
    }

    // Check weights sum to approximately 1.0 (allow small floating point errors)
    let sum: f64 = weights.iter().sum();
    if (sum - 1.0).abs() > 0.001 {
        return Err(ValidationError::new("Indicator weights must sum to 1.0"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_indicator_weights() {
        // Valid weights that sum to 1.0
        assert!(validate_indicator_weights(0.3, 0.3, 0.2, 0.2).is_ok());

        // Weights that don't sum to 1.0
        assert!(validate_indicator_weights(0.5, 0.3, 0.2, 0.2).is_err());

        // Negative weight
        assert!(validate_indicator_weights(-0.1, 0.4, 0.4, 0.3).is_err());

        // Allow small floating point errors
        assert!(validate_indicator_weights(0.3001, 0.3, 0.2, 0.1999).is_ok());
    }
}
