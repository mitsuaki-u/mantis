use crate::core::error::Error;
use crate::infra::db::repositories::TradeRepository;
use crate::infra::db::Database;
use colored::*;

/// Display trading history for the specified mode
pub async fn display_trading_history(
    db: &Database,
    is_paper: bool,
    limit: usize,
) -> Result<(), Error> {
    let mode = if is_paper {
        "Paper Trading".bright_yellow()
    } else {
        "Live Trading".bright_red()
    };
    println!("\n📊 {} Raw Trade History:", mode);

    let trade_repo = TradeRepository::new(db.clone(), is_paper);

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

        println!(
            "{:<10} {:<15} {:<18} ${:<14.4} {:<15.4} {:<25} {:<8}",
            trade.id,
            trade.token_id,
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
