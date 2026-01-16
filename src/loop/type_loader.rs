//! Loop type definitions and loading
//!
//! Loop types define the schema for different loop behaviors (plan, spec, phase, ralph).
//! Types are loaded from:
//! 1. Builtin (embedded in binary)
//! 2. User global (~/.config/taskdaemon/loops/*.yml)
//! 3. Project-specific (.taskdaemon/loops/*.yml)
//!
//! Later definitions override earlier ones with the same name.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::config::LoopConfig;
use crate::config::LoopsConfig;

/// A loop type definition as loaded from YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopType {
    /// Human-readable description
    #[serde(default)]
    pub description: String,

    /// Handlebars prompt template
    #[serde(rename = "prompt-template")]
    pub prompt_template: String,

    /// Command to run for validation
    #[serde(rename = "validation-command", default = "default_validation_command")]
    pub validation_command: String,

    /// Exit code that indicates success (usually 0)
    #[serde(rename = "success-exit-code", default)]
    pub success_exit_code: i32,

    /// Maximum iterations before giving up
    #[serde(rename = "max-iterations", default = "default_max_iterations")]
    pub max_iterations: u32,

    /// Timeout for each iteration in milliseconds
    #[serde(rename = "iteration-timeout-ms", default = "default_iteration_timeout")]
    pub iteration_timeout_ms: u64,

    /// Input template variables
    #[serde(default)]
    pub inputs: Vec<String>,

    /// Output artifacts
    #[serde(default)]
    pub outputs: Vec<String>,

    /// Tools available to this loop type
    #[serde(default = "default_tools")]
    pub tools: Vec<String>,
}

fn default_validation_command() -> String {
    "otto ci".to_string()
}

fn default_max_iterations() -> u32 {
    100
}

fn default_iteration_timeout() -> u64 {
    300_000 // 5 minutes
}

fn default_tools() -> Vec<String> {
    vec![
        "read_file".to_string(),
        "write_file".to_string(),
        "edit_file".to_string(),
        "list_directory".to_string(),
        "glob".to_string(),
        "grep".to_string(),
        "run_command".to_string(),
        "complete_task".to_string(),
    ]
}

/// Builtin loop type definitions (embedded in binary)
const BUILTIN_PLAN: &str = include_str!("builtin_types/plan.yml");
const BUILTIN_SPEC: &str = include_str!("builtin_types/spec.yml");
const BUILTIN_PHASE: &str = include_str!("builtin_types/phase.yml");
const BUILTIN_RALPH: &str = include_str!("builtin_types/ralph.yml");

/// Loader for loop type definitions
pub struct LoopTypeLoader {
    /// Loaded loop types by name
    types: HashMap<String, LoopType>,
}

impl LoopTypeLoader {
    /// Create a new loader using the given configuration
    pub fn new(config: &LoopsConfig) -> Result<Self> {
        let mut loader = Self { types: HashMap::new() };

        // Load builtin types first (if enabled)
        if config.use_builtin() {
            loader.load_builtins()?;
        }

        // Load from configured paths (later overrides earlier)
        for path in config.expanded_paths() {
            if path.exists() {
                loader.load_from_directory(&path)?;
            } else {
                debug!(?path, "Loop type directory does not exist, skipping");
            }
        }

        info!(count = loader.types.len(), "Loaded loop types");
        Ok(loader)
    }

    /// Load the builtin loop types
    fn load_builtins(&mut self) -> Result<()> {
        self.load_builtin_type("plan", BUILTIN_PLAN)?;
        self.load_builtin_type("spec", BUILTIN_SPEC)?;
        self.load_builtin_type("phase", BUILTIN_PHASE)?;
        self.load_builtin_type("ralph", BUILTIN_RALPH)?;
        debug!("Loaded 4 builtin loop types");
        Ok(())
    }

    /// Load a single builtin type from embedded YAML
    fn load_builtin_type(&mut self, name: &str, yaml_content: &str) -> Result<()> {
        let loop_type: LoopType =
            serde_yaml::from_str(yaml_content).with_context(|| format!("Failed to parse builtin type: {}", name))?;
        self.types.insert(name.to_string(), loop_type);
        Ok(())
    }

