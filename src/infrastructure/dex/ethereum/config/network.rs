use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use ethers::providers::{Http, Provider};
use ethers::types::Address;
use log::{debug, info};

use crate::config::Config;
use crate::infrastructure::dex::ethereum::config::addresses::{
    get_network_addresses, validate_addresses,
};
use crate::infrastructure::dex::ethereum::rpc;
use crate::infrastructure::errors::{Error, Result};

// Re-export addresses for convenience
pub use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub chain_id: u64,
    pub rpc_url: String,
    pub name: String,                   // e.g., "goerli", "mainnet"
    pub native_currency_symbol: String, // e.g., "ETH"
    pub native_currency_decimals: u8,   // Ensure this line is present
    pub explorer_url: Option<String>,
    pub router_address: Address,               // Uniswap V3 SwapRouter
    pub quoter_address: Address,               // Uniswap V3 Quoter
    pub weth_address: Address,                 // Wrapped ETH
    pub stablecoin_address: Address,           // USDC/USDT address
    pub weth_stablecoin_pair_address: Address, // V3: Not used (dynamic pool discovery)
    pub factory_address: Address,              // Uniswap V3 Factory
    pub rpc_request_timeout_seconds: Option<u64>,
    pub is_testnet: bool,
    pub gas_price_gwei: f64,
    pub gas_limit: u64,
}

impl NetworkConfig {
    pub fn from_config(dex_config: &Config) -> Result<Self> {
        let network_name = dex_config
            .dex
            .network
            .as_deref()
            .ok_or_else(|| Error::Config("Network not specified in configuration".to_string()))?
            .to_lowercase();

        // Determine RPC provider based on primary_provider configuration
        let primary_provider = &dex_config.rpc.primary_provider.to_lowercase();

        let (rpc_provider, api_key) = match primary_provider.as_str() {
            "alchemy" => {
                if let Some(key) = &dex_config.api_keys.alchemy {
                    ("alchemy", key.clone())
                } else {
                    return Err(Error::Config(
                        "Primary provider set to 'alchemy' but no Alchemy API key found. Set api_keys.alchemy or change rpc.primary_provider.".to_string()
                    ));
                }
            }
            "infura" => {
                if let Some(key) = &dex_config.api_keys.infura {
                    ("infura", key.clone())
                } else {
                    return Err(Error::Config(
                        "Primary provider set to 'infura' but no Infura API key found. Set api_keys.infura or change rpc.primary_provider.".to_string()
                    ));
                }
            }
            _ => {
                return Err(Error::Config(format!(
                    "Unknown primary RPC provider '{}'. Supported: alchemy, infura",
                    primary_provider
                )));
            }
        };

        debug!(
            "Using {} as RPC provider for network: {}",
            rpc_provider, network_name
        );

        // Get contract addresses for the specified network
        let addresses = get_network_addresses(&network_name)?;
        validate_addresses(&addresses)?;

        // Create base configuration using the modular addresses
        let mut config =
            Self::create_network_config(&network_name, &addresses, &api_key, rpc_provider)?;

        // Override RPC URL if custom RPC URL is provided in config
        if let Some(custom_rpc) = &dex_config.dex.custom_rpc_url {
            if !custom_rpc.is_empty() {
                config.rpc_url = custom_rpc.clone();
                debug!("Using custom RPC URL from config");
            }
        }

        // Override router address if provided in config
        if let Some(router) = &dex_config.dex.router_address {
            if !router.is_empty() {
                config.router_address = Address::from_str(router)
                    .map_err(|e| Error::Config(format!("Invalid router address: {}", e)))?;
                info!("Using custom router address from config");
            }
        }

        // Override WETH address if provided in config
        if let Some(weth) = &dex_config.dex.weth_address {
            if !weth.is_empty() {
                config.weth_address = Address::from_str(weth)
                    .map_err(|e| Error::Config(format!("Invalid WETH address: {}", e)))?;
                info!("Using custom WETH address from config");
            }
        }

        Ok(config)
    }

