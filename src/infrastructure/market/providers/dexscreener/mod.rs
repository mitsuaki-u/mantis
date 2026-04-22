use crate::core::domain::market::TokenMetrics;
use crate::infrastructure::errors::{Error, Result};
use crate::infrastructure::market::providers::traits::MarketDataProvider;
use async_trait::async_trait;
use chrono::Utc;
use log::{debug, info, warn};
use reqwest::Client;
use serde::Deserialize;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const DEXSCREENER_TOKENS_URL: &str = "https://api.dexscreener.com/latest/dex/tokens/";
// Trending tokens on DexScreener — fast, small, no auth needed.
// Returns the most actively traded Solana tokens right now.
const DEXSCREENER_TRENDING_URL: &str = "https://api.dexscreener.com/token-boosts/top/v1";
// Broad search for active Solana pairs by keyword
const DEXSCREENER_SEARCH_URLS: &[&str] = &[
    "https://api.dexscreener.com/latest/dex/search?q=bonk",
    "https://api.dexscreener.com/latest/dex/search?q=sol",
    "https://api.dexscreener.com/latest/dex/search?q=wif",
    "https://api.dexscreener.com/latest/dex/search?q=jup",
    "https://api.dexscreener.com/latest/dex/search?q=ray",
];
const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
const TOKEN_LIST_CACHE_SECS: u64 = 900;  // refresh token list every 15 min (fast endpoint)
const TOKEN_LIST_BACKOFF_SECS: u64 = 60; // after a failure, retry in 1 min

// Known stablecoins — no price movement, no momentum signals
const STABLECOIN_MINTS: &[&str] = &[
    "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", // USDC
    "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", // USDT
    "USD1ttGY1N17NEEHLmELoaybftRBUSErhqYiQzvEmuB",  // USD1
    "9vMJfxuKxXBoEa7rM12mYLMwTacLMLDJqHozw96WQL8i", // UST
    "USDH1SM1ojwWUga67PGrgFWUHibbjqMvuMaDkRJTgkX",  // USDH
    "CXLBjMMcwkc17GfJtBos6rQCo1ypeH6eDbB82Kby4MRm", // UXD
    "7kbnvuGBxxj8AG9qp8Scn56muWGaRaFqxg1FsRp3PaFT", // UXD v2
];


struct TokenListCache {
    mints: Vec<String>,
    fetched_at: Instant,
    /// True if the cache was populated from a failed fetch (backoff mode).
    /// In backoff mode the TTL is shorter so we retry sooner.
    is_backoff: bool,
}

/// DexScreener market data provider for Solana.
/// Discovers trending tokens via DexScreener's trending endpoint (refreshed every 15 min),
/// fetches current prices from DexScreener each scan cycle. No external dependencies.
pub struct DexScreenerProvider {
    client: Client,
    min_volume_usd: f64,
    min_liquidity_usd: f64,
    token_cache: Arc<RwLock<Option<TokenListCache>>>,
}

impl DexScreenerProvider {
    pub fn new(min_volume_usd: f64, min_liquidity_usd: f64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("mantis-market-data/", env!("CARGO_PKG_VERSION")))
            .gzip(true)
            .deflate(true)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            min_volume_usd,
            min_liquidity_usd,
            token_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Get the list of token mints to scan, refreshing from DexScreener trending every 15 min.
    ///
    /// Caching strategy:
    /// - Normal: list cached for TOKEN_LIST_CACHE_SECS (15 min). Prices are fetched from
    ///   DexScreener every scan cycle — only the list of which tokens to watch is cached.
    /// - On failure: back off TOKEN_LIST_BACKOFF_SECS (1 min), reuse last known list so
    ///   trading continues uninterrupted.
    /// - No prior list + failure: skip this scan cycle.
    async fn get_token_mints(&self, limit: usize) -> Vec<String> {
        {
            let cache = self.token_cache.read().await;
            if let Some(ref c) = *cache {
                let ttl = if c.is_backoff { TOKEN_LIST_BACKOFF_SECS } else { TOKEN_LIST_CACHE_SECS };
                if c.fetched_at.elapsed().as_secs() < ttl {
                    if !c.mints.is_empty() {
                        debug!("Token list cache hit ({} mints)", c.mints.len());
                        return c.mints.iter().take(limit).cloned().collect();
                    }
                    return vec![];
                }
            }
        }

        info!("Fetching trending Solana tokens from DexScreener...");
        match self.fetch_dexscreener_trending_mints(limit).await {
            Ok(mints) if !mints.is_empty() => {
                info!("DexScreener trending: discovered {} token mints", mints.len());
                let mut cache = self.token_cache.write().await;
                *cache = Some(TokenListCache {
                    mints: mints.clone(),
                    fetched_at: Instant::now(),
                    is_backoff: false,
                });
                mints
            }
            Ok(_) => {
                warn!("DexScreener trending returned empty list — backing off {}s", TOKEN_LIST_BACKOFF_SECS);
                let mut cache = self.token_cache.write().await;
                *cache = Some(TokenListCache { mints: vec![], fetched_at: Instant::now(), is_backoff: true });
                vec![]
            }
            Err(e) => {
                warn!("DexScreener trending failed: {} — backing off {}s", e, TOKEN_LIST_BACKOFF_SECS);
                let mut cache = self.token_cache.write().await;
                let last_mints = cache.as_ref().map(|c| c.mints.clone()).unwrap_or_default();
                if !last_mints.is_empty() {
                    info!("Using last known token list ({} mints) during backoff", last_mints.len());
                }
                *cache = Some(TokenListCache { mints: last_mints.clone(), fetched_at: Instant::now(), is_backoff: true });
                last_mints.into_iter().take(limit).collect()
            }
        }
    }

