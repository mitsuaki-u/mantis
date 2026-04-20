pub mod position;
pub mod token;
pub mod trade;
pub mod transactions;

pub use position::CompletedPosition;
pub use position::PositionRepository;
pub use position::RecordCloseArgs;
pub use token::TokenRepository;
pub use trade::TradeRepository;
pub use transactions::{
    Transaction, TransactionRepository, TransactionStatusHistoryEntry, TransactionWithHistory,
};

use crate::infrastructure::database::Database;
use std::sync::Arc;

/// Repository factory to create and provide instances of all repositories
#[derive(Clone)]
pub struct RepositoryFactory {
    db: Database,
    is_paper_trade: bool,
}

impl RepositoryFactory {
    /// Create a new repository factory
    pub fn new(db: Database, is_paper_trade: bool) -> Self {
        Self { db, is_paper_trade }
    }

    /// Get the token repository
    pub fn token_repository(&self) -> Arc<TokenRepository> {
        Arc::new(TokenRepository::new(self.db.clone(), self.is_paper_trade))
    }

    /// Create a position repository for the specified execution mode
    pub fn position_repository(&self) -> Arc<PositionRepository> {
        Arc::new(PositionRepository::new(
            self.db.clone(),
            self.is_paper_trade,
        ))
    }

    /// Get the trade repository
    pub fn trade_repository(&self) -> Arc<TradeRepository> {
        Arc::new(TradeRepository::new(self.db.clone(), self.is_paper_trade))
    }

    /// Get the transaction repository
    pub fn transaction_repository(&self) -> Arc<TransactionRepository> {
        Arc::new(TransactionRepository::new(Arc::new(self.db.clone())))
    }

    /// Get a clone of the underlying database connection pool provider
    pub fn get_db(&self) -> Database {
        self.db.clone()
    }
}
