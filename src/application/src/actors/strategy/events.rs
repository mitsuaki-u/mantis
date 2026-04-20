use super::StrategyActor;
use crate::application::errors::Error;
use crate::core::constants::{ENABLE_PRICE_CROSS_CHECK, MAX_PRICE_DISCREPANCY_THRESHOLD};
use crate::core::domain::market::TokenMetrics;
use crate::core::domain::trading::Signal;
use crate::core::strategies::traits::TradingStrategy;
use crate::core::utils::f64_to_decimal;
use crate::core::utils::validation::price::validate_price_discrepancy;
use crate::events::{Event, MarketEvent, StrategyEvent};
use chrono::{DateTime, Utc};
use log::{debug, error, info, trace, warn};
use std::convert::TryFrom;

/// Create fallback metrics when token data is unavailable
fn create_fallback_metrics(
    token_id: &str,
    price: f64,
    volume: f64,
    timestamp: DateTime<Utc>,
) -> TokenMetrics {
    TokenMetrics {
        id: token_id.to_string(),
        symbol: String::new(),
        name: String::new(),
        decimals: 18,
        price_usd: price,
        volume_24h: volume,
        price_change_24h: 0.0,
        chain: None,
        last_updated: timestamp,
    }
}

/// Convert database position to strategy position
fn convert_to_strategy_position(
    position: &crate::core::domain::trading::Position,
    current_price: f64,
) -> crate::core::domain::trading::Position {
    crate::core::domain::trading::Position {
        token_id: position.token_id.clone(),
        provider_id: position.provider_id.clone(),
        entry_price: position.entry_price,
        current_price,
        highest_price: position.highest_price,
        size: position.size,
        unrealized_pnl: (current_price - position.entry_price) * position.size,
        entry_time: position.entry_time,
    }
}

impl StrategyActor {
    /// Publish a trading signal event
    async fn publish_signal(
        &mut self,
        signal: Signal,
        token_id: &str,
        token_metrics: &TokenMetrics,
        timestamp: DateTime<Utc>,
    ) -> Result<(), Error> {
        info!(
            "🎯 Generated {} signal for {} (volume: ${:.2}M, price: ${:.4})",
            signal,
            token_id,
            token_metrics.volume_24h / 1_000_000.0,
            token_metrics.price_usd
        );

        let strategy_event = Event::Strategy(StrategyEvent::Signal {
            token_id: token_id.to_string(),
            signal,
            timestamp,
            metadata: crate::events::SignalMetadata::new(
                token_metrics.price_usd,
                token_metrics.volume_24h,
                self.strategy.name().to_string(),
                format!(
                    "price=${:.8}, volume=${:.2}M",
                    token_metrics.price_usd,
                    token_metrics.volume_24h / 1_000_000.0
                ),
            ),
        });

        if let Err(e) = self.event_router.publish(strategy_event).await {
            error!("Failed to publish strategy signal for {}: {}", token_id, e);
            self.state.record_error();
            return Err(e);
        }

        Ok(())
    }

    /// Validate price against on-chain data before generating BUY signal
    /// Returns the validated price (potentially updated from on-chain) or error if validation fails
    async fn validate_price_before_signal(
        &self,
        token_id: &str,
        token_metrics: &mut TokenMetrics,
    ) -> Result<f64, Error> {
        // Get on-chain price for the token
        let onchain_price_result = self.dex_client.get_token_price_usd(token_id).await;

        let onchain_price = match onchain_price_result {
            Ok(price) => price,
            Err(e) => {
                warn!(
                    "Failed to get on-chain price for {} during validation: {}. Using subgraph price.",
                    token_id, e
                );
                // If we can't get on-chain price, use subgraph price but log warning
                return Ok(token_metrics.price_usd);
            }
        };

        let subgraph_price = token_metrics.price_usd;

        // Convert prices to Decimal for validation
        let onchain_decimal = f64_to_decimal(onchain_price, "onchain_price")
            .map_err(|e| Error::Internal(format!("Price conversion failed: {}", e)))?;
        let subgraph_decimal = f64_to_decimal(subgraph_price, "subgraph_price")
            .map_err(|e| Error::Internal(format!("Price conversion failed: {}", e)))?;

        // Validate price discrepancy
        let validation_result = validate_price_discrepancy(
            subgraph_decimal,
            onchain_decimal,
            MAX_PRICE_DISCREPANCY_THRESHOLD,
            token_id,
            "strategy", // correlation_id
        );

        if !validation_result.is_valid {
            let discrepancy_pct = validation_result.discrepancy_percentage * 100.0;

            if ENABLE_PRICE_CROSS_CHECK {
                error!(
                    "🚫 Rejecting BUY signal for {} due to price discrepancy: {:.2}% (threshold: {:.2}%)",
                    token_id,
                    discrepancy_pct,
                    MAX_PRICE_DISCREPANCY_THRESHOLD * 100.0
                );
                error!(
                    "   Subgraph price: ${:.8}, On-chain price: ${:.8}",
                    subgraph_price, onchain_price
                );
                return Err(Error::Trading(format!(
                    "Price discrepancy {:.2}% exceeds threshold {:.2}%",
                    discrepancy_pct,
                    MAX_PRICE_DISCREPANCY_THRESHOLD * 100.0
                )));
            } else {
                warn!(
                    "⚠️  Price discrepancy for {}: {:.2}% (threshold: {:.2}%) - cross-check disabled, continuing with on-chain price",
                    token_id,
                    discrepancy_pct,
                    MAX_PRICE_DISCREPANCY_THRESHOLD * 100.0
                );
            }
        } else {
            debug!(
                "✅ Price validation passed for {}: Subgraph=${:.8}, On-chain=${:.8}, Discrepancy={:.2}%",
                token_id,
                subgraph_price,
                onchain_price,
                validation_result.discrepancy_percentage * 100.0
            );
        }

        // Update token_metrics with on-chain price (more accurate/recent)
        token_metrics.price_usd = onchain_price;
        token_metrics.last_updated = Utc::now();

        Ok(onchain_price)
    }

