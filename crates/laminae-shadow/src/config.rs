use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::report::VulnSeverity;

/// Minimum severity threshold that triggers automatic self-healing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealThreshold {
    Medium,
    High,
    Critical,
}

impl HealThreshold {
    pub fn matches(&self, severity: VulnSeverity) -> bool {
        match self {
            HealThreshold::Critical => severity >= VulnSeverity::Critical,
            HealThreshold::High => severity >= VulnSeverity::High,
            HealThreshold::Medium => severity >= VulnSeverity::Medium,
        }
    }
}

/// Configuration for the Shadow red-teaming engine.
///
/// Can be loaded from a JSON file or constructed programmatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ShadowConfig {
    /// Master enable/disable.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// 1 = static only, 2 = static + LLM, 3 = static + LLM + sandbox.
    #[serde(default = "default_level")]
    pub aggressiveness: u8,

    /// Whether to run the LLM adversarial reviewer.
    #[serde(default = "default_true")]
    pub llm_review_enabled: bool,

    /// Whether to attempt sandbox execution (requires Docker/Podman).
    #[serde(default)]
    pub sandbox_enabled: bool,

    /// Ollama model for the Shadow reviewer.
    #[serde(default = "default_shadow_model")]
    pub shadow_model: String,

    /// Temperature for the Shadow LLM (low = deterministic threat analysis).
    #[serde(default = "default_temperature")]
    pub temperature: f32,

    /// Max tokens for Shadow LLM response.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: i32,

    /// Severity threshold for auto-healing. None = healing disabled.
    #[serde(default)]
    pub auto_heal_threshold: Option<HealThreshold>,

    /// Docker image for sandbox execution.
    #[serde(default = "default_sandbox_image")]
    pub sandbox_image: String,

    /// TTL in seconds before sandbox container is force-killed.
    #[serde(default = "default_sandbox_ttl")]
    pub sandbox_ttl_secs: u64,

    /// Minimum code block size (chars) to trigger sandbox analysis.
    #[serde(default = "default_min_code_len")]
    pub sandbox_min_code_len: usize,

    /// Maximum characters of output to send to LLM reviewer.
    #[serde(default = "default_max_input_len")]
    pub max_input_len: usize,

    /// Path to config file. If None, uses default location.
    #[serde(skip)]
    pub config_path: Option<PathBuf>,
}

fn default_enabled() -> bool {
    true
}
fn default_level() -> u8 {
    2
}
fn default_true() -> bool {
    true
}
fn default_shadow_model() -> String {
    "qwen2.5:14b".to_string()
}
fn default_temperature() -> f32 {
    0.05
}
fn default_max_tokens() -> i32 {
    2048
}
fn default_sandbox_image() -> String {
    "python:3.12-slim".to_string()
}
fn default_sandbox_ttl() -> u64 {
    30
}
fn default_min_code_len() -> usize {
    100
}
fn default_max_input_len() -> usize {
    4000
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            aggressiveness: default_level(),
            llm_review_enabled: default_true(),
            sandbox_enabled: false,
            shadow_model: default_shadow_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            auto_heal_threshold: None,
            sandbox_image: default_sandbox_image(),
            sandbox_ttl_secs: default_sandbox_ttl(),
            sandbox_min_code_len: default_min_code_len(),
            max_input_len: default_max_input_len(),
            config_path: None,
        }
    }
}

impl ShadowConfig {
    /// Load config from disk, falling back to defaults.
    pub fn load() -> Self {
        Self::load_from(Self::default_config_path())
    }

    /// Load from a specific path.
    pub fn load_from(path: PathBuf) -> Self {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let mut config: Self = match serde_json::from_str(&content) {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse Shadow config at {}: {e}, using defaults",
                            path.display()
                        );
                        Self::default()
                    }
                };
                config.config_path = Some(path);
                config.clamp();
                config
            }
            Err(_) => {
                let mut config = Self {
                    config_path: Some(path),
                    ..Self::default()
                };
                config.clamp();
                config
            }
        }
    }

    /// Persist config to disk.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = self
            .config_path
            .clone()
            .unwrap_or_else(Self::default_config_path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Ensure all values are within sane bounds.
    pub fn clamp(&mut self) {
        self.aggressiveness = self.aggressiveness.clamp(1, 3);
        self.temperature = self.temperature.clamp(0.0, 1.0);
        self.max_tokens = self.max_tokens.clamp(256, 8192);
        self.sandbox_ttl_secs = self.sandbox_ttl_secs.clamp(5, 300);
        self.sandbox_min_code_len = self.sandbox_min_code_len.clamp(20, 10_000);
        self.max_input_len = self.max_input_len.clamp(500, 16_000);
    }

    fn default_config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("laminae/shadow.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_are_sane() {
        let config = ShadowConfig::default();
        assert!(config.enabled);
        assert_eq!(config.aggressiveness, 2);
        assert!(config.llm_review_enabled);
        assert!(!config.sandbox_enabled);
    }

    #[test]
    fn test_clamp_enforces_bounds() {
        let mut config = ShadowConfig {
            aggressiveness: 99,
            temperature: 5.0,
            max_tokens: 0,
            sandbox_ttl_secs: 1,
            ..Default::default()
        };
        config.clamp();
        assert_eq!(config.aggressiveness, 3);
        assert_eq!(config.temperature, 1.0);
        assert_eq!(config.max_tokens, 256);
        assert_eq!(config.sandbox_ttl_secs, 5);
    }

    #[test]
    fn test_roundtrip_serde() {
        let config = ShadowConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ShadowConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.aggressiveness, config.aggressiveness);
        assert_eq!(parsed.shadow_model, config.shadow_model);
    }

    #[test]
    fn test_heal_threshold() {
        assert!(HealThreshold::Critical.matches(VulnSeverity::Critical));
        assert!(!HealThreshold::Critical.matches(VulnSeverity::High));
        assert!(HealThreshold::High.matches(VulnSeverity::Critical));
        assert!(HealThreshold::High.matches(VulnSeverity::High));
    }
}
