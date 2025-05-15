use crate::core::error::Error;
use crate::core::config::Config;
use ethers::prelude::*;
use ethers::core::types::{TransactionReceipt, Address, U256};
use hex::ToHex;
use log::{info, error, debug, warn};
use std::str::FromStr;
use std::sync::Arc;
use std::convert::TryFrom;

// Uniswap V2 Router ABI (simplified for our needs)
const UNISWAP_V2_ROUTER_ABI: &str = r#"[
    {
        "inputs": [
            {"name": "amountOutMin", "type": "uint256"},
            {"name": "path", "type": "address[]"},
            {"name": "to", "type": "address"},
            {"name": "deadline", "type": "uint256"}
        ],
        "name": "swapExactETHForTokens",
        "outputs": [{"name": "amounts", "type": "uint256[]"}],
        "stateMutability": "payable",
        "type": "function"
    },
    {
        "inputs": [
            {"name": "amountIn", "type": "uint256"},
            {"name": "amountOutMin", "type": "uint256"},
            {"name": "path", "type": "address[]"},
            {"name": "to", "type": "address"},
            {"name": "deadline", "type": "uint256"}
        ],
        "name": "swapExactTokensForETH",
        "outputs": [{"name": "amounts", "type": "uint256[]"}],
        "stateMutability": "nonpayable",
        "type": "function"
    },
    {
        "inputs": [
            {"name": "amountIn", "type": "uint256"},
            {"name": "amountOutMin", "type": "uint256"},
            {"name": "path", "type": "address[]"},
            {"name": "to", "type": "address"},
            {"name": "deadline", "type": "uint256"}
        ],
        "name": "swapExactTokensForTokens",
        "outputs": [{"name": "amounts", "type": "uint256[]"}],
        "stateMutability": "nonpayable",
        "type": "function"
    }
]"#;

// ERC20 Token ABI (simplified for our needs)
const ERC20_ABI: &str = r#"[
    {
        "constant": false,
        "inputs": [
            {"name": "_spender", "type": "address"},
            {"name": "_value", "type": "uint256"}
        ],
        "name": "approve",
        "outputs": [{"name": "", "type": "bool"}],
        "type": "function"
    },
    {
        "constant": true,
        "inputs": [{"name": "_owner", "type": "address"}],
        "name": "balanceOf",
        "outputs": [{"name": "balance", "type": "uint256"}],
        "type": "function"
    }
]"#;

// Network configurations
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub chain_id: u64,
    pub rpc_url: String,
    pub router_address: Address,
    pub weth_address: Address,
    pub block_explorer_url: String,
}

// Known testnet configurations
impl NetworkConfig {
    pub fn goerli() -> Self {
        Self {
            chain_id: 5,
            rpc_url: "https://goerli.infura.io/v3/YOUR_INFURA_KEY".to_string(),
            router_address: Address::from_str("0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D").unwrap(),
            weth_address: Address::from_str("0xB4FBF271143F4FBf7B91A5ded31805e42b2208d6").unwrap(),
            block_explorer_url: "https://goerli.etherscan.io".to_string(),
        }
    }
    
    pub fn mumbai() -> Self {
        Self {
            chain_id: 80001,
            rpc_url: "https://polygon-mumbai.infura.io/v3/YOUR_INFURA_KEY".to_string(),
            router_address: Address::from_str("0x8954AfA98594b838bda56FE4C12a09D7739D179b").unwrap(),
            weth_address: Address::from_str("0x9c3C9283D3e44854697Cd22D3Faa240Cfb032889").unwrap(),
            block_explorer_url: "https://mumbai.polygonscan.com".to_string(),
        }
    }
    
