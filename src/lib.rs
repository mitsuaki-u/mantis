// Unified error types for all layers
pub mod errors;

// Layered Architecture Modules
pub mod core; // Domain models and business logic
pub mod infrastructure; // External integrations (database, network, DEX, cache)

// Application layer is in application/src/
#[path = "application/src/mod.rs"]
pub mod application; // Application services and actors

// CLI layer modules
#[path = "bin/cli/src/mod.rs"]
pub mod cli; // CLI commands and handlers

#[path = "bin/cli/src/config/mod.rs"]
pub mod config; // Configuration management

#[path = "bin/cli/src/errors.rs"]
pub mod error; // CLI errors (re-exports unified Error)

#[path = "bin/cli/src/bootstrap.rs"]
pub mod bootstrap; // Bootstrap utilities

// Re-export unified error type and Result
pub use errors::{Error, Result};

// Re-export commonly used modules
pub use application::actors::EventRouter;
pub use application::events;

// Initialize logger with default settings
pub fn init_logger() {
    let _ = infrastructure::logging::init_logger(
        Some("info"), // Default log level
        false,        // Debug mode off
        None,         // No log file
        "logs",       // Default logs directory
        "default",    // Command name
        None,         // No module filters
    );
}
