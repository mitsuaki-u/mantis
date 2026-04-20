use crate::application::errors::Error;
use crate::core::calculations::financial;
use crate::core::domain::token::TokenData;
use crate::core::domain::trading::{Position as StrategyPosition, Signal};
use crate::EventRouter;
use chrono::Utc;
use log::{debug, error, info};
use rust_decimal::Decimal;
use std::sync::Arc;

/// Represents a trading position in the execution actor
#[derive(Debug, Clone)]
pub struct Position {
    pub token_id: String,
    pub entry_price: Decimal,
    pub current_price: Decimal,
    pub highest_price: Decimal,
    pub size: Decimal,
    pub unrealized_pnl: Decimal,
    pub entry_time: chrono::DateTime<Utc>,
}

impl Position {
    /// Create a new position
    pub fn new(
        token_id: String,
        entry_price: Decimal,
        current_price: Decimal,
        size: Decimal,
        entry_time: chrono::DateTime<Utc>,
    ) -> Self {
        Self {
            token_id,
            entry_price,
            current_price,
            highest_price: current_price,
            size,
            unrealized_pnl: Decimal::ZERO,
            entry_time,
        }
    }

    /// Update the current price and recalculate unrealized PnL
    pub fn update_price(&mut self, new_price: Decimal) {
        self.current_price = new_price;
        if new_price > self.highest_price {
            self.highest_price = new_price;
        }
        self.unrealized_pnl = (new_price - self.entry_price) * self.size;
    }
}

/// Calculate position metrics for reporting
#[derive(Debug, Clone)]
pub struct PositionMetrics {
    pub current_value: Decimal,
    pub unrealized_pnl: Decimal,
    pub unrealized_pnl_percentage: Decimal,
    pub profit_loss_percentage: Decimal,
}

pub fn calculate_position_metrics(position: &Position, current_price: Decimal) -> PositionMetrics {
    let current_value = financial::calculate_position_value_decimal(position.size, current_price);
    let unrealized_pnl =
        financial::calculate_pnl_decimal(current_price, position.entry_price, position.size);
    let initial_value = position.size * position.entry_price;
    let unrealized_pnl_percentage =
        financial::calculate_unrealized_pnl_percentage_decimal(unrealized_pnl, initial_value);
    let profit_loss_percentage =
        financial::calculate_percentage_change_decimal(current_price, position.entry_price);

    PositionMetrics {
        current_value,
        unrealized_pnl,
        unrealized_pnl_percentage,
        profit_loss_percentage,
    }
}

/// Create a new position after successful buy order
pub async fn open_position(
    event_router: &Arc<EventRouter>,
    token_data: &TokenData,
    usd_amount: Decimal,
    token_amount: Decimal,
    actual_fees: Option<f64>, // Actual fees from transaction
    correlation_id: &str,
) -> Result<(), Error> {
    let symbol = &token_data.symbol.to_uppercase();
    let entry_price = token_data.price_usd;

    // Calculate entry price from actual execution
    let actual_entry_price = if token_amount > Decimal::ZERO {
        usd_amount / token_amount
    } else {
        entry_price
    };

    info!(
        "[{}] 💼 Creating position for {}: ${:.2} USD, {:.8} tokens at ${:.8} per token",
        &correlation_id[..8],
        symbol,
        usd_amount,
        token_amount,
        actual_entry_price
    );

    // Add detailed logging for debugging position size issues
    debug!(
        "[{}] 🔍 Position creation details for {}:",
        &correlation_id[..8],
        symbol
    );
    debug!(
        "[{}]   💵 USD amount: {} (type: Decimal)",
        &correlation_id[..8],
        usd_amount
    );
    debug!(
        "[{}]   🪙 Token amount: {} (type: Decimal)",
        &correlation_id[..8],
        token_amount
    );
    debug!(
        "[{}]   💲 Entry price: {} (type: Decimal)",
        &correlation_id[..8],
        actual_entry_price
    );

    // Create strategy position for database storage
    // Convert token amount to f64, failing if conversion produces invalid value
    let size_f64 = crate::core::utils::decimal_to_f64(token_amount, "token amount")
        .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_data.symbol)))?;

    if size_f64 <= 0.0 {
        return Err(Error::Conversion(format!(
            "Token amount conversion resulted in invalid size: {} for {}",
            size_f64, token_data.symbol
        )));
    }

    debug!(
        "💰 Position size conversion: {} (Decimal) -> {} (f64)",
        token_amount, size_f64
    );

    // Convert critical values to f64 for event - fail if conversion fails
    let executed_value_usd_f64 = crate::core::utils::decimal_to_f64(usd_amount, "USD amount")
        .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_data.symbol)))?;
    let price_per_token_f64 = crate::core::utils::decimal_to_f64(actual_entry_price, "entry price")
        .map_err(|e| Error::InvalidInput(format!("{} for {}", e, token_data.symbol)))?;

    // Publish OrderExecuted event for DatabaseActor (persistence) and RiskManager (tracking)
    if let Err(e) = event_router
        .publish(crate::events::Event::Execution(
            crate::events::ExecutionEvent::OrderExecuted {
                token_id: token_data.id.clone(),
                provider_token_id: token_data.id.clone(),
                signal: Signal::Buy,
                executed_value_usd: executed_value_usd_f64,
                token_quantity: size_f64,
                price_per_token: price_per_token_f64,
                timestamp: Utc::now(),
                entry_price: None, // Not needed for BUY
                entry_time: None,  // Not needed for BUY
                actual_fees,       // Actual fees from DEX transaction
                correlation_id: Some(correlation_id.to_string()), // For position slot reservation cleanup
            },
        ))
        .await
    {
        error!(
            "[{}] CRITICAL: Failed to publish OrderExecuted event for {}: {}. Position may not be persisted!",
            &correlation_id[..8], symbol, e
        );
        return Err(e);
    }

    info!(
        "[{}] Published OrderExecuted(BUY) event for {} - DatabaseActor will persist, RiskManager will track",
        &correlation_id[..8], symbol
    );

    Ok(())
}

