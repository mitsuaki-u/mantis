use super::{limits, metrics, RiskManagerActor};
use crate::core::error::Result;
use crate::core::models::token::TokenData;
use crate::domain::dex::TransactionStatus;
use crate::domain::trading::strategy::Signal;
use crate::infra::actors::{
    DexTransactionEvent, Event, ExecutionEvent, MarketEvent, RiskEvent, StrategyEvent,
};
use chrono::Utc;
use log::{debug, error, info, trace, warn};

/// Handle all types of events for the RiskManagerActor
pub async fn handle_event_internal(actor: &mut RiskManagerActor, event: Event) -> Result<()> {
    actor.touch_last_activity().await;

    match event {
        Event::Market(market_event) => handle_market_event(actor, market_event).await,
        Event::Strategy(strategy_event) => handle_strategy_event(actor, strategy_event).await,
        Event::Risk(risk_event) => handle_risk_event(actor, risk_event).await,
        Event::Execution(execution_event) => handle_execution_event(actor, execution_event).await,
        Event::Database(db_event) => handle_database_event(actor, db_event).await,
        Event::DexTransaction(dex_event) => handle_dex_transaction_event(actor, dex_event).await,
    }
}

/// Handle market events
async fn handle_market_event(actor: &mut RiskManagerActor, event: MarketEvent) -> Result<()> {
    match event {
        MarketEvent::PriceUpdate {
            token_id,
            price,
            volume: _,
            timestamp: _,
        } => {
            let normalized_id = TokenData::normalize_token_id(&token_id);
            let pnl_change_for_update =
                actor.risk_manager.update_market_data(&normalized_id, price);

            if let Some(pnl_delta) = pnl_change_for_update {
                actor.current_daily_loss -= pnl_delta;
            }
            limits::check_risk_limits(actor, &normalized_id).await?;
        }
        MarketEvent::NewTokenDiscovered {
            token_id,
            name,
            symbol,
            price,
            source,
            timestamp,
        } => {
            info!(
                "RiskManagerActor: NewTokenDiscovered: {} ({}, {}) Price: {:.4} from {} at {} - Processing for risk assessment",
                name, token_id, symbol, price, source, timestamp
            );
            super::assessment::assess_token_risk(actor, &token_id).await?;
        }
        MarketEvent::MarketAnomalyDetected {
            token_id,
            anomaly_type,
            description,
            severity,
            timestamp,
        } => {
            handle_market_anomaly(
                actor,
                token_id,
                anomaly_type,
                description,
                severity,
                timestamp,
            )
            .await?;
        }
        MarketEvent::MarketDataError(error_message_string) => {
            error!("RiskManager: MarketDataError: {}", error_message_string);
        }
        MarketEvent::SupervisorRecoveryRequest(message) => {
            info!(
                "RiskManager: SupervisorRecoveryRequest received: {}",
                message
            );
        }
        MarketEvent::VolumeUpdate {
            token_id,
            volume,
            timestamp,
        } => {
            trace!(
                "RiskManager: MarketEvent::VolumeUpdate for {} - Volume: {}, Time: {}",
                token_id,
                volume,
                timestamp
            );
            // Potentially update token risk metrics or trigger checks based on volume
        }
        MarketEvent::StatusCheck => {}
    }
    Ok(())
}

