use crate::infrastructure::errors::{Error, Result};
use ethers::types::Address;
use std::str::FromStr;

/// Contract addresses for different blockchain networks
///
/// Sources:
/// - Uniswap V2: https://docs.uniswap.org/contracts/v2/reference/smart-contracts/v2-deployments
/// - QuickSwap: https://github.com/QuickSwap/core_swap for Amoy addresses
/// - USDC: https://developers.circle.com/stablecoins/usdc-contract-addresses
/// - WETH: Official Uniswap V3 docs and Etherscan verification
///
/// Last verified: January 2025

#[derive(Debug, Clone)]
pub struct NetworkAddresses {
    pub factory: Address,
    pub router: Address,
    pub quoter: Address, // Add Quoter contract address
    pub weth: Address,
    pub wbtc: Address, // Wrapped Bitcoin
    pub usdc: Address,
    pub dai: Address,
    pub usdt: Address,           // Tether USD
    pub weth_usdc_pair: Address, // V3: Always zero (dynamic pool discovery), V2: Pair address
}

impl NetworkAddresses {
    /// Get contract addresses for Ethereum Mainnet (V3)
    /// Source: Uniswap V3 official docs, Circle USDC docs
    ///
    /// Note: unwrap() is safe here as these are hardcoded, verified Ethereum addresses
    /// that are guaranteed to parse correctly. These addresses are compile-time constants
    /// from official Uniswap V3 and token contract deployments.
    pub fn mainnet() -> Self {
        Self {
            factory: Address::from_str("0x1F98431c8aD98523631AE4a59f267346ea31F984").unwrap(), // V3 Factory
            router: Address::from_str("0xE592427A0AEce92De3Edee1F18E0157C05861564").unwrap(), // V3 SwapRouter
            quoter: Address::from_str("0xb27308f9F90D607463bb33eA1BeBb41C27CE5AB6").unwrap(), // V3 Quoter
            weth: Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap(),
            wbtc: Address::from_str("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599").unwrap(),
            usdc: Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap(),
            dai: Address::from_str("0x6B175474E89094C44Da98b954EedeAC495271d0F").unwrap(),
            usdt: Address::from_str("0xdAC17F958D2ee523a2206206994597C13D831ec7").unwrap(),
            weth_usdc_pair: Address::zero(), // V3 uses dynamic pool discovery, not fixed pairs
        }
    }

    /// Get USDC address for different networks
    ///
    /// Note: unwrap() is safe here - these are official USDC contract addresses
    /// that are hardcoded and verified. They cannot fail to parse.
    pub fn get_usdc_address(network: &str) -> Address {
        match network.to_lowercase().as_str() {
            "mainnet" | "ethereum" => {
                Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap()
            }
            "polygon" | "matic" => {
                Address::from_str("0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174").unwrap()
            }
            "optimism" | "op" => {
                Address::from_str("0x7F5c764cBc14f9669B88837ca1490cCa17c31607").unwrap()
            }
            "arbitrum" | "arbitrum-one" | "arb" => {
                Address::from_str("0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8").unwrap()
            }
            _ => Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap(), // Default to Ethereum
        }
    }

    /// Get WETH address for different networks
    ///
    /// Note: unwrap() is safe here - these are official WETH contract addresses
    /// that are hardcoded and verified. They cannot fail to parse.
    pub fn get_weth_address(network: &str) -> Address {
        match network.to_lowercase().as_str() {
            "mainnet" | "ethereum" => {
                Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap()
            }
            "polygon" | "matic" => {
                Address::from_str("0x7ceB23fD6bC0adD59E62ac25578270cFf1b9f619").unwrap()
            } // Wrapped ETH on Polygon
            "optimism" | "op" => {
                Address::from_str("0x4200000000000000000000000000000000000006").unwrap()
            } // WETH on Optimism
            "arbitrum" | "arbitrum-one" | "arb" => {
                Address::from_str("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1").unwrap()
            } // WETH on Arbitrum
            _ => Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap(), // Default to Ethereum
        }
    }
}

/// Get contract addresses for a specific network
pub fn get_network_addresses(network_name: &str) -> Result<NetworkAddresses> {
    match network_name.to_lowercase().as_str() {
        "mainnet" | "ethereum" => Ok(NetworkAddresses::mainnet()),
        _ => Err(Error::Config(format!(
            "Unsupported network for contract addresses: {}. Only Ethereum mainnet is supported.",
            network_name
        ))),
    }
}

/// Validate that all addresses in a NetworkAddresses struct are valid
pub fn validate_addresses(addresses: &NetworkAddresses) -> Result<()> {
    let zero_address = Address::zero();

    if addresses.factory == zero_address {
        return Err(Error::Config("Factory address cannot be zero".to_string()));
    }

    if addresses.router == zero_address {
        return Err(Error::Config("Router address cannot be zero".to_string()));
    }

    if addresses.quoter == zero_address {
        return Err(Error::Config("Quoter address cannot be zero".to_string()));
    }

    if addresses.weth == zero_address {
        return Err(Error::Config("WETH address cannot be zero".to_string()));
    }

    if addresses.usdc == zero_address {
        return Err(Error::Config("USDC address cannot be zero".to_string()));
    }

    if addresses.dai == zero_address {
        return Err(Error::Config("DAI address cannot be zero".to_string()));
    }

    if addresses.usdt == zero_address {
        return Err(Error::Config("USDT address cannot be zero".to_string()));
    }

    // Note: weth_usdc_pair can be zero for dynamic lookup
    // Some testnet addresses may be zero - that's acceptable

    Ok(())
}

/// Get common trading tokens for a specific network
/// Returns a vector of (symbol, address_string) tuples
pub fn get_common_tokens(network_name: &str) -> Result<Vec<(&'static str, String)>> {
    match network_name.to_lowercase().as_str() {
        "mainnet" | "ethereum" => {
            let addresses = NetworkAddresses::mainnet();
            Ok(vec![
                ("USDC", format!("{:?}", addresses.usdc)),
                ("WETH", format!("{:?}", addresses.weth)),
                ("DAI", format!("{:?}", addresses.dai)),
                ("USDT", format!("{:?}", addresses.usdt)),
            ])
        }
        _ => Err(Error::Config(format!(
            "No common tokens defined for network: {}. Only Ethereum mainnet is supported.",
            network_name
        ))),
    }
}
