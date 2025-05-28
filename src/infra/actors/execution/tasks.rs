use super::super::{Actor, Command, Event, Message, Query, QueryResult};
use crate::core::error::Error;
use crate::domain::dex::{DexClient, TransactionStatus};
use crate::domain::trading::execution::bot::is_forced_shutdown;
use crate::infra::actors::MessageBus;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

/// Start periodic position checking task
pub async fn start_periodic_check<F, Fut>(
    periodic_task_running: Arc<AtomicBool>,
    check_positions_fn: F,
) -> Result<(), Error>
where
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<(), Error>> + Send,
{
    if periodic_task_running.load(Ordering::SeqCst) {
        warn!("Periodic position check task already running");
        return Ok(());
    }

    periodic_task_running.store(true, Ordering::SeqCst);

    info!("Starting periodic position check every 30 seconds");

    let running = periodic_task_running.clone();
    tokio::spawn(async move {
        let mut check_interval = interval(Duration::from_secs(30));
        debug!("Position check interval initialized");

        // Run an initial check immediately
        debug!("Running initial position check");
        match check_positions_fn().await {
            Ok(_) => debug!("Initial position check completed successfully"),
            Err(e) => error!("Error in initial position check: {:?}", e),
        }

        // Track consecutive failures to prevent rapid cycling on persistent errors
        let mut consecutive_failures = 0;

        while running.load(Ordering::SeqCst) {
            // Check global shutdown flag first
            if is_forced_shutdown() {
                info!("Periodic position check: Global shutdown detected, exiting");
                break;
            }

            check_interval.tick().await;

            // Check again after tick in case shutdown happened during wait
            if is_forced_shutdown() {
                info!("Periodic position check: Global shutdown detected after tick, exiting");
                break;
            }

            debug!("Periodic timer triggered, running position check");

            match check_positions_fn().await {
                Ok(_) => {
                    debug!("Position check completed successfully");
                    consecutive_failures = 0; // Reset on success
                }
                Err(e) => {
                    error!("Error in periodic position check: {:?}", e);
                    consecutive_failures += 1;

                    // If we've had too many consecutive failures, add a backoff
                    if consecutive_failures > 3 {
                        let backoff = Duration::from_secs(5 * consecutive_failures as u64);
                        warn!(
                            "Multiple position check failures ({}) - backing off for {:?}",
                            consecutive_failures, backoff
                        );
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }

        info!("Periodic position check task has ended");
    });

    Ok(())
}

/// Stop periodic position checking task
pub async fn stop_periodic_check(periodic_task_running: Arc<AtomicBool>) -> Result<(), Error> {
    info!("Stopping periodic position check");
    periodic_task_running.store(false, Ordering::SeqCst);
    Ok(())
}

/// Start transaction status polling task
pub async fn start_transaction_status_polling(
    transaction_polling_task_running: Arc<AtomicBool>,
    active_transactions: Arc<Mutex<HashMap<String, TransactionStatus>>>,
    dex_client: DexClient,
    _message_bus: Arc<MessageBus>,
) -> Result<(), Error> {
    if transaction_polling_task_running.load(Ordering::Relaxed) {
        warn!("ExecutionActor: Transaction status polling task already running.");
        return Ok(());
    }

    transaction_polling_task_running.store(true, Ordering::Relaxed);
    info!("ExecutionActor: Starting transaction status polling task...");

    let task_running = transaction_polling_task_running.clone();
    tokio::spawn(async move {
        let mut tick_interval = interval(Duration::from_secs(10)); // Poll every 10 seconds

        while task_running.load(Ordering::Relaxed) {
            // Check global shutdown flag first
            if is_forced_shutdown() {
                info!("Transaction status polling: Global shutdown detected, exiting");
                break;
            }

            tick_interval.tick().await;

            // Check again after tick in case shutdown happened during wait
            if is_forced_shutdown() {
                info!("Transaction status polling: Global shutdown detected after tick, exiting");
                break;
            }

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
                // Check shutdown flag during the loop as well
                if is_forced_shutdown() {
                    info!("Transaction status polling: Global shutdown detected during polling loop, exiting");
                    break;
                }

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
                    Ok(polled_status) => {
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

                            // Note: This would need the proper event type import
                            // For now, we'll just log the status change
                            info!(
                                "ExecutionActor: Would publish StatusUpdated event for tx {}: {:?}",
                                tx_id, polled_status
                            );

                            // If terminal, remove from active tracking to stop polling
                            match polled_status {
                                TransactionStatus::Success { .. }
                                | TransactionStatus::Failed { .. }
                                | TransactionStatus::Dropped { .. } => {
                                    info!("ExecutionActor: Tx {} reached terminal state. Removing from active polling.", tx_id);
                                    active_txs_guard.remove(&tx_id);
                                }
                                _ => {}
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
