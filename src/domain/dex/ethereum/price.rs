use std::str::FromStr;
use std::sync::Arc;

use ethers::contract::Contract;
use ethers::prelude::Middleware;
use ethers::providers::{Http, Provider};
use ethers::types::{Address, U256};
use ethers::utils::format_units;

use crate::core::error::{Error, Result};
use crate::domain::dex::ethereum::abi::{get_erc20_abi, get_uniswap_v2_pair_abi};
use crate::domain::dex::ethereum::network::NetworkConfig;

#[derive(Clone)]
pub struct PriceOracle {
    provider: Arc<Provider<Http>>,
    network: NetworkConfig,
}

impl PriceOracle {
    pub fn new(provider: Arc<Provider<Http>>, network: NetworkConfig) -> Self {
        Self { provider, network }
    }

    /// Get the USD price of a token
    pub async fn get_token_price_usd(&self, token_address: &str) -> Result<f64> {
        self.get_token_price_usd_internal(token_address).await
    }

    async fn get_token_price_usd_internal(&self, token_address: &str) -> Result<f64> {
        let token_addr = Address::from_str(token_address)
            .map_err(|e| Error::Conversion(format!("Invalid token address: {}", e)))?;

        // Special case for native currency (ETH/MATIC)
        if token_address.to_lowercase() == "eth"
            || token_address.to_lowercase() == "ethereum"
            || token_addr == self.network.weth_address
        {
            return self.get_native_price_usd().await;
        }

        // Special case for stablecoin
        if token_addr == self.network.stablecoin_address {
            return Ok(1.0); // Assume stablecoin is $1
        }

        // For other tokens, try to get price via WETH pair
        match self.get_token_price_via_weth(token_addr).await {
            Ok(weth_price) => {
                let native_price_usd = self.get_native_price_usd().await?;
                Ok(weth_price * native_price_usd)
            }
            Err(_) => {
                // Fallback: try direct stablecoin pair
                self.get_token_price_via_stablecoin(token_addr).await
            }
        }
    }

    /// Get token price in terms of WETH
    async fn get_token_price_via_weth(&self, token_address: Address) -> Result<f64> {
        // Try to find a direct pair with WETH
        let pair_address = self
            .find_pair_address(token_address, self.network.weth_address)
            .await?;
        self.get_pair_price(pair_address, token_address).await
    }

    /// Get token price in terms of stablecoin
    async fn get_token_price_via_stablecoin(&self, token_address: Address) -> Result<f64> {
        let pair_address = self
            .find_pair_address(token_address, self.network.stablecoin_address)
            .await?;
        self.get_pair_price(pair_address, token_address).await
    }

    /// Get the price from a specific trading pair
    pub async fn get_pair_price(&self, pair_address: Address, base_token: Address) -> Result<f64> {
        let pair_contract = Contract::new(
            pair_address,
            get_uniswap_v2_pair_abi()?,
            self.provider.clone(),
        );

        // Get token addresses
        let token0: Address = pair_contract
            .method::<_, Address>("token0", ())
            .map_err(|e| Error::Contract(format!("Failed to get token0 method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get token0: {}", e)))?;

        let token1: Address = pair_contract
            .method::<_, Address>("token1", ())
            .map_err(|e| Error::Contract(format!("Failed to get token1 method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get token1: {}", e)))?;

        // Get reserves
        let (reserve0, reserve1, _): (u128, u128, u32) = pair_contract
            .method::<_, (u128, u128, u32)>("getReserves", ())
            .map_err(|e| Error::Contract(format!("Failed to get getReserves method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get reserves: {}", e)))?;

        if reserve0 == 0 || reserve1 == 0 {
            return Err(Error::Contract("No liquidity in pair".to_string()));
        }

        // Get decimals for both tokens
        let token0_decimals = self.get_token_decimals(token0).await?;
        let token1_decimals = self.get_token_decimals(token1).await?;

        // Calculate price based on which token is the base token
        let price = if base_token == token0 {
            // Price of token0 in terms of token1
            let reserve0_adjusted = reserve0 as f64 / 10_f64.powi(token0_decimals as i32);
            let reserve1_adjusted = reserve1 as f64 / 10_f64.powi(token1_decimals as i32);
            reserve1_adjusted / reserve0_adjusted
        } else if base_token == token1 {
            // Price of token1 in terms of token0
            let reserve0_adjusted = reserve0 as f64 / 10_f64.powi(token0_decimals as i32);
            let reserve1_adjusted = reserve1 as f64 / 10_f64.powi(token1_decimals as i32);
            reserve0_adjusted / reserve1_adjusted
        } else {
            return Err(Error::Contract("Base token not found in pair".to_string()));
        };

        Ok(price)
    }

