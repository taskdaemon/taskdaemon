//! Loop configuration types

use serde::{Deserialize, Serialize};
use tracing::debug;

/// Configuration for a loop type (from YAML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    /// Loop type name (matches a type loaded from YAML configuration)
    pub loop_type: String,

    /// Handlebars prompt template
    pub prompt_template: String,

    /// Command to run for validation
    pub validation_command: String,

    /// Exit code that indicates success (usually 0)
    #[serde(default)]
    pub success_exit_code: i32,

    /// Maximum iterations before giving up
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Maximum agentic turns within an iteration (LLM calls with tool use)
    #[serde(default = "default_max_turns")]
    pub max_turns_per_iteration: u32,

    /// Timeout for each iteration in milliseconds
    #[serde(default = "default_iteration_timeout")]
    pub iteration_timeout_ms: u64,

    /// Tools available to this loop type
    #[serde(default)]
    pub tools: Vec<String>,

    /// Progress tracking config
    #[serde(default = "default_progress_max_entries")]
    pub progress_max_entries: usize,

    #[serde(default = "default_progress_max_chars")]
    pub progress_max_chars: usize,
}

fn default_max_iterations() -> u32 {
    debug!("default_max_iterations: called");
    100
}

fn default_max_turns() -> u32 {
    debug!("default_max_turns: called");
    50
}

fn default_iteration_timeout() -> u64 {
    debug!("default_iteration_timeout: called");
    300_000 // 5 minutes
}

fn default_progress_max_entries() -> usize {
    debug!("default_progress_max_entries: called");
    5
}

fn default_progress_max_chars() -> usize {
    debug!("default_progress_max_chars: called");
    500
}

impl Default for LoopConfig {
    fn default() -> Self {
        debug!("LoopConfig::default: called");
        Self {
            // loop_type is empty by default - actual type should come from
            // the LoopLoader based on the execution's loop_type field
            loop_type: String::new(),
            prompt_template: String::new(),
            validation_command: "otto ci".to_string(),
            success_exit_code: 0,
            max_iterations: default_max_iterations(),
            max_turns_per_iteration: default_max_turns(),
            iteration_timeout_ms: default_iteration_timeout(),
            tools: vec![
                "read".to_string(),
                "write".to_string(),
                "edit".to_string(),
                "list".to_string(),
                "glob".to_string(),
                "bash".to_string(),
            ],
            progress_max_entries: default_progress_max_entries(),
            progress_max_chars: default_progress_max_chars(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = LoopConfig::default();

        assert!(config.loop_type.is_empty());
        assert_eq!(config.max_iterations, 100);
        assert_eq!(config.max_turns_per_iteration, 50);
        assert_eq!(config.iteration_timeout_ms, 300_000);
        assert_eq!(config.success_exit_code, 0);
        assert!(!config.tools.is_empty());
    }

    #[test]
    fn test_deserialize_minimal() {
        let yaml = r#"
loop_type: test
prompt_template: "Hello {{name}}"
validation_command: "cargo test"
"#;

        let config: LoopConfig = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(config.loop_type, "test");
        assert_eq!(config.prompt_template, "Hello {{name}}");
        assert_eq!(config.validation_command, "cargo test");
        // Defaults should apply
        assert_eq!(config.max_iterations, 100);
    }

    #[test]
    fn test_deserialize_full() {
        let yaml = r#"
loop_type: custom
prompt_template: "Do something"
validation_command: "make test"
success_exit_code: 0
max_iterations: 50
max_turns_per_iteration: 20
iteration_timeout_ms: 60000
tools:
  - read_file
  - write_file
progress_max_entries: 10
progress_max_chars: 1000
"#;

        let config: LoopConfig = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(config.max_iterations, 50);
        assert_eq!(config.max_turns_per_iteration, 20);
        assert_eq!(config.iteration_timeout_ms, 60000);
        assert_eq!(config.tools.len(), 2);
        assert_eq!(config.progress_max_entries, 10);
        assert_eq!(config.progress_max_chars, 1000);
    }
}
