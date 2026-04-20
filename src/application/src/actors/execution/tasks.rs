use crate::application::app::is_forced_shutdown;
use crate::application::errors::Error;
use crate::events::{DexTransactionEvent, Event};
use crate::infrastructure::dex::{DexClient, TransactionStatus};
use crate::EventRouter;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

/// Start transaction status polling task
pub async fn start_transaction_status_polling(
    transaction_polling_task_running: Arc<AtomicBool>,
    active_transactions: Arc<Mutex<HashMap<String, TransactionStatus>>>,
    dex_client: DexClient,
    event_router: Arc<EventRouter>,
) -> Result<(), Error> {
    if transaction_polling_task_running.load(Ordering::Relaxed) {
        warn!("ExecutionActor: Transaction status polling task already running.");
        return Ok(());
    }

    transaction_polling_task_running.store(true, Ordering::Relaxed);
    info!("ExecutionActor: Starting transaction status polling task...");

    let task_running = transaction_polling_task_running.clone();
    let event_router_clone = event_router.clone();
    tokio::spawn(async move {
        let mut tick_interval = interval(Duration::from_secs(10)); // Poll every 10 seconds

        while task_running.load(Ordering::Relaxed) {
            // Check global shutdown flag once per loop iteration
            if is_forced_shutdown() {
                info!("Transaction status polling: Global shutdown detected, exiting");
                break;
            }

            tick_interval.tick().await;

            trace!("ExecutionActor: Polling active transaction statuses...");

            let tx_ids_to_poll: Vec<String> = {
                let active_txs_guard = active_transactions.lock().await;
                active_txs_guard.keys().cloned().collect()
            };

            if tx_ids_to_poll.is_empty() {
                trace!("ExecutionActor: No active transactions to poll.");
                continue;
            }

            debug!(
                "ExecutionActor: Polling status for {} transactions.",
                tx_ids_to_poll.len()
            );

            for tx_id in tx_ids_to_poll {
                let current_known_status_opt: Option<TransactionStatus> = {
                    let active_txs_guard = active_transactions.lock().await;
                    active_txs_guard.get(&tx_id).cloned()
                };

                if let Some(current_known_status) = current_known_status_opt {
                    // Skip polling for already terminal states
                    match current_known_status {
                        TransactionStatus::Success { .. }
                        | TransactionStatus::Failed { .. }
                        | TransactionStatus::Dropped { .. } => {
                            trace!("ExecutionActor: Tx {} is already in a terminal state ({:?}), skipping poll.", tx_id, current_known_status);
                            continue;
                        }
                        _ => {} // Continue polling for non-terminal states
                    }
                } else {
                    warn!("ExecutionActor: Tx {} found in keys but not in map during poll? Should not happen.", tx_id);
                    continue;
                }

                debug!("ExecutionActor: Fetching status for tx_id: {}", tx_id);
                match dex_client.get_transaction_status(&tx_id).await {
                    Ok((polled_status, tx_details)) => {
                        let mut active_txs_guard = active_transactions.lock().await;
                        let mut publish_event = false;

                        if let Some(prev_status_in_map) = active_txs_guard.get(&tx_id) {
                            if polled_status != *prev_status_in_map {
                                info!(
                                    "ExecutionActor: Status changed for tx {}: {:?} -> {:?}",
                                    tx_id, prev_status_in_map, polled_status
                                );
                                publish_event = true;
                            } else {
                                trace!(
                                    "ExecutionActor: Status unchanged for tx {}: {:?}",
                                    tx_id,
                                    polled_status
                                );
                            }
                        } else {
                            info!(
                                "ExecutionActor: Tx {} not previously tracked, new status: {:?}",
                                tx_id, polled_status
                            );
                            publish_event = true;
                        }

                        if publish_event {
                            active_txs_guard.insert(tx_id.clone(), polled_status.clone());

                            // Check if terminal state and remove immediately to avoid re-acquiring lock
                            let is_terminal = matches!(
                                polled_status,
                                TransactionStatus::Success { .. }
                                    | TransactionStatus::Failed { .. }
                                    | TransactionStatus::Dropped { .. }
                            );

                            if is_terminal {
                                info!("ExecutionActor: Tx {} reached terminal state. Removing from active polling.", tx_id);
                                active_txs_guard.remove(&tx_id);
                            }

                            // Drop the lock before async operation
                            drop(active_txs_guard);

                            // Publish StatusUpdated event with complete transaction details
                            let status_updated_event = DexTransactionEvent::StatusUpdated {
                                status: polled_status.clone(),
                                details: tx_details.map(Box::new),
                            };
                            let execution_event =
                                Event::DexTransaction(Box::new(status_updated_event));

                            if let Err(e) = event_router_clone.publish(execution_event).await {
                                error!("Failed to publish DexTransactionEvent::StatusUpdated for {}: {}", tx_id, e);
                            } else {
                                info!(
                                    "Published StatusUpdated event for tx {}: {:?}",
                                    tx_id, polled_status
                                );
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "ExecutionActor: Error fetching status for tx {}: {}",
                            tx_id, e
                        );
                    }
                }
            }
        }
        info!("ExecutionActor: Transaction status polling task stopped.");
    });

    Ok(())
}

/// Stop transaction status polling task
pub async fn stop_transaction_status_polling(
    transaction_polling_task_running: Arc<AtomicBool>,
) -> Result<(), Error> {
    if !transaction_polling_task_running.load(Ordering::Relaxed) {
        warn!("ExecutionActor: Transaction status polling task not running.");
        return Ok(());
    }

    info!("ExecutionActor: Stopping transaction status polling task...");
    transaction_polling_task_running.store(false, Ordering::Relaxed);
    Ok(())
}
