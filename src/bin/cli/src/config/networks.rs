//! Network-specific configuration utilities.
//!
//! This module handles stablecoin address resolution and network-specific token recommendations.

use crate::error::Result;
use log::{info, warn};

/// Network configuration utilities
pub struct NetworkConfig;

impl NetworkConfig {
    /// Get recommended tokens for the current network
    pub fn get_recommended_tokens(_network: Option<&String>) -> Vec<String> {
        // Return common high-volume tokens for mainnet
        vec![
            "ethereum".to_string(),
            "bitcoin".to_string(),
            "usdc".to_string(),
            "usdt".to_string(),
            "bnb".to_string(),
            "solana".to_string(),
            "cardano".to_string(),
            "polygon".to_string(),
            "chainlink".to_string(),
            "avalanche".to_string(),
        ]
    }

    /// Get stablecoin address based on network
    pub fn stablecoin_address(
        network: Option<&String>,
        configured_address: Option<&String>,
    ) -> String {
        // Use configured stablecoin_address if available
        if let Some(configured_address) = configured_address {
            return configured_address.clone();
        }

        // Network-specific stablecoin addresses
        use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;
        match network.map(|s| s.as_str()) {
            Some("mainnet") | Some("ethereum") => {
                format!("{:?}", NetworkAddresses::get_usdc_address("ethereum"))
            } // USDC on Ethereum
            Some("polygon") | Some("matic") => {
                format!("{:?}", NetworkAddresses::get_usdc_address("polygon"))
            } // USDC on Polygon
            Some("optimism") | Some("op") => {
                format!("{:?}", NetworkAddresses::get_usdc_address("optimism"))
            } // USDC on Optimism
            Some("arbitrum") | Some("arbitrum-one") | Some("arb") => {
                format!("{:?}", NetworkAddresses::get_usdc_address("arbitrum"))
            } // USDC on Arbitrum
            _ => format!("{:?}", NetworkAddresses::get_usdc_address("ethereum")), // Default to Ethereum USDC
        }
    }

    /// Get stablecoin address from authoritative sources (async version)
    pub async fn stablecoin_address_from_source(
        network: Option<&String>,
        configured_address: Option<&String>,
    ) -> Result<String> {
        use crate::infrastructure::dex::ethereum::tokens::TokenRegistryService;

        // Use configured stablecoin_address if available
        if let Some(configured_address) = configured_address {
            info!(
                "✅ Using configured stablecoin address: {}",
                configured_address
            );
            return Ok(configured_address.clone());
        }

        // Determine the network name for CoinGecko resolution
        let network_name = match network.map(|s| s.as_str()) {
            Some("mainnet") | Some("ethereum") => "ethereum",
            Some("polygon") | Some("matic") => "polygon",
            Some("optimism") | Some("op") => "optimism",
            Some("arbitrum") | Some("arbitrum-one") | Some("arb") => "arbitrum",
            _ => "ethereum", // Default to Ethereum
        };

        info!(
            "🔍 Fetching USDC address from authoritative sources for network: {}",
            network_name
        );

        // Get the singleton TokenRegistry instance for resolution
        let token_registry = TokenRegistryService::get();

        // Try to resolve USDC from CoinGecko API using the "usd-coin" CoinGecko ID
        match token_registry.resolve_token("usd-coin", network_name).await {
            Ok(address) => {
                let address_str = format!("0x{:x}", address);
                info!(
                    "✅ Successfully resolved USDC address from CoinGecko API: {} (network: {})",
                    address_str, network_name
                );
                Ok(address_str)
            }
            Err(e) => {
                warn!("⚠️ Failed to resolve USDC from CoinGecko API: {}. Falling back to hardcoded address.", e);

                // Fallback to hardcoded addresses
                let fallback_address = Self::stablecoin_address(network, configured_address);

                info!(
                    "📋 Using fallback USDC address: {} (network: {})",
                    fallback_address, network_name
                );
                Ok(fallback_address)
            }
        }
    }

