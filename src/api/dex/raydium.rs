use crate::error::Error;
use crate::types::dex::DexStats;
use log::debug;
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RaydiumStats {
    #[serde(rename = "volume24h")]
    volume_24h: f64,
    tvl: f64,
    #[serde(rename = "pairCount")]
    pair_count: usize,
}

pub async fn get_dex_stats(dex: &str, chain: &str) -> Result<DexStats, Error> {
    debug!("Fetching DEX stats for {}", dex);
    
    let client = Client::new();
    let url = "https://api.raydium.io/v2/main/stats";
    debug!("Raydium URL: {}", url);
    
    let response = client.get(url)
        .send()
        .await?
        .error_for_status()?;
        
    let stats = response.json::<RaydiumStats>().await?;
    
    Ok(DexStats {
        volume_24h: stats.volume_24h,
        total_liquidity: stats.tvl,
        pair_count: stats.pair_count,
    })
} 