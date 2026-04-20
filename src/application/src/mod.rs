// Application layer modules
pub mod actors;
pub mod app;
pub mod constants;
pub mod errors;
pub mod events;

// Re-export commonly used types
pub use actors::system::actor;
pub use actors::EventRouter;
pub use errors::{Error, Result};

// Re-export app types and functions
pub use app::{apply_defaults, is_forced_shutdown, TradingBotSystem};
