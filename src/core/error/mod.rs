// use rusqlite; // Removed
use std::result;
use std::sync::PoisonError;
use thiserror::Error;
use tokio_postgres::Error as PgError; // Added for PostgreSQL errors

// Import our new logging utilities
use crate::utils::logging::{log_and_default as log_util_default, log_error as log_util_error};

/// Result type alias to simplify error handling
pub type Result<T> = result::Result<T, Error>;

/// Application error types
#[derive(Debug, Error, Clone)]
pub enum Error {
    /// Network errors
    #[error("Network error: {0}")]
    Network(String),

    /// Data parsing errors
    #[error("Parse error: {0}")]
    Parse(String),

    /// Resource not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// API rate limiting
    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    /// General API errors
    #[error("API error: {0}")]
    Api(String),

    /// I/O errors (file access, etc.)
    #[error("I/O error: {0}")]
    Io(String),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Invalid user input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Database errors
    #[error("Database error: {0}")]
    Database(String),

    /// File descriptor limit exceeded
    #[error("File descriptor limit exceeded: {0}")]
    FileLimitExceeded(String),

    /// Mutex or lock errors
    #[error("Concurrency error: {0}")]
    Concurrency(String),

    /// Task cancellation or join errors
    #[error("Task error: {0}")]
    Task(String),

    /// Trading execution errors
    #[error("Trading execution error: {0}")]
    Trading(String),

    /// Retry exhausted
    #[error("Operation failed after retries: {0}")]
    RetryExhausted(String),

    /// Internal system errors
    #[error("Internal system error: {0}")]
    Internal(String),

    /// Feature not implemented
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),

    /// External third-party errors
    #[error("External error: {0}")]
    External(String),

    /// Generic error for any other cases
    #[error("Error: {0}")]
    Other(String),

    /// Invalid argument
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// Position locked
    #[error("Position locked: {0}")]
    PositionLocked(String),

    /// Redis errors
    #[error("Redis error: {0}")]
    Redis(String),

    /// Queue operation errors
    #[error("Queue operation error: {0}")]
    QueueOperation(String),

    /// Serialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Cache operation errors
    #[error("Cache error: {0}")]
    Cache(String),
}

// Convert from reqwest errors
impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Error::Network(format!("Request timed out: {}", err))
        } else if err.is_connect() {
            Error::Network(format!("Connection failed: {}", err))
        } else if err.is_status() {
            if let Some(status) = err.status() {
                if status.as_u16() == 429 {
                    Error::RateLimit(format!("Rate limit exceeded: {}", err))
                } else {
                    Error::Api(format!("API returned error status {}: {}", status, err))
                }
            } else {
                Error::Api(format!("API error: {}", err))
            }
        } else {
            Error::Network(format!("Network error: {}", err))
        }
    }
}

// Convert from std::io errors
impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        let kind = format!("{:?}", err.kind());
        Error::Io(format!("{} ({})", err, kind))
    }
}

// Convert from tokio join errors
impl From<tokio::task::JoinError> for Error {
    fn from(err: tokio::task::JoinError) -> Self {
        if err.is_cancelled() {
            Error::Task(format!("Task was cancelled: {}", err))
        } else {
            Error::Task(format!("Task panicked: {}", err))
        }
    }
}

// Convert from string errors
impl From<String> for Error {
    fn from(err: String) -> Self {
        Error::InvalidInput(err)
    }
}

// Convert from &str errors
impl From<&str> for Error {
    fn from(err: &str) -> Self {
        Error::InvalidInput(err.to_string())
    }
}

// Convert from any PoisonError for concurrency handling
impl<T> From<PoisonError<T>> for Error {
    fn from(err: PoisonError<T>) -> Self {
        Error::Concurrency(format!("Thread holding the lock panicked: {}", err))
    }
}

// Convert from redis errors
impl From<redis::RedisError> for Error {
    fn from(err: redis::RedisError) -> Self {
        Error::Redis(format!("Redis error: {}", err))
    }
}

// Add From<tokio_postgres::Error> impl
impl From<PgError> for Error {
    fn from(err: PgError) -> Self {
        // TODO: Add specific error handling if needed (e.g., unique constraint violations)
        Error::Database(err.to_string())
    }
}

// Convert from serde_json errors
impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serialization(err.to_string())
    }
}

/// Utility methods for handling errors
pub trait ErrorExt<T> {
    /// Log the error and continue with a default value
    fn log_and_default(self, context: &str, default: T) -> T;

    /// Log the error and return the error
    fn log_error(self, context: &str) -> result::Result<T, Error>;
}

impl<T> ErrorExt<T> for Result<T> {
    fn log_and_default(self, context: &str, default: T) -> T {
        // Use our centralized logging utility
        log_util_default(self, context, default)
    }

    fn log_error(self, context: &str) -> result::Result<T, Error> {
        // Use our centralized logging utility
        log_util_error(self, context)
    }
}