    /// Get the USD price of the native currency (ETH/MATIC)
    pub async fn get_native_price_usd(&self) -> Result<f64> {
        // Get the price from the WETH/Stablecoin pair
        let price = self
            .get_pair_price(
                self.network.weth_stablecoin_pair_address,
                self.network.weth_address,
            )
            .await?;

        Ok(price)
    }

    /// Find the pair address for two tokens
    async fn find_pair_address(&self, token_a: Address, token_b: Address) -> Result<Address> {
        // This would typically use the factory contract to find the pair
        // For now, we'll implement a simple version
        let factory_abi = crate::domain::dex::ethereum::abi::get_uniswap_v2_factory_abi()?;
        let factory_contract = Contract::new(
            self.network.factory_address,
            factory_abi,
            self.provider.clone(),
        );

        let pair_address: Address = factory_contract
            .method::<_, Address>("getPair", (token_a, token_b))
            .map_err(|e| Error::Contract(format!("Failed to get getPair method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get pair address: {}", e)))?;

        if pair_address == Address::zero() {
            return Err(Error::Contract("Pair does not exist".to_string()));
        }

        Ok(pair_address)
    }

    /// Helper function to get token decimals
    async fn get_token_decimals(&self, token_address: Address) -> Result<u8> {
        let contract = Contract::new(token_address, get_erc20_abi()?, self.provider.clone());

        let decimals: u8 = contract
            .method::<_, u8>("decimals", ())
            .map_err(|e| Error::Contract(format!("Failed to get decimals method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get decimals: {}", e)))?;

        Ok(decimals)
    }

    /// Get token balance for an address
    pub async fn get_token_balance(
        &self,
        token_address: &str,
        wallet_address: Address,
    ) -> Result<f64> {
        let token_addr = Address::from_str(token_address)
            .map_err(|e| Error::Conversion(format!("Invalid token address: {}", e)))?;

        let contract = Contract::new(token_addr, get_erc20_abi()?, self.provider.clone());

        let balance: U256 = contract
            .method::<_, U256>("balanceOf", wallet_address)
            .map_err(|e| Error::Contract(format!("Failed to get balanceOf method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Contract(format!("Failed to get balance: {}", e)))?;

        let decimals = self.get_token_decimals(token_addr).await?;
        let balance_f64 = format_units(balance, decimals as usize)
            .map_err(|e| Error::Conversion(format!("Failed to format balance: {}", e)))?
            .parse::<f64>()
            .map_err(|e| Error::Conversion(format!("Failed to parse balance: {}", e)))?;

        Ok(balance_f64)
    }

    /// Get native currency balance
    pub async fn get_native_balance(&self, wallet_address: Address) -> Result<f64> {
        let balance = self
            .provider
            .get_balance(wallet_address, None)
            .await
            .map_err(|e| Error::Network(format!("Failed to get balance: {}", e)))?;

        let balance_f64 = format_units(balance, self.network.native_currency_decimals as usize)
            .map_err(|e| Error::Conversion(format!("Failed to format balance: {}", e)))?
            .parse::<f64>()
            .map_err(|e| Error::Conversion(format!("Failed to parse balance: {}", e)))?;

        Ok(balance_f64)
    }
}
