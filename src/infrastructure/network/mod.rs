//! Network utility functions
//!
//! This module provides network-related utilities that are used throughout the codebase,
//! including chain ID resolution and network name normalization.

/// Network-specific constants
pub mod constants {
    /// Ethereum mainnet chain ID
    pub const ETHEREUM_CHAIN_ID: u64 = 1;

    /// Polygon mainnet chain ID
    pub const POLYGON_CHAIN_ID: u64 = 137;

    /// Optimism mainnet chain ID
    pub const OPTIMISM_CHAIN_ID: u64 = 10;

    /// Arbitrum One chain ID
    pub const ARBITRUM_CHAIN_ID: u64 = 42161;
}

/// Get chain ID for network name
pub fn get_chain_id(network: Option<&String>) -> u64 {
    match network.map(|s| s.as_str()) {
        Some("mainnet") | Some("ethereum") => constants::ETHEREUM_CHAIN_ID,
        Some("polygon") | Some("matic") => constants::POLYGON_CHAIN_ID,
        Some("optimism") | Some("op") => constants::OPTIMISM_CHAIN_ID,
        Some("arbitrum") | Some("arbitrum-one") | Some("arb") => constants::ARBITRUM_CHAIN_ID,
        _ => constants::ETHEREUM_CHAIN_ID, // Default to Ethereum
    }
}

/// Normalize network name for consistent usage
pub fn normalize_network_name(network: Option<&String>) -> String {
    match network.map(|s| s.as_str()) {
        Some("mainnet") | Some("ethereum") => "ethereum".to_string(),
        Some("polygon") | Some("matic") => "polygon".to_string(),
        Some("optimism") | Some("op") => "optimism".to_string(),
        Some("arbitrum") | Some("arbitrum-one") | Some("arb") => "arbitrum".to_string(),
        Some(other) => other.to_string(),
        None => "ethereum".to_string(), // Default to Ethereum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_chain_id() {
        assert_eq!(
            get_chain_id(Some(&"ethereum".to_string())),
            constants::ETHEREUM_CHAIN_ID
        );
        assert_eq!(
            get_chain_id(Some(&"mainnet".to_string())),
            constants::ETHEREUM_CHAIN_ID
        );
        assert_eq!(
            get_chain_id(Some(&"polygon".to_string())),
            constants::POLYGON_CHAIN_ID
        );
        assert_eq!(
            get_chain_id(Some(&"matic".to_string())),
            constants::POLYGON_CHAIN_ID
        );
        assert_eq!(
            get_chain_id(Some(&"optimism".to_string())),
            constants::OPTIMISM_CHAIN_ID
        );
        assert_eq!(
            get_chain_id(Some(&"op".to_string())),
            constants::OPTIMISM_CHAIN_ID
        );
        assert_eq!(
            get_chain_id(Some(&"arbitrum".to_string())),
            constants::ARBITRUM_CHAIN_ID
        );
        assert_eq!(
            get_chain_id(Some(&"arbitrum-one".to_string())),
            constants::ARBITRUM_CHAIN_ID
        );
        assert_eq!(
            get_chain_id(Some(&"arb".to_string())),
            constants::ARBITRUM_CHAIN_ID
        );
        assert_eq!(get_chain_id(None), constants::ETHEREUM_CHAIN_ID);
        assert_eq!(
            get_chain_id(Some(&"unknown".to_string())),
            constants::ETHEREUM_CHAIN_ID
        );
    }

    #[test]
    fn test_normalize_network_name() {
        assert_eq!(
            normalize_network_name(Some(&"mainnet".to_string())),
            "ethereum"
        );
        assert_eq!(
            normalize_network_name(Some(&"ethereum".to_string())),
            "ethereum"
        );
        assert_eq!(
            normalize_network_name(Some(&"matic".to_string())),
            "polygon"
        );
        assert_eq!(
            normalize_network_name(Some(&"polygon".to_string())),
            "polygon"
        );
        assert_eq!(normalize_network_name(Some(&"op".to_string())), "optimism");
        assert_eq!(
            normalize_network_name(Some(&"optimism".to_string())),
            "optimism"
        );
        assert_eq!(normalize_network_name(Some(&"arb".to_string())), "arbitrum");
        assert_eq!(
            normalize_network_name(Some(&"arbitrum".to_string())),
            "arbitrum"
        );
        assert_eq!(
            normalize_network_name(Some(&"arbitrum-one".to_string())),
            "arbitrum"
        );
        assert_eq!(normalize_network_name(None), "ethereum");
    }
}
