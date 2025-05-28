use crate::core::error::{Error, Result};
use crate::domain::trading::strategy::Position as StrategyPosition;
// use crate::infra::db::database::Client as PgClient; // Removed unused
use crate::infra::db::queries;
use crate::infra::db::Database;
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn}; // Removed unused trace
                                     // use std::sync::Arc; // Removed unused Arc
                                     // use tokio_postgres::error::SqlState; // Removed unused SqlState
use tokio_postgres::types::ToSql;

/// Represents a completed (closed) position with profit/loss calculation
#[derive(Debug, Clone)]
pub struct CompletedPosition {
    pub id: i64,
    pub token_id: String,
    pub size: f64,
    pub entry_price: f64,
    pub exit_price: f64,
    pub entry_time: chrono::DateTime<Utc>,
    pub exit_time: chrono::DateTime<Utc>,
    pub profit: f64,
    pub roi: f64,
    pub fees: f64,
    pub net_profit: f64,
}

#[derive(Debug)]
pub enum PositionError {
    Locked(String),
    NotFound(String),
    InvalidData(String),
}

impl From<PositionError> for Error {
    fn from(err: PositionError) -> Self {
        match err {
            PositionError::Locked(token_id) => Error::PositionLocked(token_id),
            PositionError::NotFound(token_id) => {
                Error::NotFound(format!("Position for token {} not found", token_id))
            }
            PositionError::InvalidData(msg) => Error::InvalidInput(msg),
        }
    }
}

#[derive(Debug)]
pub struct RecordCloseArgs<'a> {
    pub token_id: &'a str,
    pub exit_price: f64,
    pub size: f64,
    pub entry_price: f64,
    pub entry_time: chrono::DateTime<Utc>,
    pub exit_time: chrono::DateTime<Utc>,
}

#[derive(Clone)]
pub struct PositionRepository {
    db: Database,
    is_paper_trade: bool,
}

impl PositionRepository {
    pub fn new(db: Database, is_paper_trade: bool) -> Self {
        Self { db, is_paper_trade }
    }

    /// Get a reference to the database pool wrapper
    pub fn get_database(&self) -> Database {
        self.db.clone()
    }

