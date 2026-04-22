use crate::application::errors::Error;
use crate::config::Config;
use crate::events::{DexTransactionEvent, Event, MarketEvent, RiskEvent};
use crate::infrastructure::database::repositories::{PositionRepository, TokenRepository};
use crate::infrastructure::dex::{DexClient, TransactionStatus};
use crate::EventRouter;

use log::{info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::orders;

/// Context containing all state needed for ExecutionActor event handling
pub struct ExecutionContext<'a> {
    pub active_transactions: &'a Arc<Mutex<HashMap<String, TransactionStatus>>>,
    pub token_repo: &'a Arc<TokenRepository>,
    pub position_repo: &'a Arc<PositionRepository>,
    pub dex_client: &'a DexClient,
    pub event_router: &'a Arc<EventRouter>,
    pub config: &'a Arc<Config>,
    pub running: bool,
}

/// Handle internal events for the execution actor
pub async fn handle_event_internal(event: Event, ctx: ExecutionContext<'_>) -> Result<(), Error> {
    match event {
        Event::Risk(risk_event) => {
            handle_risk_event(risk_event, ctx).await?;
        }
        Event::DexTransaction(dex_tx_event) => {
            handle_dex_transaction_event(*dex_tx_event, ctx.active_transactions).await?;
        }
        Event::Market(market_event) => {
            handle_market_event(market_event, ctx).await?;
        }
        Event::AIAdvisor(_) | Event::Strategy(_) | Event::Execution(_) => {
            // Silently ignore unhandled events
        }
    }
    Ok(())
}

/// Handle market events (pool discoveries)
async fn handle_market_event(
    market_event: MarketEvent,
    ctx: ExecutionContext<'_>,
) -> Result<(), Error> {
    match market_event {
        MarketEvent::PoolsDiscovered { pools, source, .. } => {
            info!(
                "📨 ExecutionActor received {} pools from {}",
                pools.len(),
                source
            );
            ctx.dex_client.update_pool_cache(pools, &source).await;
            Ok(())
        }
        _ => Ok(()), // Silently ignore other market events
    }
}

/// Handle risk events
async fn handle_risk_event(risk_event: RiskEvent, ctx: ExecutionContext<'_>) -> Result<(), Error> {
    match risk_event {
        RiskEvent::TradeApproved {
            token_id,
            signal,
            position_size,
            timestamp: _,
            signal_metadata,
        } => {
            orders::handle_risk_assessment(
                ctx.token_repo,
                ctx.position_repo,
                ctx.dex_client,
                ctx.event_router,
                ctx.config,
                ctx.running,
                orders::RiskAssessmentRequest {
                    token_id,
                    signal,
                    position_size,
                    signal_metadata,
                },
            )
            .await
        }
        _ => Ok(()), // Silently ignore other risk events
    }
}

/// Handle DEX transaction events
async fn handle_dex_transaction_event(
    dex_tx_event: DexTransactionEvent,
    active_transactions: &Arc<Mutex<HashMap<String, TransactionStatus>>>,
) -> Result<(), Error> {
    match dex_tx_event {
        DexTransactionEvent::Submitted {
            tx_id,
            submission_time,
            priority,
            submitted_details: _,
        } => {
            let mut active_txs = match active_transactions.try_lock() {
                Ok(txs) => txs,
                Err(_) => {
                    warn!(
                        "Failed to acquire lock on active_transactions for submitted event: {}",
                        tx_id
                    );
                    return Err(Error::Internal(
                        "Failed to acquire active_transactions lock".to_string(),
                    ));
                }
            };
            if !active_txs.contains_key(&tx_id) {
                let initial_status = TransactionStatus::Queued {
                    tx_id: tx_id.clone(),
                    submission_time,
                    priority,
                };
                active_txs.insert(tx_id.clone(), initial_status);
                info!(
                    "ExecutionActor: Tracking new submitted transaction: {}",
                    tx_id
                );
            } else {
                warn!(
                    "ExecutionActor: Received Submitted event for already tracked tx: {}",
                    tx_id
                );
            }
        }
        DexTransactionEvent::StatusUpdated { status, details: _ } => {
            // Extract transaction ID and update active transactions
            let tx_id_str = match &status {
                // In-progress statuses: insert/update in tracking map
                TransactionStatus::Queued { tx_id, .. }
                | TransactionStatus::Pending { tx_id, .. }
                | TransactionStatus::Confirmed { tx_id, .. } => {
                    match active_transactions.try_lock() {
                        Ok(mut active_txs) => {
                            active_txs.insert(tx_id.clone(), status.clone());
                            tx_id.clone()
                        }
                        Err(_) => {
                            warn!("Failed to acquire lock for status update: {}", tx_id);
                            tx_id.clone()
                        }
                    }
                }
                // Terminal statuses: remove from tracking map
                TransactionStatus::Success { tx_id, .. }
                | TransactionStatus::Failed { tx_id, .. }
                | TransactionStatus::Dropped { tx_id, .. } => {
                    match active_transactions.try_lock() {
                        Ok(mut active_txs) => {
                            active_txs.remove(tx_id);
                            tx_id.clone()
                        }
                        Err(_) => {
                            warn!("Failed to acquire lock to remove transaction: {}", tx_id);
                            tx_id.clone()
                        }
                    }
                }
                // Statuses without specific tx_id
                TransactionStatus::Cancelled | TransactionStatus::Unknown => "unknown".to_string(),
            };
            info!(
                "ExecutionActor: Updated status for {}: {:?}",
                tx_id_str, status
            );
        }
    }
    Ok(())
}