    /// Discover top Solana token mints using DexScreener's trending and search endpoints.
    /// Fast (small responses), no auth, no dependency on Raydium.
    async fn fetch_dexscreener_trending_mints(&self, limit: usize) -> Result<Vec<String>> {
        let mut mint_volumes: HashMap<String, f64> = HashMap::new();

        // 1. Trending/boosted tokens (small, fast)
        if let Ok(response) = self.client.get(DEXSCREENER_TRENDING_URL).send().await {
            if response.status().is_success() {
                #[derive(serde::Deserialize)]
                struct BoostEntry {
                    #[serde(rename = "chainId")]
                    chain_id: String,
                    #[serde(rename = "tokenAddress")]
                    token_address: String,
                }
                if let Ok(entries) = response.json::<Vec<BoostEntry>>().await {
                    for e in entries {
                        if e.chain_id == "solana"
                            && !STABLECOIN_MINTS.contains(&e.token_address.as_str())
                            && e.token_address != WSOL_MINT
                        {
                            mint_volumes.entry(e.token_address).or_insert(0.0);
                        }
                    }
                }
            }
        }

        // 2. Search for active pairs across popular Solana tokens
        #[derive(serde::Deserialize)]
        struct SearchResponse {
            #[serde(default)]
            pairs: Vec<DexPair>,
        }

        let search_futures: Vec<_> = DEXSCREENER_SEARCH_URLS
            .iter()
            .map(|url| {
                let client = self.client.clone();
                let url = url.to_string();
                async move {
                    client.get(&url).send().await
                        .ok()?
                        .json::<SearchResponse>().await.ok()
                }
            })
            .collect();

        let search_results = futures::future::join_all(search_futures).await;
        for result in search_results.into_iter().flatten() {
            for mut pair in result.pairs {
                if pair.chain_id != "solana" { continue; }
                // Normalise: real token in base position
                if pair.base_token.address == WSOL_MINT {
                    if let Some(qt) = pair.quote_token.take() {
                        pair.base_token = qt;
                    } else { continue; }
                }
                let addr = &pair.base_token.address;
                if addr == WSOL_MINT || STABLECOIN_MINTS.contains(&addr.as_str()) { continue; }
                let entry = mint_volumes.entry(addr.clone()).or_insert(0.0);
                *entry = entry.max(pair.volume.h24);
            }
        }

        let mut ranked: Vec<(String, f64)> = mint_volumes.into_iter().collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(ranked.into_iter().take(limit).map(|(mint, _)| mint).collect())
    }