    /// Handle market events and generate strategy signals
    pub async fn handle_market_event(&mut self, market_event: MarketEvent) -> Result<(), Error> {
        trace!("StrategyActor received Market event: {:?}", market_event);

        match market_event {
            MarketEvent::PriceUpdate {
                token_id,
                price,
                volume,
                timestamp,
                ..
            } => {
                trace!("Processing price update for {}: ${:.4}", token_id, price);

                let volume_24h = volume.ok_or_else(|| {
                    Error::Internal(format!("Missing volume data for token {}", token_id))
                })?;

                // Try to get token data from database, fall back to minimal metrics
                let mut token_metrics = match self.token_repo.get_token_price_stats(&token_id).await
                {
                    Ok(token_data) => {
                        match TokenMetrics::try_from(&token_data) {
                            Ok(mut metrics) => {
                                // Update with latest real-time data from price update event
                                metrics.price_usd = price;
                                metrics.volume_24h = volume_24h;
                                metrics.last_updated = timestamp;
                                metrics
                            }
                            Err(e) => {
                                trace!("Failed to convert token data for {}: {}", token_id, e);
                                create_fallback_metrics(&token_id, price, volume_24h, timestamp)
                            }
                        }
                    }
                    Err(_) => {
                        // Token not in database - use minimal metrics
                        create_fallback_metrics(&token_id, price, volume_24h, timestamp)
                    }
                };

                // Update the strategy's in-memory price time series (stored in core layer)
                // Each strategy maintains a circular buffer of historical prices per token
                // Used for technical indicators: RSI (14 periods), MACD (26-50), Bollinger (20), etc.
                self.strategy.update_market_data(&token_metrics);

                // Fetch position once - used for both existence check and signal generation
                let position_result = self.position_repo.get_position_by_token_id(&token_id).await;

                let signal = match position_result {
                    Ok(Some((_position_id, position))) => {
                        // Has position - check for exit signal
                        let strategy_position =
                            convert_to_strategy_position(&position, token_metrics.price_usd);

                        // Build risk params for exit analysis: (take_profit, stop_loss)
                        let risk_params = Some((self.take_profit, self.stop_loss));

                        if let Some(exit_reason) = self.strategy.analyze_for_exit(
                            &token_metrics,
                            Some(&strategy_position),
                            risk_params,
                        ) {
                            info!(
                                "🚨 Exit condition triggered for {}: {} | Current=${:.4}, Entry=${:.4}, P&L={:.2}%",
                                token_id,
                                exit_reason.reason,
                                token_metrics.price_usd,
                                position.entry_price,
                                ((token_metrics.price_usd - position.entry_price) / position.entry_price) * 100.0
                            );
                            Signal::Sell
                        } else {
                            Signal::Hold
                        }
                    }
                    Ok(None) => {
                        // No position - check for entry signal
                        if crate::infrastructure::dex::ethereum::tokens::is_stablecoin(
                            &token_metrics.symbol,
                        ) {
                            trace!(
                                "Skipping stablecoin {} for buy signal analysis",
                                token_metrics.symbol
                            );
                            Signal::NoAction
                        } else if self.strategy.analyze_for_entry(&token_metrics) {
                            // Validate price with on-chain data before generating BUY signal
                            match self
                                .validate_price_before_signal(&token_id, &mut token_metrics)
                                .await
                            {
                                Ok(validated_price) => {
                                    debug!(
                                        "✅ BUY signal generated for {} (volume: ${:.2}M, price: ${:.4})",
                                        token_id,
                                        token_metrics.volume_24h / 1_000_000.0,
                                        validated_price
                                    );
                                    Signal::Buy
                                }
                                Err(e) => {
                                    warn!(
                                        "Price validation failed for {}: {}. Skipping signal.",
                                        token_id, e
                                    );
                                    Signal::NoAction
                                }
                            }
                        } else {
                            Signal::NoAction
                        }
                    }
                    Err(e) => {
                        error!("Failed to get position for {}: {}", token_id, e);
                        Signal::NoAction
                    }
                };

                // Publish actionable signals (BUY/SELL)
                if signal.is_buy() || signal.is_sell() {
                    let _ = self
                        .publish_signal(signal, &token_id, &token_metrics, timestamp)
                        .await;
                }
            }
            _ => {
                trace!(
                    "StrategyActor ignoring non-price market event: {:?}",
                    market_event
                );
            }
        }

        Ok(())
    }
}
