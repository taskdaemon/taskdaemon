//! TaskDaemon configuration types and loading

use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Main TaskDaemon configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Log level (TRACE, DEBUG, INFO, WARN, ERROR)
    #[serde(rename = "log-level")]
    pub log_level: Option<String>,

    /// LLM provider configuration
    pub llm: LlmConfig,

    /// Concurrency limits
    pub concurrency: ConcurrencyConfig,

    /// Validation defaults
    pub validation: ValidationConfig,

    /// Progress strategy configuration
    pub progress: ProgressConfig,

    /// Git configuration
    pub git: GitConfig,

    /// Storage configuration
    pub storage: StorageConfig,

    /// Loop type paths configuration
    pub loops: LoopsConfig,

    /// Debug configuration
    pub debug: DebugConfig,
}

impl Config {
    /// Load just the log level from config files (for early logging setup)
    ///
    /// Searches for config files in the standard locations and returns the
    /// log-level value if found. This is called before full config loading
    /// to enable proper logging during startup.
    pub fn load_log_level(config_path: Option<&PathBuf>) -> Option<String> {
        // Note: Cannot use debug! here since logging isn't initialized yet
        // Helper to extract log-level from a file
        fn extract_log_level(path: &Path) -> Option<String> {
            let content = fs::read_to_string(path).ok()?;
            // Quick YAML parse just for log-level
            #[derive(Deserialize)]
            struct LogLevelOnly {
                #[serde(rename = "log-level")]
                log_level: Option<String>,
            }
            let parsed: LogLevelOnly = serde_yaml::from_str(&content).ok()?;
            parsed.log_level
        }

        // If explicit config path provided, try it
        if let Some(path) = config_path
            && let Some(level) = extract_log_level(path)
        {
            return Some(level);
        }

        // Try project-local config: .taskdaemon.yml
        let local_config = PathBuf::from(".taskdaemon.yml");
        if local_config.exists()
            && let Some(level) = extract_log_level(&local_config)
        {
            return Some(level);
        }

        // Try user config: ~/.config/taskdaemon/taskdaemon.yml
        if let Some(config_dir) = dirs::config_dir() {
            let user_config = config_dir.join("taskdaemon").join("taskdaemon.yml");
            if user_config.exists()
                && let Some(level) = extract_log_level(&user_config)
            {
                return Some(level);
            }
        }

        None
    }

    /// Validate configuration before use
    ///
    /// Checks that required environment variables and paths are set correctly.
    /// Call this early in startup to fail fast with clear error messages.
    pub fn validate(&self) -> Result<()> {
        debug!("Config::validate: called");
        // Check LLM API key is available (from env or file)
        self.llm.get_api_key().context("LLM API key validation failed")?;
        debug!("Config::validate: validation passed");
        Ok(())
    }

    /// Load configuration with fallback chain
    pub fn load(config_path: Option<&PathBuf>) -> Result<Self> {
        debug!(?config_path, "Config::load: called");
        // If explicit config path provided, try to load it
        if let Some(path) = config_path {
            debug!(?path, "Config::load: explicit config path provided");
            return Self::load_from_file(path).context(format!("Failed to load config from {}", path.display()));
        }

        // Try project-local config: .taskdaemon.yml
        let local_config = PathBuf::from(".taskdaemon.yml");
        if local_config.exists() {
            debug!(?local_config, "Config::load: trying local config");
            match Self::load_from_file(&local_config) {
                Ok(config) => {
                    debug!("Config::load: loaded from local config");
                    return Ok(config);
                }
                Err(e) => {
                    debug!(error = %e, "Config::load: failed to load local config");
                    tracing::warn!("Failed to load config from {}: {}", local_config.display(), e);
                }
            }
        } else {
            debug!(?local_config, "Config::load: local config does not exist");
        }

        // Try user config: ~/.config/taskdaemon/taskdaemon.yml
        if let Some(config_dir) = dirs::config_dir() {
            let user_config = config_dir.join("taskdaemon").join("taskdaemon.yml");
            if user_config.exists() {
                debug!(?user_config, "Config::load: trying user config");
                match Self::load_from_file(&user_config) {
                    Ok(config) => {
                        debug!("Config::load: loaded from user config");
                        return Ok(config);
                    }
                    Err(e) => {
                        debug!(error = %e, "Config::load: failed to load user config");
                        tracing::warn!("Failed to load config from {}: {}", user_config.display(), e);
                    }
                }
            } else {
                debug!(?user_config, "Config::load: user config does not exist");
            }
        }

        // No config file found, use defaults
        debug!("Config::load: no config file found, using defaults");
        tracing::info!("No config file found, using defaults");
        Ok(Self::default())
    }

    fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        debug!(path = %path.as_ref().display(), "Config::load_from_file: called");
        let content = fs::read_to_string(&path).context("Failed to read config file")?;
        debug!("Config::load_from_file: file read successfully");

        let config: Self = serde_yaml::from_str(&content).context("Failed to parse config file")?;
        debug!("Config::load_from_file: config parsed successfully");

        tracing::info!("Loaded config from: {}", path.as_ref().display());
        Ok(config)
    }
}

/// LLM provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Provider name ("anthropic" or "openai")
    pub provider: String,

    /// Model identifier
    pub model: String,

    /// Environment variable containing the API key (checked first)
    #[serde(rename = "api-key-env")]
    pub api_key_env: String,

    /// File path containing the API key (used if env var not set)
    /// Supports ~ for home directory
    #[serde(rename = "api-key-file")]
    pub api_key_file: Option<String>,

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

impl LlmConfig {
    /// Get the API key from environment variable or file
    pub fn get_api_key(&self) -> Result<String> {
        // First try environment variable
        if let Ok(key) = std::env::var(&self.api_key_env) {
            debug!(env_var = %self.api_key_env, "get_api_key: found in environment");
            return Ok(key);
        }

        // Then try file
        if let Some(file_path) = &self.api_key_file {
            let expanded = if file_path.starts_with("~/") {
                dirs::home_dir()
                    .map(|h| h.join(&file_path[2..]))
                    .unwrap_or_else(|| PathBuf::from(file_path))
            } else {
                PathBuf::from(file_path)
            };

            if expanded.exists() {
                let key = fs::read_to_string(&expanded)
                    .context(format!("Failed to read API key from {}", expanded.display()))?
                    .trim()
                    .to_string();
                debug!(file = %expanded.display(), "get_api_key: found in file");
                return Ok(key);
            }
        }

        Err(eyre::eyre!(
            "API key not found. Set the {} environment variable or configure api-key-file in your config",
            self.api_key_env
        ))
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            api_key_file: None,
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
        // Use XDG data directory (~/.local/share/taskdaemon on Linux)
        let taskstore_dir = dirs::data_dir()
            .map(|d| d.join("taskdaemon"))
            .unwrap_or_else(|| PathBuf::from(".taskstore"))
            .to_string_lossy()
            .into_owned();

        Self {
            taskstore_dir,
            jsonl_warn_mb: 100,
            jsonl_error_mb: 500,
        }
    }
}

/// Progress strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProgressConfig {
    /// Progress strategy (currently only "system-captured" supported)
    pub strategy: String,

    /// Maximum number of iterations to keep in progress
    #[serde(rename = "max-entries")]
    pub max_entries: usize,

    /// Maximum output characters per iteration
    #[serde(rename = "max-output-chars")]
    pub max_output_chars: usize,
}

impl Default for ProgressConfig {
    fn default() -> Self {
        Self {
            strategy: "system-captured".to_string(),
            max_entries: 5,
            max_output_chars: 500,
        }
    }
}

/// Loop type paths configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoopsConfig {
    /// Paths to search for loop type definitions (searched in order)
    pub paths: Vec<String>,
}

impl Default for LoopsConfig {
    fn default() -> Self {
        Self {
            paths: vec![
                "builtin".to_string(),
                "~/.config/taskdaemon/loops".to_string(),
                ".taskdaemon/loops".to_string(),
            ],
        }
    }
}

impl LoopsConfig {
    /// Expand paths (resolve ~/ and relative paths)
    pub fn expanded_paths(&self) -> Vec<PathBuf> {
        debug!(?self.paths, "LoopsConfig::expanded_paths: called");
        let paths: Vec<PathBuf> = self
            .paths
            .iter()
            .filter_map(|p| {
                if p == "builtin" {
                    debug!(%p, "LoopsConfig::expanded_paths: skipping builtin");
                    None // builtin is handled specially
                } else if p.starts_with("~/") {
                    debug!(%p, "LoopsConfig::expanded_paths: expanding home path");
                    dirs::home_dir().map(|home| home.join(&p[2..]))
                } else {
                    debug!(%p, "LoopsConfig::expanded_paths: using as-is");
                    Some(PathBuf::from(p))
                }
            })
            .collect();
        debug!(?paths, "LoopsConfig::expanded_paths: returning paths");
        paths
    }

    /// Check if builtin types should be loaded
    pub fn use_builtin(&self) -> bool {
        debug!("LoopsConfig::use_builtin: called");
        let result = self.paths.iter().any(|p| p == "builtin");
        debug!(result, "LoopsConfig::use_builtin: returning");
        result
    }
}

/// Debug configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugConfig {
    /// Log REPL conversations to files for debugging
    /// Files are written to ~/.taskdaemon/conversations/
    #[serde(rename = "log-conversations")]
    pub log_conversations: bool,
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
