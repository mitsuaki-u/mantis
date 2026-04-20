use super::MarketDataActor;
use crate::application::errors::Result;
use crate::core::domain::market::TokenMetrics;
use crate::core::utils::validation::price::validate_token_data;
use crate::events::{Event, MarketEvent};
use log::{debug, error, info, warn};
use std::time::{Duration, Instant};
use tokio::time::interval;

impl MarketDataActor {
    /// Start the background market data collection task
    pub async fn start_market_data_collection_task(&mut self) -> Result<()> {
        if self.collection_task.read().await.is_some() {
            warn!(
                "MarketDataActor Start command received, but collection task is already running."
            );
            return Ok(());
        }

        let mut actor_clone_for_task = self.clone();

        let collection_handle = tokio::spawn(async move {
            debug!(
                "Market data collection task STARTED for provider: {}",
                actor_clone_for_task.market_api.name()
            );

            let mut backoff_secs = 1;
            let max_backoff_secs = crate::application::constants::MARKET_RETRY_MAX_BACKOFF_SECS;

            let loop_result: Result<()> = loop {
                if !actor_clone_for_task.state.running {
                    info!("Actor stopped, exiting collection task normally.");
                    break Ok(());
                }

                match actor_clone_for_task.run_polling_loop().await {
                    Ok(_) => {
                        if actor_clone_for_task.state.running {
                            warn!("Market data loop exited cleanly but actor still running. Restarting loop after delay.");
                            backoff_secs = 1;
                            tokio::time::sleep(Duration::from_secs(
                                crate::application::constants::INITIAL_RETRY_DELAY_SECS,
                            ))
                            .await;
                        } else {
                            info!("Market data loop exited because actor is no longer running.");
                            break Ok(());
                        }
                    }
                    Err(e) => {
                        error!(
                            "Market data loop error: {}. Will retry in {}s",
                            e, backoff_secs
                        );
                        if !actor_clone_for_task.state.running {
                            info!("Actor stopped during error handling, exiting collection task with error: {}", e);
                            break Err(e);
                        }
                        tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                        backoff_secs = std::cmp::min(backoff_secs * 2, max_backoff_secs);
                    }
                }
            };

            info!(
                "Market data collection task finished for provider: {}. Result: {:?}",
                actor_clone_for_task.market_api.name(),
                loop_result
            );
            loop_result
        });

        *self.collection_task.write().await = Some(collection_handle);
        debug!("MarketDataActor data collection task scheduled.");
        Ok(())
    }

    /// Stop the background market data collection task
    pub async fn stop_market_data_collection_task(&mut self) -> Result<()> {
        if let Some(handle) = self.collection_task.write().await.take() {
            info!("Signalling market data collection task to stop...");
            tokio::spawn(async move {
                match tokio::time::timeout(
                    Duration::from_secs(crate::application::constants::MARKET_TASK_TIMEOUT_SECS),
                    handle,
                )
                .await
                {
                    Ok(Ok(_)) => {
                        info!("Market data collection task joined successfully.")
                    }
                    Ok(Err(e)) => {
                        error!("Market data collection task panicked: {:?}", e)
                    }
                    Err(_) => {
                        error!("Market data collection task join timed out. Aborting.");
                    }
                }
            });
        } else {
            info!("No active market data collection task to stop.");
        }
        Ok(())
    }

    /// Main polling loop - fetches market data at regular intervals
    pub async fn run_polling_loop(&mut self) -> Result<()> {
        let provider_name = self.market_api.name().to_string();

        debug!(
            "Market data collection starting: provider={}, tokens={}, interval={}s",
            provider_name,
            if !self.tokens_to_track.is_empty() {
                self.tokens_to_track.join(", ")
            } else {
                format!("auto-discovery (max: {})", self.max_tokens_to_scan)
            },
            self.scan_interval
        );

        let mut interval_timer = interval(Duration::from_secs(self.scan_interval));
        let mut poll_count = 0;
        let mut last_status_log = Instant::now();

        while self.state.running {
            // Check global shutdown flag
            if crate::application::app::is_forced_shutdown() {
                info!("MarketDataActor: Global shutdown detected, exiting polling loop");
                break;
            }

            interval_timer.tick().await;
            poll_count += 1;

            info!(
                "Market scan #{}: Fetching data for {} tokens (interval: {}s)",
                poll_count,
                if !self.tokens_to_track.is_empty() {
                    self.tokens_to_track.len()
                } else {
                    self.max_tokens_to_scan
                },
                self.scan_interval
            );

            // Get network from config, defaulting to mainnet if not specified
            let network = self.config.dex.network.as_deref().unwrap_or("mainnet");

            // Fetch market data from provider
            let data_result = self
                .market_api
                .get_market_data(self.max_tokens_to_scan, &self.tokens_to_track, network)
                .await;

            self.handle_market_data(data_result).await;

            // Log status every 5 minutes
            if last_status_log.elapsed() >= Duration::from_secs(300) {
                info!(
                    "[POLLING] [{}]: Completed {} polling cycles in {:.1} minutes",
                    provider_name,
                    poll_count,
                    last_status_log.elapsed().as_secs_f64() / 60.0
                );
                last_status_log = Instant::now();
            }
        }

        info!(
            "MarketDataActor: [{}] Polling loop ended after {} polls",
            provider_name, poll_count
        );
        Ok(())
    }

