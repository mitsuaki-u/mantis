pub mod token;
pub mod position;
pub mod trade;
pub mod price;

pub use token::TokenRepository;
pub use position::PositionRepository;
pub use trade::TradeRepository;
pub use price::PriceRepository;

use crate::db::Database;
use crate::error::Result;

/// Repository trait for basic CRUD operations
pub trait Repository<T, ID> {
    fn find_by_id(&self, id: ID) -> crate::error::Result<Option<T>>;
    fn find_all(&self) -> crate::error::Result<Vec<T>>;
    fn save(&self, entity: &T) -> crate::error::Result<ID>;
    fn delete(&self, id: ID) -> crate::error::Result<()>;
}

/// Repository factory to create and provide instances of all repositories
pub struct RepositoryFactory {
    db: Database,
    is_paper_trade: bool,
}

impl RepositoryFactory {
    /// Create a new repository factory
    pub fn new(is_paper_trade: bool) -> Result<Self> {
        let db = Database::new()?;
        Ok(Self { db, is_paper_trade })
    }
    
    /// Create a repository factory with an existing database connection
    pub fn with_db(db: Database, is_paper_trade: bool) -> Self {
        Self { db, is_paper_trade }
    }
    
    /// Get the token repository
    pub fn token_repository(&self) -> TokenRepository {
        TokenRepository::new(self.db.clone())
    }
    
    /// Get the position repository
    pub fn position_repository(&self) -> PositionRepository {
        PositionRepository::new(self.db.clone(), self.is_paper_trade)
    }
    
    /// Get the trade repository
    pub fn trade_repository(&self) -> TradeRepository {
        TradeRepository::new(self.db.clone(), self.is_paper_trade)
    }
    
    /// Get the price repository
    pub fn price_repository(&self) -> PriceRepository {
        PriceRepository::new(self.db.clone())
    }
    
    /// Get the underlying database connection
    pub fn get_db(&self) -> Database {
        self.db.clone()
    }
} 