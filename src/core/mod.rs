// Core layer modules
pub mod calculations;
pub mod constants;
pub mod domain;
pub mod errors;
pub mod indicators;
pub mod risk;
pub mod strategies;
pub mod utils;

// Re-export commonly used types
pub use errors::{Error, Result};
