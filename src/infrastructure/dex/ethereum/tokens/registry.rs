/// Simplified token registry for basic address validation
/// Since Alchemy provides all token data directly from pools, we only need basic address validation
/// This replaces the previous overengineered multi-strategy token resolution system
use crate::infrastructure::errors::{Error, Result};
use ethers::prelude::{Address, Contract, Http, Provider};
use log::{debug, error, warn};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Simple ERC20 ABI for token operations (inlined to avoid separate folder)
const ERC20_ABI_JSON: &str = r#"[{"constant":true,"inputs":[],"name":"name","outputs":[{"name":"","type":"string"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"symbol","outputs":[{"name":"","type":"string"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[],"name":"decimals","outputs":[{"name":"","type":"uint8"}],"payable":false,"stateMutability":"view","type":"function"},{"constant":true,"inputs":[{"name":"_owner","type":"address"}],"name":"balanceOf","outputs":[{"name":"balance","type":"uint256"}],"payable":false,"stateMutability":"view","type":"function"}]"#;

/// Cache TTL for token metadata (1 hour)
const CACHE_TTL_SECONDS: u64 = 3600;

fn get_erc20_abi() -> &'static ethers::abi::Abi {
    static ABI: OnceCell<ethers::abi::Abi> = OnceCell::new();
    ABI.get_or_init(|| {
        serde_json::from_str::<ethers::abi::Abi>(ERC20_ABI_JSON)
            .expect("ERC20 ABI is hardcoded and must be valid")
    })
}

/// Global access to the singleton token registry
pub struct TokenRegistryService;

impl TokenRegistryService {
    /// Get the global token registry instance
    pub fn get() -> Arc<TokenRegistry> {
        static REGISTRY: OnceCell<Arc<TokenRegistry>> = OnceCell::new();
        REGISTRY
            .get_or_init(|| Arc::new(TokenRegistry::new()))
            .clone()
    }
}

/// Basic cache entry with TTL
struct CacheEntry<T> {
    data: T,
    expires_at: u64,
}

impl<T> CacheEntry<T> {
    fn new(data: T, ttl_seconds: u64) -> Self {
        // Note: unwrap() is acceptable here - SystemTime::now() is always after UNIX_EPOCH
        // unless the system clock is set before 1970, which is a critical system misconfiguration
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + ttl_seconds;
        Self { data, expires_at }
    }

    fn is_expired(&self) -> bool {
        // Note: unwrap() is acceptable here for same reason as above
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            > self.expires_at
    }
}

/// Simplified token registry for basic address validation and symbol lookup
/// Since Alchemy provides comprehensive token data, this only handles edge cases
pub struct TokenRegistry {
    /// Address cache for performance
    cache: Arc<RwLock<HashMap<String, CacheEntry<Address>>>>,
    /// Symbol cache for token symbols
    symbol_cache: Arc<RwLock<HashMap<String, CacheEntry<String>>>>,
    /// Decimals cache for token decimals
    decimals_cache: Arc<RwLock<HashMap<String, CacheEntry<u8>>>>,
    /// Ethereum provider for on-chain queries
    provider: Arc<RwLock<Option<Arc<Provider<Http>>>>>,
}

