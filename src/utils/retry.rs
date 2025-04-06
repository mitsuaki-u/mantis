use std::future::Future;
use std::time::Duration;
use log::{warn, trace, info};
use crate::error::{Error, Result};
use std::time::{SystemTime, UNIX_EPOCH};

/// Determines if an error is transient and can be retried
pub fn is_retriable_error(err: &Error) -> bool {
    match err {
        Error::Network(msg) => {
            // Network timeouts, connection resets are often transient
            msg.contains("timed out") || 
            msg.contains("connection reset") ||
            msg.contains("connection refused")
        },
        Error::RateLimit(_) => true, // Rate limits are always retriable after some time
        Error::Database(db_err) => {
            // Some database errors are transient (locked, busy)
            matches!(db_err, 
                rusqlite::Error::SqliteFailure(code, _) if
                    code.code == rusqlite::ffi::ErrorCode::DatabaseBusy ||
                    code.code == rusqlite::ffi::ErrorCode::DatabaseLocked
            )
        },
        _ => false,
    }
}

/// Simple function to generate a small random jitter (0-99ms)
fn generate_jitter() -> u64 {
    // Get the current time in nanoseconds as a simple source of randomness
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_nanos() as u64;
    
    // Use a simple modulo operation to get a value between 0-99
    now % 100
}

/// Gets the suggested backoff time for a retriable error
pub fn get_retry_backoff(err: &Error, current_attempt: usize, base_backoff: Duration) -> Duration {
    match err {
        Error::RateLimit(msg) => {
            // Try to extract retry-after from the message if available
            if let Some(retry_after) = msg
                .split("Retry after")
                .nth(1)
                .and_then(|s| s.trim().split(' ').next())
                .and_then(|s| s.parse::<u64>().ok()) {
                return Duration::from_secs(retry_after);
            }
            
            // Default to exponential backoff with jitter
            let exp_backoff = base_backoff.as_millis() as u64 * (1 << current_attempt.min(6));
            Duration::from_millis(exp_backoff)
        },
        _ => {
            // For other errors, use exponential backoff with a bit of jitter
            let exp_backoff = base_backoff.as_millis() as u64 * (1 << current_attempt.min(6));
            let jitter = generate_jitter();
            Duration::from_millis(exp_backoff + jitter)
        }
    }
}

/// Execute an async operation with retry logic for transient errors
pub async fn with_retry<T, F, Fut>(
    operation_name: &str,
    f: F,
    max_retries: usize,
    base_backoff: Duration,
) -> Result<T>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt = 0;
    
    loop {
        attempt += 1;
        trace!("Executing operation '{}' (attempt {}/{})", 
            operation_name, attempt, max_retries + 1);
            
        match f().await {
            Ok(result) => {
                if attempt > 1 {
                    info!("Operation '{}' succeeded after {} attempts", operation_name, attempt);
                }
                return Ok(result);
            },
            Err(err) => {
                if attempt > max_retries || !is_retriable_error(&err) {
                    if attempt > 1 {
                        warn!("Operation '{}' failed after {} attempts: {}", 
                            operation_name, attempt, err);
                    }
                    return Err(if attempt > max_retries {
                        Error::RetryExhausted(format!(
                            "Operation '{}' failed after {} attempts: {}", 
                            operation_name, attempt, err
                        ))
                    } else {
                        err
                    });
                }
                
                let backoff = get_retry_backoff(&err, attempt, base_backoff);
                warn!("Attempt {} for '{}' failed: {}. Retrying in {:?}...", 
                    attempt, operation_name, err, backoff);
                    
                tokio::time::sleep(backoff).await;
            }
        }
    }
} 