use crate::core::error::Error;
use crate::domain::trading::strategy::Position as StrategyPosition;
use crate::infra::db::repositories::{PositionRepository, TokenRepository};
use crate::infra::db::Database;
use chrono::Utc;
use log::{error, info, warn};
use std::sync::Arc;

/// Display all open positions
pub async fn display_open_positions(db: &Database, is_paper: bool) -> Result<(), Error> {
    let position_repo = Arc::new(PositionRepository::new(db.clone(), is_paper));

    info!("📊 Fetching open positions (paper: {})...", is_paper);

    let positions = position_repo.get_open_positions().await?;

    if positions.is_empty() {
        println!("📭 No open positions found.");
        return Ok(());
    }

    println!(
        "\n📈 Open Positions ({}):",
        if is_paper {
            "Paper Trading"
        } else {
            "Live Trading"
        }
    );
    println!("{:-<100}", "");
    println!(
        "{:<15} {:<12} {:<12} {:<12} {:<12} {:<12} {:<20}",
        "Token", "Entry Price", "Current", "Highest", "Size", "Unrealized P&L", "Entry Time"
    );
    println!("{:-<100}", "");

    let mut total_unrealized_pnl = 0.0;

    for position in &positions {
        let pnl_color = if position.unrealized_pnl >= 0.0 {
            "🟢"
        } else {
            "🔴"
        };

        println!(
            "{:<15} ${:<11.4} ${:<11.4} ${:<11.4} {:<12.6} {}{:<11.2} {}",
            position.token_id,
            position.entry_price,
            position.current_price,
            position.highest_price,
            position.size,
            pnl_color,
            position.unrealized_pnl,
            position.entry_time.format("%Y-%m-%d %H:%M")
        );

        total_unrealized_pnl += position.unrealized_pnl;
    }

    println!("{:-<100}", "");
    println!(
        "Total Positions: {} | Total Unrealized P&L: {}${:.2}",
        positions.len(),
        if total_unrealized_pnl >= 0.0 {
            "🟢"
        } else {
            "🔴"
        },
        total_unrealized_pnl
    );

    Ok(())
}

/// Close a specific position
pub async fn close_position(
    db: &Database,
    token_id: &str,
    exit_price: Option<f64>,
    is_paper: bool,
) -> Result<(), Error> {
    let position_repo = Arc::new(PositionRepository::new(db.clone(), is_paper));
    let token_repo = Arc::new(TokenRepository::new(db.clone(), is_paper));

    info!("🔄 Attempting to close position for token: {}", token_id);

    // Get the position from database
    let position_result = position_repo.get_position_by_token_id(token_id).await?;

    let (position_id, position) = match position_result {
        Some((id, pos)) => (id, pos),
        None => {
            warn!("❌ No open position found for token: {}", token_id);
            println!("❌ No open position found for token: {}", token_id);
            return Ok(());
        }
    };

    // Determine exit price
    let actual_exit_price = if let Some(price) = exit_price {
        price
    } else {
        // Get current market price
        match token_repo.get_token_price_stats(token_id).await {
            Ok(token_data) => {
                info!(
                    "📊 Using current market price: ${:.4}",
                    token_data.price_usd
                );
                token_data.price_usd
            }
            Err(e) => {
                error!("Failed to get current price for {}: {}", token_id, e);
                return Err(Error::Api(format!("Failed to get current price: {}", e)));
            }
        }
    };

    if actual_exit_price <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "Invalid exit price: ${:.4}",
            actual_exit_price
        )));
    }

    // Calculate profit/loss
    let profit = position.size * (actual_exit_price - position.entry_price);
    let roi = if position.entry_price > 0.0 {
        (actual_exit_price - position.entry_price) / position.entry_price * 100.0
    } else {
        0.0
    };

    println!("📊 Position Summary:");
    println!("   Token: {}", token_id);
    println!("   Entry Price: ${:.4}", position.entry_price);
    println!("   Exit Price: ${:.4}", actual_exit_price);
    println!("   Size: {:.6}", position.size);
    println!("   Profit/Loss: ${:.2}", profit);
    println!("   ROI: {:.2}%", roi);

    // Close the position in database
    let close_args = crate::infra::db::repositories::position::RecordCloseArgs {
        token_id,
        exit_price: actual_exit_price,
        size: position.size,
        entry_price: position.entry_price,
        entry_time: position.entry_time,
        exit_time: Utc::now(),
    };

    match position_repo
        .record_position_close_with_trade(position_id, close_args)
        .await
    {
        Ok(completed_position) => {
            println!("✅ Position closed successfully!");
            println!("   Final Profit: ${:.2}", completed_position.profit);
            println!("   Fees: ${:.2}", completed_position.fees);
            println!("   Net Profit: ${:.2}", completed_position.net_profit);
            info!(
                "✅ Successfully closed position {} with net profit: ${:.2}",
                token_id, completed_position.net_profit
            );
        }
        Err(e) => {
            error!("Failed to close position in database: {}", e);
            return Err(e);
        }
    }

    Ok(())
}

/// Manually open a new position
pub async fn open_position(
    db: &Database,
    token_id: &str,
    entry_price: Option<f64>,
    amount_usd: f64,
    is_paper: bool,
) -> Result<(), Error> {
    let position_repo = Arc::new(PositionRepository::new(db.clone(), is_paper));
    let token_repo = Arc::new(TokenRepository::new(db.clone(), is_paper));

    info!("🔄 Attempting to open position for token: {}", token_id);

    if amount_usd <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "Invalid USD amount: ${:.2}",
            amount_usd
        )));
    }

    // Check if position already exists
    if position_repo.position_exists(token_id).await? {
        warn!("❌ Position already exists for token: {}", token_id);
        println!("❌ Position already exists for token: {}", token_id);
        return Ok(());
    }

    // Determine entry price
    let actual_entry_price = if let Some(price) = entry_price {
        price
    } else {
        // Get current market price
        match token_repo.get_token_price_stats(token_id).await {
            Ok(token_data) => {
                info!(
                    "📊 Using current market price: ${:.4}",
                    token_data.price_usd
                );
                token_data.price_usd
            }
            Err(e) => {
                error!("Failed to get current price for {}: {}", token_id, e);
                return Err(Error::Api(format!("Failed to get current price: {}", e)));
            }
        }
    };

    if actual_entry_price <= 0.0 {
        return Err(Error::InvalidInput(format!(
            "Invalid entry price: ${:.4}",
            actual_entry_price
        )));
    }

    // Calculate size (quantity of tokens) from USD amount and price
    let size = amount_usd / actual_entry_price;

    // Get provider_id (for now, use token_id as provider_id)
    let provider_id = token_id.to_string();

    // Create the position
    let position = StrategyPosition::new(
        token_id.to_string(),
        provider_id,
        actual_entry_price,
        size,
        Utc::now(),
    );

    println!("📊 Position Details:");
    println!("   Token: {}", token_id);
    println!("   Entry Price: ${:.4}", actual_entry_price);
    println!("   USD Amount: ${:.2}", amount_usd);
    println!("   Size (Quantity): {:.6}", size);

    // Record the position in database
    match position_repo
        .record_position_with_trade(&position, actual_entry_price, size, Utc::now())
        .await
    {
        Ok(position_id) => {
            println!("✅ Position opened successfully!");
            println!("   Position ID: {}", position_id);
            info!(
                "✅ Successfully opened position {} with ID: {}",
                token_id, position_id
            );
        }
        Err(e) => {
            error!("Failed to record position in database: {}", e);
            return Err(e);
        }
    }

    Ok(())
}