    /// Handle market data result and process token metrics
    async fn handle_market_data(&mut self, data_result: Result<Vec<TokenMetrics>>) {
        let start_time = Instant::now();

        match data_result {
            Ok(token_metrics_vec) => {
                let metrics_count = token_metrics_vec.len();
                let network = self.config.dex.network.as_deref().unwrap_or("mainnet");

                debug!(
                    "Processing {} token metrics from API (max_tokens: {}, network: {})",
                    metrics_count, self.max_tokens_to_scan, network
                );

                // Log token tracking configuration for clarity
                self.log_token_discovery_info(metrics_count, network);

                let mut processed_event_count = 0;
                let mut tracking_filtered_count = 0;

                debug!(
                    "Starting validation of {} tokens from market data provider",
                    metrics_count
                );

                for metrics in token_metrics_vec.iter() {
                    debug!(
                        "{} ({}): Price=${:.6}, Volume=${:.2}",
                        metrics.symbol,
                        &metrics.id[..10],
                        metrics.price_usd,
                        metrics.volume_24h
                    );

                    // Validate token data (price > 0)
                    if !validate_token_data(metrics) {
                        debug!("Skipping token {} (failed validation)", metrics.id);
                        continue;
                    }

                    // Check if token is in tracking list (if tracking specific tokens)
                    if !self.tokens_to_track.is_empty() {
                        let normalized_token_id =
                            crate::core::domain::token::TokenData::normalize_token_id(&metrics.id);
                        if !self.tokens_to_track.contains(&normalized_token_id) {
                            debug!(
                                "Skipping token {} (not in tracked list: {:?})",
                                metrics.id, self.tokens_to_track
                            );
                            tracking_filtered_count += 1;
                            continue;
                        }
                    }

                    // Log token processing
                    if !self.tokens_to_track.is_empty() {
                        debug!(
                            "Processing tracked token: {} at ${:.4}",
                            metrics.id, metrics.price_usd
                        );
                    } else {
                        debug!(
                            "Processing discovered token: {} at ${:.4}",
                            metrics.id, metrics.price_usd
                        );
                    }

                    // Publish MarketEvent for DatabaseActor to persist
                    let market_event = Event::Market(MarketEvent::PriceUpdate {
                        token_id: metrics.id.clone(),
                        price: metrics.price_usd,
                        volume: Some(metrics.volume_24h),
                        symbol: metrics.symbol.clone(),
                        name: metrics.name.clone(),
                        decimals: metrics.decimals,
                        timestamp: metrics.last_updated,
                    });

                    if let Err(e) = self.event_router.publish(market_event).await {
                        error!("Failed to publish market event for {}: {}", metrics.id, e);
                    }
                    processed_event_count += 1;
                }

                // Log summary
                if !self.tokens_to_track.is_empty() {
                    debug!(
                        "Processed {} out of {} available tokens (tracking {} specific tokens: {} skipped)",
                        processed_event_count, metrics_count, self.tokens_to_track.len(), tracking_filtered_count
                    );
                } else {
                    debug!(
                        "Processed {} token events from {} available tokens (auto-discovery, max: {})",
                        processed_event_count, metrics_count, self.max_tokens_to_scan
                    );
                }
            }
            Err(e) => {
                error!("Error fetching market data: {}", e);
                let error_event = Event::Market(MarketEvent::MarketDataError(e.to_string()));
                if let Err(pub_err) = self.event_router.publish(error_event).await {
                    error!("Failed to publish market data error event: {}", pub_err);
                }
            }
        }

        // Update scan duration metrics
        if let Ok(mut duration) = self.last_scan_duration.lock() {
            *duration = Some(start_time.elapsed());
        } else {
            error!("Failed to update scan duration: mutex poisoned");
        }
    }

    /// Log token discovery configuration
    fn log_token_discovery_info(&self, metrics_count: usize, network: &str) {
        if self.tokens_to_track.is_empty() {
            if self.max_tokens_to_scan == 0 {
                debug!(
                    "Auto-discovery mode: Processing ALL {} available tokens on {}",
                    metrics_count, network
                );
            } else {
                debug!(
                    "Auto-discovery mode: Processing up to {} tokens from {} available on {} (limit: {})",
                    metrics_count.min(self.max_tokens_to_scan), metrics_count, network, self.max_tokens_to_scan
                );
            }
        } else {
            debug!(
                "Tracking {} specific tokens: {:?}",
                self.tokens_to_track.len(),
                self.tokens_to_track
            );
        }
    }
}
