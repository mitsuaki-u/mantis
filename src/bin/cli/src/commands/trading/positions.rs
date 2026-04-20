use crate::config::Config;
use crate::error::Error;
use crate::infrastructure::database::repositories::PositionRepository;
use crate::infrastructure::database::Database;
use crate::infrastructure::dex::DexClient;
use log::info;
use std::sync::Arc;

// Re-export display functions for public use
pub use crate::core::utils::display::{display_closed_positions, display_open_positions};

/// List all trading positions
pub async fn positions(
    db: &Database,
    is_paper: bool,
    _config: &Config,
    _dex_client: Option<Arc<DexClient>>,
) -> Result<(), Error> {
    // Create position repository - DexClient is no longer needed for basic operations
    let position_repo = PositionRepository::new(db.clone(), is_paper);

    // Display open positions first
    info!("📊 Fetching open positions (paper: {})...", is_paper);
    let open_positions = position_repo.get_open_positions().await?;

    if !open_positions.is_empty() {
        display_open_positions(&open_positions, is_paper, db).await?;
    } else {
        println!("📭 No open positions found.");
    }

    // Display closed positions
    info!(
        "📊 Fetching recent closed positions (paper: {})...",
        is_paper
    );
    let closed_positions = position_repo.get_closed_positions(Some(10)).await?;

    if !closed_positions.is_empty() {
        display_closed_positions(&closed_positions, is_paper, db).await?;
    } else {
        println!("\n📪 No closed positions found.");
    }

    Ok(())
}
