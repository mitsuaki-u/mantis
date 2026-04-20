use super::types::TOKEN_METADATA_TTL;

/// Configuration management for cache operations
pub struct CacheConfig;

impl CacheConfig {
    /// Get configured token cache TTL from config, with fallback to defaults
    pub fn get_token_cache_ttl() -> usize {
        match crate::config::Config::load() {
            Ok(_) => crate::infrastructure::constants::TOKEN_CACHE_TTL_SECS as usize,
            Err(_) => {
                // Fallback to default constant if config is not available
                TOKEN_METADATA_TTL
            }
        }
    }
}
