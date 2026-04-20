//! Environment variable loading utilities for configuration.
//!
//! This module provides generic utilities for loading configuration values
//! from environment variables with proper fallback handling.

use log::debug;

/// Generic environment variable loader that handles fallback logic
pub struct EnvLoader;

impl EnvLoader {
    /// Load an API key from environment variables with fallback to config value
    pub fn load_api_key(
        env_var_name: &str,
        service_name: &str,
        config_value: Option<String>,
    ) -> Option<String> {
        if let Ok(key) = std::env::var(env_var_name) {
            if !key.is_empty() {
                debug!("Loaded {} API key from environment variable", service_name);
                return Some(key);
            }
        }

        if config_value.is_some() {
            debug!("Using {} API key from config file", service_name);
        }

        config_value
    }

    /// Load all API keys from environment variables
    pub fn load_all_api_keys(
        infura: Option<String>,
        alchemy: Option<String>,
    ) -> (Option<String>, Option<String>) {
        let infura = Self::load_api_key("MANTIS_INFURA_KEY", "Infura", infura);
        let alchemy = Self::load_api_key("MANTIS_ALCHEMY_KEY", "Alchemy", alchemy);

        (infura, alchemy)
    }

    /// Load a string value from environment variable with fallback
    pub fn load_string(env_var_name: &str, fallback: String) -> String {
        std::env::var(env_var_name).unwrap_or(fallback)
    }

    /// Load an optional string value from environment variable
    pub fn load_optional_string(env_var_name: &str) -> Option<String> {
        std::env::var(env_var_name).ok().filter(|s| !s.is_empty())
    }

    /// Load a boolean value from environment variable with fallback
    pub fn load_bool(env_var_name: &str, fallback: bool) -> bool {
        std::env::var(env_var_name)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(fallback)
    }

    /// Load a numeric value from environment variable with fallback
    pub fn load_numeric<T>(env_var_name: &str, fallback: T) -> T
    where
        T: std::str::FromStr + Copy,
    {
        std::env::var(env_var_name)
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(fallback)
    }

    /// Get Redis URL with fallback logic
    pub fn get_redis_url(config_url: Option<&String>, cache_enabled: bool) -> Option<String> {
        // If cache is enabled and config has a valid Redis URL, use it
        if cache_enabled {
            if let Some(url) = config_url {
                if url.starts_with("redis://") {
                    return Some(url.clone());
                } else {
                    log::warn!("Invalid Redis URL format: {}", url);
                }
            }
        }

        // Try fallback to environment variable
        if let Ok(url) = std::env::var("REDIS_URL") {
            if !url.is_empty() {
                debug!("Using Redis URL from environment variable");
                return Some(url);
            }
        }

        None
    }
}

/// Trait for types that can load their values from environment variables
pub trait FromEnv {
    /// Load configuration values from environment variables
    fn load_from_env(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_load_api_key_from_env() {
        env::set_var("TEST_API_KEY", "env_value");

        let result =
            EnvLoader::load_api_key("TEST_API_KEY", "Test", Some("config_value".to_string()));

        assert_eq!(result, Some("env_value".to_string()));
        env::remove_var("TEST_API_KEY");
    }

    #[test]
    fn test_load_api_key_from_config() {
        env::remove_var("TEST_API_KEY_2");

        let result =
            EnvLoader::load_api_key("TEST_API_KEY_2", "Test", Some("config_value".to_string()));

        assert_eq!(result, Some("config_value".to_string()));
    }

    #[test]
    fn test_load_api_key_none() {
        env::remove_var("TEST_API_KEY_3");

        let result = EnvLoader::load_api_key("TEST_API_KEY_3", "Test", None);

        assert_eq!(result, None);
    }

    #[test]
    fn test_load_string() {
        env::set_var("TEST_STRING", "env_string");

        let result = EnvLoader::load_string("TEST_STRING", "fallback".to_string());

        assert_eq!(result, "env_string");
        env::remove_var("TEST_STRING");
    }

    #[test]
    fn test_load_string_fallback() {
        env::remove_var("TEST_STRING_2");

        let result = EnvLoader::load_string("TEST_STRING_2", "fallback".to_string());

        assert_eq!(result, "fallback");
    }

    #[test]
    fn test_load_bool() {
        env::set_var("TEST_BOOL", "true");

        let result = EnvLoader::load_bool("TEST_BOOL", false);

        assert!(result);
        env::remove_var("TEST_BOOL");
    }

    #[test]
    fn test_load_numeric() {
        env::set_var("TEST_NUM", "42");

        let result = EnvLoader::load_numeric("TEST_NUM", 0u32);

        assert_eq!(result, 42);
        env::remove_var("TEST_NUM");
    }

    #[test]
    fn test_get_redis_url_from_config() {
        let config_url = Some("redis://localhost:6379".to_string());

        let result = EnvLoader::get_redis_url(config_url.as_ref(), true);

        assert_eq!(result, Some("redis://localhost:6379".to_string()));
    }

    #[test]
    fn test_get_redis_url_from_env() {
        env::set_var("REDIS_URL", "redis://env:6379");

        let result = EnvLoader::get_redis_url(None, false);

        assert_eq!(result, Some("redis://env:6379".to_string()));
        env::remove_var("REDIS_URL");
    }
}
