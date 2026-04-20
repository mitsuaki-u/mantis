pub mod conversion;
pub mod display;
pub mod formatting;
pub mod normalization;
pub mod validation;

// Re-export commonly used formatting functions
pub use formatting::{format_price_safe, format_roi_safe, format_size_safe};

// Re-export commonly used conversion functions
pub use conversion::{decimal_to_f64, f64_to_decimal};
