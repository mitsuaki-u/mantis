//! API key management utilities.
//!
//! This module handles API key loading, validation, and management
//! for various external services.

use crate::config::env::EnvLoader;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use validator::Validate;

/// API keys for various services
#[derive(Debug, Clone, Serialize, Deserialize, Validate, Default)]
pub struct ApiKeys {
    pub infura: Option<String>,
    pub alchemy: Option<String>,
}

impl ApiKeys {
    /// Create a new ApiKeys instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Load API keys from environment variables with fallback to config values
    pub fn load_from_env(&mut self) {
        let (infura, alchemy) =
            EnvLoader::load_all_api_keys(self.infura.clone(), self.alchemy.clone());

        self.infura = infura;
        self.alchemy = alchemy;
    }

    /// Set an API key for a specific service
    pub fn set_api_key(&mut self, service: &str, key: String) -> Result<()> {
        match service.to_lowercase().as_str() {
            "infura" => self.infura = Some(key),
            "alchemy" => self.alchemy = Some(key),
            _ => return Err(Error::Config(format!("Unknown service: {}", service))),
        }
        Ok(())
    }

    /// Get an API key for a specific service
    pub fn get_api_key(&self, service: &str) -> Option<&String> {
        match service.to_lowercase().as_str() {
            "infura" => self.infura.as_ref(),
            "alchemy" => self.alchemy.as_ref(),
            _ => None,
        }
    }

    /// Check if any API keys are configured
    pub fn has_any_keys(&self) -> bool {
        self.infura.is_some() || self.alchemy.is_some()
    }

    /// Check if a specific service has an API key configured
    pub fn has_key_for_service(&self, service: &str) -> bool {
        self.get_api_key(service).is_some()
    }

    /// Get available RPC providers (services that have API keys configured)
    pub fn get_available_rpc_providers(&self) -> Vec<String> {
        let mut providers = Vec::new();

        if self.infura.is_some() {
            providers.push("infura".to_string());
        }
        if self.alchemy.is_some() {
            providers.push("alchemy".to_string());
        }

        providers
    }

    /// Get the primary RPC provider based on availability and preference
    pub fn get_primary_rpc_provider(&self, preferred: Option<&str>) -> Option<String> {
        let available = self.get_available_rpc_providers();

        if available.is_empty() {
            return None;
        }

        // If a preference is specified and available, use it
        if let Some(pref) = preferred {
            if available.contains(&pref.to_string()) {
                return Some(pref.to_string());
            }
        }

        // Otherwise, use priority order: infura > alchemy
        for provider in &["infura", "alchemy"] {
            if available.contains(&provider.to_string()) {
                return Some(provider.to_string());
            }
        }

        // Fallback to first available
        available.first().cloned()
    }

    /// Validate all configured API keys (basic format validation)
    pub fn validate_keys(&self) -> Result<()> {
        // Basic validation - check that keys are not empty if present
        let services = [("infura", &self.infura), ("alchemy", &self.alchemy)];

        for (service, key) in services.iter() {
            if let Some(k) = key {
                if k.trim().is_empty() {
                    return Err(Error::Config(format!("API key for {} is empty", service)));
                }

                // Basic length validation (most API keys are at least 10 characters)
                if k.len() < 10 {
                    return Err(Error::Config(format!(
                        "API key for {} appears to be too short (< 10 characters)",
                        service
                    )));
                }
            }
        }

        Ok(())
    }

    /// Get a masked version of the API keys for display purposes
    pub fn get_masked_keys(&self) -> ApiKeys {
        ApiKeys {
            infura: self.infura.as_ref().map(|k| mask_api_key(k)),
            alchemy: self.alchemy.as_ref().map(|k| mask_api_key(k)),
        }
    }

    /// Clear all API keys
    pub fn clear_all(&mut self) {
        self.infura = None;
        self.alchemy = None;
    }

    /// Get list of all supported services
    pub fn supported_services() -> Vec<&'static str> {
        vec!["infura", "alchemy"]
    }
}

/// Mask an API key for display purposes
fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        "*".repeat(key.len())
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}

