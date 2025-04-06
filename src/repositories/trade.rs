use crate::db::{Database, CompletedTrade, TradingStats};
use crate::error::Result;

#[derive(Clone)]
pub struct TradeRepository {
    db: Database,
    is_paper_trade: bool,
}

impl TradeRepository {
    pub fn new(db: Database, is_paper_trade: bool) -> Self {
        Self { db, is_paper_trade }
    }
    
    /// Get recent trading history with limit
    pub fn get_trading_history(&self, limit: usize) -> Result<Vec<CompletedTrade>> {
        self.db.get_trading_history(self.is_paper_trade, limit)
    }
    
    /// Get trading performance statistics
    pub fn get_performance_stats(&self) -> Result<TradingStats> {
        self.db.get_performance_stats(self.is_paper_trade)
    }
} 