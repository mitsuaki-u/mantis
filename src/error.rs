use rusqlite;
use std::sync::PoisonError;
use std::result;
use thiserror::Error;

/// Result type alias to simplify error handling
pub type Result<T> = result::Result<T, Error>;

/// Application error types
#[derive(Debug, Error)]
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
    Database(#[from] rusqlite::Error),
    
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

// Convert from serde_json errors
impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Parse(format!("JSON parsing error: {} at line {}, column {}", 
            err, err.line(), err.column()))
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

/// Utility methods for handling errors
pub trait ErrorExt<T> {
    /// Log the error and continue with a default value
    fn log_and_default(self, context: &str, default: T) -> T;
    
    /// Log the error and return the error
    fn log_error(self, context: &str) -> result::Result<T, Error>;
}

impl<T> ErrorExt<T> for Result<T> {
    fn log_and_default(self, context: &str, default: T) -> T {
        match self {
            Ok(value) => value,
            Err(err) => {
                log::error!("{}: {}", context, err);
                default
            }
        }
    }
    
    fn log_error(self, context: &str) -> result::Result<T, Error> {
        self.map_err(|err| {
            log::error!("{}: {}", context, err);
            err
        })
    }
} 