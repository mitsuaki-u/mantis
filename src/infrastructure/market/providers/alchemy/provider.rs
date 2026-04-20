use super::super::queries;
use super::{events, graphql, pricing, quality, types};
use crate::config::TradingConfig;
use crate::core::constants::MIN_POOL_AGE_SECS;
use crate::core::domain::market::TokenMetrics;
use crate::infrastructure::errors::{Error, Result};
use crate::infrastructure::market::providers::traits::MarketDataProvider;
use async_trait::async_trait;
use chrono::Utc;
use log::{debug, info, warn};
use reqwest::Client;
use rust_decimal::Decimal;
use std::any::Any;
use std::collections::HashMap;
use std::str::FromStr;
use types::{AlchemyV3Response, PoolDayDataResponse, TokenPriceData, UniswapV3Pool};

/// Uniswap V3 subgraph provider
pub struct AlchemyUniswapV3Provider {
    client: Client,
    network: String,
    subgraph_url: String,
    subgraph_api_key: Option<String>,
    trading_config: TradingConfig,
    request_timeout_secs: u64,
    event_router: Option<std::sync::Arc<crate::EventRouter>>,
    // Cached ETH price in USD, updated from WETH/USDC pools
    cached_eth_price_usd: std::sync::RwLock<Option<f64>>,
}

