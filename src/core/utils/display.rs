//! Display utilities for formatted output
//!
//! This module contains all UI display functions for tables, summaries, and reports.
//! These functions handle the presentation layer, formatting data for terminal output.

use crate::core::domain::trading::Position as StrategyPosition;
use crate::core::utils::{f64_to_decimal, format_price_safe, format_roi_safe, format_size_safe};
use crate::infrastructure::database::repositories::{
    PositionRepository, TokenRepository, TradeRepository, TransactionRepository,
};
use crate::infrastructure::database::Database;
use chrono::{DateTime, Utc};
use colored::*;
use log::warn;

/// Display session summary with trading statistics and performance
pub async fn display_exit_summary(start_time: DateTime<Utc>, db: &Database, is_paper: bool) {
    let stop_time = Utc::now();
    let duration = stop_time.signed_duration_since(start_time);

    let hours = duration.num_hours();
    let minutes = duration.num_minutes() % 60;
    let seconds = duration.num_seconds() % 60;

    let mode = if is_paper {
        "Paper Trading"
    } else {
        "Live Trading"
    };

    println!("\n{}", "═".repeat(80));
    println!("🤖 MANTIS TRADING BOT - SESSION SUMMARY");
    println!("{}", "═".repeat(80));
    println!();

    // Runtime
    println!("⏱️  RUNTIME");
    println!(
        "   Started:  {}",
        start_time.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("   Stopped:  {}", stop_time.format("%Y-%m-%d %H:%M:%S UTC"));
    println!("   Duration: {}h {}m {}s", hours, minutes, seconds);
    println!();

    // Trading Activity - query database for statistics
    let pos_repo = PositionRepository::new(db.clone(), is_paper);
    let token_repo = TokenRepository::new(db.clone(), is_paper);

    // Get session statistics from database
    let open_positions = pos_repo.get_open_positions().await.unwrap_or_default();
    let closed_positions = pos_repo
        .get_closed_positions(None)
        .await
        .unwrap_or_default();

    // Filter positions by session timing
    // Positions opened during session: entry_time >= start (includes both open and closed)
    let session_opened_count = open_positions
        .iter()
        .filter(|p| p.entry_time >= start_time)
        .count()
        + closed_positions
            .iter()
            .filter(|p| p.entry_time >= start_time)
            .count();

    // Positions closed during session: exit_time >= start (regardless of when opened)
    let session_closed: Vec<_> = closed_positions
        .iter()
        .filter(|p| p.exit_time >= start_time)
        .collect();

    let positions_opened = session_opened_count;
    let positions_closed = session_closed.len();

    println!("📊 TRADING ACTIVITY ({})", mode);
    println!("   New Positions:       {}", positions_opened);
    println!("   Positions Closed:    {}", positions_closed);
    println!("   Currently Open:      {}", open_positions.len());
    println!();

    // Performance metrics
    if !session_closed.is_empty() {
        let total_realized_pnl: f64 = session_closed.iter().map(|p| p.net_profit).sum();
        let total_unrealized_pnl: f64 = open_positions.iter().map(|p| p.unrealized_pnl).sum();
        let total_pnl = total_realized_pnl + total_unrealized_pnl;

        let winners = session_closed.iter().filter(|p| p.net_profit > 0.0).count();
        let win_rate = if positions_closed > 0 {
            (winners as f64 / positions_closed as f64) * 100.0
        } else {
            0.0
        };

        let best_trade = session_closed
            .iter()
            .max_by(|a, b| a.net_profit.partial_cmp(&b.net_profit).unwrap());
        let worst_trade = session_closed
            .iter()
            .min_by(|a, b| a.net_profit.partial_cmp(&b.net_profit).unwrap());

        println!("💰 PERFORMANCE");
        println!("   Realized P&L:        ${:.2}", total_realized_pnl);
        println!("   Unrealized P&L:      ${:.2}", total_unrealized_pnl);
        println!("   Total P&L:           ${:.2}", total_pnl);
        println!(
            "   Win Rate:            {:.1}% ({}/{})",
            win_rate, winners, positions_closed
        );
        println!();

        if let Some(best) = best_trade {
            let symbol = token_repo.get_token_symbol(&best.token_id).await;
            println!(
                "   Best Trade:          ${:.2} ({})",
                best.net_profit, symbol
            );
        }

        if let Some(worst) = worst_trade {
            let symbol = token_repo.get_token_symbol(&worst.token_id).await;
            println!(
                "   Worst Trade:         ${:.2} ({})",
                worst.net_profit, symbol
            );
        }
        println!();
    }

    println!("{}", "═".repeat(80));
    println!();
}

/// Display trading history for the specified mode
pub async fn display_trading_history(
    db: &Database,
    is_paper: bool,
    limit: usize,
) -> Result<(), crate::core::errors::Error> {
    let mode = if is_paper {
        "Paper Trading".bright_yellow()
    } else {
        "Live Trading".bright_red()
    };
    println!("\n📊 {} Raw Trade History:", mode);

    let trade_repo = TradeRepository::new(db.clone(), is_paper);
    let token_repo = TokenRepository::new(db.clone(), is_paper);

    // Get recent raw trades
    let history = trade_repo.get_trading_history(limit).await?;

    if history.is_empty() {
        println!("\nNo trading history found.");
        return Ok(());
    }

    // Display trade history table
    println!(
        "\n{:<10} {:<15} {:<6} {:<15} {:<15} {:<25} {:<8}",
        "Trade ID", "Token", "Side", "Price", "Size", "Timestamp", "Pos ID"
    );
    println!("{}", "-".repeat(100));

    for trade in &history {
        let side_str = if trade.is_buy {
            "BUY".bright_green()
        } else {
            "SELL".bright_red()
        };
        let pos_id_str = trade
            .position_id
            .map_or_else(|| "N/A".dimmed().to_string(), |id| id.to_string());

        // Get token symbol (fallback to token_id if not found)
        let token_symbol = token_repo.get_token_symbol(&trade.token_id).await;

        println!(
            "{:<10} {:<15} {:<18} ${:<14.4} {:<15.4} {:<25} {:<8}",
            trade.id,
            token_symbol,
            side_str,
            trade.price,
            trade.size,
            trade.timestamp.format("%Y-%m-%d %H:%M:%S"),
            pos_id_str
        );
    }
    println!("\nDisplaying last {} trades.", history.len());

    Ok(())
}

/// Display DEX transaction logs with optional blockchain sync
pub async fn display_transactions(
    db: &Database,
    limit: usize,
    failed: bool,
    pending: bool,
    confirmed: bool,
) -> Result<(), crate::core::errors::Error> {
    let repo = TransactionRepository::new(std::sync::Arc::new(db.clone()));

    // Show multiple transactions with filters
    println!("📋 DEX Transaction History");
    println!("{}", "=".repeat(120));

    // Get transactions from database
    let transactions = repo.get_recent_transactions(limit as i64, None).await?;

    if transactions.is_empty() {
        println!("📭 No transactions found in database.");
        println!("💡 Tip: Run some trades first, or use --sync to check for missed transactions.");
        return Ok(());
    }

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

    if filtered_transactions.is_empty() {
        println!("📭 No transactions match the specified filters.");
        return Ok(());
    }

    // Display header
    println!(
        "{:<66} {:<12} {:<10} {:<10} {:<8} {:<12} {:<20}",
        "TX Hash", "Status", "From", "To", "Gas $", "Amount", "Time"
    );
    println!("{}", "-".repeat(120));

    // Display each transaction
    for tx in &filtered_transactions {
        let status_emoji = match tx.current_status.as_str() {
            "Success" => "✅",
            "Confirmed" => "🔄",
            "Failed" => "❌",
            "Pending" => "⏳",
            "Queued" => "📋",
            "Dropped" => "💨",
            "Cancelled" => "🚫",
            _ => "❓",
        };

        let gas_cost = tx
            .network_fee_usd
            .map(|cost| format!("${:.2}", cost))
            .unwrap_or_else(|| "N/A".to_string());

        let amount = format!("${:.2}", tx.amount_in);

        let time_str = format_time_ago(tx.created_at);

        println!(
            "{:<66} {:<1}{:<11} {:<10} {:<10} {:<8} {:<12} {:<20}",
            truncate_hash(&tx.tx_hash),
            status_emoji,
            tx.current_status,
            truncate_address(&tx.token_in_address),
            truncate_address(&tx.token_out_address),
            gas_cost,
            amount,
            time_str
        );

        // Show error message for failed transactions
        if tx.current_status == "Failed" {
            println!("    └─ Error: Transaction failed (check status history for details)");
        }
    }

    println!();
    println!("📊 Summary:");
    println!("   Total transactions: {}", filtered_transactions.len());

    let confirmed_count = filtered_transactions
        .iter()
        .filter(|tx| tx.current_status == "Success" || tx.current_status == "Confirmed")
        .count();
    let failed_count = filtered_transactions
        .iter()
        .filter(|tx| tx.current_status == "Failed")
        .count();
    let pending_count = filtered_transactions
        .iter()
        .filter(|tx| tx.current_status == "Pending")
        .count();

    println!("   ✅ Confirmed: {}", confirmed_count);
    println!("   ❌ Failed: {}", failed_count);
    println!("   ⏳ Pending: {}", pending_count);

    let total_gas_cost: f64 = filtered_transactions
        .iter()
        .filter_map(|tx| tx.network_fee_usd)
        .sum();

    if total_gas_cost > 0.0 {
        println!("   ⛽ Total gas cost: ${:.2}", total_gas_cost);
    }

    Ok(())
}

/// Display open positions
pub async fn display_open_positions(
    positions: &[StrategyPosition],
    is_paper: bool,
    db: &Database,
) -> Result<(), crate::core::errors::Error> {
    println!(
        "\n📈 Open Positions ({}):",
        if is_paper {
            "Paper Trading"
        } else {
            "Live Trading"
        }
    );
    println!("{:-<140}", "");
    println!(
        "{:<15} {:<12} {:<12} {:<12} {:<14} {:<14} {:<16} {:<12} {:<20}",
        "Token",
        "Entry Price",
        "Current",
        "Highest",
        "Qty (Tokens)",
        "Current Value",
        "Net Gain/Loss",
        "ROI %",
        "Entry Time"
    );
    println!("{:-<140}", "");

    let mut total_unrealized_pnl = 0.0;
    let mut total_invested = 0.0;
    let mut total_current_value = 0.0;

    // Create token repository to get symbols
    let token_repo = TokenRepository::new(db.clone(), is_paper);

    for position in positions {
        let pnl_color = if position.unrealized_pnl >= 0.0 {
            "🟢 "
        } else {
            "🔴 "
        };
        let invested_amount = position.size * position.entry_price;
        let current_value = position.size * position.current_price;
        let roi_percentage = if invested_amount > 0.0 {
            (position.unrealized_pnl / invested_amount) * 100.0
        } else {
            0.0
        };

        // Convert to Decimal for safe formatting - skip position if conversion fails
        let entry_price_decimal = match f64_to_decimal(position.entry_price, "entry_price") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping position {} - invalid entry_price {}: {}",
                    position.token_id, position.entry_price, e
                );
                continue;
            }
        };
        let current_price_decimal = match f64_to_decimal(position.current_price, "current_price") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping position {} - invalid current_price {}: {}",
                    position.token_id, position.current_price, e
                );
                continue;
            }
        };
        let highest_price_decimal = match f64_to_decimal(position.highest_price, "highest_price") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping position {} - invalid highest_price {}: {}",
                    position.token_id, position.highest_price, e
                );
                continue;
            }
        };
        let size_decimal = match f64_to_decimal(position.size, "size") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping position {} - invalid size {}: {}",
                    position.token_id, position.size, e
                );
                continue;
            }
        };
        let pnl_decimal = match f64_to_decimal(position.unrealized_pnl, "unrealized_pnl") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping position {} - invalid unrealized_pnl {}: {}",
                    position.token_id, position.unrealized_pnl, e
                );
                continue;
            }
        };
        let roi_decimal = match f64_to_decimal(roi_percentage, "roi_percentage") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping position {} - invalid roi_percentage {}: {}",
                    position.token_id, roi_percentage, e
                );
                continue;
            }
        };

        // Get token symbol (fallback to token_id if not found)
        let token_symbol = token_repo.get_token_symbol(&position.token_id).await;

        // Convert current value to Decimal for display
        let current_value_decimal = match f64_to_decimal(current_value, "current_value") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping position {} - invalid current_value {}: {}",
                    position.token_id, current_value, e
                );
                continue;
            }
        };

        println!(
            "{:<15} {:<12} {:<12} {:<12} {:<14} {:<14} {}{:<15} {:<12} {}",
            token_symbol,
            format_price_safe(entry_price_decimal),
            format_price_safe(current_price_decimal),
            format_price_safe(highest_price_decimal),
            format_size_safe(size_decimal),
            format_price_safe(current_value_decimal),
            pnl_color,
            format_price_safe(pnl_decimal),
            format_roi_safe(roi_decimal),
            position.entry_time.format("%Y-%m-%d %H:%M")
        );

        total_unrealized_pnl += position.unrealized_pnl;
        total_invested += invested_amount;
        total_current_value += current_value;
    }

    let total_roi = if total_invested > 0.0 {
        (total_unrealized_pnl / total_invested) * 100.0
    } else {
        0.0
    };
    println!("{:-<140}", "");

    // Convert totals - if conversion fails, display error message instead of misleading zeros
    match (
        f64_to_decimal(total_invested, "total_invested"),
        f64_to_decimal(total_current_value, "total_current_value"),
        f64_to_decimal(total_unrealized_pnl, "total_unrealized_pnl"),
        f64_to_decimal(total_roi, "total_roi"),
    ) {
        (Ok(invested), Ok(current), Ok(pnl), Ok(_roi)) => {
            println!(
                "Total: {} positions | Invested: {} | Current: {} | Net: {} ({:.2}%)",
                positions.len(),
                format_price_safe(invested),
                format_price_safe(current),
                format_price_safe(pnl),
                total_roi
            );
        }
        _ => {
            warn!("⚠️  Failed to convert totals - displaying raw values");
            println!(
                "Total: {} positions | Invested: ${:.2} | Current: ${:.2} | Net: ${:.2} ({:.2}%)",
                positions.len(),
                total_invested,
                total_current_value,
                total_unrealized_pnl,
                total_roi
            );
        }
    }

    Ok(())
}

