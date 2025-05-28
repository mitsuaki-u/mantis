pub mod args;
pub mod bot_control;
pub mod history;
pub mod positions;
pub mod status;

pub use args::TradingArgs;
pub use bot_control::{restart_actor, start_trading, stop_trading};
pub use history::display_trading_history;
pub use positions::{close_position, display_open_positions, open_position};
pub use status::{get_health_report, get_trading_status};

use crate::core::config::Config;
use crate::core::error::Error;
use crate::infra::actors::MessageBus;
use crate::infra::db::Database;
use std::sync::Arc;

/// Handle trading commands that require the MessageBus (real-time bot operations)
pub async fn handle_trading_command(
    cmd: TradingArgs,
    config: Config,
    db: Database,
    message_bus: Arc<MessageBus>,
) -> Result<(), Error> {
    match cmd {
        TradingArgs::Start { .. } => {
            start_trading(cmd, config, db, message_bus).await?;
        }
        TradingArgs::Status => get_trading_status(message_bus, db).await?,
        TradingArgs::Health => get_health_report(message_bus, db).await?,
        TradingArgs::Restart { actor_id } => {
            restart_actor(message_bus, &actor_id, db).await?;
        }
        TradingArgs::Stop => {
            stop_trading(message_bus, db).await?;
        }
        // These commands don't need MessageBus and are handled in main
        TradingArgs::History { .. }
        | TradingArgs::Positions { .. }
        | TradingArgs::Close { .. }
        | TradingArgs::Open { .. } => {
            return Err(Error::InvalidInput(
                "This command should be handled directly in main, not through handle_trading_command".to_string(),
            ));
        }
    }
    Ok(())
}
