use crate::infrastructure::database::queries;
use crate::infrastructure::database::Database;
use crate::infrastructure::errors::{Error, Result};
use chrono::{DateTime, Utc};
use log::{debug, error, info};
use tokio_postgres::types::ToSql;

/// Detailed trade record from the database
#[derive(Debug, Clone)]
pub struct Trade {
    pub id: i64,
    pub token_id: String,
    pub price: f64,
    pub size: f64,
    pub timestamp: DateTime<Utc>,
    pub is_buy: bool,
    pub is_paper: bool,
    pub position_id: Option<i64>,
}

#[derive(Clone)]
pub struct TradeRepository {
    db: Database,
    is_paper_trade: bool,
}

impl TradeRepository {
    pub fn new(db: Database, is_paper_trade: bool) -> Self {
        Self { db, is_paper_trade }
    }

    /// Get the underlying database instance
    pub fn get_db(&self) -> &Database {
        &self.db
    }

    /// Get recent trading history with limit (async)
    /// Fetches individual trades, not completed positions.
    pub async fn get_trading_history(&self, limit: usize) -> Result<Vec<Trade>> {
        info!(
            "Fetching raw trade history (limit: {}, paper: {})",
            limit, self.is_paper_trade
        );
        let client = self.db.get_connection().await?;

        let rows = client
            .query(
                queries::trade::GET_ALL_TRADES_HISTORY, // Use the new query
                &[
                    &self.is_paper_trade as &(dyn ToSql + Sync),
                    &(limit as i64) as &(dyn ToSql + Sync), // Cast limit to i64 for query
                ],
            )
            .await
            .map_err(|e| {
                error!("Failed to execute GET_ALL_TRADES_HISTORY query: {}", e);
                Error::Database(e.to_string())
            })?;

        let trades: Vec<Trade> = rows
            .iter()
            .map(|row| Trade {
                id: row.get(0),
                token_id: row.get(1),
                price: row.get(2),
                size: row.get(3),
                is_buy: row.get(4),
                timestamp: row.get(5),
                is_paper: row.get(6),
                position_id: row.get(7),
            })
            .collect();

        info!("Fetched {} individual trade records", trades.len());
        Ok(trades)
    }

    /// Get trades for a specific token (async)
    pub async fn get_trades_by_token(&self, token_id: &str, limit: usize) -> Result<Vec<Trade>> {
        debug!("Getting trades for token: {}", token_id);
        let client = self.db.get_connection().await?;

        let rows = client
            .query(
                queries::trade::GET_TRADES_BY_TOKEN,
                &[&token_id as &(dyn ToSql + Sync), &(limit as i64)],
            )
            .await
            .map_err(|e| {
                error!("Failed to execute GET_TRADES_BY_TOKEN query: {}", e);
                Error::Database(e.to_string())
            })?;

        let trades = rows
            .iter()
            .map(|row| Trade {
                id: row.get(0),
                token_id: row.get(1),
                price: row.get(2),
                size: row.get(3),
                is_buy: row.get(4),
                timestamp: row.get(5), // Directly get TIMESTAMPTZ
                is_paper: row.get(6),
                position_id: row.get(7),
            })
            .collect();

        Ok(trades)
    }

    /// Record a trade linked to a position (async)
    pub async fn record_trade_with_position(
        &self,
        token_id: &str,
        provider_id: &str,
        price: f64,
        size: f64,
        is_buy: bool,
        position_id: i64,
    ) -> Result<i64> {
        let client = self.db.get_connection().await?;
        let now = Utc::now();

        let row = client
            .query_one(
                queries::trade::INSERT_TRADE_WITH_POSITION_ID,
                &[
                    &token_id as &(dyn ToSql + Sync),
                    &provider_id,
                    &price,
                    &size,
                    &now,
                    &is_buy,
                    &self.is_paper_trade,
                    &position_id,
                ],
            )
            .await
            .map_err(|e| {
                error!(
                    "Failed to execute INSERT_TRADE_WITH_POSITION_ID query: {}",
                    e
                );
                Error::Database(e.to_string())
            })?;

        let trade_id: i64 = row.get(0);
        info!(
            "Recorded trade ID {} linked to position {}",
            trade_id, position_id
        );
        Ok(trade_id)
    }

    /// Record a standalone trade (async)
    /// Assumes provider_id exists in the table now (based on updated query)
    pub async fn record_trade(
        &self,
        token_id: &str,
        provider_id: &str,
        price: f64,
        size: f64,
        is_buy: bool,
        timestamp: DateTime<Utc>,
    ) -> Result<i64> {
        let client = self.db.get_connection().await?;

        let row = client
            .query_one(
                queries::trade::INSERT_TRADE,
                &[
                    &token_id as &(dyn ToSql + Sync),
                    &provider_id,
                    &price,
                    &size,
                    &is_buy,
                    &timestamp,
                    &self.is_paper_trade,
                ],
            )
            .await
            .map_err(|e| {
                error!("Failed to execute INSERT_TRADE query: {}", e);
                Error::Database(e.to_string())
            })?;

        let trade_id: i64 = row.get(0);
        info!("Recorded standalone trade ID {}", trade_id);
        Ok(trade_id)
    }
}