    fn create_network_config(
        network: &str,
        addresses: &NetworkAddresses,
        api_key: &str,
        rpc_provider: &str,
    ) -> Result<Self> {
        let rpc_url = match (network, rpc_provider) {
            ("mainnet" | "ethereum", "alchemy") => {
                format!("https://eth-mainnet.g.alchemy.com/v2/{}", api_key)
            }
            ("mainnet" | "ethereum", "infura") => {
                format!("https://mainnet.infura.io/v3/{}", api_key)
            }
            ("mainnet" | "ethereum", _) => {
                return Err(Error::Config(format!(
                    "Unknown RPC provider: {}",
                    rpc_provider
                )))
            }
            _ => return Err(Error::Config(format!("Unsupported network: {}", network))),
        };

        match network {
            "mainnet" | "ethereum" => Ok(Self {
                chain_id: 1,
                rpc_url,
                name: "mainnet".to_string(),
                native_currency_symbol: "ETH".to_string(),
                native_currency_decimals: 18,
                explorer_url: Some("https://etherscan.io".to_string()),
                router_address: addresses.router,
                quoter_address: addresses.quoter,
                weth_address: addresses.weth,
                stablecoin_address: addresses.usdc,
                weth_stablecoin_pair_address: addresses.weth_usdc_pair,
                factory_address: addresses.factory,
                rpc_request_timeout_seconds: Some(30),
                is_testnet: false,
                gas_price_gwei: 20.0,
                gas_limit: 200000,
            }),
            _ => Err(Error::Config(format!("Unsupported network: {}", network))),
        }
    }

    /// Check if a network is supported
    pub fn is_network_supported(network_name: &str) -> bool {
        get_network_addresses(network_name).is_ok()
    }

    /// Get contract addresses for the current network
    pub fn get_current_addresses(&self) -> Result<NetworkAddresses> {
        get_network_addresses(&self.name)
    }

    pub fn with_infura_key(&mut self, new_key: &str) {
        if self.rpc_url.contains("infura.io") {
            // Replace the API key in the URL
            if let Some(start) = self.rpc_url.rfind('/') {
                self.rpc_url = format!("{}/{}", &self.rpc_url[..start], new_key);
            }
        }
    }

    pub fn create_provider(&self) -> Result<Arc<Provider<Http>>> {
        let provider = Provider::<Http>::try_from(&self.rpc_url)
            .map_err(|e| Error::Network(format!("Failed to create provider: {}", e)))?;

        // Set timeout if specified
        let provider = if let Some(timeout_secs) = self.rpc_request_timeout_seconds {
            provider.interval(Duration::from_secs(timeout_secs))
        } else {
            provider
        };

        Ok(Arc::new(provider))
    }

    /// Create an RPC provider using configured provider settings (Alchemy, Infura, or Free)
    pub fn create_configured_provider(
        &self,
        config: &crate::config::Config,
    ) -> Result<Arc<Provider<Http>>> {
        rpc::create_provider(self.chain_id, config)
            .map_err(|e| Error::Network(format!("Failed to create RPC provider: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_support() {
        // Test that we can normalize supported network names
        assert_eq!(
            crate::config::NetworkConfig::normalize_network_name(Some(&"mainnet".to_string())),
            "ethereum"
        );
        assert_eq!(
            crate::config::NetworkConfig::normalize_network_name(Some(&"ethereum".to_string())),
            "ethereum"
        );
        assert_eq!(
            crate::config::NetworkConfig::normalize_network_name(Some(&"polygon".to_string())),
            "polygon"
        );
        assert_eq!(
            crate::config::NetworkConfig::normalize_network_name(Some(&"amoy".to_string())),
            "amoy"
        );

        // Test that unsupported networks default to testnet
        assert_eq!(
            crate::config::NetworkConfig::normalize_network_name(Some(&"unknown".to_string())),
            "unknown"
        );
    }

    #[test]
    fn test_addresses_validation() {
        let mainnet_addresses = get_network_addresses("mainnet").unwrap();
        assert!(validate_addresses(&mainnet_addresses).is_ok());
    }
}
