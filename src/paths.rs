//! Centralised platform-specific path helpers for the `tome` application.
//!
//! All modules should derive their paths from these helpers rather than
//! constructing `dirs::*_dir().join("tome")` independently.

use std::path::PathBuf;

/// `~/Library/Application Support/tome/` (macOS) or equivalent.
pub fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tome")
}

/// `~/Library/Preferences/tome/` (macOS) / `~/.config/tome/` (Linux) or equivalent.
pub fn app_config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tome")
}

/// `~/Library/Caches/tome/` (macOS) or equivalent.
pub fn app_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("tome")
}
