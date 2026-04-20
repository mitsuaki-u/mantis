//! Trading Strategy Module
//!
//! This module provides a comprehensive framework for implementing and managing
//! trading strategies. It is organized into clear submodules:
//!
//! - [`traits`] - Core trait definitions and types (`TradingStrategy`, `Signal`, etc.)
//! - [`exit_conditions`] - Shared exit condition utilities
//! - [`custom`] - Custom strategy implementations (momentum, rsi, etc.)
//! - [`factory`] - Strategy creation and registration
//!
//! ## Usage
//!
//! ```rust
//! use crate::core::trading::strategies::{
//!     factory::create_strategy,
//!     traits::{Strategy, Signal},
//! };
//!
//! // Create a momentum strategy
//! let strategy = create_strategy("momentum", 5.0, 1_000_000.0, 10.0, None)?;
//!
//! // Analyze a token for entry signals
//! let should_buy = strategy.analyze_for_entry(&token_metrics);
//! ```

pub mod custom;
pub mod exit_conditions;
pub mod factory;
pub mod traits;
