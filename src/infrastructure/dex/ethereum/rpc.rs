use crate::infrastructure::errors::Error;
use ethers::providers::{Http, Provider};
use log::{debug, info, warn};
use std::sync::Arc;
use std::time::Duration;

/// RPC provider factory - creates blockchain providers for different networks
/// Simplified from previous RpcProvider struct wrapper
/// Returns Arc<Provider<Http>> directly instead of wrapping it
///
/// Create an RPC provider using the primary configured provider
pub fn create_provider(
    chain_id: u64,
    config: &crate::config::Config,
) -> Result<Arc<Provider<Http>>, Error> {
    let primary_provider = &config.rpc.primary_provider;

    match primary_provider.to_lowercase().as_str() {
        "infura" => {
            if let Some(api_key) = &config.api_keys.infura {
                create_infura_provider(chain_id, api_key)
            } else {
                Err(Error::Config("Infura API key not found".to_string()))
            }
        }
        "alchemy" => {
            if let Some(api_key) = &config.api_keys.alchemy {
                create_alchemy_provider(chain_id, api_key)
            } else {
                Err(Error::Config("Alchemy API key not found".to_string()))
            }
        }
        _ => {
            warn!(
                "Unknown provider {}, falling back to free provider",
                primary_provider
            );
            create_free_provider(chain_id)
        }
    }
}

fn create_infura_provider(chain_id: u64, api_key: &str) -> Result<Arc<Provider<Http>>, Error> {
    let url = match chain_id {
        1 => format!("https://mainnet.infura.io/v3/{}", api_key),
        5 => format!("https://goerli.infura.io/v3/{}", api_key),
        11155111 => format!("https://sepolia.infura.io/v3/{}", api_key),
        137 => format!("https://polygon-mainnet.infura.io/v3/{}", api_key),
        _ => {
            return Err(Error::Config(format!(
                "Infura not supported for chain {}",
                chain_id
            )))
        }
    };

    let provider = Provider::<Http>::try_from(&url)
        .map_err(|e| Error::Network(format!("Failed to create Infura provider: {}", e)))?
        .interval(Duration::from_secs(30));

    info!("✅ Created Infura RPC provider for chain {}", chain_id);
    Ok(Arc::new(provider))
}

fn create_alchemy_provider(chain_id: u64, api_key: &str) -> Result<Arc<Provider<Http>>, Error> {
    let url = match chain_id {
        1 => format!("https://eth-mainnet.g.alchemy.com/v2/{}", api_key),
        5 => format!("https://eth-goerli.g.alchemy.com/v2/{}", api_key),
        11155111 => format!("https://eth-sepolia.g.alchemy.com/v2/{}", api_key),
        137 => format!("https://polygon-mainnet.g.alchemy.com/v2/{}", api_key),
        _ => {
            return Err(Error::Config(format!(
                "Alchemy not supported for chain {}",
                chain_id
            )))
        }
    };

    let provider = Provider::<Http>::try_from(&url)
        .map_err(|e| Error::Network(format!("Failed to create Alchemy provider: {}", e)))?
        .interval(Duration::from_secs(30));

    debug!("Created Alchemy RPC provider for chain {}", chain_id);
    Ok(Arc::new(provider))
}

fn create_free_provider(chain_id: u64) -> Result<Arc<Provider<Http>>, Error> {
    let url = match chain_id {
        1 => "https://1rpc.io/eth",
        137 => "https://1rpc.io/matic",
        _ => {
            return Err(Error::Config(format!(
                "No free provider for chain {}",
                chain_id
            )))
        }
    };

    let provider = Provider::<Http>::try_from(url)
        .map_err(|e| Error::Network(format!("Failed to create free provider: {}", e)))?
        .interval(Duration::from_secs(30));

    info!("✅ Created free RPC provider for chain {}", chain_id);
    Ok(Arc::new(provider))
}