    pub fn from_config(config: &Config) -> Result<Self, Error> {
        debug!("Getting network from config: dex.testnet={}, dex.network={:?}", 
               config.dex.testnet, config.dex.network);
        
        let network = config.dex.network.clone().unwrap_or_else(|| {
            debug!("No network specified in config, defaulting to goerli");
            "goerli".to_string()
        });
        
        debug!("Using network: {}", network);
        
        match network.as_str() {
            "goerli" => {
                debug!("Creating Goerli testnet network configuration");
                Ok(Self::goerli())
            },
            "mumbai" => {
                debug!("Creating Mumbai testnet network configuration");
                Ok(Self::mumbai())
            },
            _ => Err(Error::Config(format!("Unsupported network: '{}'. Valid options are 'goerli' or 'mumbai'", network))),
        }
    }
    
    pub fn with_infura_key(&mut self, key: &str) {
        if self.rpc_url.contains("YOUR_INFURA_KEY") {
            debug!("Replacing placeholder Infura key with provided key");
            self.rpc_url = self.rpc_url.replace("YOUR_INFURA_KEY", key);
        }
    }
}

#[derive(Debug, Clone)]
pub struct TestnetDexClient {
    provider: Option<Provider<Http>>,
    wallet: Option<LocalWallet>,
    network: NetworkConfig,
}

impl TestnetDexClient {
    pub fn new(config: &Config) -> Result<Self, Error> {
        let mut network = NetworkConfig::from_config(config)?;
        
        // Set Infura key if available
        if let Some(infura_key) = &config.api_keys.infura {
            debug!("Infura key found in config, applying to network RPC URL");
            network.with_infura_key(infura_key);
        } else {
            warn!("No Infura key found in config. Set HONEYBADGER_INFURA_KEY environment variable or add to config.json");
        }
        
        info!("Creating TestnetDexClient for network: {} (chain ID: {})", 
             config.dex.network.clone().unwrap_or_else(|| "goerli".to_string()),
             network.chain_id);
        
        Ok(Self {
            provider: None,
            wallet: None,
            network,
        })
    }
    
    pub async fn connect(&mut self, private_key: &str) -> Result<(), Error> {
        // Create provider
        let provider = Provider::<Http>::try_from(self.network.rpc_url.clone())
            .map_err(|e| Error::External(format!("Failed to create provider: {}", e)))?;
        
        // Create wallet from private key
        let wallet = private_key.parse::<LocalWallet>()
            .map_err(|e| Error::External(format!("Invalid private key: {}", e)))?
            .with_chain_id(self.network.chain_id);
        
        info!("Connected to testnet with wallet: {}", wallet.address());
        
        self.provider = Some(provider);
        self.wallet = Some(wallet);
        
        Ok(())
    }
    
    pub async fn execute_order(
        &self,
        token_address: &str,
        size: f64,
        price: f64,
        is_buy: bool,
    ) -> Result<String, Error> {
        let provider = self.provider.clone()
            .ok_or_else(|| Error::InvalidInput("Provider not initialized".to_string()))?;
        
        let wallet = self.wallet.clone()
            .ok_or_else(|| Error::InvalidInput("Wallet not initialized".to_string()))?;
        
        let client = Arc::new(SignerMiddleware::new(provider, wallet));
        
        // Parse token address
        let token_address = Address::from_str(token_address)
            .map_err(|e| Error::InvalidInput(format!("Invalid token address: {}", e)))?;
        
        // Get current block for deadline calculation
        let current_block = client.get_block_number().await
            .map_err(|e| Error::External(format!("Failed to get block number: {}", e)))?;
        
        // Set deadline to current block + 10 minutes of blocks (assuming 12s block time)
        let deadline = U256::from(current_block.as_u64() + 50);
        
        // Execute order based on buy/sell direction
        if is_buy {
            self.execute_buy_order(client, token_address, size, price, deadline).await
        } else {
            self.execute_sell_order(client, token_address, size, price, deadline).await
        }
    }
    
