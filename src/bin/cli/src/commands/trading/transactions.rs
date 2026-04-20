use crate::config::Config;
use crate::error::{Error, Result};
use crate::infrastructure::database::repositories::{Transaction, TransactionRepository};
use crate::infrastructure::database::Database;
use ethers::providers::{Http, Middleware, Provider};
use log::{info, warn};
use std::sync::Arc;

// Import display function from utils
use crate::core::utils::display::display_transactions as display_transactions_impl;

/// Display DEX transaction logs with optional blockchain sync
pub async fn display_transactions(
    config: &Config,
    db: Database,
    limit: usize,
    sync: bool,
    failed: bool,
    pending: bool,
    confirmed: bool,
) -> Result<()> {
    // Use the display module function
    display_transactions_impl(&db, limit, failed, pending, confirmed).await?;

    // Handle blockchain sync if requested
    if sync {
        let repo = TransactionRepository::new(Arc::new(db));
        let transactions = repo.get_recent_transactions(limit as i64, None).await?;

        // Filter based on status if requested
        let filtered_transactions: Vec<_> = transactions
            .into_iter()
            .filter(|tx| {
                if failed && tx.current_status != "Failed" {
                    return false;
                }
                if pending && tx.current_status != "Pending" {
                    return false;
                }
                if confirmed && tx.current_status != "Confirmed" {
                    return false;
                }
                true
            })
            .collect();

        println!();
        println!("🔄 Syncing recent transactions with blockchain...");
        sync_recent_transactions(&filtered_transactions, config).await?;
    }

    Ok(())
}

/// Sync a specific transaction with blockchain data
async fn sync_transaction_with_blockchain(tx_hash: &str, config: &Config) -> Result<()> {
    info!("🔄 Syncing transaction {} with blockchain", tx_hash);

    // Create provider for blockchain queries
    let provider_url = format!(
        "https://mainnet.infura.io/v3/{}",
        config.api_keys.infura.as_deref().unwrap_or("demo")
    );

    let provider = Provider::<Http>::try_from(provider_url)
        .map_err(|e| Error::Network(format!("Failed to create provider: {}", e)))?;

    // Parse transaction hash
    let tx_hash_h256 = tx_hash
        .parse::<ethers::types::H256>()
        .map_err(|e| Error::Conversion(format!("Invalid transaction hash: {}", e)))?;

    // Get transaction receipt from blockchain
    match provider.get_transaction_receipt(tx_hash_h256).await {
        Ok(Some(receipt)) => {
            info!("✅ Transaction found on blockchain");

            let status = if receipt.status == Some(1.into()) {
                "confirmed"
            } else {
                "failed"
            };

            let gas_used = receipt.gas_used.map(|g| g.as_u64()).unwrap_or(0);
            let block_number = receipt.block_number.map(|b| b.as_u64()).unwrap_or(0);

            println!("📊 Blockchain Status: {}", status);
            println!("⛽ Gas Used: {}", gas_used);
            println!("🏗️  Block Number: {}", block_number);

            info!("Synced transaction data from blockchain");
        }
        Ok(None) => {
            warn!("❓ Transaction not found on blockchain (may be pending or dropped)");
        }
        Err(e) => {
            warn!("⚠️  Failed to query blockchain: {}", e);
        }
    }

    Ok(())
}

/// Sync multiple recent transactions with blockchain
async fn sync_recent_transactions(transactions: &[Transaction], config: &Config) -> Result<()> {
    info!(
        "🔄 Syncing {} transactions with blockchain",
        transactions.len()
    );

    let pending_transactions: Vec<_> = transactions
        .iter()
        .filter(|tx| tx.current_status == "Pending" || tx.current_status == "Queued")
        .collect();

    if pending_transactions.is_empty() {
        println!("✅ All transactions are already confirmed or failed.");
        return Ok(());
    }

    println!(
        "🔍 Checking {} pending transactions...",
        pending_transactions.len()
    );

    for tx in pending_transactions {
        if let Err(e) = sync_transaction_with_blockchain(&tx.tx_hash, config).await {
            warn!("Failed to sync {}: {}", tx.tx_hash, e);
        }

        // Small delay to avoid rate limiting
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    println!("✅ Blockchain sync complete!");
    Ok(())
}
