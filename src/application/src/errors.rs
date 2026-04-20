//! Application layer errors
//!
//! Re-exports the unified Error type from the root module.
//! All error variants are available through crate::errors::Error

pub use crate::errors::{Error, ErrorExt, Result};
