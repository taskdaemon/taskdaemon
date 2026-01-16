//! TaskDaemon configuration types and loading

use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Main TaskDaemon configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// LLM provider configuration
    pub llm: LlmConfig,

    /// Concurrency limits
    pub concurrency: ConcurrencyConfig,

    /// Validation defaults
    pub validation: ValidationConfig,

    /// Git configuration
    pub git: GitConfig,

    /// Storage configuration
    pub storage: StorageConfig,
}

impl Config {
    /// Load configuration with fallback chain
    pub fn load(config_path: Option<&PathBuf>) -> Result<Self> {
        // If explicit config path provided, try to load it
        if let Some(path) = config_path {
            return Self::load_from_file(path).context(format!("Failed to load config from {}", path.display()));
        }

        // Try project-local config: .taskdaemon.yml
        let local_config = PathBuf::from(".taskdaemon.yml");
        if local_config.exists() {
            match Self::load_from_file(&local_config) {
                Ok(config) => return Ok(config),
                Err(e) => {
                    tracing::warn!("Failed to load config from {}: {}", local_config.display(), e);
                }
            }
        }

        // Try user config: ~/.config/taskdaemon/taskdaemon.yml
        if let Some(config_dir) = dirs::config_dir() {
            let user_config = config_dir.join("taskdaemon").join("taskdaemon.yml");
            if user_config.exists() {
                match Self::load_from_file(&user_config) {
                    Ok(config) => return Ok(config),
                    Err(e) => {
                        tracing::warn!("Failed to load config from {}: {}", user_config.display(), e);
                    }
                }
            }
        }

        // No config file found, use defaults
        tracing::info!("No config file found, using defaults");
        Ok(Self::default())
    }

    fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path).context("Failed to read config file")?;

        let config: Self = serde_yaml::from_str(&content).context("Failed to parse config file")?;

        tracing::info!("Loaded config from: {}", path.as_ref().display());
        Ok(config)
    }
}

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Provider name (currently only "anthropic" supported)
    pub provider: String,

    /// Model identifier
    pub model: String,

    /// Environment variable containing the API key
    #[serde(rename = "api-key-env")]
    pub api_key_env: String,

    /// API base URL
    #[serde(rename = "base-url")]
    pub base_url: String,

    /// Maximum tokens per response
    #[serde(rename = "max-tokens")]
    pub max_tokens: u32,

    /// Request timeout in milliseconds
    #[serde(rename = "timeout-ms")]
    pub timeout_ms: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            max_tokens: 16384,
            timeout_ms: 300_000,
        }
    }
}

/// Concurrency limits
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ConcurrencyConfig {
    /// Maximum concurrent loops
    #[serde(rename = "max-loops")]
    pub max_loops: u32,

    /// Maximum concurrent API calls
    #[serde(rename = "max-api-calls")]
    pub max_api_calls: u32,

    /// Maximum worktrees
    #[serde(rename = "max-worktrees")]
    pub max_worktrees: u32,
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            max_loops: 50,
            max_api_calls: 10,
            max_worktrees: 50,
        }
    }
}

/// Validation defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ValidationConfig {
    /// Default validation command
    pub command: String,

    /// Iteration timeout in milliseconds
    #[serde(rename = "iteration-timeout-ms")]
    pub iteration_timeout_ms: u64,

    /// Maximum iterations before giving up
    #[serde(rename = "max-iterations")]
    pub max_iterations: u32,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            command: "otto ci".to_string(),
            iteration_timeout_ms: 300_000,
            max_iterations: 100,
        }
    }
}

/// Git configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GitConfig {
    /// Directory for git worktrees
    #[serde(rename = "worktree-dir")]
    pub worktree_dir: PathBuf,

    /// Disk quota for worktrees in GB
    #[serde(rename = "disk-quota-gb")]
    pub disk_quota_gb: u32,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            worktree_dir: PathBuf::from("/tmp/taskdaemon/worktrees"),
            disk_quota_gb: 100,
        }
    }
}

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Directory for TaskStore data
    #[serde(rename = "taskstore-dir")]
    pub taskstore_dir: String,

    /// Warning threshold for JSONL file size in MB
    #[serde(rename = "jsonl-warn-mb")]
    pub jsonl_warn_mb: u32,

    /// Error threshold for JSONL file size in MB
    #[serde(rename = "jsonl-error-mb")]
    pub jsonl_error_mb: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            taskstore_dir: ".taskstore".to_string(),
            jsonl_warn_mb: 100,
            jsonl_error_mb: 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();

        assert_eq!(config.llm.provider, "anthropic");
        assert_eq!(config.concurrency.max_loops, 50);
        assert_eq!(config.validation.max_iterations, 100);
    }

    #[test]
    fn test_llm_config_defaults() {
        let config = LlmConfig::default();

        assert_eq!(config.provider, "anthropic");
        assert!(config.model.contains("sonnet"));
        assert_eq!(config.api_key_env, "ANTHROPIC_API_KEY");
        assert_eq!(config.base_url, "https://api.anthropic.com");
    }

    #[test]
    fn test_deserialize_config() {
        let yaml = r#"
llm:
  provider: anthropic
  model: claude-opus-4
  api-key-env: MY_API_KEY
  base-url: https://api.example.com
  max-tokens: 8192
  timeout-ms: 60000

concurrency:
  max-loops: 25
  max-api-calls: 5
  max-worktrees: 25

validation:
  command: "make test"
  iteration-timeout-ms: 120000
  max-iterations: 50
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(config.llm.model, "claude-opus-4");
        assert_eq!(config.llm.api_key_env, "MY_API_KEY");
        assert_eq!(config.llm.max_tokens, 8192);
        assert_eq!(config.concurrency.max_loops, 25);
        assert_eq!(config.validation.command, "make test");
        assert_eq!(config.validation.max_iterations, 50);
    }

    #[test]
    fn test_partial_config_uses_defaults() {
        let yaml = r#"
llm:
  model: claude-haiku
"#;

        let config: Config = serde_yaml::from_str(yaml).unwrap();

        // Specified value
        assert_eq!(config.llm.model, "claude-haiku");

        // Defaults for unspecified
        assert_eq!(config.llm.provider, "anthropic");
        assert_eq!(config.llm.api_key_env, "ANTHROPIC_API_KEY");
        assert_eq!(config.concurrency.max_loops, 50);
    }
}