    /// Get all open positions (async)
    pub async fn get_open_positions(&self) -> Result<Vec<StrategyPosition>> {
        debug!("Getting open positions (paper: {})", self.is_paper_trade);
        let client = self.db.get_connection().await?;

        let rows = client
            .query(
                queries::position::GET_OPEN_POSITIONS,
                &[&self.is_paper_trade as &(dyn ToSql + Sync)],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut positions = Vec::with_capacity(rows.len());
        for row in rows {
            let entry_time: DateTime<Utc> = row.get(7);
            positions.push(StrategyPosition {
                token_id: row.get(1),
                provider_id: row.get(2),
                entry_price: row.get(3),
                current_price: row.get(4),
                highest_price: row.get(5),
                size: row.get(6),
                entry_time,
                unrealized_pnl: row.get(9),
            });
        }

        info!("Found {} open positions", positions.len());
        Ok(positions)
    }

    /// Record a new position opening with its associated trade (async)
    pub async fn record_position_with_trade(
        &self,
        position: &StrategyPosition,
        price: f64,
        size: f64,
        timestamp: chrono::DateTime<Utc>,
    ) -> Result<i64> {
        if size <= 0.0 || price <= 0.0 {
            return Err(PositionError::InvalidData(format!(
                "Invalid position data: size={}, price={}",
                size, price
            ))
            .into());
        }

        let mut client = self.db.get_connection().await?;
        let transaction = client
            .transaction()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let position_id: i64;
        let new_position_created: bool;

        let position_params_for_insert: [&(dyn ToSql + Sync); 9] = [
            &position.token_id,
            &position.provider_id,
            &position.entry_price,
            &position.current_price,
            &position.highest_price,
            &position.size,
            &position.entry_time,
            &self.is_paper_trade,
            &position.unrealized_pnl,
        ];

        // Attempt to insert a new position. If it conflicts with an existing open position, DO NOTHING.
        let maybe_row = transaction
            .query_opt(
                queries::position::INSERT_POSITION_ON_CONFLICT_DO_NOTHING,
                &position_params_for_insert,
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        if let Some(row) = maybe_row {
            // New position was inserted
            position_id = row.get(0);
            new_position_created = true;
            info!(
                "✅ New position created for token {} with ID: {}",
                position.token_id, position_id
            );
        } else {
            // Conflict occurred (existing open position found), DO NOTHING was executed.
            // Fetch the ID of the existing open position.
            new_position_created = false;
            info!(
                "Existing open position found for token {}. No changes made to core position details.",
                position.token_id
            );

            let existing_pos_row = transaction
                .query_one(
                    // Re-use GET_POSITION_BY_TOKEN_ID, which also selects the ID as the first column.
                    queries::position::GET_POSITION_BY_TOKEN_ID,
                    &[
                        &position.token_id as &(dyn ToSql + Sync),
                        &self.is_paper_trade,
                    ],
                )
                .await
                .map_err(|e| {
                    error!(
                        "Failed to fetch existing position ID for {}: {}",
                        position.token_id, e
                    );
                    Error::Database(e.to_string())
                })?;
            position_id = existing_pos_row.get(0); // ID is the first column
            info!(
                "Fetched ID for existing open position {}: {}",
                position.token_id, position_id
            );
        }

        // Only insert a trade if a new position was created
        if new_position_created {
            transaction
                .execute(
                    queries::trade::INSERT_TRADE_WITH_POSITION_ID,
                    &[
                        &position.token_id,
                        &position.provider_id,
                        &price,     // Trade price
                        &size,      // Trade size
                        &timestamp, // Trade timestamp
                        &true,      // is_buy
                        &self.is_paper_trade,
                        &position_id,
                    ],
                )
                .await
                .map_err(|e| Error::Database(e.to_string()))?;

            info!(
                "✅ Recorded BUY trade for new position {} at ${:.4} with size ${:.2}, linked to position ID {}",
                position.token_id, price, size, position_id
            );
        } else {
            info!(
                "Skipped recording new trade for token {} as an existing open position was found (ID: {}).",
                position.token_id, position_id
            );
        }

        transaction
            .commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(position_id)
    }

    /// Record a position closure with its associated trade (async)
    pub async fn record_position_close_with_trade(
        &self,
        position_id: i64,
        args: RecordCloseArgs<'_>,
    ) -> Result<CompletedPosition> {
        if args.size <= 0.0 || args.exit_price <= 0.0 {
            return Err(PositionError::InvalidData(format!(
                "Invalid position data: size={}, exit_price={}",
                args.size, args.exit_price
            ))
            .into());
        }

        // Calculations remain the same
        let profit = args.size * (args.exit_price - args.entry_price);
        let roi = if args.entry_price == 0.0 {
            0.0
        } else {
            (args.exit_price - args.entry_price) / args.entry_price * 100.0
        };
        let fee_rate = 0.001; // TODO: Get from config
        let entry_fee = args.size * args.entry_price * fee_rate;
        let exit_fee = args.size * args.exit_price * fee_rate;
        let total_fee = entry_fee + exit_fee;
        let net_profit = profit - total_fee;

        let mut client = self.db.get_connection().await?;
        let transaction = client
            .transaction()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        // First get the provider_id from the position
        let position_row = transaction
            .query_one(
                "SELECT provider_id FROM positions WHERE id = $1 AND is_paper = $2",
                &[&position_id, &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        let provider_id: String = position_row.get(0);

        // Insert the SELL trade first, get its ID
        let trade_id: i64;
        let trade_row = transaction
            .query_one(
                queries::trade::INSERT_TRADE_SELL,
                &[
                    &args.token_id as &(dyn ToSql + Sync),
                    &provider_id,
                    &args.exit_price,
                    &args.size,
                    &args.exit_time,
                    &self.is_paper_trade,
                    &position_id,
                ],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        trade_id = trade_row.get(0);

        // Update the position as closed, linking the sell trade ID
        let rows_affected = transaction
            .execute(
                queries::position::CLOSE_POSITION,
                &[
                    &args.exit_price as &(dyn ToSql + Sync),
                    &profit, // Use calculated profit
                    &args.exit_time,
                    &trade_id,            // Link the sell trade ID
                    &position_id,         // WHERE condition
                    &self.is_paper_trade, // WHERE condition
                ],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        if rows_affected == 0 {
            error!("❌ Failed to update position ID {} as closed (maybe already closed or wrong paper type?)", position_id);
            let _ = transaction.rollback().await; // Attempt rollback
            return Err(Error::NotFound(format!(
                "Open position {} not found for update",
                position_id
            )));
        }

        // Commit the transaction
        transaction
            .commit()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        info!("✅ Closed position ID {} for {} with size ${:.2}, entry ${:.4}, exit ${:.4}, profit ${:.2}", 
                position_id, args.token_id, args.size, args.entry_price, args.exit_price, profit);

        // Construct and return CompletedPosition
        let completed_position = CompletedPosition {
            id: position_id,
            token_id: args.token_id.to_string(),
            size: args.size,
            entry_price: args.entry_price,
            exit_price: args.exit_price,
            entry_time: args.entry_time,
            exit_time: args.exit_time,
            profit,
            roi,
            fees: total_fee,
            net_profit,
        };
        Ok(completed_position)
    }

    /// Update an existing position with new price/PnL data (async)
    pub async fn update_position(
        &self,
        token_id: &str,
        price: f64,
        _highest_price: f64,
    ) -> Result<()> {
        // No need for internal method or spawn_blocking anymore
        let client = self.db.get_connection().await?;
        let now = Utc::now();

        // Calculate PnL - requires fetching entry price first
        let position_data_opt = client.query_opt(
            "SELECT size, entry_price FROM positions WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE",
            &[&token_id as &(dyn ToSql + Sync), &self.is_paper_trade]
        ).await.map_err(|e| Error::Database(e.to_string()))?;

        let pnl = match position_data_opt {
            Some(row) => {
                let size: f64 = row.get(0);
                let entry_price: f64 = row.get(1);
                size * (price - entry_price)
            }
            None => {
                // Position not found, maybe return error or log warning
                warn!(
                    "Position not found for PnL calculation during update: {}",
                    token_id
                );
                return Err(PositionError::NotFound(token_id.to_string()).into());
            }
        };

        // Execute update
        let rows_affected = client
            .execute(
                queries::position::UPDATE_POSITION,
                &[
                    &price as &(dyn ToSql + Sync),
                    &pnl as &(dyn ToSql + Sync),
                    &now as &(dyn ToSql + Sync),
                    &token_id as &(dyn ToSql + Sync),
                    &self.is_paper_trade as &(dyn ToSql + Sync),
                ],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        if rows_affected == 0 {
            // This might happen if the position was closed between the PnL calc and update
            warn!(
                "Position update for {} affected 0 rows (may have been closed)",
                token_id
            );
            // Consider returning NotFound or Ok depending on desired strictness
            return Err(PositionError::NotFound(token_id.to_string()).into());
        }

        Ok(())
    }

    /// Delete an open position by token ID (async)
    pub async fn delete_open_position(&self, token_id: &str) -> Result<()> {
        let client = self.db.get_connection().await?;
        let rows_affected = client
            .execute(
                queries::position::DELETE_OPEN_POSITION,
                &[&token_id as &(dyn ToSql + Sync), &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        if rows_affected > 0 {
            info!("✅ Deleted position for token {}", token_id);
        } else {
            warn!(
                "Attempted to delete non-existent open position for token: {}",
                token_id
            );
        }
        Ok(())
    }

    /// Delete an open position by position ID (async)
    pub async fn delete_open_position_by_id(&self, position_id: i64) -> Result<()> {
        let client = self.db.get_connection().await?;
        let rows_affected = client
            .execute(
                queries::position::DELETE_OPEN_POSITION_BY_ID,
                &[&position_id as &(dyn ToSql + Sync), &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        if rows_affected > 0 {
            info!("✅ Deleted position with ID {}", position_id);
        } else {
            warn!(
                "Attempted to delete non-existent open position with ID: {}",
                position_id
            );
        }
        Ok(())
    }

    /// Check if an open position exists for the given token ID (async)
    pub async fn position_exists(&self, token_id: &str) -> Result<bool> {
        debug!("Checking if position exists for token: {}", token_id);
        let client = self.db.get_connection().await?;

        let row = client
            .query_one(
                queries::position::POSITION_EXISTS,
                &[&token_id as &(dyn ToSql + Sync), &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let count: i64 = row.get(0);
        Ok(count > 0)
    }

    /// Get the provider_id for an open position by its canonical token ID (async)
    pub async fn get_provider_id_for_token(
        &self,
        canonical_token_id: &str,
    ) -> Result<Option<String>> {
        let client = self.db.get_connection().await?;
        let row_opt = client
            .query_opt(
                "SELECT provider_id FROM positions WHERE token_id = LOWER($1) AND is_paper = $2 AND closed = FALSE",
                &[&canonical_token_id as &(dyn ToSql + Sync), &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        Ok(row_opt.map(|row| row.get(0)))
    }

    /// Get an open position by token ID, including its database ID (async)
    pub async fn get_position_by_token_id(
        &self,
        token_id: &str,
    ) -> Result<Option<(i64, StrategyPosition)>> {
        let client = self.db.get_connection().await?;

        let row_opt = client
            .query_opt(
                queries::position::GET_POSITION_BY_TOKEN_ID,
                &[&token_id as &(dyn ToSql + Sync), &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        match row_opt {
            Some(row) => {
                let id: i64 = row.get(0);
                let entry_time: DateTime<Utc> = row.get(7);
                let position = StrategyPosition {
                    token_id: row.get(1),
                    provider_id: row.get(2),
                    entry_price: row.get(3),
                    current_price: row.get(4),
                    highest_price: row.get(5),
                    size: row.get(6),
                    entry_time,
                    unrealized_pnl: row.get(9),
                };
                debug!("Found position ID {} for token {}", id, token_id);
                Ok(Some((id, position)))
            }
            None => {
                debug!("No open position found for token {}", token_id);
                Ok(None)
            }
        }
    }

    /// Delete a position by token ID (async) - Newly added
    pub async fn delete_position_by_token_id(&self, token_id: &str) -> Result<u64> {
        let client = self.db.get_connection().await?;
        let rows_affected = client
            .execute(
                "DELETE FROM positions WHERE token_id = $1 AND is_paper = $2",
                &[&token_id, &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(rows_affected)
    }

    /// Get total profit and loss for all completed positions (async)
    pub async fn get_total_pnl(&self) -> Result<f64> {
        let client = self.db.get_connection().await?;
        let row = client
            .query_one(queries::position::GET_TOTAL_PNL, &[&self.is_paper_trade])
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        Ok(row.get::<_, f64>(0))
    }

    /// Get count of open positions (async)
    pub async fn get_open_position_count(&self) -> Result<usize> {
        let client = self.db.get_connection().await?;
        let row = client
            .query_one(
                queries::position::COUNT_OPEN_POSITIONS,
                &[&self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;
        let count: i64 = row.get(0);
        Ok(count as usize)
    }
}

// No standard Repository implementation since Position doesn't have a simple ID field
// and requires specialized repository methods