/// Handle market anomaly detection
async fn handle_market_anomaly(
    actor: &mut RiskManagerActor,
    token_id: String,
    anomaly_type: String,
    description: String,
    severity: String,
    timestamp: chrono::DateTime<Utc>,
) -> Result<()> {
    let log_level = match severity.to_lowercase().as_str() {
        "high" => log::Level::Error,
        "medium" => log::Level::Warn,
        _ => log::Level::Info,
    };

    log::log!(
        log_level,
        "RiskManager: MarketAnomalyDetected - Token: {}, Type: {}, Severity: {}, Desc: '{}', Time: {}",
        token_id, anomaly_type, severity, description, timestamp
    );

    let current_score = actor.risk_scores.get(&token_id).copied().unwrap_or(0.5);
    let new_score_increase = match severity.to_lowercase().as_str() {
        "high" => 0.3,
        "medium" => 0.15,
        _ => 0.05,
    };
    let new_score = (current_score + new_score_increase).min(1.0);
    actor.risk_scores.insert(token_id.clone(), new_score);

    warn!(
        "RiskManager: Updated risk score for {} from {} to {} due to {} anomaly.",
        token_id, current_score, new_score, severity
    );

    if severity.to_lowercase() == "high" {
        let normalized_token_id_for_halt = TokenData::normalize_token_id(&token_id);
        if actor
            .halted_tokens
            .insert(normalized_token_id_for_halt.clone())
        {
            error!(
                "RiskManager: TRADING HALTED for token {} (norm: {}) due to High severity market anomaly: {}. Desc: {}. Risk score to {}.",
                token_id, normalized_token_id_for_halt, anomaly_type, description, new_score
            );

            let halt_event = Event::Risk(RiskEvent::TradingHalted {
                token_id: normalized_token_id_for_halt,
                reason: format!(
                    "High severity market anomaly: {} - {}",
                    anomaly_type, description
                ),
                timestamp: Utc::now(),
            });

            if let Err(e) = actor.message_bus.publish(halt_event).await {
                error!(
                    "RiskManager: Failed to publish TradingHalted event for {}: {}",
                    token_id, e
                );
            }
        } else {
            info!(
                "RiskManager: Trading for token {} (norm: {}) was already halted.",
                token_id, normalized_token_id_for_halt
            );
        }
    }

    Ok(())
}

/// Handle strategy events
async fn handle_strategy_event(actor: &mut RiskManagerActor, event: StrategyEvent) -> Result<()> {
    match event {
        StrategyEvent::Signal {
            token_id,
            signal,
            confidence,
            timestamp,
        } => {
            let normalized_id = TokenData::normalize_token_id(&token_id);
            info!(
                "RiskManager: StrategyEvent::Signal for {} (norm: {}): {:?} Conf: {} at {}.",
                token_id, normalized_id, signal, confidence, timestamp
            );
            handle_strategy_signal(actor, normalized_id, signal, confidence).await?;
        }
        StrategyEvent::StatusCheck => {}
    }
    Ok(())
}

/// Handle strategy signal processing
async fn handle_strategy_signal(
    actor: &mut RiskManagerActor,
    token_id: String,
    signal: Signal,
    confidence: f64,
) -> Result<()> {
    if !actor.running {
        return Ok(());
    }

    // Normalize token_id to lowercase to match database storage format
    let normalized_id = TokenData::normalize_token_id(&token_id);

    // Check if trading is halted for this token
    if actor.halted_tokens.contains(&normalized_id) {
        warn!(
            "RiskManager: Signal {:?} for token {} (normalized: {}) ignored because trading is halted for this token.",
            signal, token_id, normalized_id
        );
        return Ok(());
    }

    // Log original and normalized token ID
    info!(
        "🔍 Processing strategy signal for token {} (normalized to {}), Signal: {:?}, Confidence: {:.2}",
        token_id, normalized_id, signal, confidence
    );

    // Check if the token already has price data / exists
    let token_actually_exists = match actor.token_repo.token_exists(&normalized_id).await {
        Ok(exists) => exists,
        Err(e) => {
            error!(
                "Failed to check if token '{}' (normalized from '{}') exists: {}. Discarding signal.",
                normalized_id, token_id, e
            );
            return Ok(());
        }
    };

    if !token_actually_exists {
        error!(
            "Token '{}' (normalized from '{}') does not exist in the database. Discarding {:?} signal.",
            normalized_id, token_id, signal
        );

        let bad_signal_event = Event::Risk(RiskEvent::InvalidSignalReceived {
            token_id: token_id.clone(),
            reason: format!(
                "Token '{}' (normalized to '{}') not found in database.",
                token_id, normalized_id
            ),
            timestamp: Utc::now(),
        });

        if let Err(e) = actor.message_bus.publish(bad_signal_event).await {
            warn!("Failed to publish InvalidSignalReceived event: {}", e);
        }
        return Ok(());
    }

    // Continue with signal processing...
    // This would include the rest of the original handle_strategy_signal logic
    // For now, we'll just log that we're processing it
    info!("Processing valid signal for token: {}", normalized_id);

    Ok(())
}

