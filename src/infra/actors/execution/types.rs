use chrono::Utc;

/// Represents a trading position in the execution actor
#[derive(Debug, Clone)]
pub struct Position {
    pub token_id: String,
    pub entry_price: f64,
    pub current_price: f64,
    pub highest_price: f64,
    pub size: f64,
    pub unrealized_pnl: f64,
    pub entry_time: chrono::DateTime<Utc>,
}

impl Position {
    /// Create a new position
    pub fn new(
        token_id: String,
        entry_price: f64,
        current_price: f64,
        size: f64,
        entry_time: chrono::DateTime<Utc>,
    ) -> Self {
        Self {
            token_id,
            entry_price,
            current_price,
            highest_price: current_price,
            size,
            unrealized_pnl: 0.0,
            entry_time,
        }
    }

    /// Update the current price and recalculate unrealized PnL
    pub fn update_price(&mut self, new_price: f64) {
        self.current_price = new_price;
        if new_price > self.highest_price {
            self.highest_price = new_price;
        }
        self.unrealized_pnl = (new_price - self.entry_price) * self.size;
    }
}