/// Close position after successful sell order
pub async fn close_position(
    event_router: &Arc<EventRouter>,
    position_data: &StrategyPosition,
    usd_received: Decimal,
    exit_price: Decimal,
    actual_fees: Option<f64>, // Actual fees from transaction
    correlation_id: &str,
) -> Result<(), Error> {
    let symbol = &position_data.token_id;

    // Calculate P&L
    let initial_value = crate::core::utils::f64_to_decimal(
        position_data.size * position_data.entry_price,
        "initial value",
    )
    .map_err(|e| Error::Conversion(format!("{} for {}", e, position_data.token_id)))?;
    let pnl = usd_received - initial_value;
    let pnl_percentage = if initial_value > Decimal::ZERO {
        (pnl / initial_value) * Decimal::from(100)
    } else {
        Decimal::ZERO
    };

    info!(
        "[{}] 💼 Closing position for {}: P&L = ${:.2} ({:.2}%)",
        &correlation_id[..8],
        symbol,
        pnl,
        pnl_percentage
    );

    // Convert critical values for event - fail if conversion fails
    let executed_value_usd_f64 =
        crate::core::utils::decimal_to_f64(usd_received, "USD received")
            .map_err(|e| Error::InvalidInput(format!("{} for {}", e, position_data.token_id)))?;
    let price_per_token_f64 = crate::core::utils::decimal_to_f64(exit_price, "exit price")
        .map_err(|e| Error::InvalidInput(format!("{} for {}", e, position_data.token_id)))?;

    // Publish OrderExecuted event for DatabaseActor (persistence) and RiskManager (tracking)
    // For SELL, we include entry data so DatabaseActor can properly close the position
    if let Err(e) = event_router
        .publish(crate::events::Event::Execution(
            crate::events::ExecutionEvent::OrderExecuted {
                token_id: position_data.token_id.clone(),
                provider_token_id: position_data.token_id.clone(),
                signal: Signal::Sell,
                executed_value_usd: executed_value_usd_f64,
                token_quantity: position_data.size,
                price_per_token: price_per_token_f64,
                timestamp: Utc::now(),
                entry_price: Some(position_data.entry_price), // For position close
                entry_time: Some(position_data.entry_time),   // For position close
                actual_fees,                                  // Transaction fees
                correlation_id: Some(correlation_id.to_string()), // For position slot reservation cleanup
            },
        ))
        .await
    {
        error!(
            "[{}] CRITICAL: Failed to publish OrderExecuted event for {}: {}. Position close may not be persisted!",
            &correlation_id[..8], symbol, e
        );
        return Err(e);
    }

    info!(
        "[{}] Published OrderExecuted(SELL) event for {} with P&L ${:.2}",
        &correlation_id[..8],
        symbol,
        pnl
    );

    Ok(())
}
