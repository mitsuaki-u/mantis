//! Configuration file I/O operations.
//!
//! This module handles loading and saving configuration files,
//! including path resolution and file system operations.

use crate::error::{Error, Result};
use log::debug;
use std::fs;
use std::path::PathBuf;

/// Configuration file management utilities
pub struct ConfigIO;

impl ConfigIO {
    /// Get the platform-specific path to the configuration file
    pub fn get_config_path() -> Result<PathBuf> {
        Self::get_config_path_internal()
    }

    /// Internal implementation of config path resolution
    fn get_config_path_internal() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().ok_or_else(|| {
            Error::Config("Could not determine configuration directory".to_string())
        })?;

        let app_config_dir = config_dir.join(crate::config::APP_NAME);

        // Create directory if it doesn't exist
        if !app_config_dir.exists() {
            fs::create_dir_all(&app_config_dir)
                .map_err(|e| Error::Config(format!("Failed to create config directory: {}", e)))?;
        }

        Ok(app_config_dir.join(crate::config::CONFIG_FILENAME))
    }

    /// Load configuration from file
    pub fn load_from_file<T>(path: &PathBuf) -> Result<T>
    where
        T: serde::de::DeserializeOwned + Default,
    {
        if path.exists() {
            let contents = fs::read_to_string(path)
                .map_err(|e| Error::Config(format!("Failed to read config file: {}", e)))?;

            serde_json::from_str(&contents)
                .map_err(|e| Error::Config(format!("Failed to parse config file: {}", e)))
        } else {
            debug!("Config file not found at {:?}, using defaults", path);
            Ok(T::default())
        }
    }

    /// Save configuration to file
    pub fn save_to_file<T>(config: &T, path: &PathBuf) -> Result<()>
    where
        T: serde::Serialize,
    {
        // Ensure directory exists
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| Error::Config(format!("Failed to create config directory: {}", e)))?;
        }

        let contents = serde_json::to_string_pretty(config)
            .map_err(|e| Error::Config(format!("Failed to serialize config: {}", e)))?;

        fs::write(path, contents)
            .map_err(|e| Error::Config(format!("Failed to write config: {}", e)))?;

        debug!("Configuration saved to {:?}", path);
        Ok(())
    }

    /// Check if configuration file exists
    pub fn config_exists() -> Result<bool> {
        let config_path = Self::get_config_path()?;
        Ok(config_path.exists())
    }

    /// Get configuration file size in bytes
    pub fn get_config_size() -> Result<u64> {
        let config_path = Self::get_config_path()?;
        if config_path.exists() {
            let metadata = fs::metadata(&config_path)
                .map_err(|e| Error::Config(format!("Failed to get config file metadata: {}", e)))?;
            Ok(metadata.len())
        } else {
            Ok(0)
        }
    }

    /// Backup existing configuration file
    pub fn backup_config() -> Result<PathBuf> {
        let config_path = Self::get_config_path()?;
        if !config_path.exists() {
            return Err(Error::Config("No configuration file to backup".to_string()));
        }

        let backup_path = config_path.with_extension("json.backup");
        fs::copy(&config_path, &backup_path)
            .map_err(|e| Error::Config(format!("Failed to backup config file: {}", e)))?;

        debug!("Configuration backed up to {:?}", backup_path);
        Ok(backup_path)
    }

    /// Restore configuration from backup
    pub fn restore_from_backup() -> Result<()> {
        let config_path = Self::get_config_path()?;
        let backup_path = config_path.with_extension("json.backup");

        if !backup_path.exists() {
            return Err(Error::Config("No backup file found".to_string()));
        }

        fs::copy(&backup_path, &config_path)
            .map_err(|e| Error::Config(format!("Failed to restore from backup: {}", e)))?;

        debug!("Configuration restored from backup");
        Ok(())
    }

    /// Delete configuration file
    pub fn delete_config() -> Result<()> {
        let config_path = Self::get_config_path()?;
        if config_path.exists() {
            fs::remove_file(&config_path)
                .map_err(|e| Error::Config(format!("Failed to delete config file: {}", e)))?;
            debug!("Configuration file deleted");
        }
        Ok(())
    }

    /// Validate configuration file format
    pub fn validate_config_file() -> Result<()> {
        let config_path = Self::get_config_path()?;
        if !config_path.exists() {
            return Err(Error::Config(
                "Configuration file does not exist".to_string(),
            ));
        }

        let contents = fs::read_to_string(&config_path)
            .map_err(|e| Error::Config(format!("Failed to read config file: {}", e)))?;

        // Try to parse as JSON to validate format
        serde_json::from_str::<serde_json::Value>(&contents)
            .map_err(|e| Error::Config(format!("Invalid JSON in config file: {}", e)))?;

        Ok(())
    }

    /// Get configuration directory path
    pub fn get_config_dir() -> Result<PathBuf> {
        let config_dir = dirs::config_dir().ok_or_else(|| {
            Error::Config("Could not determine configuration directory".to_string())
        })?;

        Ok(config_dir.join(crate::config::APP_NAME))
    }

    /// List all configuration-related files in the config directory
    pub fn list_config_files() -> Result<Vec<PathBuf>> {
        let config_dir = Self::get_config_dir()?;
        if !config_dir.exists() {
            return Ok(vec![]);
        }

        let mut files = Vec::new();
        let entries = fs::read_dir(&config_dir)
            .map_err(|e| Error::Config(format!("Failed to read config directory: {}", e)))?;

        for entry in entries {
            let entry = entry
                .map_err(|e| Error::Config(format!("Failed to read directory entry: {}", e)))?;
            let path = entry.path();

            if path.is_file() {
                if let Some(extension) = path.extension() {
                    if extension == "json" || extension == "backup" {
                        files.push(path);
                    }
                }
            }
        }

        files.sort();
        Ok(files)
    }

    /// Get configuration file info
    pub fn get_config_info() -> Result<ConfigFileInfo> {
        let config_path = Self::get_config_path()?;

        if !config_path.exists() {
            return Ok(ConfigFileInfo {
                path: config_path,
                exists: false,
                size: 0,
                created: None,
                modified: None,
                readable: false,
                writable: false,
            });
        }

        let metadata = fs::metadata(&config_path)
            .map_err(|e| Error::Config(format!("Failed to get config file metadata: {}", e)))?;

        let created = metadata.created().ok();
        let modified = metadata.modified().ok();

        // Test readability and writability
        let readable = fs::File::open(&config_path).is_ok();
        let writable = fs::OpenOptions::new()
            .write(true)
            .open(&config_path)
            .is_ok();

        Ok(ConfigFileInfo {
            path: config_path,
            exists: true,
            size: metadata.len(),
            created,
            modified,
            readable,
            writable,
        })
    }
}