/// Format an API key for display (alias for mask_api_key)
pub fn format_api_key(key: &str) -> String {
    mask_api_key(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_new_api_keys() {
        let keys = ApiKeys::new();
        assert!(keys.infura.is_none());
        assert!(keys.alchemy.is_none());
    }

    #[test]
    fn test_set_and_get_api_key() {
        let mut keys = ApiKeys::new();

        keys.set_api_key("infura", "test_key_123".to_string())
            .unwrap();
        assert_eq!(
            keys.get_api_key("infura"),
            Some(&"test_key_123".to_string())
        );

        let result = keys.set_api_key("invalid_service", "key".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_has_any_keys() {
        let mut keys = ApiKeys::new();
        assert!(!keys.has_any_keys());

        keys.infura = Some("test_key".to_string());
        assert!(keys.has_any_keys());
    }

    #[test]
    fn test_has_key_for_service() {
        let mut keys = ApiKeys::new();
        keys.infura = Some("infura_key".to_string());

        assert!(keys.has_key_for_service("infura"));
        assert!(!keys.has_key_for_service("alchemy"));
    }

    #[test]
    fn test_get_available_rpc_providers() {
        let mut keys = ApiKeys::new();
        keys.infura = Some("infura_key".to_string());
        keys.alchemy = Some("alchemy_key".to_string());

        let providers = keys.get_available_rpc_providers();
        assert_eq!(providers.len(), 2);
        assert!(providers.contains(&"infura".to_string()));
        assert!(providers.contains(&"alchemy".to_string()));
    }

    #[test]
    fn test_get_primary_rpc_provider() {
        let mut keys = ApiKeys::new();
        keys.infura = Some("infura_key".to_string());
        keys.alchemy = Some("alchemy_key".to_string());

        // Test with preference
        assert_eq!(
            keys.get_primary_rpc_provider(Some("alchemy")),
            Some("alchemy".to_string())
        );

        // Test priority order (infura should be preferred)
        assert_eq!(
            keys.get_primary_rpc_provider(None),
            Some("infura".to_string())
        );

        // Test with no keys
        let empty_keys = ApiKeys::new();
        assert_eq!(empty_keys.get_primary_rpc_provider(None), None);
    }

    #[test]
    fn test_validate_keys() {
        let mut keys = ApiKeys::new();

        // Empty keys should be valid
        assert!(keys.validate_keys().is_ok());

        // Valid key
        keys.infura = Some("valid_key_1234567890".to_string());
        assert!(keys.validate_keys().is_ok());

        // Empty key should be invalid
        keys.alchemy = Some("".to_string());
        assert!(keys.validate_keys().is_err());

        // Too short key should be invalid
        keys.alchemy = Some("short".to_string());
        assert!(keys.validate_keys().is_err());
    }

    #[test]
    fn test_mask_api_key() {
        assert_eq!(mask_api_key("1234567890abcdef"), "1234...cdef");
        assert_eq!(mask_api_key("short"), "*****");
        assert_eq!(mask_api_key("12345678"), "********");
    }

    #[test]
    fn test_get_masked_keys() {
        let mut keys = ApiKeys::new();
        keys.alchemy = Some("1234567890abcdef".to_string());
        keys.infura = Some("short".to_string());

        let masked = keys.get_masked_keys();
        assert_eq!(masked.alchemy, Some("1234...cdef".to_string()));
        assert_eq!(masked.infura, Some("*****".to_string()));
    }

    #[test]
    fn test_clear_all() {
        let mut keys = ApiKeys::new();
        keys.alchemy = Some("key1".to_string());
        keys.infura = Some("key2".to_string());

        assert!(keys.has_any_keys());

        keys.clear_all();
        assert!(!keys.has_any_keys());
    }

    #[test]
    fn test_supported_services() {
        let services = ApiKeys::supported_services();
        assert_eq!(services.len(), 2);
        assert!(services.contains(&"infura"));
        assert!(services.contains(&"alchemy"));
    }

    #[test]
    fn test_load_from_env() {
        env::set_var("MANTIS_INFURA_KEY", "env_infura_key");

        let mut keys = ApiKeys::new();
        keys.infura = Some("config_infura_key".to_string());
        keys.alchemy = Some("config_alchemy_key".to_string());

        keys.load_from_env();

        // Environment should override config
        assert_eq!(keys.infura, Some("env_infura_key".to_string()));

        // Config value should remain if no env var
        assert_eq!(keys.alchemy, Some("config_alchemy_key".to_string()));

        env::remove_var("MANTIS_INFURA_KEY");
    }
}