    /// Get WETH address based on network
    pub fn weth_address(network: Option<&String>, configured_address: Option<&String>) -> String {
        // Use configured weth_address if available
        if let Some(configured_address) = configured_address {
            return configured_address.clone();
        }

        // Network-specific WETH addresses
        use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;
        match network.map(|s| s.as_str()) {
            Some("mainnet") | Some("ethereum") => {
                format!("{:?}", NetworkAddresses::mainnet().weth)
            }
            Some("polygon") | Some("matic") => {
                format!("{:?}", NetworkAddresses::get_weth_address("polygon"))
            }
            Some("optimism") | Some("op") => {
                format!("{:?}", NetworkAddresses::get_weth_address("optimism"))
            }
            Some("arbitrum") | Some("arbitrum-one") | Some("arb") => {
                format!("{:?}", NetworkAddresses::get_weth_address("arbitrum"))
            }
            _ => format!("{:?}", NetworkAddresses::mainnet().weth), // Default to Ethereum WETH
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

    /// Get network-specific RPC endpoints
    pub fn get_default_rpc_endpoints(network: Option<&String>) -> Vec<String> {
        match network.map(|s| s.as_str()) {
            Some("mainnet") | Some("ethereum") => vec![
                "https://eth-mainnet.g.alchemy.com/v2".to_string(),
                "https://mainnet.infura.io/v3".to_string(),
            ],
            Some("polygon") | Some("matic") => vec![
                "https://polygon-mainnet.g.alchemy.com/v2".to_string(),
                "https://polygon-mainnet.infura.io/v3".to_string(),
            ],
            Some("optimism") | Some("op") => vec![
                "https://opt-mainnet.g.alchemy.com/v2".to_string(),
                "https://optimism-mainnet.infura.io/v3".to_string(),
            ],
            Some("arbitrum") | Some("arbitrum-one") | Some("arb") => vec![
                "https://arb-mainnet.g.alchemy.com/v2".to_string(),
                "https://arbitrum-mainnet.infura.io/v3".to_string(),
            ],
            _ => vec![
                "https://eth-mainnet.g.alchemy.com/v2".to_string(),
                "https://mainnet.infura.io/v3".to_string(),
            ],
        }
    }
}

/// Network-specific constants
pub mod constants {
    // Re-export from shared utils for backward compatibility
    pub use crate::infrastructure::network::constants::*;
    pub use crate::infrastructure::network::get_chain_id;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_recommended_tokens() {
        let tokens = NetworkConfig::get_recommended_tokens(Some(&"ethereum".to_string()));
        assert!(tokens.contains(&"ethereum".to_string()));
        assert!(tokens.contains(&"bitcoin".to_string()));
        assert_eq!(tokens.len(), 10);
    }

    #[test]
    fn test_stablecoin_address() {
        // Test with configured address
        let configured = Some("0x1234567890123456789012345678901234567890".to_string());
        assert_eq!(
            NetworkConfig::stablecoin_address(None, configured.as_ref()),
            "0x1234567890123456789012345678901234567890"
        );

        // Test Ethereum (case-insensitive comparison since Debug fmt uses lowercase)
        assert_eq!(
            NetworkConfig::stablecoin_address(Some(&"ethereum".to_string()), None).to_lowercase(),
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        );

        // Test Polygon
        assert_eq!(
            NetworkConfig::stablecoin_address(Some(&"polygon".to_string()), None).to_lowercase(),
            "0x2791bca1f2de4661ed88a30c99a7a9449aa84174"
        );

        // Test default
        assert_eq!(
            NetworkConfig::stablecoin_address(Some(&"unknown".to_string()), None).to_lowercase(),
            "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"
        );
    }

    #[test]
    fn test_normalize_network_name() {
        assert_eq!(
            NetworkConfig::normalize_network_name(Some(&"mainnet".to_string())),
            "ethereum"
        );
        assert_eq!(
            NetworkConfig::normalize_network_name(Some(&"ethereum".to_string())),
            "ethereum"
        );
        assert_eq!(
            NetworkConfig::normalize_network_name(Some(&"matic".to_string())),
            "polygon"
        );
        assert_eq!(
            NetworkConfig::normalize_network_name(Some(&"polygon".to_string())),
            "polygon"
        );
        assert_eq!(NetworkConfig::normalize_network_name(None), "ethereum");
    }

    #[test]
    fn test_get_chain_id() {
        use crate::infrastructure::network::constants::*;
        use crate::infrastructure::network::get_chain_id;

        assert_eq!(
            get_chain_id(Some(&"ethereum".to_string())),
            ETHEREUM_CHAIN_ID
        );
        assert_eq!(get_chain_id(Some(&"polygon".to_string())), POLYGON_CHAIN_ID);
        assert_eq!(get_chain_id(None), ETHEREUM_CHAIN_ID);
    }

    #[test]
    fn test_get_default_rpc_endpoints() {
        let eth_endpoints = NetworkConfig::get_default_rpc_endpoints(Some(&"ethereum".to_string()));
        assert_eq!(eth_endpoints.len(), 2);
        assert!(eth_endpoints.iter().any(|e| e.contains("alchemy")));
        assert!(eth_endpoints.iter().any(|e| e.contains("infura")));
    }
}
