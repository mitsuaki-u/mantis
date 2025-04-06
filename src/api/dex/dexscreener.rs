use crate::error::Error;
use crate::types::dex::{DexPair, Token};
use log::debug;
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct DexScreenerResponse {
    pairs: Vec<DexScreenerPair>,
}

#[derive(Debug, Deserialize)]
struct DexScreenerPair {
    #[serde(rename = "baseToken")]
    base_token: DexScreenerToken,
    #[serde(rename = "quoteToken")]
    quote_token: DexScreenerToken,
    #[serde(rename = "priceUsd")]
    price_usd: String,
    volume: DexScreenerVolume,
    liquidity: f64,
}

#[derive(Debug, Deserialize)]
struct DexScreenerToken {
    address: String,
    name: String,
    symbol: String,
}

#[derive(Debug, Deserialize)]
struct DexScreenerVolume {
    h24: Option<f64>,
}

pub async fn get_dex_pair(address: &str, chain: &str) -> Result<Vec<DexPair>, Error> {
    debug!("Fetching DEX pairs for {} on {}", address, chain);
    
    let client = Client::new();
    let url = format!(
        "https://api.dexscreener.com/latest/dex/tokens/{},{}",
        address, chain
    );
    debug!("DexScreener URL: {}", url);
    
    let response = client.get(&url).send().await?.error_for_status()?;
    let data = response.json::<DexScreenerResponse>().await?;
    
    if data.pairs.is_empty() {
        return Err(Error::NotFound("No pairs found on DexScreener".to_string()));
    }

    Ok(data.pairs.into_iter()
        .map(|pair| DexPair {
            token0: Token {
                address: pair.base_token.address,
                symbol: pair.base_token.symbol,
                name: pair.base_token.name,
            },
            token1: Token {
                address: pair.quote_token.address,
                symbol: pair.quote_token.symbol,
                name: pair.quote_token.name,
            },
            price: pair.price_usd.parse().unwrap_or_default(),
            volume_24h: pair.volume.h24.unwrap_or_default(),
            liquidity: pair.liquidity,
        })
        .collect())
} 