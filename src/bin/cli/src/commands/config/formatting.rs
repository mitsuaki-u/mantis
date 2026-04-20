//! Formatting utilities for config command display

/// Mask an API key for display purposes
/// Shows first 4 and last 4 characters, masks the rest with asterisks
pub fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        // For very short keys, just show first 2 and last 2
        if key.len() <= 4 {
            return "*".repeat(key.len());
        }
        let first = &key[..2];
        let last = &key[key.len() - 2..];
        format!("{}***{}", first, last)
    } else {
        // For longer keys, show first 4 and last 4
        let first = &key[..4];
        let last = &key[key.len() - 4..];
        format!("{}***{}", first, last)
    }
}

/// Format an API key for display based on whether secrets should be shown
pub fn format_api_key(key: Option<&str>, show_secrets: bool) -> String {
    match key {
        Some(k) if show_secrets => k.to_string(),
        Some(k) => mask_api_key(k),
        None => "Not set".to_string(),
    }
}
