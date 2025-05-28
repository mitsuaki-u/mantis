use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use ethers::providers::{Http, Provider};
use ethers::types::Address;
use log::info;

use crate::config::Config;
use crate::core::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub chain_id: u64,
    pub rpc_url: String,
    pub name: String,                   // e.g., "goerli", "mainnet"
    pub native_currency_symbol: String, // e.g., "ETH"
    pub native_currency_decimals: u8,   // Ensure this line is present
    pub explorer_url: Option<String>,
    pub router_address: Address,               // Uniswap V2 Router
    pub weth_address: Address,                 // Wrapped ETH
    pub stablecoin_address: Address,           // USDC/USDT address
    pub weth_stablecoin_pair_address: Address, // WETH/Stablecoin pair for price oracle
    pub factory_address: Address,              // Uniswap V2 Factory
    pub rpc_request_timeout_seconds: Option<u64>,
}

impl NetworkConfig {
    pub fn from_config(dex_config: &Config) -> Result<Self> {
        let network_name = dex_config
            .dex
            .network
            .as_deref()
            .unwrap_or("goerli")
            .to_lowercase();
        let infura_api_key = dex_config.api_keys.infura.as_ref().ok_or_else(|| {
            Error::Config("Infura API key not provided in dex_config".to_string())
        })?;

        // Common configuration for all networks
        let mut config = match network_name.as_str() {
            "goerli" => NetworkConfig {
                chain_id: 5,
                rpc_url: format!("https://goerli.infura.io/v3/{}", infura_api_key),
                name: "goerli".to_string(),
                native_currency_symbol: "ETH".to_string(),
                native_currency_decimals: 18,
                explorer_url: Some("https://goerli.etherscan.io".to_string()),
                router_address: Address::from_str("0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D")
                    .unwrap(), // Uniswap V2 Router on Goerli
                weth_address: Address::from_str("0xB4FBF271143F4FBf7B91A5ded31805e42b2208d6")
                    .unwrap(), // WETH on Goerli
                stablecoin_address: Address::from_str("0xD87Ba7A50B2E7E660f678A895E4B72E7CB4CCd9C")
                    .unwrap(), // USDC on Goerli
                weth_stablecoin_pair_address: Address::from_str(
                    "0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc",
                )
                .unwrap(), // WETH/USDC pair on Goerli
                factory_address: Address::from_str("0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f")
                    .unwrap(), // Uniswap V2 Factory on Goerli
                rpc_request_timeout_seconds: Some(30),
            },
            "mainnet" | "ethereum" => NetworkConfig {
                chain_id: 1,
                rpc_url: format!("https://mainnet.infura.io/v3/{}", infura_api_key),
                name: "mainnet".to_string(),
                native_currency_symbol: "ETH".to_string(),
                native_currency_decimals: 18,
                explorer_url: Some("https://etherscan.io".to_string()),
                router_address: Address::from_str("0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D")
                    .unwrap(), // Uniswap V2 Router on Mainnet
                weth_address: Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")
                    .unwrap(), // WETH on Mainnet
                stablecoin_address: Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")
                    .unwrap(), // USDC on Mainnet
                weth_stablecoin_pair_address: Address::from_str(
                    "0xB4e16d0168e52d35CaCD2c6185b44281Ec28C9Dc",
                )
                .unwrap(), // WETH/USDC pair on Mainnet
                factory_address: Address::from_str("0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f")
                    .unwrap(), // Uniswap V2 Factory on Mainnet
                rpc_request_timeout_seconds: Some(30),
            },
            "sepolia" => NetworkConfig {
                chain_id: 11155111,
                rpc_url: format!("https://sepolia.infura.io/v3/{}", infura_api_key),
                name: "sepolia".to_string(),
                native_currency_symbol: "ETH".to_string(),
                native_currency_decimals: 18,
                explorer_url: Some("https://sepolia.etherscan.io".to_string()),
                router_address: Address::from_str("0xC532a74256D3Db42D0Bf7a0400fEFDbad7694008")
                    .unwrap(), // Uniswap V2 Router on Sepolia
                weth_address: Address::from_str("0x7b79995e5f793A07Bc00c21412e50Ecae098E7f9")
                    .unwrap(), // WETH on Sepolia
                stablecoin_address: Address::from_str("0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238")
                    .unwrap(), // USDC on Sepolia
                weth_stablecoin_pair_address: Address::from_str(
                    "0x4d1f38D3cB24F31c665d0f01dB0d6E14Eb0B8921",
                )
                .unwrap(), // WETH/USDC pair on Sepolia
                factory_address: Address::from_str("0x5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f")
                    .unwrap(), // Uniswap V2 Factory on Sepolia
                rpc_request_timeout_seconds: Some(30),
            },
            "polygon" | "matic" => NetworkConfig {
                chain_id: 137,
                rpc_url: format!("https://polygon-mainnet.infura.io/v3/{}", infura_api_key),
                name: "polygon".to_string(),
                native_currency_symbol: "MATIC".to_string(),
                native_currency_decimals: 18,
                explorer_url: Some("https://polygonscan.com".to_string()),
                router_address: Address::from_str("0xa5E0829CaCEd8fFDD4De3c43696c57F7D7A678ff")
                    .unwrap(), // QuickSwap Router on Polygon
                weth_address: Address::from_str("0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619")
                    .unwrap(), // WETH on Polygon
                stablecoin_address: Address::from_str("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174")
                    .unwrap(), // USDC on Polygon
                weth_stablecoin_pair_address: Address::from_str(
                    "0x853Ee4b2A13f8a742d64C8F088bE7bA2131f670d",
                )
                .unwrap(), // WETH/USDC pair on Polygon
                factory_address: Address::from_str("0x5757371414417b8C6CAad45bAeF941aBc7d3Ab32")
                    .unwrap(), // QuickSwap Factory on Polygon
                rpc_request_timeout_seconds: Some(30),
            },
            "mumbai" => NetworkConfig {
                chain_id: 80001,
                rpc_url: format!("https://polygon-mumbai.infura.io/v3/{}", infura_api_key),
                name: "mumbai".to_string(),
                native_currency_symbol: "MATIC".to_string(),
                native_currency_decimals: 18,
                explorer_url: Some("https://mumbai.polygonscan.com".to_string()),
                router_address: Address::from_str("0xa5E0829CaCEd8fFDD4De3c43696c57F7D7A678ff")
                    .unwrap(), // QuickSwap Router on Mumbai
                weth_address: Address::from_str("0xA6FA4fB5f76172d178d61B04b0ecd319C5d1C0aa")
                    .unwrap(), // WETH on Mumbai
                stablecoin_address: Address::from_str("0xe6b8a5CF854791412c1f6EFC7CAf629f5Df1c747")
                    .unwrap(), // USDC on Mumbai
                weth_stablecoin_pair_address: Address::from_str(
                    "0x572dDec9087154dC5dfBB1546Bb62713147e0Ab0",
                )
                .unwrap(), // WETH/USDC pair on Mumbai
                factory_address: Address::from_str("0x5757371414417b8C6CAad45bAeF941aBc7d3Ab32")
                    .unwrap(), // QuickSwap Factory on Mumbai
                rpc_request_timeout_seconds: Some(30),
            },
            _ => {
                return Err(Error::Config(format!(
                    "Unsupported network: {}",
                    network_name
                )))
            }
        };

        // Override RPC URL if custom RPC URL is provided in config
        if let Some(custom_rpc) = &dex_config.dex.custom_rpc_url {
            if !custom_rpc.is_empty() {
                config.rpc_url = custom_rpc.clone();
                info!("Using custom RPC URL from config");
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
}
