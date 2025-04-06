use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::fs;
use log::{info, debug, warn, error};
use crate::trading::strategy::Position;
use chrono::{DateTime, Utc};
use crate::error::{Error, Result, ErrorExt};
use chrono::Duration as ChronoDuration;
use crate::config::Config;

// Add transaction module
pub mod transaction;

/// Database interface for storing trading data
#[derive(Clone)]
pub struct Database {
    path: PathBuf,
    query_logging: bool,
}

impl Database {
    /// Create a new database connection
    pub fn new() -> Result<Self> {
        // Load configuration
        let config = Config::load()?;
        let db_path = config.db_path()?;
        
        // Ensure directory exists
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::Io(format!("Failed to create database directory: {}", e)))?;
        }
        
        // Initialize the database
        let conn = Connection::open(&db_path)
            .map_err(|e| Error::Database(e))?;
            
        Self::init_db(&conn)
            .map_err(|e| Error::Database(e))?;
        
        info!("Database initialized at: {:?}", db_path);
        Ok(Database { 
            path: db_path,
            query_logging: config.database.query_logging,
        })
    }
    
    /// Get the connection for thread-safe operations
    fn get_connection(&self) -> Result<Connection> {
        debug!("Opening database connection to {:?}", self.path);
        
        // Start timing the connection
        let start = std::time::Instant::now();
        
        let mut conn = match Connection::open_with_flags(&self.path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | 
                                                                rusqlite::OpenFlags::SQLITE_OPEN_CREATE) {
            Ok(conn) => {
                let elapsed = start.elapsed();
                debug!("Database connection opened successfully in {:?}", elapsed);
                conn
            },
            Err(e) => {
                error!("Failed to open database connection to {:?}: {}", self.path, e);
                return Err(Error::Database(e)).log_error("Failed to open database connection");
            }
        };
            
        // Enable foreign keys
        if let Err(e) = conn.execute("PRAGMA foreign_keys = ON", []) {
            warn!("Failed to enable foreign keys: {}", e);
        }
        
        // Set busy timeout to avoid "database locked" errors
        if let Err(e) = conn.busy_timeout(std::time::Duration::from_secs(5)) {
            warn!("Failed to set busy timeout: {}", e);
        }
        
        // Enable query logging if configured
        if self.query_logging {
            conn.trace(Some(|sql| {
                debug!("SQL: {}", sql);
            }));
        }
        
        Ok(conn)
    }

    /// Initialize the database schema
    fn init_db(conn: &Connection) -> rusqlite::Result<()> {
        // Create trades table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS paper_trades (
                id INTEGER PRIMARY KEY,
                token_id TEXT NOT NULL,
                symbol TEXT NOT NULL,
                entry_price REAL NOT NULL,
                exit_price REAL NOT NULL,
                size REAL NOT NULL,
                pnl REAL NOT NULL,
                entry_time TEXT NOT NULL,
                exit_time TEXT NOT NULL,
                strategy TEXT NOT NULL
            )",
            [],
        )?;
        
        // Create live trades table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS live_trades (
                id INTEGER PRIMARY KEY,
                token_id TEXT NOT NULL,
                symbol TEXT NOT NULL,
                entry_price REAL NOT NULL,
                exit_price REAL NOT NULL,
                size REAL NOT NULL,
                pnl REAL NOT NULL,
                entry_time TEXT NOT NULL,
                exit_time TEXT NOT NULL,
                strategy TEXT NOT NULL
            )",
            [],
        )?;
        
        // Create paper positions table for currently open positions
        conn.execute(
            "CREATE TABLE IF NOT EXISTS paper_positions (
                id INTEGER PRIMARY KEY,
                token_id TEXT NOT NULL,
                coingecko_id TEXT NOT NULL,
                entry_price REAL NOT NULL,
                size REAL NOT NULL,
                entry_time TEXT NOT NULL
            )",
            [],
        )?;
        
        // Create live positions table for currently open positions
        conn.execute(
            "CREATE TABLE IF NOT EXISTS live_positions (
                id INTEGER PRIMARY KEY,
                token_id TEXT NOT NULL,
                coingecko_id TEXT NOT NULL,
                entry_price REAL NOT NULL,
                size REAL NOT NULL,
                entry_time TEXT NOT NULL
            )",
            [],
        )?;
        
        // Create price history table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS price_history (
                id INTEGER PRIMARY KEY,
                token_id TEXT NOT NULL,
                price REAL NOT NULL,
                volume REAL NOT NULL,
                timestamp TEXT NOT NULL
            )",
            [],
        )?;
        
        // Create tokens table for token metadata
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tokens (
                id TEXT PRIMARY KEY,
                symbol TEXT NOT NULL,
                name TEXT NOT NULL,
                last_updated TEXT NOT NULL
            )",
            [],
        )?;
        
        Ok(())
    }
    
    /// Record a position opening
    pub fn record_position_open(&self, position: &Position, is_paper_trade: bool) -> Result<i64> {
        info!("Recording position open for {} at ${:.4} with size ${:.2}", 
             position.token_id, position.entry_price, position.size);
        
        let mut conn = self.get_connection()?;
        
        // Use the transaction helper for better error handling and retries
        let result = transaction::with_transaction(&mut conn, |tx| {
            let query = format!("
                INSERT INTO {}_positions (
                    token_id, 
                    coingecko_id,
                    entry_price, 
                    size,
                    entry_time
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5
                )",
                if is_paper_trade { "paper" } else { "live" }
            );
            
            debug!("Executing position open SQL");
            let entry_time_str = position.entry_time.to_rfc3339();
            
            tx.execute(
                &query,
                params![
                    position.token_id,
                    position.coingecko_id,
                    position.entry_price,
                    position.size,
                    entry_time_str
                ],
            ).map_err(|e| {
                error!("Database error recording position open: {}", e);
                Error::Database(e)
            })?;
            
            let position_id = tx.last_insert_rowid();
            info!("Recorded position opening for {} with ID {}", position.token_id, position_id);
            
            Ok(position_id)
        });
        
        match result {
            Ok(id) => {
                info!("Successfully recorded position open with ID {}", id);
                Ok(id)
            },
            Err(e) => {
                error!("Failed to record position open: {}", e);
                Err(e)
            }
        }
    }
    
    /// Update token metadata in the tokens table
    pub fn update_token_metadata(&self, token_id: &str, symbol: &str) -> Result<()> {
        let conn = self.get_connection()?;
        let now = Utc::now().to_rfc3339();
        
        // Check if token exists
        let count: i64 = {
            let mut stmt = conn.prepare("SELECT COUNT(*) FROM tokens WHERE id = ?")
                .map_err(Error::from)?;
                
            stmt.query_row([token_id], |row| row.get(0))
                .map_err(Error::from)?
        };
            
        if count == 0 {
            // Insert new token record
            conn.execute(
                "INSERT INTO tokens (id, symbol, name, last_updated) VALUES (?, ?, ?, ?)",
                params![token_id, symbol, symbol, now]
            )
            .map_err(Error::from)?;
        } else {
            // Update last_updated
            conn.execute(
                "UPDATE tokens SET last_updated = ? WHERE id = ?",
                params![now, token_id]
            )
            .map_err(Error::from)?;
        }
        
        Ok(())
    }
    
    /// Record a trade when closing a position
    pub fn record_position_close(&self, position: &Position, profit_loss: f64, profit_loss_pct: f64, is_paper_trade: bool) -> Result<i64> {
        info!("Recording position close for {} at ${:.4} with P/L: ${:.2} ({:.2}%)", 
             position.token_id, position.current_price, profit_loss, profit_loss_pct);
        
        let conn = self.get_connection()?;
        
        // First delete the open position
        match self.delete_open_position(&position.token_id, is_paper_trade) {
            Ok(_) => debug!("Successfully deleted open position for {}", position.token_id),
            Err(e) => {
                error!("Failed to delete open position: {}", e);
                // Continue anyway to record the trade
            }
        }
        
        // Then record the trade with its profit/loss
        let query = format!("
            INSERT INTO {}_trades (
                token_id, 
                symbol, 
                entry_price, 
                exit_price, 
                size, 
                pnl, 
                entry_time, 
                exit_time,
                strategy
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9
            )",
            if is_paper_trade { "paper" } else { "live" }
        );
        
        debug!("Executing trade insert SQL: {}", query);
        
        let now = chrono::Utc::now();
        let entry_time_str = position.entry_time.to_rfc3339();
        let exit_time_str = now.to_rfc3339();
        
        let result = conn.execute(
            &query,
            params![
                position.token_id,
                // Use token_id as symbol since Position doesn't have symbol field
                position.token_id, 
                position.entry_price,
                position.current_price,
                position.size,
                profit_loss,                 // Use the provided profit_loss value
                entry_time_str,
                exit_time_str,
                "momentum" // Default strategy name since Position doesn't have strategy field
            ],
        );
        
        match result {
            Ok(rows) => {
                let trade_id = conn.last_insert_rowid();
                info!("Successfully recorded trade close with ID {} (affected {} rows)", trade_id, rows);
                Ok(trade_id)
            },
            Err(e) => {
                error!("Failed to record trade: {}", e);
                Err(Error::Database(e))
            }
        }
    }
    
    /// Update an existing position
    pub fn update_position(&self, position: &Position, is_paper_trade: bool) -> Result<()> {
        let conn = self.get_connection()?;
        let entry_time = position.entry_time.to_rfc3339();
        let table_name = if is_paper_trade { "paper_positions" } else { "live_positions" };
        
        let query = format!(
            "UPDATE {} SET current_price = ?1, unrealized_pnl = ?2
             WHERE token_id = ?3 AND entry_time = ?4",
            table_name
        );
        
        conn.execute(
            &query,
            params![
                position.current_price,
                position.unrealized_pnl,
                position.token_id,
                entry_time
            ],
        )
        .map_err(Error::from)?;
        
        Ok(())
    }
    
    /// Get all open positions
    pub fn get_open_positions(&self, is_paper_trade: bool) -> Result<Vec<Position>> {
        let conn = self.get_connection()?;
        let table_name = if is_paper_trade { "paper_positions" } else { "live_positions" };
        
        let query = format!(
            "SELECT token_id, coingecko_id, entry_price, size, entry_time
             FROM {}",
            table_name
        );
        
        let mut stmt = conn.prepare(&query)
            .map_err(Error::from)?;
        
        let position_iter = stmt.query_map([], |row| {
            let entry_time_str: String = row.get(4)?;
            let entry_time = DateTime::parse_from_rfc3339(&entry_time_str)
                .map_err(|e| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(1),
                        Some(format!("Invalid datetime format: {}", entry_time_str))
                    )
                })?
                .with_timezone(&Utc);
            
            let position = Position {
                token_id: row.get(0)?,
                coingecko_id: row.get(1)?,
                entry_price: row.get(2)?,
                current_price: 0.0,  // Default value
                highest_price: 0.0,  // Default value
                size: row.get(3)?,
                unrealized_pnl: 0.0, // Default value
                entry_time,
            };
            
            Ok(position)
        })
        .map_err(Error::from)?;
        
        let mut positions = Vec::new();
        for position in position_iter {
            positions.push(position.map_err(Error::from)?);
        }
        
        Ok(positions)
    }
    
    /// Get trading history (closed trades)
    pub fn get_trading_history(&self, is_paper_trade: bool, limit: usize) -> Result<Vec<CompletedTrade>> {
        let conn = self.get_connection()?;
        let table_name = if is_paper_trade { "paper_trades" } else { "live_trades" };
        
        let query = format!(
            "SELECT token_id, entry_price, exit_price, size, pnl, entry_time, exit_time
             FROM {}
             ORDER BY exit_time DESC
             LIMIT ?1",
            table_name
        );
        
        let mut stmt = conn.prepare(&query)
            .map_err(Error::from)?;
        
        let trade_iter = stmt.query_map(params![limit as i64], |row| {
            let entry_time_str: String = row.get(5)?;
            let exit_time_str: String = row.get(6)?;
            
            let entry_time = DateTime::parse_from_rfc3339(&entry_time_str)
                .map_err(|e| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(1),
                        Some(format!("Invalid datetime format: {}", entry_time_str))
                    )
                })?
                .with_timezone(&Utc);
                
            let exit_time = DateTime::parse_from_rfc3339(&exit_time_str)
                .map_err(|e| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(1),
                        Some(format!("Invalid datetime format: {}", exit_time_str))
                    )
                })?
                .with_timezone(&Utc);
            
            Ok(CompletedTrade {
                token_id: row.get(0)?,
                entry_price: row.get(1)?,
                exit_price: row.get(2)?,
                size: row.get(3)?,
                pnl: row.get(4)?,
                entry_time,
                exit_time,
            })
        })
        .map_err(Error::from)?;
        
        let mut trades = Vec::new();
        for trade in trade_iter {
            trades.push(trade.map_err(Error::from)?);
        }
        
        Ok(trades)
    }
    
    /// Get trading performance statistics
    pub fn get_performance_stats(&self, is_paper_trade: bool) -> Result<TradingStats> {
        let conn = self.get_connection()?;
        let table_name = if is_paper_trade { "paper_trades" } else { "live_trades" };
        
        let query = format!(
            "SELECT 
                COUNT(*) as total_trades,
                SUM(CASE WHEN pnl > 0 THEN 1 ELSE 0 END) as winning_trades,
                SUM(CASE WHEN pnl < 0 THEN 1 ELSE 0 END) as losing_trades,
                SUM(pnl) as total_pnl,
                AVG(pnl) as avg_pnl,
                MAX(pnl) as max_profit,
                MIN(pnl) as max_loss
             FROM {}",
            table_name
        );
        
        let mut stmt = conn.prepare(&query)
            .map_err(Error::from)?;
        
        let stats = stmt.query_row([], |row| {
            let total_trades: i64 = row.get(0)?;
            let winning_trades: i64 = row.get(1)?;
            let losing_trades: i64 = row.get(2)?;
            
            let win_rate = if total_trades > 0 {
                (winning_trades as f64 / total_trades as f64) * 100.0
            } else {
                0.0
            };
            
            Ok(TradingStats {
                total_trades: total_trades as usize,
                winning_trades: winning_trades as usize,
                losing_trades: losing_trades as usize,
                win_rate,
                total_pnl: row.get(3)?,
                avg_pnl: row.get(4)?,
                max_profit: row.get(5)?,
                max_loss: row.get(6)?,
            })
        })
        .map_err(Error::from)?;
        
        Ok(stats)
    }
    
    /// Store a price data point
    pub fn store_price_data(&self, token_id: &str, price: f64, volume: f64) -> Result<()> {
        let conn = self.get_connection()?;
        let timestamp = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO price_history (token_id, price, volume, timestamp) 
             VALUES (?1, ?2, ?3, ?4)",
            params![token_id, price, volume, timestamp],
        )
        .map_err(Error::from)?;
        
        Ok(())
    }
    
    /// Get historical price data
    pub fn get_price_history(&self, token_id: &str, limit: usize) -> Result<Vec<(f64, f64, DateTime<Utc>)>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT price, volume, timestamp 
             FROM price_history 
             WHERE token_id = ?1 
             ORDER BY timestamp DESC 
             LIMIT ?2"
        )
        .map_err(Error::from)?;
        
        let rows = stmt.query_map([token_id, &limit.to_string()], |row| {
            let timestamp_str: String = row.get(2)?;
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map_err(|_| {
                    rusqlite::Error::SqliteFailure(
                        rusqlite::ffi::Error::new(1),
                        Some(format!("Invalid datetime format: {}", timestamp_str))
                    )
                })?
                .with_timezone(&Utc);
                
            Ok((row.get(0)?, row.get(1)?, timestamp))
        })
        .map_err(Error::from)?;
        
        let mut history = Vec::new();
        for row in rows {
            history.push(row.map_err(Error::from)?);
        }
        
        Ok(history)
    }
    
    /// Get latest market data for all tokens
    pub fn get_latest_market_data(&self) -> Result<Vec<crate::types::token::TokenData>> {
        let conn = self.get_connection()?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT 
                    p.token_id,
                    COALESCE(t.symbol, p.token_id) as symbol,
                    COALESCE(t.name, p.token_id) as name,
                    p.price,
                    0.0 as price_change_24h,  -- Placeholder until we calculate this
                    p.volume
             FROM price_history p
             LEFT JOIN tokens t ON p.token_id = t.id
             WHERE p.timestamp IN (
                 SELECT MAX(timestamp) 
                 FROM price_history 
                 GROUP BY token_id
             )
             ORDER BY p.volume DESC"
        )
        .map_err(Error::from)?;

        let tokens_result = stmt.query_map([], |row| {
            // First create a db::TokenMetrics struct
            let db_metrics = TokenMetrics {
                id: row.get(0)?,
                symbol: row.get(1)?,
                name: row.get(2)?,
                price_usd: row.get(3)?,
                price_change_24h: row.get(4)?,
                volume_24h: row.get(5)?,
            };
            
            // Convert to our canonical TokenData model
            Ok(crate::types::token::TokenData::from(&db_metrics))
        })
        .map_err(Error::from)?;

        // Convert to Vec and handle potential errors
        let mut tokens = Vec::new();
        for token in tokens_result {
            tokens.push(token.map_err(Error::from)?);
        }

        Ok(tokens)
    }

    /// Delete an open position
    pub fn delete_open_position(&self, token_id: &str, is_paper_trade: bool) -> Result<()> {
        let conn = self.get_connection()?;
        let table_name = if is_paper_trade { "paper_positions" } else { "live_positions" };
        
        // Format the query with the correct table name
        let query = format!(
            "DELETE FROM {} WHERE token_id = ?1",
            table_name
        );
        
        conn.execute(
            &query,
            params![token_id],
        )
        .map_err(Error::from)?;
        
        Ok(())
    }

    /// Calculate price change statistics for a token
    pub fn get_token_price_stats(&self, token_id: &str) -> Result<crate::types::token::TokenData> {
        let conn = self.get_connection()?;
        
        // Get the latest price data point
        let mut latest_stmt = conn.prepare(
            "SELECT price, volume, timestamp 
             FROM price_history 
             WHERE token_id = ? 
             ORDER BY timestamp DESC 
             LIMIT 1"
        )
        .map_err(Error::from)?;
        
        let latest = latest_stmt.query_row([token_id], |row| {
            let price: f64 = row.get(0)?;
            let volume: f64 = row.get(1)?;
            let timestamp_str: String = row.get(2)?;
            
            Ok((price, volume, timestamp_str))
        })
        .map_err(Error::from)
        .log_error(&format!("Failed to get latest price data for token {}", token_id))?;
        
        // Get price 24 hours ago
        let now = Utc::now();
        let yesterday = now - ChronoDuration::days(1);
        let yesterday_str = yesterday.to_rfc3339();
        
        let mut prev_stmt = conn.prepare(
            "SELECT price 
             FROM price_history 
             WHERE token_id = ? AND timestamp <= ? 
             ORDER BY timestamp DESC 
             LIMIT 1"
        )
        .map_err(Error::from)?;
        
        let prev_price = prev_stmt.query_row([token_id, &yesterday_str], |row| {
            let price: f64 = row.get(0)?;
            Ok(price)
        })
        .map_err(|_| Error::NotFound(format!("No historical price data found for token {}", token_id)))
        .log_and_default(&format!("No historical price data for token {}, using current price", token_id), latest.0);
        
        // Calculate price change
        let price_change_pct = if prev_price > 0.0 {
            ((latest.0 - prev_price) / prev_price) * 100.0
        } else {
            0.0
        };
        
        // Get token name and symbol if we have it in the tokens table
        // This is optional as we might not have this info for all tokens
        let token_info = conn.prepare(
            "SELECT symbol, name FROM tokens WHERE id = ?"
        )
        .map_err(Error::from)
        .and_then(|mut stmt| {
            stmt.query_row([token_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(Error::from)
        })
        .log_and_default(&format!("No token metadata found for {}", token_id), (token_id.to_string(), token_id.to_string()));
        
        // Create TokenData with the calculated statistics
        let mut token_data = crate::types::token::TokenData::new(
            token_id,
            &token_info.0,
            &token_info.1,
            latest.0
        );
        
        token_data.price_change_24h = price_change_pct;
        token_data.volume_24h = latest.1;
        token_data.last_updated = Some(Utc::now());
        
        Ok(token_data)
    }
}

/// Struct to represent a completed trade
#[derive(Debug, Clone)]
pub struct CompletedTrade {
    pub token_id: String,
    pub entry_price: f64,
    pub exit_price: f64,
    pub size: f64,
    pub pnl: f64,
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
}

/// Struct to represent trading statistics
#[derive(Debug, Clone)]
pub struct TradingStats {
    pub total_trades: usize,
    pub winning_trades: usize,
    pub losing_trades: usize,
    pub win_rate: f64,
    pub total_pnl: f64,
    pub avg_pnl: f64,
    pub max_profit: f64,
    pub max_loss: f64,
}

/// DB-specific token metrics struct
#[derive(Debug, Clone)]
pub struct TokenMetrics {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub price_usd: f64,
    pub price_change_24h: f64,
    pub volume_24h: f64,
} 