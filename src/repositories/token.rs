use crate::db::Database;
use crate::error::{Result, Error};
use crate::types::token::TokenData;
use super::Repository;
use chrono::Utc;

#[derive(Clone)]
pub struct TokenRepository {
    db: Database,
}

impl TokenRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Get price history and token data
    pub fn get_token_history(&self, token_id: &str, limit: usize) -> Result<Option<(TokenData, Vec<(f64, f64, chrono::DateTime<Utc>)>)>> {
        // Get token data first
        let token_data = match self.get_token_price_stats(token_id) {
            Ok(data) => data,
            Err(_) => {
                // If not found or error, create a minimal TokenData object
                TokenData::new(token_id, token_id, token_id, 0.0)
            }
        };
        
        // Get price history
        match self.db.get_price_history(token_id, limit) {
            Ok(history) if !history.is_empty() => Ok(Some((token_data, history))),
            Ok(_) => Ok(None), // No price history found
            Err(e) => Err(e)
        }
    }

    /// Get latest market data for all tokens
    pub fn get_latest_market_data(&self) -> Result<Vec<TokenData>> {
        self.db.get_latest_market_data()
    }

    /// Calculate price change statistics for a token
    pub fn get_token_price_stats(&self, token_id: &str) -> Result<TokenData> {
        self.db.get_token_price_stats(token_id)
    }

    /// Store token metadata
    pub fn update_token_metadata(&self, token_id: &str, symbol: &str) -> Result<()> {
        self.db.update_token_metadata(token_id, symbol)
    }
    
    /// Get the underlying database connection
    pub fn get_db(&self) -> Database {
        self.db.clone()
    }

    /// Store trade execution data
    pub fn store_trade_execution(&self, token_id: &str, price: f64, size: f64, is_buy: bool, timestamp: chrono::DateTime<Utc>) -> Result<()> {
        log::info!("STORING TRADE EXECUTION: token: {}, price: ${:.4}, size: ${:.2}, is_buy: {}, time: {}", 
            token_id, price, size, is_buy, timestamp);
        
        // Store the price data
        self.db.store_price_data(token_id, price, size)?;
        
        // If this is a buy order, create a position
        if is_buy {
            // Create a minimal position object
            let position = crate::trading::strategy::Position {
                token_id: token_id.to_string(),
                coingecko_id: token_id.to_string(), // Using token_id as coingecko_id
                entry_price: price,
                current_price: price,
                highest_price: price,
                size,
                unrealized_pnl: 0.0,
                entry_time: timestamp,
            };
            
            // Record the position opening - explicitly using true for paper trading
            let position_id = self.db.record_position_open(&position, true)?;
            log::info!("Created new paper position for {} at ${:.4} with size ${:.2}, ID: {}", 
                token_id, price, size, position_id);
        } else {
            log::info!("Not creating position since is_buy=false (sell order)");
        }
        
        Ok(())
    }

    /// Store position update data
    pub fn store_position_update(&self, token_id: &str, price: f64, pnl: f64, timestamp: chrono::DateTime<Utc>) -> Result<()> {
        // For now, we'll just record the price update in the price history
        // In a real implementation, this would store position-specific data
        self.db.store_price_data(token_id, price, 0.0)
    }

    /// Get count of tokens in the database
    pub fn get_token_count(&self) -> Result<usize> {
        // Get all tokens and count them
        let tokens = self.get_latest_market_data()?;
        Ok(tokens.len())
    }
    
    /// Get count of trades in the database
    pub fn get_trade_count(&self) -> Result<usize> {
        // Get the trading history with a large limit and count them
        let paper_trades = self.db.get_trading_history(true, 1000)?;
        let live_trades = self.db.get_trading_history(false, 1000)?;
        
        // Return the total count
        Ok(paper_trades.len() + live_trades.len())
    }
}

impl Repository<TokenData, String> for TokenRepository {
    fn find_by_id(&self, id: String) -> Result<Option<TokenData>> {
        match self.get_token_price_stats(&id) {
            Ok(token) => Ok(Some(token)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn find_all(&self) -> Result<Vec<TokenData>> {
        self.get_latest_market_data()
    }

    fn save(&self, entity: &TokenData) -> Result<String> {
        self.update_token_metadata(&entity.id, &entity.symbol)?;
        Ok(entity.id.clone())
    }

    fn delete(&self, id: String) -> Result<()> {
        // No actual deletion for tokens, just log a warning
        log::warn!("Token deletion not supported: {}", id);
        Ok(())
    }
} 