/// Display closed positions
pub async fn display_closed_positions(
    positions: &[crate::infrastructure::database::repositories::position::CompletedPosition],
    is_paper: bool,
    db: &Database,
) -> Result<(), crate::core::errors::Error> {
    println!(
        "\n📉 Closed Positions ({}) - Last {}:",
        if is_paper {
            "Paper Trading"
        } else {
            "Live Trading"
        },
        positions.len()
    );
    println!("{:-<150}", "");
    println!(
        "{:<15} {:<12} {:<12} {:<14} {:<12} {:<12} {:<16} {:<10} {:<20} {:<20}",
        "Token",
        "Entry Price",
        "Exit Price",
        "Qty (Tokens)",
        "Gross P&L",
        "Fees",
        "Net Gain/Loss",
        "ROI %",
        "Entry Time",
        "Exit Time"
    );
    println!("{:-<150}", "");

    let mut total_realized_pnl = 0.0;
    let mut total_fees = 0.0;
    let mut total_net_pnl = 0.0;

    // Create token repository to get symbols
    let token_repo = TokenRepository::new(db.clone(), is_paper);

    for position in positions {
        let pnl_color = if position.profit >= 0.0 {
            "🟢 "
        } else {
            "🔴 "
        };

        // Calculate ROI percentage
        let invested_amount = position.entry_price * position.size;
        let roi_percentage = if invested_amount > 0.0 {
            (position.net_profit / invested_amount) * 100.0
        } else {
            0.0
        };

        // Get token symbol (fallback to token_id if not found)
        let token_symbol = token_repo.get_token_symbol(&position.token_id).await;

        // Convert values - skip position if any conversion fails
        let entry_price_decimal = match f64_to_decimal(position.entry_price, "entry_price") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping closed position {} - invalid entry_price {}: {}",
                    position.token_id, position.entry_price, e
                );
                continue;
            }
        };
        let exit_price_decimal = match f64_to_decimal(position.exit_price, "exit_price") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping closed position {} - invalid exit_price {}: {}",
                    position.token_id, position.exit_price, e
                );
                continue;
            }
        };
        let size_decimal = match f64_to_decimal(position.size, "size") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping closed position {} - invalid size {}: {}",
                    position.token_id, position.size, e
                );
                continue;
            }
        };
        let profit_decimal = match f64_to_decimal(position.profit, "profit") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping closed position {} - invalid profit {}: {}",
                    position.token_id, position.profit, e
                );
                continue;
            }
        };
        let fees_decimal = match f64_to_decimal(position.fees, "fees") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping closed position {} - invalid fees {}: {}",
                    position.token_id, position.fees, e
                );
                continue;
            }
        };
        let net_profit_decimal = match f64_to_decimal(position.net_profit, "net_profit") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping closed position {} - invalid net_profit {}: {}",
                    position.token_id, position.net_profit, e
                );
                continue;
            }
        };
        let roi_decimal = match f64_to_decimal(roi_percentage, "roi_percentage") {
            Ok(val) => val,
            Err(e) => {
                warn!(
                    "⚠️  Skipping closed position {} - invalid roi_percentage {}: {}",
                    position.token_id, roi_percentage, e
                );
                continue;
            }
        };

        println!(
            "{:<15} {:<12} {:<12} {:<14} {}{:<11} {:<12} {}{:<15} {:<10} {:<20} {}",
            token_symbol,
            format_price_safe(entry_price_decimal),
            format_price_safe(exit_price_decimal),
            format_size_safe(size_decimal),
            pnl_color,
            format_price_safe(profit_decimal),
            format_price_safe(fees_decimal),
            pnl_color,
            format_price_safe(net_profit_decimal),
            format_roi_safe(roi_decimal),
            position.entry_time.format("%Y-%m-%d %H:%M"),
            position.exit_time.format("%Y-%m-%d %H:%M")
        );

        total_realized_pnl += position.profit;
        total_fees += position.fees;
        total_net_pnl += position.net_profit;
    }

    println!("{:-<150}", "");

    // Calculate total ROI percentage
    let mut total_invested = 0.0;
    for position in positions {
        total_invested += position.entry_price * position.size;
    }
    let total_roi_percentage = if total_invested > 0.0 {
        (total_net_pnl / total_invested) * 100.0
    } else {
        0.0
    };

    // Convert totals - if conversion fails, display raw values instead of misleading zeros
    match (
        f64_to_decimal(total_realized_pnl, "total_realized_pnl"),
        f64_to_decimal(total_fees, "total_fees"),
        f64_to_decimal(total_net_pnl, "total_net_pnl"),
    ) {
        (Ok(pnl), Ok(fees), Ok(net)) => {
            println!(
                "Total: {} | Gross P&L: {} | Fees: {} | Net Gain/Loss: {} ({:.2}%)",
                positions.len(),
                format_price_safe(pnl),
                format_price_safe(fees),
                format_price_safe(net),
                total_roi_percentage
            );
        }
        _ => {
            warn!("⚠️  Failed to convert totals - displaying raw values");
            println!(
                "Total: {} | Gross P&L: ${:.2} | Fees: ${:.2} | Net Gain/Loss: ${:.2} ({:.2}%)",
                positions.len(),
                total_realized_pnl,
                total_fees,
                total_net_pnl,
                total_roi_percentage
            );
        }
    }

    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Format timestamp as "X ago" string
pub fn format_time_ago(timestamp: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(timestamp);

    if duration.num_days() > 0 {
        format!("{}d ago", duration.num_days())
    } else if duration.num_hours() > 0 {
        format!("{}h ago", duration.num_hours())
    } else if duration.num_minutes() > 0 {
        format!("{}m ago", duration.num_minutes())
    } else {
        "now".to_string()
    }
}

/// Truncate transaction hash for display
pub fn truncate_hash(hash: &str) -> String {
    if hash.len() > 16 {
        format!("{}...{}", &hash[..10], &hash[hash.len() - 6..])
    } else {
        hash.to_string()
    }
}

/// Truncate address for display
pub fn truncate_address(address: &str) -> String {
    if address.len() > 10 {
        format!("{}...", &address[..8])
    } else {
        address.to_string()
    }
}
