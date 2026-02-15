//! Application configuration.
//!
//! The configuration is loaded from a JSON file whose path is passed on the
//! command line (`--config <path>`).  The top-level schema uses a `"gestures"`
//! key so the file can be extended with additional sections later without
//! breaking backward compatibility.
//!
//! # Example
//!
//! ```json
//! {
//!   "gestures": {
//!     "sensitivity": 200.0,
//!     "commit_threshold": 0.3,
//!     "commit_while_dragging_threshold": 0.8,
//!     "switch_fingers": 3,
//!     "move_fingers": 4,
//!     "natural_swiping": true
//!   }
//! }
//! ```

use crate::hyprland::gestures::GestureConfig;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level configuration.
///
/// Every field is optional — a minimal `{}` file is valid and all sections
/// fall back to their compiled-in defaults.
///
/// # Example
///
/// ```json
/// {
///   "gestures": { "sensitivity": 200.0 },
///   "visualizer": {
///     "cursor_animation_ms": 80,
///     "linger_ms": 300,
///     "fade_out_ms": 200
///   }
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Gesture recognition settings.
    #[serde(default)]
    pub gestures: GestureConfig,

    /// Visualizer overlay timing and animation settings.
    #[serde(default)]
    pub visualizer: VisualizerConfig,
}

/// Visualizer overlay timing and animation settings.
///
/// All durations are in **milliseconds**.  Set a value to `0` to disable
/// that particular animation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualizerConfig {
    /// How long the cursor takes to glide between cells (ms).
    pub cursor_animation_ms: u64,
    /// How long the overlay stays fully opaque after a workspace switch,
    /// before the fade-out begins (ms).
    pub linger_ms: u64,
    /// Duration of the fade-out animation (ms).  Set to `0` for an
    /// instant hide.
    pub fade_out_ms: u64,
}

impl Default for VisualizerConfig {
    fn default() -> Self {
        Self {
            cursor_animation_ms: 80,
            linger_ms: 300,
            fade_out_ms: 200,
        }
    }
}

impl Config {
    /// Load configuration from a JSON file at `path`.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| ConfigError(format!("failed to read {}: {}", path.display(), e)))?;
        let config: Self = serde_json::from_str(&contents)
            .map_err(|e| ConfigError(format!("failed to parse {}: {}", path.display(), e)))?;
        Ok(config)
    }
}

/// Error from loading or parsing a configuration file.
#[derive(Debug, thiserror::Error)]
#[error("config error: {0}")]
pub struct ConfigError(String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_full_config() {
        let json = r#"{
            "gestures": {
                "sensitivity": 150.0,
                "commit_threshold": 0.5,
                "switch_fingers": 3,
                "move_fingers": 4
            },
            "visualizer": {
                "cursor_animation_ms": 100,
                "linger_ms": 500,
                "fade_out_ms": 300
            }
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.gestures.sensitivity, 150.0);
        assert_eq!(cfg.gestures.commit_threshold, 0.5);
        assert_eq!(cfg.visualizer.cursor_animation_ms, 100);
        assert_eq!(cfg.visualizer.linger_ms, 500);
        assert_eq!(cfg.visualizer.fade_out_ms, 300);
    }

    #[test]
    fn deserialize_empty_uses_defaults() {
        let json = "{}";
        let cfg: Config = serde_json::from_str(json).unwrap();
        let gd = GestureConfig::default();
        assert_eq!(cfg.gestures.sensitivity, gd.sensitivity);
        assert_eq!(cfg.gestures.commit_threshold, gd.commit_threshold);
        assert_eq!(cfg.gestures.commit_while_dragging_threshold, gd.commit_while_dragging_threshold);
        assert_eq!(cfg.gestures.switch_fingers, gd.switch_fingers);
        assert_eq!(cfg.gestures.move_fingers, gd.move_fingers);
        assert_eq!(cfg.gestures.natural_swiping, gd.natural_swiping);
        let vd = VisualizerConfig::default();
        assert_eq!(cfg.visualizer.cursor_animation_ms, vd.cursor_animation_ms);
        assert_eq!(cfg.visualizer.linger_ms, vd.linger_ms);
        assert_eq!(cfg.visualizer.fade_out_ms, vd.fade_out_ms);
    }

    #[test]
    fn deserialize_partial_gestures() {
        let json = r#"{ "gestures": { "sensitivity": 300.0 } }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.gestures.sensitivity, 300.0);
        let defaults = GestureConfig::default();
        assert_eq!(cfg.gestures.commit_threshold, defaults.commit_threshold);
    }

    #[test]
    fn deserialize_partial_visualizer() {
        let json = r#"{ "visualizer": { "linger_ms": 600 } }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.visualizer.linger_ms, 600);
        let vd = VisualizerConfig::default();
        assert_eq!(cfg.visualizer.cursor_animation_ms, vd.cursor_animation_ms);
        assert_eq!(cfg.visualizer.fade_out_ms, vd.fade_out_ms);
    }

    #[test]
    fn unknown_top_level_keys_ignored() {
        let json = r#"{ "gestures": {}, "future_section": { "key": 42 } }"#;
        // Should not fail — unknown keys are silently ignored.
        let _cfg: Config = serde_json::from_str(json).unwrap();
    }
}


