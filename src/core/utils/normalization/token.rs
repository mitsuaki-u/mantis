//! Token ID utility functions
//!
//! This module provides utilities for working with the secure token ID format:
//! `chain_id:contract_address` (e.g., "1:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")

use crate::core::errors::{Error, Result};
use ethers::types::Address;
use std::str::FromStr;

/// Create a secure token ID from chain ID and contract address
pub fn create_token_id(chain_id: u64, contract_address: &str) -> Result<String> {
    // Validate contract address format
    Address::from_str(contract_address)
        .map_err(|e| Error::InvalidInput(format!("Invalid contract address: {}", e)))?;

    Ok(format!("{}:{}", chain_id, contract_address.to_lowercase()))
}

/// Parse a token ID into chain ID and contract address
pub fn parse_token_id(token_id: &str) -> Result<(u64, String)> {
    let parts: Vec<&str> = token_id.split(':').collect();
    if parts.len() != 2 {
        return Err(Error::InvalidInput(
            "Token ID must be in format 'chain_id:contract_address'".to_string(),
        ));
    }

    let chain_id = parts[0]
        .parse::<u64>()
        .map_err(|e| Error::InvalidInput(format!("Invalid chain ID: {}", e)))?;

    let contract_address = parts[1].to_string();

    // Validate contract address format
    Address::from_str(&contract_address)
        .map_err(|e| Error::InvalidInput(format!("Invalid contract address: {}", e)))?;

    Ok((chain_id, contract_address))
}

/// Extract contract address from a token ID
/// Returns the address as-is if not in token ID format
pub fn extract_address(token_id: &str) -> String {
    if token_id.contains(':') {
        // Extract address portion from chain_id:address format
        match parse_token_id(token_id) {
            Ok((_chain_id, address)) => address,
            Err(_) => token_id.to_string(), // Fallback to original if parsing fails
        }
    } else {
        token_id.to_string()
    }
}

/// Get chain name from chain ID
pub fn get_chain_name(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "ethereum",
        137 => "polygon",
        42161 => "arbitrum",
        10 => "optimism",
        8453 => "base",
        43114 => "avalanche",
        56 => "bsc",
        _ => "unknown",
    }
}

/// Create display-friendly token representation
pub fn format_token_display(token_id: &str, symbol: &str) -> Result<String> {
    let (chain_id, _) = parse_token_id(token_id)?;
    let chain_name = get_chain_name(chain_id);
    Ok(format!("{} ({})", symbol, chain_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_token_id() {
        let result = create_token_id(1, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "1:0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        );

        // Test invalid address
        let result = create_token_id(1, "invalid_address");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_token_id() {
        let token_id = "1:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
        let result = parse_token_id(token_id);
        assert!(result.is_ok());

        let (chain_id, address) = result.unwrap();
        assert_eq!(chain_id, 1);
        assert_eq!(address, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

        // Test invalid format
        assert!(parse_token_id("invalid").is_err());
        assert!(parse_token_id("1:2:3").is_err());
        assert!(parse_token_id("abc:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").is_err());
    }

    #[test]
    fn test_extract_address() {
        // Test with full token ID
        let address = extract_address("1:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        assert_eq!(address, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

        // Test with just address (no chain prefix)
        let address = extract_address("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        assert_eq!(address, "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

        // Test with invalid format (fallback to original)
        let address = extract_address("invalid:format");
        assert_eq!(address, "invalid:format");
    }

    #[test]
    fn test_get_chain_name() {
        assert_eq!(get_chain_name(1), "ethereum");
        assert_eq!(get_chain_name(137), "polygon");
        assert_eq!(get_chain_name(42161), "arbitrum");
        assert_eq!(get_chain_name(10), "optimism");
        assert_eq!(get_chain_name(999), "unknown");
    }

    #[test]
    fn test_format_token_display() {
        let token_id = "1:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48";
        let display = format_token_display(token_id, "USDC").unwrap();
        assert_eq!(display, "USDC (ethereum)");

        let token_id = "137:0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174";
        let display = format_token_display(token_id, "USDC").unwrap();
        assert_eq!(display, "USDC (polygon)");
    }
}