/// Configuration file information
#[derive(Debug)]
pub struct ConfigFileInfo {
    pub path: PathBuf,
    pub exists: bool,
    pub size: u64,
    pub created: Option<std::time::SystemTime>,
    pub modified: Option<std::time::SystemTime>,
    pub readable: bool,
    pub writable: bool,
}

impl ConfigFileInfo {
    /// Get a human-readable description of the config file status
    pub fn status_description(&self) -> String {
        if !self.exists {
            format!("Configuration file does not exist at {:?}", self.path)
        } else {
            let size_str = if self.size < 1024 {
                format!("{} bytes", self.size)
            } else {
                format!("{:.1} KB", self.size as f64 / 1024.0)
            };

            let permissions = match (self.readable, self.writable) {
                (true, true) => "read-write",
                (true, false) => "read-only",
                (false, true) => "write-only",
                (false, false) => "no access",
            };

            format!(
                "Configuration file exists at {:?} ({}, {})",
                self.path, size_str, permissions
            )
        }
    }
}

/// Constants used by the config I/O system
pub(crate) mod constants {
    pub const APP_NAME: &str = "mantis";
    pub const CONFIG_FILENAME: &str = "config.json";
}

// Re-export constants at the parent module level
pub use constants::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::env;

    #[derive(Debug, Serialize, Deserialize, Default, PartialEq)]
    struct TestConfig {
        pub name: String,
        pub value: i32,
    }

    #[test]
    fn test_get_config_path() {
        let path = ConfigIO::get_config_path().unwrap();
        assert!(path.to_string_lossy().contains("mantis"));
        assert!(path.to_string_lossy().contains("config.json"));
    }

    #[test]
    fn test_config_exists() {
        // This test depends on whether a config file actually exists
        let exists = ConfigIO::config_exists().unwrap();
        let _ = exists; // Just ensure it doesn't panic
    }

    #[test]
    fn test_get_config_dir() {
        let dir = ConfigIO::get_config_dir().unwrap();
        assert!(dir.to_string_lossy().contains("mantis"));
    }

    #[test]
    fn test_save_and_load_config() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("test_config.json");

        let original_config = TestConfig {
            name: "test".to_string(),
            value: 42,
        };

        // Save config
        ConfigIO::save_to_file(&original_config, &test_path).unwrap();

        // Load config
        let loaded_config: TestConfig = ConfigIO::load_from_file(&test_path).unwrap();

        assert_eq!(original_config, loaded_config);

        // Cleanup
        let _ = fs::remove_file(&test_path);
    }

    #[test]
    fn test_load_nonexistent_config() {
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("nonexistent_config.json");

        // Ensure file doesn't exist
        let _ = fs::remove_file(&test_path);

        // Should return default config
        let config: TestConfig = ConfigIO::load_from_file(&test_path).unwrap();
        assert_eq!(config, TestConfig::default());
    }

    #[test]
    fn test_backup_and_restore() {
        let temp_dir = env::temp_dir();
        let config_path = temp_dir.join("test_config_backup.json");
        let backup_path = temp_dir.join("test_config_backup.json.backup");

        let original_config = TestConfig {
            name: "original".to_string(),
            value: 100,
        };

        // Create original config
        ConfigIO::save_to_file(&original_config, &config_path).unwrap();

        // Create backup (simulate by copying to backup location)
        fs::copy(&config_path, &backup_path).unwrap();

        // Modify original
        let modified_config = TestConfig {
            name: "modified".to_string(),
            value: 200,
        };
        ConfigIO::save_to_file(&modified_config, &config_path).unwrap();

        // Restore from backup
        fs::copy(&backup_path, &config_path).unwrap();

        // Verify restoration
        let restored_config: TestConfig = ConfigIO::load_from_file(&config_path).unwrap();
        assert_eq!(restored_config, original_config);

        // Cleanup
        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_file(&backup_path);
    }

    #[test]
    fn test_get_config_info() {
        let info = ConfigIO::get_config_info().unwrap();
        // Just ensure it doesn't panic and returns reasonable data
        assert!(info.path.to_string_lossy().contains("config.json"));

        if info.exists {
            assert!(info.readable || info.writable); // Should have some access if it exists
        }
    }

    #[test]
    fn test_config_file_info_status_description() {
        let info = ConfigFileInfo {
            path: PathBuf::from("/test/config.json"),
            exists: false,
            size: 0,
            created: None,
            modified: None,
            readable: false,
            writable: false,
        };

        let description = info.status_description();
        assert!(description.contains("does not exist"));

        let info_exists = ConfigFileInfo {
            path: PathBuf::from("/test/config.json"),
            exists: true,
            size: 1024,
            created: None,
            modified: None,
            readable: true,
            writable: true,
        };

        let description = info_exists.status_description();
        assert!(description.contains("1.0 KB"));
        assert!(description.contains("read-write"));
    }
}
