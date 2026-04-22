use super::{limits, operations, RiskManagerActor};
use crate::application::errors::Result;
use crate::core::domain::token::TokenData;
use crate::core::domain::trading::Signal;
use crate::events::{AIAdvisorEvent, Event, ExecutionEvent, MarketEvent, RiskEvent, StrategyEvent};
use chrono::Utc;
use log::{debug, error, info, warn};

/// Handle all types of events for the RiskManagerActor
pub async fn handle_event_internal(actor: &mut RiskManagerActor, event: Event) -> Result<()> {
    match event {
        Event::Market(market_event) => handle_market_event(actor, market_event).await,
        Event::Strategy(strategy_event) => handle_strategy_event(actor, strategy_event).await,
        Event::Risk(risk_event) => handle_risk_event(actor, risk_event).await,
        Event::Execution(execution_event) => handle_execution_event(actor, execution_event).await,
        Event::AIAdvisor(ai_event) => handle_ai_advisor_event(actor, ai_event).await,
        Event::DexTransaction(_) => Ok(()), // ExecutionActor handles transaction lifecycle
    }
}

// ============================================================================
// Market Events
// ============================================================================

async fn handle_market_event(actor: &mut RiskManagerActor, event: MarketEvent) -> Result<()> {
    match event {
        MarketEvent::PriceUpdate {
            token_id, price, ..
        } => {
            let normalized_id = TokenData::normalize_token_id(&token_id);
            let pnl_delta = actor.update_position_price(&normalized_id, price);

            if let Some(pnl_change) = pnl_delta {
                actor.risk_metrics.current_daily_loss -= pnl_change;
            }
        }
        MarketEvent::MarketAnomalyDetected {
            token_id,
            anomaly_type,
            description,
            severity,
            ..
        } => {
            handle_market_anomaly(actor, token_id, anomaly_type, description, severity).await?;
        }
        MarketEvent::MarketDataError(error_msg) => {
            error!("RiskManager: MarketDataError: {}", error_msg);
        }
        // Ignore events not relevant to risk management
        _ => {}
    }
    Ok(())
}

/// Handle market anomaly detection and update risk scores
async fn handle_market_anomaly(
    actor: &mut RiskManagerActor,
    token_id: String,
    anomaly_type: String,
    description: String,
    severity: String,
) -> Result<()> {
    // Log with appropriate level
    let log_level = match severity.to_lowercase().as_str() {
        "high" => log::Level::Error,
        "medium" => log::Level::Warn,
        _ => log::Level::Info,
    };

    log::log!(
        log_level,
        "Market anomaly detected - Token: {}, Type: {}, Severity: {}, Description: '{}'",
        token_id,
        anomaly_type,
        severity,
        description
    );

    // Halt trading for high severity anomalies
    if severity.to_lowercase() == "high" {
        let normalized_id = TokenData::normalize_token_id(&token_id);
        if actor.halted_tokens.insert(normalized_id.clone()) {
            error!(
                "TRADING HALTED for {} due to high severity anomaly: {} - {}",
                normalized_id, anomaly_type, description
            );

            let halt_event = Event::Risk(RiskEvent::TradingHalted {
                token_id: normalized_id,
                reason: format!(
                    "High severity market anomaly: {} - {}",
                    anomaly_type, description
                ),
                timestamp: Utc::now(),
            });

            if let Err(e) = actor.event_router.publish(halt_event).await {
                error!(
                    "Failed to publish TradingHalted event for {}: {}",
                    token_id, e
                );
            }
        }
    }

    Ok(())
}

// ============================================================================
// AI Advisor Events
// ============================================================================

async fn handle_ai_advisor_event(actor: &mut RiskManagerActor, event: AIAdvisorEvent) -> Result<()> {
    match event {
        AIAdvisorEvent::SignalAnalysed { token_id, signal, approved, confidence, reasoning, metadata } => {
            if approved {
                debug!(
                    "RiskManager: received AI-approved signal for {} ({}% confidence) — {}",
                    &token_id[..token_id.len().min(10)], confidence, reasoning
                );
                // Route the approved signal through the same path as a normal strategy signal
                handle_strategy_event(actor, StrategyEvent::Signal {
                    token_id,
                    signal,
                    timestamp: Utc::now(),
                    metadata,
                }).await
            } else {
                debug!(
                    "RiskManager: AI-rejected signal for {} skipped — {}",
                    &token_id[..token_id.len().min(10)], reasoning
                );
                Ok(())
            }
        }
    }
}

// Strategy Events
// ============================================================================