impl AlchemyUniswapV3Provider {
    pub fn new(
        network: &str,
        subgraph_url: String,
        subgraph_api_key: Option<String>,
        trading_config: TradingConfig,
        request_timeout_secs: u64,
    ) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(request_timeout_secs))
            .user_agent(concat!("mantis-market-data/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            network: network.to_string(),
            subgraph_url,
            subgraph_api_key,
            trading_config,
            request_timeout_secs,
            event_router: None,
            cached_eth_price_usd: std::sync::RwLock::new(None),
        }
    }

    /// Set the event router for pool event publishing
    pub fn with_event_router(mut self, event_router: std::sync::Arc<crate::EventRouter>) -> Self {
        self.event_router = Some(event_router);
        self
    }

    /// Fetch top V3 pools with high liquidity and volume
    async fn fetch_top_v3_pools(
        &self,
        min_tvl_usd: Decimal,
        min_transaction_count: u32,
        limit: usize,
    ) -> Result<Vec<UniswapV3Pool>> {
        let min_liquidity =
            crate::core::utils::f64_to_decimal(self.trading_config.min_liquidity, "min_liquidity")
                .map_err(|e| Error::InvalidInput(format!("Invalid min_liquidity config: {}", e)))?;

        let min_volume =
            crate::core::utils::f64_to_decimal(self.trading_config.min_volume, "min_volume")
                .map_err(|e| Error::InvalidInput(format!("Invalid min_volume config: {}", e)))?;

        debug!(
            "Querying V3 pools (TVL>${}, Vol24h>${}, Liq>${}, Txs>{}, Age>6mo, Limit:{})",
            min_tvl_usd, min_volume, min_liquidity, min_transaction_count, limit
        );

        let max_created_timestamp = chrono::Utc::now().timestamp() - MIN_POOL_AGE_SECS as i64;

        use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;
        let weth_address = if self.network == "ethereum" || self.network == "mainnet" {
            Some(format!("{:?}", NetworkAddresses::mainnet().weth))
        } else {
            None
        };

        debug!(
            "WETH filtering: {}",
            weth_address
                .as_ref()
                .map(|addr| format!("enabled ({})", addr))
                .unwrap_or_else(|| "disabled (non-mainnet network)".to_string())
        );

        let query = queries::query_quality_pools(
            limit,
            &min_tvl_usd.to_string(),
            &min_liquidity.to_string(),
            min_transaction_count,
            max_created_timestamp,
        );

        let response = graphql::execute_v3_query(&self.client, &self.subgraph_url, self.subgraph_api_key.as_deref(), &query).await?;

        let mut pools_data: AlchemyV3Response = serde_json::from_value(response)
            .map_err(|e| Error::Parse(format!("Failed to parse V3 pools response: {}", e)))?;

        let initial_pool_count = pools_data.data.pools.len();
        if let Some(ref weth_addr) = weth_address {
            let weth_lower = weth_addr.to_lowercase();
            pools_data.data.pools.retain(|pool| {
                let token0_addr = pool.token0.id.to_lowercase();
                let token1_addr = pool.token1.id.to_lowercase();
                token0_addr == weth_lower || token1_addr == weth_lower
            });

            let filtered_count = initial_pool_count - pools_data.data.pools.len();
            if filtered_count > 0 {
                debug!(
                    "Pre-filtered {} non-WETH pools (before volume queries) - {} WETH pairs remaining",
                    filtered_count,
                    pools_data.data.pools.len()
                );
            }
        }

        self.enrich_pools_with_24h_volume(&mut pools_data.data.pools)
            .await?;

        let initial_count = pools_data.data.pools.len();
        pools_data.data.pools.retain(|pool| {
            match Decimal::from_str(&pool.volume_usd) {
                Ok(volume) if volume >= min_volume => true,
                Ok(_volume) => false, // Filtered out - summary logged below
                Err(_e) => false,     // Invalid volume - filtered out
            }
        });

        let filtered_count = initial_count - pools_data.data.pools.len();
        if filtered_count > 0 {
            debug!(
                "Filtered out {} pools with insufficient 24h volume (< ${})",
                filtered_count, min_volume
            );
        }

        let final_pool_count = pools_data.data.pools.len();
        debug!(
            "Found {} pools passing all quality filters (requested: {})",
            final_pool_count, limit
        );

        if final_pool_count < limit / 2 {
            warn!(
                "Low pool count: Only {} pools found out of {} requested. This may indicate strict filters or low market liquidity.",
                final_pool_count, limit
            );
        }

        Ok(pools_data.data.pools)
    }

    /// Fetch specific tokens if provided
    async fn fetch_specific_v3_tokens(
        &self,
        tokens_to_track: &[String],
    ) -> Result<Vec<UniswapV3Pool>> {
        if tokens_to_track.is_empty() {
            return Ok(Vec::new());
        }

        let where_conditions = match queries::build_token_where_conditions(tokens_to_track) {
            Some(conditions) => conditions,
            None => return Ok(Vec::new()),
        };

        let query = queries::query_pools_by_tokens(&where_conditions);

        let response = graphql::execute_v3_query(&self.client, &self.subgraph_url, self.subgraph_api_key.as_deref(), &query).await?;

        let mut pools_data: AlchemyV3Response = serde_json::from_value(response).map_err(|e| {
            Error::Parse(format!("Failed to parse specific tokens response: {}", e))
        })?;

        self.enrich_pools_with_24h_volume(&mut pools_data.data.pools)
            .await?;

        let min_volume =
            crate::core::utils::f64_to_decimal(self.trading_config.min_volume, "min_volume")
                .map_err(|e| Error::InvalidInput(format!("Invalid min_volume config: {}", e)))?;

        let initial_count = pools_data.data.pools.len();
        pools_data.data.pools.retain(|pool| {
            match Decimal::from_str(&pool.volume_usd) {
                Ok(volume) if volume >= min_volume => true,
                Ok(_volume) => false, // Filtered out - summary logged below
                Err(_e) => false,     // Invalid volume - filtered out
            }
        });

        let filtered_count = initial_count - pools_data.data.pools.len();
        if filtered_count > 0 {
            warn!(
                "Filtered out {} pools with insufficient 24h volume for specified tokens (safety threshold: ${})",
                filtered_count, min_volume
            );
        }

        debug!(
            "Found {} pools for specific tokens: {:?}",
            pools_data.data.pools.len(),
            tokens_to_track
        );

        Ok(pools_data.data.pools)
    }

    /// Enrich pools with accurate 24h volume data using separate poolDayDatas query
    ///
    /// Since poolDayDatas cannot be nested in pools query (schema limitation),
    /// we use a two-query approach:
    /// 1. Query pools by quality filters (TVL, liquidity, tx count, age)
    /// 2. Query poolDayDatas for those pool IDs to get 24h volume
    /// 3. Match results and update pool.volume_usd with accurate 24h data
    async fn enrich_pools_with_24h_volume(&self, pools: &mut [UniswapV3Pool]) -> Result<()> {
        if pools.is_empty() {
            return Ok(());
        }

        debug!("Fetching 24h volume data for {} pools", pools.len());

        let pool_ids: Vec<String> = pools.iter().map(|p| p.id.clone()).collect();

        use chrono::{Duration, Utc};
        let min_date = (Utc::now() - Duration::days(1)).timestamp();

        let query = queries::query_pool_day_data_for_pools(&pool_ids, min_date);

        match graphql::execute_v3_query(&self.client, &self.subgraph_url, self.subgraph_api_key.as_deref(), &query).await {
            Ok(response) => {
                let pool_day_data_response: PoolDayDataResponse = serde_json::from_value(response)
                    .map_err(|e| {
                        Error::Parse(format!("Failed to parse poolDayDatas response: {}", e))
                    })?;

                let mut volume_map: HashMap<String, String> = HashMap::new();

                for pool_day_data in pool_day_data_response.data.pool_day_datas {
                    let pool_id = pool_day_data.pool.id.to_lowercase();
                    volume_map
                        .entry(pool_id)
                        .or_insert(pool_day_data.volume_usd);
                }

                let mut updated_count = 0;
                let mut no_data_count = 0;
                for pool in pools.iter_mut() {
                    if let Some(volume_24h) = volume_map.get(&pool.id.to_lowercase()) {
                        pool.volume_usd = volume_24h.clone();
                        updated_count += 1;
                    } else {
                        pool.volume_usd = "0".to_string();
                        no_data_count += 1;
                    }
                }

                if no_data_count > 0 {
                    debug!(
                        "Updated 24h volume for {}/{} pools ({} with no data, set to 0)",
                        updated_count,
                        pools.len(),
                        no_data_count
                    );
                } else {
                    debug!(
                        "Updated 24h volume for {}/{} pools",
                        updated_count,
                        pools.len()
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to fetch poolDayDatas, keeping all-time cumulative volume: {}",
                    e
                );
            }
        }

        Ok(())
    }

    /// Aggregate token data from multiple V3 pools
    async fn aggregate_token_data_from_pools(
        &self,
        pools: Vec<UniswapV3Pool>,
    ) -> Result<Vec<TokenPriceData>> {
        if let Err(e) = pricing::update_eth_price_cache(
            &pools,
            &self.network,
            &self.cached_eth_price_usd,
            &self.client,
            &self.subgraph_url,
            self.subgraph_api_key.as_deref(),
        )
        .await
        {
            warn!("Failed to update ETH price cache: {}", e);
        }

        let mut token_data_map: HashMap<String, TokenPriceData> = HashMap::new();

        for pool in pools {
            let tvl_usd = match Decimal::from_str(&pool.tvl_usd) {
                Ok(val) => val,
                Err(e) => {
                    warn!(
                        "Skipping pool {} - invalid TVL {}: {}",
                        pool.id, pool.tvl_usd, e
                    );
                    continue;
                }
            };

            let volume_usd = match Decimal::from_str(&pool.volume_usd) {
                Ok(val) => val,
                Err(e) => {
                    warn!(
                        "Skipping pool {} - invalid 24h volume {}: {}",
                        pool.id, pool.volume_usd, e
                    );
                    continue;
                }
            };
            let fee_tier = pool.fee_tier.parse::<u32>().unwrap_or(
                crate::infrastructure::dex::ethereum::providers::uniswap_v3::V3FeeTier::Standard
                    as u32,
            );

            let min_liquidity = crate::core::utils::f64_to_decimal(
                self.trading_config.min_liquidity,
                "min_liquidity",
            )
            .map_err(|e| Error::InvalidInput(format!("Invalid min_liquidity config: {}", e)))?;

            if tvl_usd < min_liquidity {
                debug!(
                    "⏭️ Skipping pool {} - TVL ${} below minimum ${}",
                    pool.id, tvl_usd, min_liquidity
                );
                continue;
            }

            use crate::infrastructure::dex::ethereum::config::addresses::NetworkAddresses;
            let addresses = if self.network == "ethereum" {
                NetworkAddresses::mainnet()
            } else {
                continue;
            };

            let weth_addr = format!("{:?}", addresses.weth).to_lowercase();
            let token0_addr = pool.token0.id.to_lowercase();
            let token1_addr = pool.token1.id.to_lowercase();

            let is_weth_pair = token0_addr == weth_addr || token1_addr == weth_addr;
            if !is_weth_pair {
                warn!(
                    "Unexpected non-WETH pool {} (token0={}, token1={}) - should have been filtered earlier",
                    pool.id, pool.token0.symbol, pool.token1.symbol
                );
                continue;
            }

            let (target_token, is_target_token0) = if token0_addr == weth_addr {
                (&pool.token1, false) // WETH is token0, so target is token1
            } else {
                (&pool.token0, true) // WETH is token1, so target is token0
            };

            if target_token.id.to_lowercase() == weth_addr {
                continue;
            }

            debug!(
                "Processing WETH pair pool {} - Token: {}, TVL=${}, 24h Volume=${}, Fee={}",
                pool.id, target_token.symbol, tvl_usd, volume_usd, fee_tier
            );

            let token = target_token;
            let is_token0 = is_target_token0;
            {
                let token_address = token.id.to_lowercase();

                if token.symbol.is_empty() {
                    warn!(
                        "Skipping token {} in pool {} - empty symbol (subgraph data quality issue)",
                        token_address, pool.id
                    );
                    continue;
                }

                let decimals = token
                    .decimals
                    .parse::<u8>()
                    .unwrap_or(crate::infrastructure::constants::ERC20_STANDARD_DECIMALS);

                let price_usd = pricing::calculate_token_price_from_pool(
                    &pool,
                    is_token0,
                    &self.network,
                    &self.cached_eth_price_usd,
                )?;

                let entry = token_data_map
                    .entry(token_address.clone())
                    .or_insert_with(|| TokenPriceData {
                        address: token_address.clone(),
                        symbol: token.symbol.clone(),
                        name: token.name.clone(),
                        decimals,
                        price_usd,
                        volume_24h: Decimal::ZERO,
                        liquidity_usd: Decimal::ZERO,
                        pool_count: 0,
                        best_fee_tier: fee_tier,
                    });

                let weight = tvl_usd;
                let total_weight = entry.liquidity_usd + weight;

                if total_weight > Decimal::ZERO {
                    entry.price_usd =
                        (entry.price_usd * entry.liquidity_usd + price_usd * weight) / total_weight;
                }

                let two = Decimal::from(2);
                entry.volume_24h += volume_usd / two; // Divide by 2 since volume is for the pair
                entry.liquidity_usd += tvl_usd / two; // Divide by 2 since TVL is for the pair
                entry.pool_count += 1;

                let threshold = entry.liquidity_usd * Decimal::new(1, 1); // 0.1
                if tvl_usd > threshold && fee_tier < entry.best_fee_tier {
                    entry.best_fee_tier = fee_tier;
                }
            } // End of single token processing block
        }

        let total_tokens_created = token_data_map.len();
        info!(
            "Created {} unique tokens from pools (before filtering)",
            total_tokens_created
        );

        let min_liquidity =
            crate::core::utils::f64_to_decimal(self.trading_config.min_liquidity, "min_liquidity")
                .map_err(|e| Error::InvalidInput(format!("Invalid min_liquidity config: {}", e)))?;

        let filtered_tokens: Vec<TokenPriceData> = token_data_map
            .into_values()
            .filter(|token| {
                let sufficient_liquidity = token.liquidity_usd >= min_liquidity;
                let quality_check =
                    quality::is_token_quality_acceptable(&token.symbol, &token.name, &token.address);
                
                let volume_liquidity_ratio = if token.liquidity_usd > Decimal::ZERO {
                    token.volume_24h / token.liquidity_usd
                } else {
                    Decimal::ZERO
                };

                // 24h volume shouldn't exceed 20x liquidity (filters wash trading scams at 50x-100x+)
                let max_aggregate_volume_ratio = Decimal::from(20);
                let volume_check = volume_liquidity_ratio <= max_aggregate_volume_ratio;
                
                if !volume_check {
                    match crate::core::utils::decimal_to_f64(volume_liquidity_ratio, "volume_liquidity_ratio") {
                        Ok(ratio_f64) => {
                            warn!(
                                "🚫 Rejecting token {} - suspicious aggregated volume/liquidity ratio: {:.2}x (volume: ${}, liquidity: ${})",
                                token.symbol,
                                ratio_f64,
                                token.volume_24h,
                                token.liquidity_usd
                            );
                        }
                        Err(_) => {
                            warn!(
                                "🚫 Rejecting token {} - suspicious aggregated volume/liquidity ratio: {} (invalid conversion) (volume: ${}, liquidity: ${})",
                                token.symbol,
                                volume_liquidity_ratio,
                                token.volume_24h,
                                token.liquidity_usd
                            );
                        }
                    }
                }

                let final_decision = sufficient_liquidity && quality_check && volume_check;

                if !final_decision {
                    if !sufficient_liquidity {
                        debug!(
                            "Rejected {} - Insufficient liquidity: ${} < ${}",
                            token.symbol, token.liquidity_usd, min_liquidity
                        );
                    } else if !quality_check {
                        debug!("Rejected {} - Failed quality check", token.symbol);
                    } else if !volume_check {
                        debug!(
                            "Rejected {} - Suspicious volume/liquidity ratio",
                            token.symbol
                        );
                    }
                }

                match crate::core::utils::decimal_to_f64(volume_liquidity_ratio, "volume_liquidity_ratio") {
                    Ok(ratio_f64_debug) => {
                        debug!(
                            "Token {} ({}): liquidity=${}, volume=${}, vol/liq={:.2}x, pools={}, quality_ok={}, volume_ok={}, accepted={}",
                            token.symbol,
                            &token.address[..8],
                            token.liquidity_usd,
                            token.volume_24h,
                            ratio_f64_debug,
                            token.pool_count,
                            quality_check,
                            volume_check,
                            final_decision
                        );
                    }
                    Err(_) => {
                        debug!(
                            "Token {} ({}): liquidity=${}, volume=${}, vol/liq={} (invalid), pools={}, quality_ok={}, volume_ok={}, accepted={}",
                            token.symbol,
                            &token.address[..8],
                            token.liquidity_usd,
                            token.volume_24h,
                            volume_liquidity_ratio,
                            token.pool_count,
                            quality_check,
                            volume_check,
                            final_decision
                        );
                    }
                }

                final_decision
            })
            .collect();

        let rejected_count = total_tokens_created - filtered_tokens.len();
        info!(
            "Filtered {} tokens with liquidity >= ${} (rejected: {}/{})",
            filtered_tokens.len(),
            min_liquidity,
            rejected_count,
            total_tokens_created
        );

        Ok(filtered_tokens)
    }

    /// Convert TokenPriceData to TokenMetrics
    fn token_price_data_to_metrics(&self, token_data: &TokenPriceData) -> Option<TokenMetrics> {
        let price_usd_f64 =
            crate::core::utils::decimal_to_f64(token_data.price_usd, "price_usd").ok()?;
        let volume_24h_f64 =
            crate::core::utils::decimal_to_f64(token_data.volume_24h, "volume_24h").ok()?;

        let chain_id = crate::infrastructure::network::get_chain_id(Some(&self.network));
        let token_id =
            crate::core::utils::normalization::create_token_id(chain_id, &token_data.address)
                .ok()?;

        Some(TokenMetrics {
            id: token_id,
            symbol: token_data.symbol.clone(),
            name: token_data.name.clone(),
            decimals: token_data.decimals,
            price_usd: price_usd_f64,
            volume_24h: volume_24h_f64,
            price_change_24h: 0.0, // Would need historical data
            chain: Some(self.network.clone()),
            last_updated: Utc::now(),
        })
    }
}