impl Default for TokenRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenRegistry {
    /// Create a new simplified token registry
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            symbol_cache: Arc::new(RwLock::new(HashMap::new())),
            decimals_cache: Arc::new(RwLock::new(HashMap::new())),
            provider: Arc::new(RwLock::new(None)),
        }
    }

    /// Simplified token resolution - mainly for address validation
    /// Alchemy provides most token data, this is just for edge cases
    pub async fn resolve_token(&self, token_identifier: &str, network: &str) -> Result<Address> {
        debug!(
            "🔍 Validating token {} on network {}",
            token_identifier, network
        );

        // Trim and normalize input
        let token_identifier = token_identifier.trim();
        let network = network.trim().to_lowercase();

        // Check cache first
        let cache_key = format!("{}:{}", token_identifier, network);
        {
            let cache = self.cache.read().await;
            if let Some(entry) = cache.get(&cache_key) {
                if !entry.is_expired() {
                    debug!("✅ Cache hit for {}: {}", token_identifier, entry.data);
                    return Ok(entry.data);
                }
            }
        }

        // Extract address from token ID (handles both "chain_id:address" and plain address formats)
        let address_str = crate::core::utils::normalization::extract_address(token_identifier);

        // If it's already a valid address, validate and return
        if let Ok(address) = Address::from_str(&address_str) {
            debug!("✅ Valid address provided: {}", address);

            // Cache the result
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, CacheEntry::new(address, CACHE_TTL_SECONDS));
            return Ok(address);
        }

        // For non-address identifiers, provide helpful error
        // Since Alchemy handles token discovery, this shouldn't happen in normal operation
        error!(
            "❌ Token identifier '{}' resolved to '{}' which is not a valid Ethereum address",
            token_identifier, address_str
        );

        Err(Error::Config(format!(
            "Token identifier '{}' resolved to '{}' which is not a valid Ethereum address. Expected format: 0x... or chain_id:0x...",
            token_identifier, address_str
        )))
    }

    /// Helper to get provider or return None if not set
    async fn get_provider(&self) -> Option<Arc<Provider<Http>>> {
        let provider_guard = self.provider.read().await;
        provider_guard.clone()
    }

    /// Get authoritative symbol for a token address from the contract
    pub async fn get_authoritative_symbol(&self, token_address: Address) -> Result<String> {
        debug!("🏷️  Getting authoritative symbol for {}", token_address);

        // Check symbol cache first
        let cache_key = format!("symbol:{}", token_address);
        {
            let symbol_cache = self.symbol_cache.read().await;
            if let Some(entry) = symbol_cache.get(&cache_key) {
                if !entry.is_expired() {
                    debug!("✅ Symbol cache hit for {}: {}", token_address, entry.data);
                    return Ok(entry.data.clone());
                }
            }
        }

        // Get provider - if not available, use address as fallback symbol
        let Some(provider) = self.get_provider().await else {
            warn!("⚠️ No provider available for symbol lookup, using address as symbol");
            return Ok(format!("{:?}", token_address));
        };

        // Query the contract for symbol
        match self.query_contract_symbol(token_address, &provider).await {
            Ok(symbol) => {
                debug!("✅ Retrieved symbol from contract: {}", symbol);

                // Cache the symbol
                let mut symbol_cache = self.symbol_cache.write().await;
                symbol_cache.insert(
                    cache_key,
                    CacheEntry::new(symbol.clone(), CACHE_TTL_SECONDS),
                );

                Ok(symbol)
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to get symbol from contract {}: {}. Using address as fallback",
                    token_address, e
                );
                Ok(format!("{:?}", token_address))
            }
        }
    }

    /// Query the ERC20 contract for its symbol
    async fn query_contract_symbol(
        &self,
        token_address: Address,
        provider: &Arc<Provider<Http>>,
    ) -> Result<String> {
        let contract = Contract::new(token_address, get_erc20_abi().clone(), provider.clone());

        contract
            .method::<_, String>("symbol", ())
            .map_err(|e| Error::Dex(format!("Failed to create symbol method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Dex(format!("Failed to call symbol method: {}", e)))
    }

    /// Get token decimals from cache or contract
    pub async fn get_token_decimals(&self, token_address: Address) -> Result<u8> {
        debug!("🔢 Getting decimals for {}", token_address);

        let cache_key = format!("decimals:{}", token_address);
        {
            let decimals_cache = self.decimals_cache.read().await;
            if let Some(entry) = decimals_cache.get(&cache_key) {
                if !entry.is_expired() {
                    debug!(
                        "✅ Decimals cache hit for {}: {}",
                        token_address, entry.data
                    );
                    return Ok(entry.data);
                }
            }
        }

        let Some(provider) = self.get_provider().await else {
            warn!("⚠️ No provider available for decimals lookup, using default 18");
            return Ok(18);
        };

        match self.query_contract_decimals(token_address, &provider).await {
            Ok(decimals) => {
                debug!("✅ Retrieved decimals from contract: {}", decimals);
                let mut decimals_cache = self.decimals_cache.write().await;
                decimals_cache.insert(cache_key, CacheEntry::new(decimals, CACHE_TTL_SECONDS));

                Ok(decimals)
            }
            Err(e) => {
                warn!(
                    "⚠️ Failed to get decimals for {}: {}. Using default 18.",
                    token_address, e
                );
                Ok(18) // Default to 18 on error
            }
        }
    }

    /// Cache token decimals (called when Alchemy provides decimals)
    pub async fn cache_token_decimals(&self, token_address: Address, decimals: u8) {
        let cache_key = format!("decimals:{}", token_address);
        let mut decimals_cache = self.decimals_cache.write().await;
        decimals_cache.insert(cache_key, CacheEntry::new(decimals, CACHE_TTL_SECONDS));
        debug!("✅ Cached decimals for {}: {}", token_address, decimals);
    }

    /// Query the ERC20 contract for its decimals
    async fn query_contract_decimals(
        &self,
        token_address: Address,
        provider: &Arc<Provider<Http>>,
    ) -> Result<u8> {
        let contract = Contract::new(token_address, get_erc20_abi().clone(), provider.clone());

        contract
            .method::<_, u8>("decimals", ())
            .map_err(|e| Error::Dex(format!("Failed to create decimals method: {}", e)))?
            .call()
            .await
            .map_err(|e| Error::Dex(format!("Failed to call decimals method: {}", e)))
    }

    /// Set the Ethereum provider for on-chain queries
    pub async fn set_provider(&self, provider: Arc<Provider<Http>>) {
        let mut provider_guard = self.provider.write().await;
        *provider_guard = Some(provider);
        debug!("Ethereum provider set for token registry");
    }
}

/// Check if a token is a stablecoin by symbol
/// Returns true if the token should be excluded from buy signals
///
/// Simple stablecoin detection for trading strategy
/// Alchemy provides token symbols which are checked here to avoid generating buy signals
/// for stablecoins (which should remain as base currency)
pub fn is_stablecoin(symbol: &str) -> bool {
    let symbol_upper = symbol.to_uppercase();
    matches!(
        symbol_upper.as_str(),
        "USDC" | "USDT" | "DAI" | "BUSD" | "TUSD" | "USDP" | "FRAX" | "LUSD" | "GUSD" | "SUSD"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stablecoin_detection() {
        assert!(is_stablecoin("USDC"));
        assert!(is_stablecoin("USDT"));
        assert!(is_stablecoin("DAI"));
        assert!(is_stablecoin("BUSD"));
        assert!(is_stablecoin("FRAX"));

        assert!(is_stablecoin("usdc"));
        assert!(is_stablecoin("Usdt"));
        assert!(is_stablecoin("dAi"));

        assert!(!is_stablecoin("BTC"));
        assert!(!is_stablecoin("ETH"));
        assert!(!is_stablecoin("WETH"));
        assert!(!is_stablecoin("WBTC"));
        assert!(!is_stablecoin("UNI"));
    }
}
