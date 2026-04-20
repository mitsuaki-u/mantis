use crate::infrastructure::errors::{Error, Result};
use log::{debug, trace, warn};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

// Track retry frequencies to avoid log spam
static RETRY_COUNTERS: Lazy<Mutex<HashMap<String, (usize, SystemTime)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// How often to log repeated retries of the same operation (in seconds)
const RETRY_LOG_INTERVAL: u64 = 60;

/// Determines if an error is transient and can be retried
pub fn is_retriable_error(err: &Error) -> bool {
    match err {
        Error::Network(msg) => {
            // Network timeouts, connection resets are often transient
            msg.contains("timed out")
                || msg.contains("connection reset")
                || msg.contains("connection refused")
        }
        Error::RateLimit(_) => true, // Rate limits are always retriable after some time
        Error::Database(db_err) => {
            // Some database errors are transient (locked, busy)
            db_err.contains("database is locked")
                || db_err.contains("database is busy")
                || db_err.contains("unable to open database file")
                || db_err.contains("disk i/o error")
        }
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
                .and_then(|s| s.parse::<u64>().ok())
            {
                return Duration::from_secs(retry_after);
            }

            // Default to exponential backoff with jitter
            let exp_backoff = base_backoff.as_millis() as u64 * (1 << current_attempt.min(6));
            Duration::from_millis(exp_backoff)
        }
        _ => {
            // For other errors, use exponential backoff with a bit of jitter
            let exp_backoff = base_backoff.as_millis() as u64 * (1 << current_attempt.min(6));
            let jitter = generate_jitter();
            Duration::from_millis(exp_backoff + jitter)
        }
    }
}

/// Check if we should log this retry based on rate limiting
fn should_log_retry(operation_name: &str, attempt: usize) -> bool {
    // Always log the first retry
    if attempt == 1 {
        return true;
    }

    let now = SystemTime::now();
    let mut counters = match RETRY_COUNTERS.lock() {
        Ok(guard) => guard,
        Err(_) => {
            // If we can't lock, just allow logging
            return true;
        }
    };

    // Check if we've seen this operation recently
    if let Some((last_attempt, last_time)) = counters.get(operation_name) {
        // If it's been a while since we logged this operation, log it again
        if let Ok(elapsed) = now.duration_since(*last_time) {
            if elapsed.as_secs() > RETRY_LOG_INTERVAL {
                counters.insert(operation_name.to_string(), (attempt, now));
                return true;
            }
        }

        // If the attempt number is significantly different, log it
        if *last_attempt > 0 && (attempt / 2) > *last_attempt {
            counters.insert(operation_name.to_string(), (attempt, now));
            return true;
        }

        // Otherwise, don't log to avoid spam
        false
    } else {
        // First time seeing this operation, log it
        counters.insert(operation_name.to_string(), (attempt, now));
        true
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

        // Only log at trace level and only on the first attempt or periodically
        if attempt == 1 {
            trace!("Executing operation '{}'", operation_name);
        }

        match f().await {
            Ok(result) => {
                // Only log success after retries to reduce noise
                if attempt > 1 {
                    debug!(
                        "Operation '{}' succeeded after {} attempts",
                        operation_name, attempt
                    );
                }
                return Ok(result);
            }
            Err(err) => {
                if attempt > max_retries || !is_retriable_error(&err) {
                    // Only log final failures at warning level
                    if attempt > 1 {
                        warn!(
                            "Operation '{}' failed after {} attempts: {}",
                            operation_name, attempt, err
                        );
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

                // Rate limit logging to avoid spamming logs
                if should_log_retry(operation_name, attempt) {
                    debug!(
                        "Attempt {} for '{}' failed: {}. Retrying in {:?}...",
                        attempt, operation_name, err, backoff
                    );
                }

                tokio::time::sleep(backoff).await;
            }
        }
    }
}