/// Handle risk events
async fn handle_risk_event(actor: &mut RiskManagerActor, event: RiskEvent) -> Result<()> {
    match event {
        RiskEvent::PositionOpened {
            token_id,
            position_id,
            amount,
            price,
            timestamp,
        } => {
            let normalized_id = TokenData::normalize_token_id(&token_id);
            info!(
                "RiskManager: RiskEvent::PositionOpened for {} (norm: {}), ID: {}, Amount: {}, Price: {}, Time: {}",
                token_id, normalized_id, position_id, amount, price, timestamp
            );
            actor
                .risk_manager
                .add_position(&normalized_id, amount, price);
            limits::check_risk_limits(actor, &normalized_id).await?;
        }
        RiskEvent::PositionClosed {
            token_id,
            pnl,
            timestamp,
            entry_price,
            exit_price,
            size,
            entry_time,
            delete_position,
        } => {
            let normalized_id = TokenData::normalize_token_id(&token_id);
            info!(
                "RiskManager: RiskEvent::PositionClosed for {} (norm: {}), PnL: {}, Entry: {:.4}@{:.4}, Exit: {:.4}@{:.4}, Size: {}, Del: {}",
                token_id, normalized_id, pnl, entry_price, entry_time, exit_price, timestamp, size, delete_position
            );
            metrics::update_risk_metrics(actor, pnl).await?;
            actor.risk_manager.remove_position(&normalized_id);
            limits::check_overall_risk_limits(actor).await?;
        }
        _ => trace!("RiskManager: Unhandled RiskEvent: {:?}", event),
    }
    Ok(())
}

/// Handle execution events
async fn handle_execution_event(actor: &mut RiskManagerActor, event: ExecutionEvent) -> Result<()> {
    match event {
        ExecutionEvent::OrderExecuted { .. } => {
            trace!(
                "RiskManager: ExecutionEvent::OrderExecuted received, details: {:?}",
                event
            );
        }
        ExecutionEvent::PositionUpdate { .. } => {
            trace!("RiskManager: ExecutionEvent::PositionUpdate received, details: {:?}. Potentially update internal position state.", event);
        }
        ExecutionEvent::OrderFailed {
            token_id,
            order_id,
            reason,
            timestamp,
            ..
        } => {
            error!(
                "RiskManager: ExecutionEvent::OrderFailed - Token: {}, OrderID: {:?}, Reason: '{}', Timestamp: {}",
                token_id, order_id, reason, timestamp
            );

            // Increase risk score for the token
            let current_score = actor.risk_scores.get(&token_id).copied().unwrap_or(0.5);
            let new_score = (current_score + 0.2).min(1.0);
            actor.risk_scores.insert(token_id.clone(), new_score);

            warn!(
                "RiskManager: Updated risk score for token {} from {} to {} due to order failure.",
                token_id, current_score, new_score
            );
        }
        ExecutionEvent::StatusCheck => {
            trace!("RiskManager: Execution StatusCheck ignored.");
        }
    }
    Ok(())
}

/// Handle database events
async fn handle_database_event(
    _actor: &mut RiskManagerActor,
    db_event: crate::infra::actors::DatabaseEvent,
) -> Result<()> {
    trace!("RiskManager: Received DatabaseEvent: {:?}", db_event);
    Ok(())
}

