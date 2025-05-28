// use rusqlite; // Removed
use ethers::prelude::*;
use std::result;
use std::sync::PoisonError;
use thiserror::Error;
// Added import
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

    /// Data conversion errors (e.g., U256 to f64)
    #[error("Conversion error: {0}")]
    Conversion(String),

    /// ABI encoding/decoding errors
    #[error("ABI error: {0}")]
    Abi(String),

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
    #[error("Not implemented: {0}")]
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

    /// Errors related to DEX operations or connectivity
    #[error("DEX error: {0}")]
    Dex(String),

    /// Error when a connection (e.g. to provider or wallet) is not established
    #[error("Not connected: {0}")]
    NotConnected(String),

    /// Contract errors
    #[error("Contract error: {0}")]
    Contract(String),

    /// Wallet connection and operation errors
    #[error("Wallet error: {0}")]
    Wallet(String),

    /// Transaction execution errors
    #[error("Transaction error: {0}")]
    Transaction(String),

    /// Input validation errors
    #[error("Validation error: {0}")]
    Validation(String),
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

// Convert from mutex poison errors
impl<T> From<PoisonError<T>> for Error {
    fn from(err: PoisonError<T>) -> Self {
        Error::Concurrency(format!("Mutex poisoned: {}", err))
    }
}

// Convert from redis errors
impl From<redis::RedisError> for Error {
    fn from(err: redis::RedisError) -> Self {
        Error::Redis(format!("Redis error: {}", err))
    }
}

// Convert from postgres errors
impl From<PgError> for Error {
    fn from(err: PgError) -> Self {
        Error::Database(format!("PostgreSQL error: {}", err))
    }
}

// Convert from ethers contract errors
impl<T: Middleware> From<ContractError<T>> for Error {
    fn from(err: ContractError<T>) -> Self {
        Error::Contract(format!("Contract error: {}", err))
    }
}

// Convert from ethers provider errors
impl From<ProviderError> for Error {
    fn from(err: ProviderError) -> Self {
        Error::Network(format!("Provider error: {}", err))
    }
}

// Convert from ethers ABI errors
impl From<ethers::abi::Error> for Error {
    fn from(err: ethers::abi::Error) -> Self {
        Error::Abi(format!("ABI error: {}", err))
    }
}

// Convert from serde_json errors
impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Serialization(format!("JSON error: {}", err))
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