    /// Load all .yml files from a directory
    fn load_from_directory(&mut self, dir: &Path) -> Result<()> {
        debug!(?dir, "Loading loop types from directory");

        let entries = fs::read_dir(dir).with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|e| e == "yml" || e == "yaml").unwrap_or(false)
                && let Err(e) = self.load_from_file(&path)
            {
                warn!(?path, error = %e, "Failed to load loop type file");
            }
        }

        Ok(())
    }

    /// Load a loop type from a YAML file
    fn load_from_file(&mut self, path: &Path) -> Result<()> {
        let content = fs::read_to_string(path).with_context(|| format!("Failed to read: {}", path.display()))?;

        // The file can contain a map of name -> definition, or just a definition
        // Try parsing as a map first (like taskdaemon.yml format)
        if let Ok(map) = serde_yaml::from_str::<HashMap<String, LoopType>>(&content) {
            for (name, loop_type) in map {
                debug!(?path, name, "Loaded loop type");
                self.types.insert(name, loop_type);
            }
            return Ok(());
        }

        // Fall back to single definition with filename as name
        let loop_type: LoopType =
            serde_yaml::from_str(&content).with_context(|| format!("Failed to parse: {}", path.display()))?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| eyre::eyre!("Invalid filename: {}", path.display()))?;

        debug!(?path, name, "Loaded loop type");
        self.types.insert(name.to_string(), loop_type);

        Ok(())
    }

    /// Get a loop type by name
    pub fn get(&self, name: &str) -> Option<&LoopType> {
        self.types.get(name)
    }

    /// Get all loaded loop type names
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.types.keys().map(|s| s.as_str())
    }

    /// Get the number of loaded types
    pub fn len(&self) -> usize {
        self.types.len()
    }

    /// Check if no types are loaded
    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Convert to a map for iteration
    pub fn iter(&self) -> impl Iterator<Item = (&str, &LoopType)> {
        self.types.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Convert all loop types to LoopConfig format for the LoopManager
    pub fn to_configs(&self) -> HashMap<String, LoopConfig> {
        self.types
            .iter()
            .map(|(name, loop_type)| {
                (
                    name.clone(),
                    LoopConfig {
                        loop_type: name.clone(),
                        prompt_template: loop_type.prompt_template.clone(),
                        validation_command: loop_type.validation_command.clone(),
                        success_exit_code: loop_type.success_exit_code,
                        max_iterations: loop_type.max_iterations,
                        max_turns_per_iteration: 50, // Default
                        iteration_timeout_ms: loop_type.iteration_timeout_ms,
                        tools: loop_type.tools.clone(),
                        progress_max_entries: 5, // Default
                        progress_max_chars: 500, // Default
                    },
                )
            })
            .collect()
    }
}

impl From<LoopType> for LoopConfig {
    fn from(lt: LoopType) -> Self {
        LoopConfig {
            loop_type: "unknown".to_string(),
            prompt_template: lt.prompt_template,
            validation_command: lt.validation_command,
            success_exit_code: lt.success_exit_code,
            max_iterations: lt.max_iterations,
            max_turns_per_iteration: 50,
            iteration_timeout_ms: lt.iteration_timeout_ms,
            tools: lt.tools,
            progress_max_entries: 5,
            progress_max_chars: 500,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_plan_parses() {
        let loop_type: LoopType = serde_yaml::from_str(BUILTIN_PLAN).unwrap();
        assert!(!loop_type.prompt_template.is_empty());
        assert_eq!(loop_type.validation_command, "otto ci");
    }

    #[test]
    fn test_builtin_spec_parses() {
        let loop_type: LoopType = serde_yaml::from_str(BUILTIN_SPEC).unwrap();
        assert!(!loop_type.prompt_template.is_empty());
    }

    #[test]
    fn test_builtin_phase_parses() {
        let loop_type: LoopType = serde_yaml::from_str(BUILTIN_PHASE).unwrap();
        assert!(!loop_type.prompt_template.is_empty());
        assert!(loop_type.tools.contains(&"run_command".to_string()));
    }

    #[test]
    fn test_builtin_ralph_parses() {
        let loop_type: LoopType = serde_yaml::from_str(BUILTIN_RALPH).unwrap();
        assert!(!loop_type.prompt_template.is_empty());
    }

    #[test]
    fn test_load_builtins() {
        let config = LoopsConfig::default();
        let loader = LoopTypeLoader::new(&config).unwrap();

        assert!(loader.get("plan").is_some());
        assert!(loader.get("spec").is_some());
        assert!(loader.get("phase").is_some());
        assert!(loader.get("ralph").is_some());
        assert_eq!(loader.len(), 4);
    }

    #[test]
    fn test_deserialize_loop_type() {
        let yaml = r#"
description: "Test loop type"
prompt-template: |
  Hello {{name}}
validation-command: "make test"
max-iterations: 50
inputs:
  - name
outputs:
  - result
tools:
  - read_file
  - write_file
"#;

        let loop_type: LoopType = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(loop_type.description, "Test loop type");
        assert_eq!(loop_type.validation_command, "make test");
        assert_eq!(loop_type.max_iterations, 50);
        assert_eq!(loop_type.inputs, vec!["name"]);
        assert_eq!(loop_type.outputs, vec!["result"]);
        assert_eq!(loop_type.tools.len(), 2);
    }

    #[test]
    fn test_default_values() {
        let yaml = r#"
prompt-template: "Do something"
"#;

        let loop_type: LoopType = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(loop_type.validation_command, "otto ci");
        assert_eq!(loop_type.max_iterations, 100);
        assert_eq!(loop_type.iteration_timeout_ms, 300_000);
        assert_eq!(loop_type.success_exit_code, 0);
        assert!(!loop_type.tools.is_empty()); // Default tools
    }
}
