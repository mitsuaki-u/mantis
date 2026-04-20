use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Database-specific Position type with all database fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbPosition {
    pub id: Option<i64>,
    pub token_id: String,
    pub provider_id: String,
    pub entry_price: Decimal,
    pub current_price: Decimal,
    pub highest_price: Decimal,
    pub size: Decimal,
    pub entry_time: DateTime<Utc>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub is_paper: bool,
    pub unrealized_pnl: Option<Decimal>,
    pub unrealized_pnl_percentage: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbTrade {
    pub id: Option<i64>,
    pub token_id: String,
    pub provider_id: String,
    pub price: Decimal,
    pub size: Decimal,
    pub is_buy: bool,
    pub timestamp: DateTime<Utc>,
    pub is_paper: bool,
    pub position_id: Option<i64>,
    pub close_position: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbPriceHistory {
    pub id: Option<i64>,
    pub token_id: String,
    pub price: Decimal,
    pub volume: Option<Decimal>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionSummary {
    pub token_symbol: String,
    pub entry_price: Decimal,
    pub current_price: Decimal,
    pub highest_price: Decimal,
    pub size: Decimal,
    pub unrealized_pnl: Decimal,
    pub unrealized_pnl_percentage: Decimal,
    pub entry_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosedPositionSummary {
    pub token_symbol: String,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub size: Decimal,
    pub gross_pnl: Decimal,
    pub fees: Decimal,
    pub net_pnl: Decimal,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
}

impl DbPosition {
    pub fn calculate_unrealized_pnl(&self) -> Decimal {
        (self.current_price - self.entry_price) * self.size
    }

    pub fn calculate_unrealized_pnl_percentage(&self) -> Decimal {
        if self.entry_price.is_zero() {
            return Decimal::ZERO;
        }
        ((self.current_price - self.entry_price) / self.entry_price) * Decimal::new(100, 0)
    }

    pub fn update_current_price(&mut self, new_price: Decimal) {
        self.current_price = new_price;

        // Update highest price if new price is higher
        if new_price > self.highest_price {
            self.highest_price = new_price;
        }

        // Recalculate P&L
        self.unrealized_pnl = Some(self.calculate_unrealized_pnl());
        self.unrealized_pnl_percentage = Some(self.calculate_unrealized_pnl_percentage());
    }
}