async fn handle_strategy_event(actor: &mut RiskManagerActor, event: StrategyEvent) -> Result<()> {
    match event {
        StrategyEvent::Signal {
            token_id,
            signal,
            timestamp,
            metadata,
        } => {
            let normalized_id = TokenData::normalize_token_id(&token_id);
            info!(
                "[{}] Received strategy signal for {}: {:?} at {}",
                &metadata.correlation_id[..8],
                normalized_id,
                signal,
                timestamp
            );
            handle_strategy_signal(actor, normalized_id, signal, metadata).await?;
        }
    }
    Ok(())
}

/// Process strategy signal through risk assessment
async fn handle_strategy_signal(
    actor: &mut RiskManagerActor,
    token_id: String,
    signal: Signal,
    signal_metadata: crate::events::SignalMetadata,
) -> Result<()> {
    if !actor.state.running {
        warn!(
            "RiskManagerActor received signal for {} while not running - signal ignored. Call start() on actor first.",
            token_id
        );
        return Ok(());
    }

    // Check if trading is halted
    if actor.halted_tokens.contains(&token_id) {
        warn!(
            "Signal {:?} for {} ignored - trading halted",
            signal, token_id
        );
        return Ok(());
    }

    info!(
        "[{}] 🔍 Processing {} signal for {} at ${:.8} (Strategy: {})",
        &signal_metadata.correlation_id[..8],
        signal,
        token_id,
        signal_metadata.signal_price,
        signal_metadata.strategy_name
    );

    // Validate token exists in database
    let token_exists = match actor.token_repo.token_exists(&token_id).await {
        Ok(exists) => exists,
        Err(e) => {
            error!("Failed to check if token {} exists: {}", token_id, e);
            return Ok(());
        }
    };

    if !token_exists {
        error!(
            "Token {} not found in database - discarding signal",
            token_id
        );
        return Ok(());
    }

    // Get token metrics for risk assessment
    let token_metrics = match actor.token_repo.get_token_metrics(&token_id).await {
        Ok(Some(metrics)) => metrics,
        Ok(None) => {
            error!("No token metrics found for {}", token_id);
            return Ok(());
        }
        Err(e) => {
            error!("Failed to get token metrics for {}: {}", token_id, e);
            return Ok(());
        }
    };

    // Check token volatility
    if !operations::check_token_risk(actor, &token_id).await? {
        warn!(
            "Token {} failed volatility check - signal rejected",
            token_id
        );
        return Ok(());
    }

    if !limits::check_trading_allowed(actor, &token_id, &signal, &signal_metadata).await? {
        warn!(
            "[{}] Trading not allowed for {} based on risk limits",
            &signal_metadata.correlation_id[..8],
            token_id
        );

        // Release reservation if BUY signal was rejected
        if signal.is_buy() {
            if let Err(e) = actor
                .position_repo
                .release_reservation(&signal_metadata.correlation_id)
                .await
            {
                warn!(
                    "[{}] Failed to release reservation after rejection: {}",
                    &signal_metadata.correlation_id[..8],
                    e
                );
            }
        }
        return Ok(());
    }

    // For buy signals, check for existing positions
    if signal.is_buy() {
        match actor.position_repo.position_exists(&token_id).await {
            Ok(true) => {
                info!(
                    "[{}] Already have position for {} - ignoring buy signal",
                    &signal_metadata.correlation_id[..8],
                    token_id
                );

                // Release reservation since we're rejecting this buy signal
                if let Err(e) = actor
                    .position_repo
                    .release_reservation(&signal_metadata.correlation_id)
                    .await
                {
                    warn!(
                        "[{}] Failed to release reservation after duplicate position check: {}",
                        &signal_metadata.correlation_id[..8],
                        e
                    );
                }
                return Ok(());
            }
            Ok(false) => {}
            Err(e) => {
                error!(
                    "[{}] Failed to check position for {}: {}",
                    &signal_metadata.correlation_id[..8],
                    token_id,
                    e
                );

                // Release reservation since we can't proceed
                if let Err(e) = actor
                    .position_repo
                    .release_reservation(&signal_metadata.correlation_id)
                    .await
                {
                    warn!(
                        "[{}] Failed to release reservation after position check error: {}",
                        &signal_metadata.correlation_id[..8],
                        e
                    );
                }
                return Ok(());
            }
        }
    }

    // Calculate position size (using simplified fixed sizing)
    let final_position_size =
        super::operations::calculate_position_size(actor, &token_id, &signal, &token_metrics)
            .await?;

    // Apply position size constraints
    let capped_size = super::operations::apply_position_size_constraints(
        actor,
        &token_id,
        final_position_size,
        &signal,
    )?;

    info!(
        "[{}] 🎯 Approved {} signal for {} with position size ${:.2}",
        &signal_metadata.correlation_id[..8],
        signal,
        token_id,
        capped_size
    );

    // Capture correlation ID before moving signal_metadata
    let correlation_id = signal_metadata.correlation_id.clone();

    // Publish trade approval to execution actor
    let trade_approved_event = Event::Risk(RiskEvent::TradeApproved {
        token_id: token_id.clone(),
        signal,
        position_size: capped_size,
        timestamp: Utc::now(),
        signal_metadata,
    });

    if let Err(e) = actor.event_router.publish(trade_approved_event).await {
        error!(
            "Failed to publish TradeApproved event for {}: {}",
            token_id, e
        );
        actor.state.record_error();
    } else {
        info!(
            "[{}] ✅ Published TradeApproved for {} to ExecutionActor",
            &correlation_id[..8],
            token_id
        );
    }

    Ok(())
}

