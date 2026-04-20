use crate::core::calculations::financial;
use crate::core::domain::trading::Position as StrategyPosition;
use crate::infrastructure::database::queries;
use crate::infrastructure::database::Database;
use crate::infrastructure::errors::{Error, Result};
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
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
    /// Create a new PositionRepository
    pub fn new(db: Database, is_paper_trade: bool) -> Self {
        Self { db, is_paper_trade }
    }

    /// Get a reference to the database pool wrapper
    pub fn get_database(&self) -> Database {
        self.db.clone()
    }

    /// Health check to verify database connectivity
    pub async fn health_check(&self) -> Result<()> {
        debug!("Performing position repository health check");
        let client = self.db.get_connection().await?;

        // Simple query to test database connectivity
        let _row = client
            .query_one(queries::position::HEALTH_CHECK, &[])
            .await
            .map_err(|e| {
                Error::Database(format!("Position repository health check failed: {}", e))
            })?;

        debug!("Position repository health check passed");
        Ok(())
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

        debug!("Found {} open positions", positions.len());
        Ok(positions)
    }

    /// Get closed positions (async)
    pub async fn get_closed_positions(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<CompletedPosition>> {
        debug!(
            "Getting closed positions (paper: {}, limit: {:?})",
            self.is_paper_trade, limit
        );
        let client = self.db.get_connection().await?;

        let limit_value = limit.unwrap_or(10) as i64;
        let rows = client
            .query(
                queries::position::GET_COMPLETED_POSITIONS,
                &[&self.is_paper_trade as &(dyn ToSql + Sync), &limit_value],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let mut completed_positions = Vec::with_capacity(rows.len());
        for row in rows {
            let entry_time: DateTime<Utc> = row.get(5);
            let exit_time: DateTime<Utc> = row.get(6);
            let entry_price: f64 = row.get(2);
            let exit_price: f64 = row.get(3);
            let size: f64 = row.get(4);
            let gross_profit: f64 = row.get(7); // This is gross profit, not net
            let actual_fees: f64 = row.get(8); // Actual fees paid (stored in database)

            // Calculate net profit using actual fees and centralized function
            let net_profit = financial::calculate_net_profit_f64(gross_profit, actual_fees);

            // Calculate initial investment including actual entry fees
            let initial_investment = financial::calculate_initial_investment_f64(
                size,
                entry_price,
                Some(actual_fees / 2.0), // Assume half fees on entry
            );

            // Calculate ROI based on net profit and initial investment
            let roi = financial::calculate_roi_from_profit_f64(net_profit, initial_investment);

            completed_positions.push(CompletedPosition {
                id: row.get(0),
                token_id: row.get(1),
                size,
                entry_price,
                exit_price,
                entry_time,
                exit_time,
                profit: gross_profit, // Keep gross profit for backward compatibility
                roi,
                fees: actual_fees, // Use actual fees from database
                net_profit,
            });
        }

        debug!("Found {} closed positions", completed_positions.len());
        Ok(completed_positions)
    }

    /// Record a new position opening with its associated trade (async)
    pub async fn record_position_with_trade(
        &self,
        position: &StrategyPosition,
        price: f64,
        size: f64,
        timestamp: chrono::DateTime<Utc>,
    ) -> Result<i64> {
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
        fee_calculation: Option<f64>, // Accept calculated fees from caller
    ) -> Result<CompletedPosition> {
        let mut client = self.db.get_connection().await?;

        // **IDEMPOTENT CHECK: First check if position is already closed**
        let position_check = client
            .query_opt(
                queries::position::CHECK_POSITION_STATUS,
                &[&position_id, &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let provider_id = match position_check {
            Some(row) => {
                let closed: bool = row.get(0);
                let provider_id: String = row.get(4);

                if closed {
                    // Position already closed - return existing completed position
                    let close_price: f64 = row.get(1);
                    let profit: f64 = row.get(2);
                    let exit_time: chrono::DateTime<Utc> = row.get(3);

                    info!("✅ Position ID {} for {} already closed (idempotent operation), returning existing result", 
                          position_id, args.token_id);

                    let total_fee = fee_calculation.unwrap_or(0.0);
                    let net_profit = financial::calculate_net_profit_f64(profit, total_fee);
                    let initial_investment = financial::calculate_initial_investment_f64(
                        args.size,
                        args.entry_price,
                        Some(total_fee / 2.0),
                    );
                    let roi =
                        financial::calculate_roi_from_profit_f64(net_profit, initial_investment);

                    return Ok(CompletedPosition {
                        id: position_id,
                        token_id: args.token_id.to_string(),
                        size: args.size,
                        entry_price: args.entry_price,
                        exit_price: close_price,
                        entry_time: args.entry_time,
                        exit_time,
                        profit,
                        roi,
                        fees: total_fee,
                        net_profit,
                    });
                }
                provider_id
            }
            None => {
                return Err(Error::NotFound(format!(
                    "Position {} not found",
                    position_id
                )));
            }
        };

        // **PROCEED WITH NORMAL CLOSURE (position is open)**
        // Calculate gross profit using centralized function
        let gross_profit =
            financial::calculate_pnl_f64(args.entry_price, args.exit_price, args.size);

        // Use provided fee calculation or default to 0
        let total_fee = fee_calculation.unwrap_or(0.0);
        let net_profit = financial::calculate_net_profit_f64(gross_profit, total_fee);

        // Calculate ROI based on initial investment (including entry fees if provided)
        let initial_investment = financial::calculate_initial_investment_f64(
            args.size,
            args.entry_price,
            Some(total_fee / 2.0),
        ); // Assume half fees on entry
        let roi = financial::calculate_roi_from_profit_f64(net_profit, initial_investment);

        if total_fee > 0.0 {
            info!(
                "💰 Using provided fee calculation for {}: Total Fee: ${:.2}",
                args.token_id, total_fee
            );
        }

        let transaction = client
            .transaction()
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        // Insert the SELL trade first, get its ID (idempotent via ON CONFLICT DO NOTHING)

        let trade_result = transaction
            .query_opt(
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

        let trade_id: i64 = match trade_result {
            Some(row) => row.get(0), // New trade inserted
            None => {
                // Trade already exists due to conflict - find existing trade ID
                let existing_trade = transaction
                    .query_one(
                        queries::trade::GET_EXISTING_SELL_TRADE_ID,
                        &[
                            &args.token_id as &(dyn ToSql + Sync),
                            &args.exit_price,
                            &args.size,
                            &args.exit_time,
                            &self.is_paper_trade,
                        ],
                    )
                    .await
                    .map_err(|e| Error::Database(e.to_string()))?;
                existing_trade.get(0)
            }
        };

        // Update the position as closed, linking the sell trade ID and storing actual fees
        let rows_affected = transaction
            .execute(
                queries::position::CLOSE_POSITION,
                &[
                    &args.exit_price as &(dyn ToSql + Sync),
                    &gross_profit, // Use calculated gross profit
                    &total_fee,    // Store actual fees paid
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

        info!("✅ Closed position ID {} for {} - {:.8} tokens @ entry ${:.4}, exit ${:.4}, P&L ${:.2}",
                position_id, args.token_id, args.size, args.entry_price, args.exit_price, gross_profit);

        // Construct and return CompletedPosition
        let completed_position = CompletedPosition {
            id: position_id,
            token_id: args.token_id.to_string(),
            size: args.size,
            entry_price: args.entry_price,
            exit_price: args.exit_price,
            entry_time: args.entry_time,
            exit_time: args.exit_time,
            profit: gross_profit,
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
        let position_data_opt = client
            .query_opt(
                queries::position::GET_POSITION_FOR_PNL_CALC,
                &[&token_id as &(dyn ToSql + Sync), &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(e.to_string()))?;

        let pnl = match position_data_opt {
            Some(row) => {
                let size: f64 = row.get(0);
                let entry_price: f64 = row.get(1);
                financial::calculate_unrealized_pnl_f64(entry_price, price, size)
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

    // NOTE: Use record_position_close_with_trade() to properly close positions while maintaining audit trail.

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

    /// Get the provider_id for an open position by its token ID (async)
    pub async fn get_provider_id_for_token(&self, token_id: &str) -> Result<Option<String>> {
        let client = self.db.get_connection().await?;
        let row_opt = client
            .query_opt(
                queries::position::GET_PROVIDER_ID_FOR_TOKEN,
                &[&token_id as &(dyn ToSql + Sync), &self.is_paper_trade],
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
                Ok(Some((id, position)))
            }
            None => Ok(None),
        }
    }

    // NOTE: delete_position_by_token_id method removed to prevent foreign key violations.
    // Use record_position_close_with_trade() instead to properly close positions while maintaining audit trail.

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

    // =========================================================================
    // Position Reservation Methods (Race Condition Prevention)
    // =========================================================================

    /// Try to atomically reserve a position slot
    ///
    /// Returns Ok(true) if slot was successfully reserved
    /// Returns Ok(false) if max_positions limit would be exceeded
    ///
    /// This prevents race conditions where multiple concurrent signals
    /// check position count before any have been persisted.
    pub async fn try_reserve_position_slot(
        &self,
        correlation_id: &str,
        max_positions: usize,
    ) -> Result<bool> {
        use chrono::Utc;

        let client = self.db.get_connection().await?;

        // First, cleanup any expired reservations
        let cleanup_result = client
            .execute(
                "DELETE FROM position_reservations WHERE expires_at < NOW()",
                &[],
            )
            .await;

        if let Ok(cleaned) = cleanup_result {
            if cleaned > 0 {
                log::debug!("Cleaned up {} expired position reservations", cleaned);
            }
        }

        // Count current positions + active reservations atomically
        let count_query = "
            SELECT
                (SELECT COUNT(*) FROM positions WHERE closed = false AND is_paper = $1) +
                (SELECT COUNT(*) FROM position_reservations WHERE is_paper = $1 AND expires_at > NOW())
            AS total
        ";

        let row = client
            .query_one(count_query, &[&self.is_paper_trade])
            .await
            .map_err(|e| {
                Error::Database(format!("Failed to count positions+reservations: {}", e))
            })?;

        let current_total: i64 = row.get(0);

        log::debug!(
            "Position slot check: {} positions+reservations vs {} max (paper: {})",
            current_total,
            max_positions,
            self.is_paper_trade
        );

        if current_total >= max_positions as i64 {
            log::info!(
                "Position slot reservation denied: {}/{} slots used (correlation_id: {})",
                current_total,
                max_positions,
                &correlation_id[..8.min(correlation_id.len())]
            );
            return Ok(false);
        }

        // Try to insert reservation (will fail if correlation_id already exists)
        let expires_at = Utc::now() + chrono::Duration::minutes(5);
        let insert_result = client
            .execute(
                "INSERT INTO position_reservations (correlation_id, is_paper, expires_at)
                 VALUES ($1, $2, $3)
                 ON CONFLICT (correlation_id) DO NOTHING",
                &[&correlation_id, &self.is_paper_trade, &expires_at],
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to reserve position slot: {}", e)))?;

        if insert_result == 0 {
            // Reservation already exists for this correlation_id
            log::warn!(
                "Position slot already reserved for correlation_id: {}",
                &correlation_id[..8.min(correlation_id.len())]
            );
            return Ok(true); // Already reserved, treat as success
        }

        log::info!(
            "✅ Position slot reserved: {}/{} used (correlation_id: {}, expires: {})",
            current_total + 1,
            max_positions,
            &correlation_id[..8.min(correlation_id.len())],
            expires_at.format("%H:%M:%S")
        );

        Ok(true)
    }

    /// Release a position slot reservation
    ///
    /// Called when:
    /// - Position is successfully created (reservation no longer needed)
    /// - Trade execution fails (release slot for retry)
    /// - Signal is rejected for other reasons
    pub async fn release_reservation(&self, correlation_id: &str) -> Result<()> {
        let client = self.db.get_connection().await?;

        let deleted = client
            .execute(
                "DELETE FROM position_reservations WHERE correlation_id = $1 AND is_paper = $2",
                &[&correlation_id, &self.is_paper_trade],
            )
            .await
            .map_err(|e| Error::Database(format!("Failed to release reservation: {}", e)))?;

        if deleted > 0 {
            log::debug!(
                "Released position reservation for correlation_id: {}",
                &correlation_id[..8.min(correlation_id.len())]
            );
        }

        Ok(())
    }

    /// Cleanup expired reservations (periodic maintenance)
    ///
    /// Returns the number of expired reservations cleaned up
    pub async fn cleanup_expired_reservations(&self) -> Result<usize> {
        let client = self.db.get_connection().await?;

        let deleted = client
            .execute(
                "DELETE FROM position_reservations WHERE expires_at < NOW()",
                &[],
            )
            .await
            .map_err(|e| {
                Error::Database(format!("Failed to cleanup expired reservations: {}", e))
            })?;

        if deleted > 0 {
            log::info!("Cleaned up {} expired position reservations", deleted);
        }

        Ok(deleted as usize)
    }
}

// No standard Repository implementation since Position doesn't have a simple ID field
// and requires specialized repository methods
