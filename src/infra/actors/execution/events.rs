use super::super::{DexTransactionEvent, Event, RiskEvent};
use crate::core::error::Error;
use crate::domain::dex::TransactionStatus;
use log::{info, trace, warn};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Handle internal events for the execution actor
pub async fn handle_event_internal<F, Fut>(
    event: Event,
    active_transactions: &Arc<Mutex<HashMap<String, TransactionStatus>>>,
    handle_risk_assessment_fn: F,
) -> Result<(), Error>
where
    F: FnOnce(String, crate::domain::trading::strategy::Signal, f64, f64) -> Fut,
    Fut: std::future::Future<Output = Result<(), Error>>,
{
    trace!("ExecutionActor received event: {:?}", event);
    match event {
        Event::Risk(risk_event) => match risk_event {
            RiskEvent::RiskAssessment {
                token_id,
                signal,
                confidence,
                position_size,
                timestamp: _,
            } => handle_risk_assessment_fn(token_id, signal, confidence, position_size).await?,
            RiskEvent::PositionClosed { token_id, .. } => {
                active_transactions.lock().await.remove(&token_id);
                info!("Position closed for {}. If related to a DEX tx, ensure active_transactions is cleared appropriately.", token_id);
            }
            _ => trace!("ExecutionActor ignoring other RiskEvent: {:?}", risk_event),
        },
        Event::DexTransaction(dex_tx_event) => {
            handle_dex_transaction_event(dex_tx_event, active_transactions).await?;
        }
        Event::Market(_) => trace!("ExecutionActor ignoring MarketEvent"),
        Event::Strategy(_) => trace!("ExecutionActor ignoring StrategyEvent"),
        Event::Database(_) => trace!("ExecutionActor ignoring DatabaseEvent"),
        Event::Execution(_) => {
            trace!("ExecutionActor ignoring its own ExecutionEvent types via this path")
        }
    }
    Ok(())
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
            let mut active_txs = active_transactions.lock().await;
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
        DexTransactionEvent::StatusUpdated { status } => {
            let tx_id_str = match &status {
                TransactionStatus::Queued { tx_id, .. } => tx_id.clone(),
                TransactionStatus::Pending { tx_id, .. } => tx_id.clone(),
                TransactionStatus::Confirmed { details, .. } => details.tx_id.clone(),
                TransactionStatus::Success { details, .. } => details.tx_id.clone(),
                TransactionStatus::Failed { tx_id, .. } => tx_id.clone(),
                TransactionStatus::Dropped { tx_id, .. } => tx_id.clone(),
            };
            info!(
                "ExecutionActor: Received StatusUpdated for {}: {:?}. (Usually self-published)",
                tx_id_str, status
            );
        }
    }
    Ok(())
}