/// Handle DEX transaction events
async fn handle_dex_transaction_event(
    actor: &mut RiskManagerActor,
    event: DexTransactionEvent,
) -> Result<()> {
    match event {
        DexTransactionEvent::Submitted {
            tx_id,
            submission_time,
            priority,
            ..
        } => {
            trace!(
                "RiskManager: DexTransaction Submitted - TxID: {}, SubmissionTime: {}, Priority: {:?}",
                tx_id, submission_time, priority
            );
        }
        DexTransactionEvent::StatusUpdated { status } => {
            handle_transaction_status_update(actor, status).await?;
        }
    }
    Ok(())
}

/// Handle transaction status updates
async fn handle_transaction_status_update(
    actor: &mut RiskManagerActor,
    status: TransactionStatus,
) -> Result<()> {
    let involved_tokens_opt: Option<(Option<String>, Option<String>)> = match &status {
        TransactionStatus::Success { details, .. } => Some((
            Some(details.token_in_address.clone()),
            Some(details.token_out_address.clone()),
        )),
        _ => None,
    };

    match status {
        TransactionStatus::Failed {
            tx_id,
            reason,
            error_code,
            gas_used,
            revert_reason,
            recovery_suggestion,
        } => {
            error!(
                "RiskManager: DexTransaction Failed - TxID: {}, Reason: '{}', ErrorCode: {:?}, GasUsed: {:?}, RevertReason: {:?}, Suggestion: {:?}",
                tx_id, reason, error_code, gas_used, revert_reason, recovery_suggestion
            );

            if let Some((Some(token_in), Some(token_out))) = involved_tokens_opt {
                for token_id in [token_in, token_out].iter().filter_map(|t| {
                    t.strip_prefix("0x")
                        .map(String::from)
                        .or_else(|| Some(t.clone()))
                }) {
                    let current_score = actor.risk_scores.get(&token_id).copied().unwrap_or(0.5);
                    let new_score = (current_score + 0.25).min(1.0);
                    actor.risk_scores.insert(token_id.clone(), new_score);
                    warn!(
                        "RiskManager: Updated risk score for token {} (from failed tx {}) to {}",
                        token_id, tx_id, new_score
                    );
                }
            }
        }
        TransactionStatus::Dropped { tx_id, reason, .. } => {
            error!(
                "RiskManager: DexTransaction Dropped - TxID: {}, Reason: '{}'",
                tx_id, reason
            );

            if let Some((Some(token_in), Some(token_out))) = involved_tokens_opt {
                for token_id in [token_in, token_out].iter().filter_map(|t| {
                    t.strip_prefix("0x")
                        .map(String::from)
                        .or_else(|| Some(t.clone()))
                }) {
                    let current_score = actor.risk_scores.get(&token_id).copied().unwrap_or(0.5);
                    let new_score = (current_score + 0.25).min(1.0);
                    actor.risk_scores.insert(token_id.clone(), new_score);
                    warn!(
                        "RiskManager: Updated risk score for token {} (from dropped tx {}) to {}",
                        token_id, tx_id, new_score
                    );
                }
            }
        }
        TransactionStatus::Success { details, .. } => {
            info!(
                "RiskManager: DexTransaction Success - TxID: {}",
                details.tx_id
            );
        }
        TransactionStatus::Confirmed {
            details,
            confirmations,
            ..
        } => {
            info!(
                "RiskManager: DexTransaction Confirmed - TxID: {}, Confirmations: {}",
                details.tx_id, confirmations
            );
        }
        TransactionStatus::Pending { tx_id, .. } => {
            debug!("RiskManager: DexTransaction Pending - TxID: {}", tx_id);
        }
        TransactionStatus::Queued { tx_id, .. } => {
            debug!("RiskManager: DexTransaction Queued - TxID: {}", tx_id);
        }
    }

    Ok(())
}
