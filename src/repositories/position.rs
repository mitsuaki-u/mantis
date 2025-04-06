use crate::db::Database;
use crate::error::Result;
use crate::trading::strategy::Position;

#[derive(Clone)]
pub struct PositionRepository {
    db: Database,
    is_paper_trade: bool,
}

impl PositionRepository {
    pub fn new(db: Database, is_paper_trade: bool) -> Self {
        Self { db, is_paper_trade }
    }

    /// Get all open positions
    pub fn get_open_positions(&self) -> Result<Vec<Position>> {
        self.db.get_open_positions(self.is_paper_trade)
    }
    
    /// Record a new position opening
    pub fn record_position_open(&self, position: &Position) -> Result<i64> {
        self.db.record_position_open(position, self.is_paper_trade)
    }
    
    /// Record a position closure with profit/loss
    pub fn record_position_close(&self, position: &Position, profit_loss: f64, profit_loss_pct: f64) -> Result<i64> {
        self.db.record_position_close(position, profit_loss, profit_loss_pct, self.is_paper_trade)
    }
    
    /// Update an existing position with new price/PnL data
    pub fn update_position(&self, position: &Position) -> Result<()> {
        self.db.update_position(position, self.is_paper_trade)
    }
    
    /// Delete an open position
    pub fn delete_open_position(&self, token_id: &str) -> Result<()> {
        self.db.delete_open_position(token_id, self.is_paper_trade)
    }
}

// No standard Repository implementation since Position doesn't have a simple ID field
// and requires specialized repository methods 