    async fn execute_buy_order(
        &self,
        client: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
        token_address: Address,
        size: f64,
        price: f64,
        deadline: U256,
    ) -> Result<String, Error> {
        debug!("Executing buy order for token: {:?}, size: {}, price: {}", token_address, size, price);
        
        // Create router contract instance
        let router = ethers::contract::Contract::new(
            self.network.router_address,
            serde_json::from_str::<ethers::abi::Abi>(UNISWAP_V2_ROUTER_ABI).unwrap(),
            client.clone(),
        );
        
        // Calculate amount of ETH to send (size in USD / ETH price in USD)
        // In a real implementation, we would get the actual ETH price from an oracle
        let eth_price_usd = 2000.0; // Placeholder ETH price
        let eth_amount = size / eth_price_usd;
        
        // Convert ETH amount to Wei
        let eth_amount_wei = U256::from_dec_str(&format!("{:.0}", eth_amount * 1e18))
            .map_err(|e| Error::InvalidInput(format!("Invalid ETH amount: {}", e)))?;
        
        // Calculate minimum tokens to receive (with 5% slippage)
        let expected_tokens = size / price;
        let min_tokens_amount = (expected_tokens * 0.95) * 1e18;
        let min_tokens_amount_wei = U256::from_dec_str(&format!("{:.0}", min_tokens_amount))
            .map_err(|e| Error::InvalidInput(format!("Invalid token amount: {}", e)))?;
        
        info!(
            "Swapping {} ETH for token {:?}, min output: {} tokens",
            eth_amount, token_address, expected_tokens * 0.95
        );
        
        // Create path: [WETH, token]
        let path = vec![self.network.weth_address, token_address];
        
        // Call swapExactETHForTokens
        let tx = router.method::<_, Vec<U256>>(
            "swapExactETHForTokens",
            (
                min_tokens_amount_wei,
                path,
                client.address(),
                deadline,
            ),
        )
        .map_err(|e| Error::External(format!("Failed to create transaction: {}", e)))?
        .value(eth_amount_wei);
        
        // Send transaction
        let pending_tx = tx.send().await
            .map_err(|e| Error::External(format!("Failed to send transaction: {}", e)))?;
        
        // Wait for transaction to be mined
        let receipt = pending_tx.await
            .map_err(|e| Error::External(format!("Transaction failed: {}", e)))?
            .ok_or_else(|| Error::External("Transaction not found".to_string()))?;
        
        let tx_hash = format!("0x{}", receipt.transaction_hash.encode_hex::<String>());
        
        info!("Buy order executed successfully: {}", tx_hash);
        
        Ok(tx_hash)
    }
    