    /// Batch-query DexScreener for current price/volume data on a list of mints.
    /// Chunks are fetched concurrently (DexScreener accepts up to 30 addresses per request).
    async fn fetch_dexscreener_data(&self, mints: &[String], limit: usize) -> Result<Vec<DexPair>> {
        let chunk_size = 30;

        #[derive(serde::Deserialize)]
        struct TokenResponse {
            #[serde(default)]
            pairs: Vec<DexPair>,
        }

        // Build one future per chunk, then run all concurrently
        let chunk_futures: Vec<_> = mints
            .chunks(chunk_size)
            .map(|chunk| {
                let client = self.client.clone();
                let url = format!("{}{}", DEXSCREENER_TOKENS_URL, chunk.join(","));
                async move {
                    let response = client
                        .get(&url)
                        .send()
                        .await
                        .map_err(|e| Error::Network(format!("DexScreener request failed: {}", e)))?;

                    if !response.status().is_success() {
                        return Err(Error::Network(format!(
                            "DexScreener returned status: {}",
                            response.status()
                        )));
                    }

                    let resp: TokenResponse = response
                        .json()
                        .await
                        .map_err(|e| Error::Parse(format!("Failed to parse DexScreener response: {}", e)))?;

                    Ok(resp.pairs)
                }
            })
            .collect();

        let results = futures::future::join_all(chunk_futures).await;

        // Collect results — fail fast on any chunk error
        let mut all_pairs: Vec<DexPair> = Vec::new();
        for result in results {
            all_pairs.extend(result?);
        }

        // For each token, keep the highest-volume Solana pair
        let mut best: HashMap<String, DexPair> = HashMap::new();

        for mut p in all_pairs {
            if p.chain_id != "solana" {
                continue;
            }
            // Normalise: real token should be in base_token position
            if p.base_token.address == WSOL_MINT {
                if let Some(qt) = p.quote_token.take() {
                    p.base_token = qt;
                } else {
                    continue;
                }
            }
            if p.base_token.address == WSOL_MINT
                || STABLECOIN_MINTS.contains(&p.base_token.address.as_str())
            {
                continue;
            }
            if p.liquidity.usd < self.min_liquidity_usd {
                continue;
            }
            let mint = p.base_token.address.clone();
            let entry = best.entry(mint).or_insert_with(|| p.clone());
            if p.volume.h24 > entry.volume.h24 {
                *entry = p;
            }
        }

        let mut result: Vec<DexPair> = best
            .into_values()
            .filter(|p| p.volume.h24 >= self.min_volume_usd)
            .collect();

        result.sort_by(|a, b| b.volume.h24.partial_cmp(&a.volume.h24).unwrap());
        result.truncate(limit);
        Ok(result)
    }
}

#[async_trait]
impl MarketDataProvider for DexScreenerProvider {
    fn name(&self) -> &str {
        "dexscreener_solana"
    }

    async fn get_market_data(
        &self,
        max_tokens_to_scan: usize,
        tokens_to_track: &[String],
        _network: &str,
    ) -> Result<Vec<TokenMetrics>> {
        let limit = if max_tokens_to_scan == 0 { 100 } else { max_tokens_to_scan };

        let mints: Vec<String> = if tokens_to_track.is_empty() {
            self.get_token_mints(limit).await
        } else {
            tokens_to_track.to_vec()
        };

        if mints.is_empty() {
            warn!("No token mints to scan");
            return Ok(vec![]);
        }

        let pairs = self.fetch_dexscreener_data(&mints, limit).await?;

        if pairs.is_empty() {
            warn!(
                "DexScreener: no tokens passed filters (vol>${:.0}, liq>${:.0})",
                self.min_volume_usd, self.min_liquidity_usd
            );
            return Ok(vec![]);
        }

        let now = Utc::now();
        let mut metrics = Vec::with_capacity(pairs.len());

        for pair in &pairs {
            let price_usd = pair.price_usd.unwrap_or(0.0);
            if price_usd <= 0.0 {
                continue;
            }
            metrics.push(TokenMetrics {
                id: pair.base_token.address.clone(),
                symbol: pair.base_token.symbol.clone(),
                name: pair.base_token.name.clone(),
                decimals: 9,
                price_usd,
                price_change_24h: pair.price_change.h24,
                volume_24h: pair.volume.h24,
                chain: Some("solana".to_string()),
                last_updated: now,
            });
        }

        info!(
            "DexScreener: {} Solana tokens (vol>${:.0}, liq>${:.0})",
            metrics.len(),
            self.min_volume_usd,
            self.min_liquidity_usd,
        );

        Ok(metrics)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn MarketDataProvider> {
        // Share the token cache Arc so clones don't redundantly re-fetch Raydium
        Box::new(DexScreenerProvider {
            client: self.client.clone(),
            min_volume_usd: self.min_volume_usd,
            min_liquidity_usd: self.min_liquidity_usd,
            token_cache: self.token_cache.clone(),
        })
    }
}

// ── DexScreener response types ────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DexPair {
    chain_id: String,
    #[allow(dead_code)]
    #[serde(default)]
    dex_id: String,
    base_token: DexToken,
    #[serde(default)]
    quote_token: Option<DexToken>,
    #[serde(default, deserialize_with = "deserialize_string_f64")]
    price_usd: Option<f64>,
    #[serde(default)]
    volume: DexVolume,
    #[serde(default)]
    liquidity: DexLiquidity,
    #[serde(default)]
    price_change: DexPriceChange,
}

#[derive(Debug, Deserialize, Clone)]
struct DexToken {
    address: String,
    name: String,
    symbol: String,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct DexVolume {
    #[serde(default)]
    h24: f64,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct DexLiquidity {
    #[serde(default)]
    usd: f64,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct DexPriceChange {
    #[serde(default)]
    h24: f64,
}

fn deserialize_string_f64<'de, D>(deserializer: D) -> std::result::Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    let v = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(v.and_then(|val| match val {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }))
}