#[async_trait]
impl MarketDataProvider for AlchemyUniswapV3Provider {
    fn name(&self) -> &str {
        "Alchemy Uniswap V3"
    }

    async fn get_market_data(
        &self,
        max_tokens_to_scan: usize,
        tokens_to_track: &[String],
        network: &str,
    ) -> Result<Vec<TokenMetrics>> {
        debug!(
            "Starting token discovery for {} (max tokens: {})",
            network, max_tokens_to_scan
        );

        if !tokens_to_track.is_empty() {
            debug!(
                "Using targeted queries for {} specific tokens",
                tokens_to_track.len()
            );

            let pools = self.fetch_specific_v3_tokens(tokens_to_track).await?;
            let _ =
                events::publish_pool_events(&pools, "targeted_tokens", &self.event_router).await;

            let token_data = self.aggregate_token_data_from_pools(pools).await?;

            let metrics: Vec<TokenMetrics> = token_data
                .iter()
                .filter_map(|td| self.token_price_data_to_metrics(td))
                .collect();
            debug!("Returning {} targeted tokens", metrics.len());
            return Ok(metrics);
        }

        let base_liquidity =
            crate::core::utils::f64_to_decimal(self.trading_config.min_liquidity, "min_liquidity")
                .map_err(|e| Error::InvalidInput(format!("Invalid min_liquidity config: {}", e)))?;

        use crate::core::constants::{
            HIGH_QUALITY_SCAN_LIQ_MULTIPLIER, LOW_QUALITY_SCAN_LIQ_MULTIPLIER,
            MEDIUM_QUALITY_SCAN_LIQ_MULTIPLIER, MEDIUM_SCAN_THRESHOLD, SMALL_SCAN_THRESHOLD,
            UNLIMITED_SCAN_MAX_LIMIT,
        };

        let (min_tvl, limit) = if max_tokens_to_scan == 0 {
            (base_liquidity, UNLIMITED_SCAN_MAX_LIMIT)
        } else if max_tokens_to_scan <= SMALL_SCAN_THRESHOLD {
            (
                base_liquidity * Decimal::from(HIGH_QUALITY_SCAN_LIQ_MULTIPLIER),
                max_tokens_to_scan,
            )
        } else if max_tokens_to_scan <= MEDIUM_SCAN_THRESHOLD {
            (
                base_liquidity * Decimal::from(MEDIUM_QUALITY_SCAN_LIQ_MULTIPLIER),
                max_tokens_to_scan,
            )
        } else {
            (
                base_liquidity * Decimal::from(LOW_QUALITY_SCAN_LIQ_MULTIPLIER),
                max_tokens_to_scan,
            )
        };

        debug!(
            "Using discovery mode for up to {} tokens with min TVL: ${}",
            limit, min_tvl
        );

        let pools = self
            .fetch_top_v3_pools(
                min_tvl,
                self.trading_config.min_pool_transaction_count,
                limit,
            )
            .await?;

        let discovery_mode = format!("{}_token_discovery", limit);
        let _ = events::publish_pool_events(&pools, &discovery_mode, &self.event_router).await;

        let token_data = self.aggregate_token_data_from_pools(pools).await?;

        let mut metrics: Vec<TokenMetrics> = token_data
            .iter()
            .filter_map(|td| self.token_price_data_to_metrics(td))
            .collect();

        metrics.sort_by(|a, b| {
            b.volume_24h
                .partial_cmp(&a.volume_24h)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        metrics.truncate(limit);

        debug!("Returning {} discovered tokens", metrics.len());
        Ok(metrics)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn clone_box(&self) -> Box<dyn MarketDataProvider> {
        Box::new(AlchemyUniswapV3Provider::new(
            &self.network,
            self.subgraph_url.clone(),
            self.subgraph_api_key.clone(),
            self.trading_config.clone(),
            self.request_timeout_secs,
        ))
    }
}
