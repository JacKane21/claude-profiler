use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

pub const ENV_AUTH_TOKEN: &str = "ANTHROPIC_AUTH_TOKEN";
pub const ENV_BASE_URL: &str = "ANTHROPIC_BASE_URL";
pub const ENV_DEFAULT_HAIKU_MODEL: &str = "ANTHROPIC_DEFAULT_HAIKU_MODEL";
pub const ENV_DEFAULT_SONNET_MODEL: &str = "ANTHROPIC_DEFAULT_SONNET_MODEL";
pub const ENV_DEFAULT_OPUS_MODEL: &str = "ANTHROPIC_DEFAULT_OPUS_MODEL";
pub const ENV_MODEL: &str = "ANTHROPIC_MODEL";
pub const ENV_SMALL_FAST_MODEL: &str = "ANTHROPIC_SMALL_FAST_MODEL";
pub const ENV_DISABLE_NONESSENTIAL_TRAFFIC: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC";
pub const ENV_API_TIMEOUT_MS: &str = "API_TIMEOUT_MS";

/// A single profile configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Unique profile name (used as the identifier)
    pub name: String,

    /// Human-readable description
    #[serde(default)]
    pub description: String,

    /// Environment variables to set when launching Claude Code
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Root configuration file structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// List of all profiles
    #[serde(default)]
    pub profiles: Vec<Profile>,

    /// Name of the default profile to select on startup
    #[serde(default)]
    pub default_profile: Option<String>,
}

impl Config {
    /// Returns the default config directory path (macOS)
    /// ~/Library/Application Support/claude-profiler
    pub fn config_dir() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("claude-profiler"))
    }

    /// Returns the full path to the config file
    pub fn config_file_path() -> Option<PathBuf> {
        Self::config_dir().map(|p| p.join("profiles.toml"))
    }

    /// Load config from disk, creating default if not exists
    pub fn load() -> Result<Self> {
        let config_path =
            Self::config_file_path().context("Could not determine config directory")?;

        if !config_path.exists() {
            // Create default config
            let config = Self::create_default();
            config.save()?;
            return Ok(config);
        }

        let contents = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;

        Ok(config)
    }

    /// Save config to disk
    pub fn save(&self) -> Result<()> {
        let config_dir = Self::config_dir().context("Could not determine config directory")?;

        // Create the directory if it doesn't exist
        fs::create_dir_all(&config_dir).with_context(|| {
            format!(
                "Failed to create config directory: {}",
                config_dir.display()
            )
        })?;

        let config_path =
            Self::config_file_path().context("Could not determine config file path")?;

        let contents = toml::to_string_pretty(self).context("Failed to serialize config")?;

        fs::write(&config_path, contents)
            .with_context(|| format!("Failed to write config file: {}", config_path.display()))?;

        Ok(())
    }

    /// Create a default config with example profiles
    pub fn create_default() -> Self {
        Config {
            default_profile: Some("default".to_string()),
            profiles: vec![
                Profile {
                    name: "default".to_string(),
                    description: "Default profile - uses existing environment".to_string(),
                    env: HashMap::new(),
                },
                Profile {
                    name: "zai".to_string(),
                    description: "Z.ai API proxy (edit profiles.toml to add your API key)"
                        .to_string(),
                    env: HashMap::from([
                        (
                            ENV_AUTH_TOKEN.to_string(),
                            "YOUR_ZAI_API_KEY_HERE".to_string(),
                        ),
                        (
                            ENV_BASE_URL.to_string(),
                            "https://api.z.ai/api/anthropic".to_string(),
                        ),
                        (
                            ENV_DEFAULT_HAIKU_MODEL.to_string(),
                            "glm-4.5-air".to_string(),
                        ),
                        (ENV_DEFAULT_SONNET_MODEL.to_string(), "glm-4.7".to_string()),
                        (ENV_DEFAULT_OPUS_MODEL.to_string(), "glm-4.7".to_string()),
                        (ENV_API_TIMEOUT_MS.to_string(), "3000000".to_string()),
                    ]),
                },
                Profile {
                    name: "minimax".to_string(),
                    description: "MiniMax API proxy (edit profiles.toml to add your API key)"
                        .to_string(),
                    env: HashMap::from([
                        (
                            ENV_AUTH_TOKEN.to_string(),
                            "YOUR_MINIMAX_API_KEY_HERE".to_string(),
                        ),
                        (
                            ENV_BASE_URL.to_string(),
                            "https://api.minimax.io/anthropic".to_string(),
                        ),
                        (ENV_MODEL.to_string(), "MiniMax-M2.1".to_string()),
                        (ENV_SMALL_FAST_MODEL.to_string(), "MiniMax-M2.1".to_string()),
                        (
                            ENV_DEFAULT_HAIKU_MODEL.to_string(),
                            "MiniMax-M2.1".to_string(),
                        ),
                        (
                            ENV_DEFAULT_SONNET_MODEL.to_string(),
                            "MiniMax-M2.1".to_string(),
                        ),
                        (
                            ENV_DEFAULT_OPUS_MODEL.to_string(),
                            "MiniMax-M2.1".to_string(),
                        ),
                        (
                            ENV_DISABLE_NONESSENTIAL_TRAFFIC.to_string(),
                            "1".to_string(),
                        ),
                        (ENV_API_TIMEOUT_MS.to_string(), "3000000".to_string()),
                    ]),
                },
                Profile {
                    name: "lmstudio".to_string(),
                    description: "Local models via LMStudio (press 'l' to select model)"
                        .to_string(),
                    env: HashMap::new(),
                },
            ],
        }
    }

    /// Get the index of the default profile
    pub fn default_profile_index(&self) -> usize {
        if let Some(ref name) = self.default_profile {
            self.profiles
                .iter()
                .position(|p| &p.name == name)
                .unwrap_or(0)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_index_uses_named_default() {
        let config = Config::create_default();
        assert_eq!(config.default_profile_index(), 0);
    }

    #[test]
    fn default_profile_index_falls_back_when_missing() {
        let config = Config {
            profiles: vec![Profile {
                name: "first".to_string(),
                description: String::new(),
                env: HashMap::new(),
            }],
            default_profile: Some("missing".to_string()),
        };
        assert_eq!(config.default_profile_index(), 0);
    }
}
