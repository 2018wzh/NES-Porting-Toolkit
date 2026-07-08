//! nes-tools: configuration helpers
//!
//! Utilities for loading CLI tool configurations (logging level, default
//! paths, etc.) from TOML or JSON files.

use std::path::Path;

/// General CLI tool configuration.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ToolConfig {
    /// Default ROM search path.
    #[serde(default = "default_roms_path")]
    pub roms_path: String,

    /// Default profile search path.
    #[serde(default = "default_profiles_path")]
    pub profiles_path: String,

    /// Default output directory for generated/exported files.
    #[serde(default = "default_output_path")]
    pub output_path: String,

    /// Log level (trace, debug, info, warn, error).
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_roms_path() -> String { "roms".into() }
fn default_profiles_path() -> String { "profiles".into() }
fn default_output_path() -> String { "generated".into() }
fn default_log_level() -> String { "info".into() }

impl Default for ToolConfig {
    fn default() -> Self {
        ToolConfig {
            roms_path: default_roms_path(),
            profiles_path: default_profiles_path(),
            output_path: default_output_path(),
            log_level: default_log_level(),
        }
    }
}

/// Load tool configuration from a TOML file, falling back to defaults.
pub fn load_tool_config(path: Option<&Path>) -> ToolConfig {
    let path = match path {
        Some(p) if p.exists() => p,
        _ => return ToolConfig::default(),
    };

    match std::fs::read_to_string(path) {
        Ok(contents) => {
            match toml::de::from_str::<ToolConfig>(&contents) {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::warn!("Failed to parse tool config '{}': {}. Using defaults.", path.display(), e);
                    ToolConfig::default()
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to read tool config '{}': {}. Using defaults.", path.display(), e);
            ToolConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = ToolConfig::default();
        assert_eq!(cfg.roms_path, "roms");
        assert_eq!(cfg.profiles_path, "profiles");
        assert_eq!(cfg.output_path, "generated");
        assert_eq!(cfg.log_level, "info");
    }

    #[test]
    fn test_load_missing_file() {
        let cfg = load_tool_config(Some(Path::new("nonexistent.toml")));
        // Should fall back to defaults
        assert_eq!(cfg.roms_path, "roms");
    }
}
