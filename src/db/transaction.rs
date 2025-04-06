use crate::error::{Error, Result, ErrorExt};
use log::{debug, warn, error};
use rusqlite::{Connection, Transaction};
use std::time::Duration;
use std::time::Instant;

const MAX_TRANSACTION_RETRIES: usize = 3;
const TRANSACTION_RETRY_BACKOFF_MS: u64 = 100;

/// Execute a function within a database transaction with automatic retries for transient errors
pub fn with_transaction<F, T>(conn: &mut Connection, f: F) -> Result<T>
where
    F: FnMut(&Transaction) -> Result<T>,
{
    with_transaction_options(conn, f, MAX_TRANSACTION_RETRIES, Duration::from_millis(TRANSACTION_RETRY_BACKOFF_MS))
}

/// Execute a function within a database transaction with custom retry options
pub fn with_transaction_options<F, T>(
    conn: &mut Connection, 
    mut f: F, 
    max_retries: usize,
    backoff: Duration
) -> Result<T>
where
    F: FnMut(&Transaction) -> Result<T>,
{
    let mut attempts = 0;
    let mut last_error = None;
    let start_time = Instant::now();

    loop {
        attempts += 1;
        debug!("Attempt {} of transaction operation", attempts);

        match execute_transaction(conn, &mut f) {
            Ok(result) => {
                if attempts > 1 {
                    debug!("Transaction succeeded after {} attempts in {:?}", 
                        attempts, start_time.elapsed());
                }
                return Ok(result);
            },
            Err(err) => {
                if is_retriable_database_error(&err) && attempts <= max_retries {
                    warn!("Transaction failed with retriable error (attempt {}/{}): {}. Retrying in {:?}...", 
                        attempts, max_retries + 1, err, backoff);
                    std::thread::sleep(backoff);
                    
                    // Increase backoff for next attempt (exponential backoff)
                    let next_backoff = Duration::from_millis(backoff.as_millis() as u64 * 2);
                    last_error = Some(err);
                } else {
                    // Non-retriable error or max retries reached
                    if attempts > 1 {
                        error!("Transaction failed after {} attempts: {}", attempts, err);
                    }
                    return Err(err);
                }
            }
        }
    }
}

/// Execute a single transaction attempt
fn execute_transaction<F, T>(conn: &mut Connection, f: &mut F) -> Result<T>
where
    F: FnMut(&Transaction) -> Result<T>,
{
    let tx = conn.transaction()
        .map_err(|e| Error::Database(e))?;
    
    match f(&tx) {
        Ok(result) => {
            tx.commit()
                .map_err(|e| Error::Database(e))
                .log_error("Failed to commit transaction")?;
            Ok(result)
        },
        Err(err) => {
            // Try to roll back, but don't mask the original error
            if let Err(rollback_err) = tx.rollback() {
                warn!("Transaction rollback failed: {}", rollback_err);
            }
            Err(err)
        }
    }
}

/// Determine if a database error is transient and retriable
fn is_retriable_database_error(err: &Error) -> bool {
    match err {
        Error::Database(db_err) => {
            matches!(db_err,
                rusqlite::Error::SqliteFailure(code, _) if 
                    code.code == rusqlite::ffi::ErrorCode::DatabaseBusy ||
                    code.code == rusqlite::ffi::ErrorCode::DatabaseLocked ||
                    code.extended_code == rusqlite::ffi::SQLITE_BUSY_RECOVERY ||
                    code.extended_code == rusqlite::ffi::SQLITE_BUSY_SNAPSHOT ||
                    code.extended_code == rusqlite::ffi::SQLITE_LOCKED_SHAREDCACHE
            )
        },
        _ => false,
    }
} 