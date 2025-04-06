use crate::db::Database;
use crate::error::Result;
use chrono::{DateTime, Utc};

#[derive(Clone)]
pub struct PriceRepository {
    db: Database,
}

impl PriceRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    
    /// Store a price data point
    pub fn store_price_data(&self, token_id: &str, price: f64, volume: f64) -> Result<()> {
        self.db.store_price_data(token_id, price, volume)
    }
    
    /// Get historical price data for a token
    pub fn get_price_history(&self, token_id: &str, limit: usize) -> Result<Vec<(f64, f64, DateTime<Utc>)>> {
        self.db.get_price_history(token_id, limit)
    }
} 