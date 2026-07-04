use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const DEFAULT_CONFIG: &str = include_str!("../../config/default.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub cps: CpsConfig,
    pub click: ClickConfig,
    pub hotkey: HotkeyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpsConfig {
    pub default: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickConfig {
    pub button: String,
    pub jitter_enabled: bool,
    pub jitter_min_ms: u32,
    pub jitter_max_ms: u32,
    pub burst_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub key: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        toml::from_str(DEFAULT_CONFIG).expect("embedded default config must be valid")
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mut cfg) = toml::from_str::<AppConfig>(&content) {
                    // Validate and sanitize loaded config
                    cfg.validate_and_fix();
                    return cfg;
                }
            }
        }
        AppConfig::default()
    }

    /// Validate and fix config values to safe ranges
    fn validate_and_fix(&mut self) {
        // Clamp CPS to safe range (1-1000)
        self.cps.default = self.cps.default.clamp(1, 1000);
        
        // Ensure jitter values are reasonable
        self.click.jitter_min_ms = self.click.jitter_min_ms.clamp(1, 1000);
        self.click.jitter_max_ms = self.click.jitter_max_ms.clamp(1, 1000);
        
        // Ensure jitter_min <= jitter_max
        if self.click.jitter_min_ms > self.click.jitter_max_ms {
            std::mem::swap(&mut self.click.jitter_min_ms, &mut self.click.jitter_max_ms);
        }
        
        // Clamp burst count to reasonable range
        self.click.burst_count = self.click.burst_count.clamp(1, 10000);
        
        // Validate hotkey (reject system-critical keys)
        if !is_valid_hotkey(&self.hotkey.key) {
            tracing::warn!("Invalid hotkey '{}' in config, resetting to F6", self.hotkey.key);
            self.hotkey.key = "F6".to_string();
        }
        
        // Validate mouse button
        if self.click.button != "left" && self.click.button != "right" {
            self.click.button = "left".to_string();
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, content).map_err(|e| e.to_string())
    }
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("clix")
        .join("config.toml")
}

/// Check if a hotkey is valid (can be parsed by the hotkey system).
/// Safety checks against system-critical keys are done at capture time
/// via `is_safe_key` in the hotkey module. This function simply verifies
/// the key string can be resolved to a key code.
fn is_valid_hotkey(key: &str) -> bool {
    crate::system::hotkey::parse_key_code(key).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = AppConfig::default();
        assert_eq!(config.cps.default, 20);
        assert_eq!(config.click.button, "left");
        assert!(!config.click.jitter_enabled);
        assert_eq!(config.click.jitter_min_ms, 5);
        assert_eq!(config.click.jitter_max_ms, 20);
        assert_eq!(config.click.burst_count, 10);
        assert_eq!(config.hotkey.key, "F6");
    }

    #[test]
    fn test_validate_and_fix_cps_bounds() {
        let mut config = AppConfig::default();
        
        // Test upper bound
        config.cps.default = 2000;
        config.validate_and_fix();
        assert_eq!(config.cps.default, 1000);
        
        // Test lower bound
        config.cps.default = 0;
        config.validate_and_fix();
        assert_eq!(config.cps.default, 1);
    }

    #[test]
    fn test_validate_and_fix_jitter_swap() {
        let mut config = AppConfig::default();
        
        // Min > Max should be swapped
        config.click.jitter_min_ms = 100;
        config.click.jitter_max_ms = 10;
        config.validate_and_fix();
        
        assert_eq!(config.click.jitter_min_ms, 10);
        assert_eq!(config.click.jitter_max_ms, 100);
    }

    #[test]
    fn test_validate_and_fix_jitter_bounds() {
        let mut config = AppConfig::default();
        
        config.click.jitter_min_ms = 2000;
        config.click.jitter_max_ms = 3000;
        config.validate_and_fix();
        
        assert_eq!(config.click.jitter_min_ms, 1000);
        assert_eq!(config.click.jitter_max_ms, 1000);
    }

    #[test]
    fn test_validate_and_fix_burst_count() {
        let mut config = AppConfig::default();
        
        // Test lower bound
        config.click.burst_count = 0;
        config.validate_and_fix();
        assert_eq!(config.click.burst_count, 1);
        
        // Test upper bound
        config.click.burst_count = 20000;
        config.validate_and_fix();
        assert_eq!(config.click.burst_count, 10000);
    }

    #[test]
    fn test_validate_and_fix_invalid_button() {
        let mut config = AppConfig::default();
        
        config.click.button = "middle".to_string();
        config.validate_and_fix();
        assert_eq!(config.click.button, "left");
    }

    #[test]
    fn test_validate_and_fix_invalid_hotkey() {
        let mut config = AppConfig::default();
        
        config.hotkey.key = "CTRL+C".to_string();
        config.validate_and_fix();
        assert_eq!(config.hotkey.key, "F6"); // Should reset to default
    }

    #[test]
    fn test_is_valid_hotkey_allowed_keys() {
        assert!(is_valid_hotkey("F6"));
        assert!(is_valid_hotkey("F1"));
        assert!(is_valid_hotkey("F12"));
        assert!(is_valid_hotkey("SPACE"));
        assert!(is_valid_hotkey("TAB"));
        assert!(is_valid_hotkey("ESC"));
        assert!(is_valid_hotkey("ESCAPE"));
        assert!(is_valid_hotkey("ENTER"));
        assert!(is_valid_hotkey("DELETE"));
        assert!(is_valid_hotkey("BACKSPACE"));
        assert!(is_valid_hotkey("UP"));
        assert!(is_valid_hotkey("DOWN"));
        assert!(is_valid_hotkey("LEFT"));
        assert!(is_valid_hotkey("RIGHT"));
        assert!(is_valid_hotkey("HOME"));
        assert!(is_valid_hotkey("END"));
        assert!(is_valid_hotkey("PAGEUP"));
        assert!(is_valid_hotkey("PAGEDOWN"));
        assert!(is_valid_hotkey("INSERT"));
    }

    #[test]
    fn test_is_valid_hotkey_case_insensitive() {
        assert!(is_valid_hotkey("f6"));
        assert!(is_valid_hotkey("F6"));
        assert!(is_valid_hotkey("space"));
        assert!(is_valid_hotkey("SPACE"));
    }

    #[test]
    fn test_is_valid_hotkey_forbidden_keys() {
        assert!(!is_valid_hotkey("CTRL"));
        assert!(!is_valid_hotkey("ALT"));
        assert!(!is_valid_hotkey("SUPER"));
        assert!(!is_valid_hotkey("SHIFT"));
        assert!(!is_valid_hotkey("CTRL+C"));
    }

    #[test]
    fn test_is_valid_hotkey_invalid_keys() {
        assert!(!is_valid_hotkey("F13"));
        assert!(!is_valid_hotkey("INVALID"));
    }

    #[test]
    fn test_config_path() {
        let path = config_path();
        assert!(path.to_string_lossy().contains("clix"));
        assert!(path.to_string_lossy().ends_with("config.toml"));
    }
}