// ============================================================================
// Risk Events
// ============================================================================

async fn handle_risk_event(actor: &mut RiskManagerActor, event: RiskEvent) -> Result<()> {
    if let RiskEvent::PositionClosed { token_id, pnl, .. } = event {
        let normalized_id = TokenData::normalize_token_id(&token_id);
        info!("Position closed for {}, P&L: ${:.2}", normalized_id, pnl);

        operations::update_risk_metrics(actor, pnl).await?;
        actor.remove_position(&normalized_id);
        limits::check_overall_risk_limits(actor).await?;
    }
    // Ignore other risk events (handled elsewhere)
    Ok(())
}

// ============================================================================
// Execution Events
// ============================================================================

async fn handle_execution_event(actor: &mut RiskManagerActor, event: ExecutionEvent) -> Result<()> {
    match event {
        ExecutionEvent::OrderExecuted {
            token_id,
            signal,
            token_quantity,
            price_per_token,
            entry_price,
            timestamp,
            ..
        } => {
            let normalized_id = TokenData::normalize_token_id(&token_id);

            match signal {
                Signal::Buy => {
                    info!(
                        "Order executed (BUY) for {}: {} @ ${:.6} at {}",
                        normalized_id, token_quantity, price_per_token, timestamp
                    );

                    let before = actor.get_all_positions().len();
                    actor.add_position(&normalized_id, token_quantity, price_per_token);
                    let after = actor.get_all_positions().len();

                    info!(
                        "🔢 Position count: {} → {} (added {})",
                        before, after, normalized_id
                    );
                }
                Signal::Sell => {
                    let entry_price_val = entry_price.unwrap_or(price_per_token);
                    let pnl = (price_per_token - entry_price_val) * token_quantity;

                    info!(
                        "Order executed (SELL) for {}: P&L ${:.2}, Entry: ${:.4}, Exit: ${:.4}, Size: {}",
                        normalized_id, pnl, entry_price_val, price_per_token, token_quantity
                    );

                    operations::update_risk_metrics(actor, pnl).await?;
                    actor.remove_position(&normalized_id);
                    limits::check_overall_risk_limits(actor).await?;
                }
                Signal::Hold | Signal::NoAction => {
                    warn!(
                        "Unexpected OrderExecuted with signal {:?} for {}",
                        signal, token_id
                    );
                }
            }
        }
        ExecutionEvent::OrderFailed {
            token_id,
            order_id,
            reason,
            timestamp,
            signal,
            correlation_id,
        } => {
            let corr_id_str = correlation_id.as_deref().unwrap_or("unknown");
            error!(
                "[{}] Order failed - Token: {}, Signal: {:?}, OrderID: {:?}, Reason: '{}', Time: {}",
                &corr_id_str[..8.min(corr_id_str.len())], token_id, signal, order_id, reason, timestamp
            );

            // Release position slot reservation if this was a BUY signal
            if signal.is_buy() {
                if let Some(ref corr_id) = correlation_id {
                    if let Err(e) = actor.position_repo.release_reservation(corr_id).await {
                        warn!(
                            "[{}] Failed to release position reservation after order failure: {}",
                            &corr_id[..8],
                            e
                        );
                    } else {
                        debug!(
                            "[{}] Released position slot reservation after order failure",
                            &corr_id[..8]
                        );
                    }
                }
            }

            // Note order failure for this token
            warn!("Order execution failed for {}", token_id);
        }
    }
    Ok(())
}
