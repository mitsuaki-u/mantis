// Export the testnet module
pub mod testnet;

// Re-export the TestnetDexClient
pub use testnet::TestnetDexClient;

#[derive(Clone)]
pub enum DexClient {
    Paper,
    Testnet(testnet::TestnetDexClient),
    Live, // Placeholder for future live trading implementation
}

impl DexClient {
    pub fn new_paper_trading() -> Self {
        DexClient::Paper
    }
    
    pub fn new_testnet(config: &crate::config::Config) -> Result<Self, crate::error::Error> {
        let client = testnet::TestnetDexClient::new(config)?;
        Ok(DexClient::Testnet(client))
    }
    
    pub fn new_live() -> Self {
        DexClient::Live
    }
    
    pub async fn execute_order(
        &self,
        token_id: &str,
        size: f64,
        price: f64,
        is_buy: bool,
    ) -> Result<(), crate::error::Error> {
        match self {
            DexClient::Paper => {
                // Simulated order execution for paper trading
                let order_type = if is_buy { "BUY" } else { "SELL" };
                log::info!(
                    "📝 PAPER TRADING: {} order for {} {} at ${:.4}",
                    order_type,
                    size,
                    token_id,
                    price
                );
                
                // Simulate a small delay for network latency
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                
                log::info!("✅ PAPER ORDER EXECUTION COMPLETE FOR {} {}", token_id, order_type);
                
                Ok(())
            },
            DexClient::Testnet(client) => {
                // Convert token ID to address format if needed
                // This is a simplification; in a real app we'd have token ID to address mapping
                let token_address = match token_id {
                    // Handle some well-known tokens
                    "weth" => "0xB4FBF271143F4FBf7B91A5ded31805e42b2208d6", // Goerli WETH
                    "usdc" => "0x07865c6e87b9f70255377e024ace6630c1eaa37f", // Goerli USDC
                    "dai" => "0x73967c6a0904aa032c103b4104747e88c566b1a2", // Goerli DAI
                    // Default to passing through the token ID as an address
                    _ => token_id,
                };
                
                let order_type = if is_buy { "BUY" } else { "SELL" };
                log::info!(
                    "⚡ TESTNET TRADING: {} order for {} {} at ${:.4}",
                    order_type,
                    size,
                    token_id,
                    price
                );
                
                // Execute order on testnet
                match client.execute_order(token_address, size, price, is_buy).await {
                    Ok(tx_hash) => {
                        log::info!("✅ TESTNET ORDER EXECUTION COMPLETE: {} - TX: {}", token_id, tx_hash);
                        Ok(())
                    },
                    Err(e) => {
                        log::error!("❌ TESTNET ORDER EXECUTION FAILED: {}", e);
                        Err(e)
                    }
                }
            },
            DexClient::Live => {
                log::warn!("🚨 LIVE TRADING NOT IMPLEMENTED YET!");
                Err(crate::error::Error::NotImplemented("Live trading not implemented yet".to_string()))
            }
        }
    }
    
    // Helper method to connect a wallet to the testnet client
    pub async fn connect_wallet(&mut self, private_key: &str) -> Result<(), crate::error::Error> {
        match self {
            DexClient::Testnet(client) => {
                client.connect(private_key).await
            },
            _ => {
                log::warn!("Wallet connection only supported for testnet trading");
                Ok(())
            }
        }
    }
    
    // Get the current balance of a token (only implemented for testnet)
    pub async fn get_token_balance(&self, token_address: &str) -> Result<f64, crate::error::Error> {
        match self {
            DexClient::Testnet(client) => {
                client.get_token_balance(token_address).await
            },
            _ => {
                log::warn!("Token balance checking only supported for testnet trading");
                Ok(0.0)
            }
        }
    }
    
    // Get the current ETH balance (only implemented for testnet)
    pub async fn get_eth_balance(&self) -> Result<f64, crate::error::Error> {
        match self {
            DexClient::Testnet(client) => {
                client.get_eth_balance().await
            },
            _ => {
                log::warn!("ETH balance checking only supported for testnet trading");
                Ok(0.0)
            }
        }
    }
} 