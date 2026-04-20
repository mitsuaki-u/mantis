pub mod args;
pub mod control;
pub mod history;
pub mod positions;
pub mod status;
pub mod transactions;

pub use args::TradingArgs;
pub use control::{restart_actor, start_trading, stop_trading};
pub use history::display_trading_history;
pub use positions::{display_closed_positions, positions};
pub use status::{get_health_report, get_trading_status};
pub use transactions::display_transactions;

use crate::config::Config;
use crate::error::Error;
use crate::infrastructure::database::Database;
use crate::EventRouter;
use std::sync::Arc;

/// Handle trading commands that require the EventRouter (real-time bot operations)
pub async fn handle_trading_command(
    cmd: TradingArgs,
    config: Config,
    db: Database,
    event_router: Arc<EventRouter>,
) -> Result<(), Error> {
    match cmd {
        TradingArgs::Start(_) => {
            start_trading(cmd, config, db, event_router).await?;
        }
        TradingArgs::Status => get_trading_status(event_router, db).await?,
        TradingArgs::Health => get_health_report(event_router, db).await?,
        TradingArgs::Restart { actor_id } => {
            restart_actor(event_router, &actor_id, db).await?;
        }
        TradingArgs::Stop => {
            stop_trading(event_router, db).await?;
        }
        // These commands don't need EventRouter and are handled in main
        TradingArgs::History { .. }
        | TradingArgs::Positions { .. }
        | TradingArgs::Transactions { .. } => {
            return Err(Error::InvalidInput(
                "This command should be handled directly in main, not through handle_trading_command".to_string(),
            ));
        }
    }
    Ok(())
}