    async fn execute_sell_order(
        &self,
        client: Arc<SignerMiddleware<Provider<Http>, LocalWallet>>,
        token_address: Address,
        size: f64,
        price: f64,
        deadline: U256,
    ) -> Result<String, Error> {
        debug!("Executing sell order for token: {:?}, size: {}, price: {}", token_address, size, price);
        
        // Create router contract instance
        let router = ethers::contract::Contract::new(
            self.network.router_address,
            serde_json::from_str::<ethers::abi::Abi>(UNISWAP_V2_ROUTER_ABI).unwrap(),
            client.clone(),
        );
        
        // Create token contract instance
        let token = ethers::contract::Contract::new(
            token_address,
            serde_json::from_str::<ethers::abi::Abi>(ERC20_ABI).unwrap(),
            client.clone(),
        );
        
        // Calculate token amount to sell
        let token_amount = size / price;
        let token_amount_wei = U256::from_dec_str(&format!("{:.0}", token_amount * 1e18))
            .map_err(|e| Error::InvalidInput(format!("Invalid token amount: {}", e)))?;
        
        // Calculate minimum ETH to receive (with 5% slippage)
        let eth_price_usd = 2000.0; // Placeholder ETH price
        let expected_eth = size / eth_price_usd;
        let min_eth_amount = (expected_eth * 0.95) * 1e18;
        let min_eth_amount_wei = U256::from_dec_str(&format!("{:.0}", min_eth_amount))
            .map_err(|e| Error::InvalidInput(format!("Invalid ETH amount: {}", e)))?;
        
        info!(
            "Swapping {} tokens for ETH, min output: {} ETH",
            token_amount, expected_eth * 0.95
        );
        
        // Approve router to spend tokens
        let approve_tx = token.method::<_, bool>(
            "approve",
            (self.network.router_address, token_amount_wei),
        )
        .map_err(|e| Error::External(format!("Failed to create approval transaction: {}", e)))?;
        
        let pending_approve = approve_tx.send().await
            .map_err(|e| Error::External(format!("Failed to send approval transaction: {}", e)))?;
        
        let approve_receipt = pending_approve.await
            .map_err(|e| Error::External(format!("Approval transaction failed: {}", e)))?
            .ok_or_else(|| Error::External("Approval transaction not found".to_string()))?;
        
        info!("Token approval successful: 0x{}", approve_receipt.transaction_hash.encode_hex::<String>());
        
        // Create path: [token, WETH]
        let path = vec![token_address, self.network.weth_address];
        
        // Call swapExactTokensForETH
        let tx = router.method::<_, Vec<U256>>(
            "swapExactTokensForETH",
            (
                token_amount_wei,
                min_eth_amount_wei,
                path,
                client.address(),
                deadline,
            ),
        )
        .map_err(|e| Error::External(format!("Failed to create transaction: {}", e)))?;
        
        // Send transaction
        let pending_tx = tx.send().await
            .map_err(|e| Error::External(format!("Failed to send transaction: {}", e)))?;
        
        // Wait for transaction to be mined
        let receipt = pending_tx.await
            .map_err(|e| Error::External(format!("Transaction failed: {}", e)))?
            .ok_or_else(|| Error::External("Transaction not found".to_string()))?;
        
        let tx_hash = format!("0x{}", receipt.transaction_hash.encode_hex::<String>());
        
        info!("Sell order executed successfully: {}", tx_hash);
        
        Ok(tx_hash)
    }
    
    pub async fn get_token_balance(&self, token_address: &str) -> Result<f64, Error> {
        let provider = self.provider.clone()
            .ok_or_else(|| Error::InvalidInput("Provider not initialized".to_string()))?;
        
        let wallet = self.wallet.clone()
            .ok_or_else(|| Error::InvalidInput("Wallet not initialized".to_string()))?;
        
        let client = Arc::new(SignerMiddleware::new(provider, wallet.clone()));
        
        // Parse token address
        let token_address = Address::from_str(token_address)
            .map_err(|e| Error::InvalidInput(format!("Invalid token address: {}", e)))?;
        
        // Create token contract instance
        let token = ethers::contract::Contract::new(
            token_address,
            serde_json::from_str::<ethers::abi::Abi>(ERC20_ABI).unwrap(),
            client,
        );
        
        // Call balanceOf
        let balance: U256 = token.method::<_, U256>(
            "balanceOf",
            wallet.address(),
        )
        .map_err(|e| Error::External(format!("Failed to create balance query: {}", e)))?
        .call().await
        .map_err(|e| Error::External(format!("Failed to query balance: {}", e)))?;
        
        // Convert from Wei to human-readable
        let balance_float = balance.as_u128() as f64 / 1e18;
        
        Ok(balance_float)
    }
    
    pub async fn get_eth_balance(&self) -> Result<f64, Error> {
        let provider = self.provider.clone()
            .ok_or_else(|| Error::InvalidInput("Provider not initialized".to_string()))?;
        
        let wallet = self.wallet.clone()
            .ok_or_else(|| Error::InvalidInput("Wallet not initialized".to_string()))?;
        
        // Get ETH balance
        let balance = provider.get_balance(wallet.address(), None).await
            .map_err(|e| Error::External(format!("Failed to get ETH balance: {}", e)))?;
        
        // Convert from Wei to human-readable
        let balance_float = balance.as_u128() as f64 / 1e18;
        
        Ok(balance_float)
    }
} 