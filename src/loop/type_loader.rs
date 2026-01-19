//! Loop type definitions and loading
//!
//! Loop types define the schema for different loop behaviors (plan, spec, phase, ralph).
//! Types are loaded from:
//! 1. Builtin (embedded in binary)
//! 2. User global (~/.config/taskdaemon/loops/*.yml)
//! 3. Project-specific (.taskdaemon/loops/*.yml)
//!
//! Later definitions override earlier ones with the same name.
//!
//! ## Inheritance
//!
//! Loop types can extend other types using the `extends` field:
//! ```yaml
//! extends: ralph
//! prompt-template: |
//!   Custom prompt...
//! ```
//!
//! Inherited fields are merged with the child type's fields, with child values
//! taking precedence.
//!
//! ## Hot-Reload
//!
//! The loader supports hot-reloading via `reload()` method, allowing config
//! changes without daemon restart.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eyre::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use super::config::LoopConfig;
use crate::config::LoopsConfig;

/// A loop type definition as loaded from YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopType {
    /// Parent type to extend (inheritance for config values)
    #[serde(default)]
    pub extends: Option<String>,

    /// Parent loop type for cascade relationships
    /// e.g., spec has parent: plan, phase has parent: spec
    /// When a parent loop completes, it spawns child loops
    #[serde(default)]
    pub parent: Option<String>,

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

impl LoopType {
    /// Merge another LoopType as a parent (inheritance)
    ///
    /// Child values take precedence over parent values.
    /// Vectors (inputs, outputs, tools) are merged.
    pub fn merge_parent(&mut self, parent: &LoopType) {
        debug!(?self.extends, "LoopType::merge_parent: called");
        // Use parent description if child is empty
        if self.description.is_empty() {
            debug!("merge_parent: using parent description");
            self.description = parent.description.clone();
        }

        // Merge prompt template: child takes precedence if non-empty
        // (prompt_template is required, so this is mostly for safety)

        // Use parent validation_command if child uses default
        if self.validation_command == default_validation_command() {
            debug!("merge_parent: using parent validation_command");
            self.validation_command = parent.validation_command.clone();
        }

        // Use parent success_exit_code if child uses default
        if self.success_exit_code == 0 {
            debug!("merge_parent: using parent success_exit_code");
            self.success_exit_code = parent.success_exit_code;
        }

        // Use parent max_iterations if child uses default
        if self.max_iterations == default_max_iterations() {
            debug!("merge_parent: using parent max_iterations");
            self.max_iterations = parent.max_iterations;
        }

        // Use parent iteration_timeout_ms if child uses default
        if self.iteration_timeout_ms == default_iteration_timeout() {
            debug!("merge_parent: using parent iteration_timeout_ms");
            self.iteration_timeout_ms = parent.iteration_timeout_ms;
        }

        // Merge inputs: add parent inputs that child doesn't have
        for input in &parent.inputs {
            if !self.inputs.contains(input) {
                debug!(%input, "merge_parent: adding parent input");
                self.inputs.push(input.clone());
            }
        }

        // Merge outputs: add parent outputs that child doesn't have
        for output in &parent.outputs {
            if !self.outputs.contains(output) {
                debug!(%output, "merge_parent: adding parent output");
                self.outputs.push(output.clone());
            }
        }

        // Merge tools: child tools + parent tools not in child
        let default = default_tools();
        let child_is_default = self.tools == default;
        if child_is_default {
            debug!("merge_parent: child uses default tools, using parent tools");
            // Use parent tools if child has defaults
            self.tools = parent.tools.clone();
        } else {
            debug!("merge_parent: merging tools from parent");
            // Add parent tools that child doesn't have
            for tool in &parent.tools {
                if !self.tools.contains(tool) {
                    debug!(%tool, "merge_parent: adding parent tool");
                    self.tools.push(tool.clone());
                }
            }
        }
        debug!("merge_parent: complete");
    }
}

fn default_validation_command() -> String {
    debug!("default_validation_command: called");
    "otto ci".to_string()
}

fn default_max_iterations() -> u32 {
    debug!("default_max_iterations: called");
    100
}

fn default_iteration_timeout() -> u64 {
    debug!("default_iteration_timeout: called");
    300_000 // 5 minutes
}

fn default_tools() -> Vec<String> {
    debug!("default_tools: called");
    vec![
        "read".to_string(),
        "write".to_string(),
        "edit".to_string(),
        "list".to_string(),
        "glob".to_string(),
        "grep".to_string(),
        "bash".to_string(),
        "complete_task".to_string(),
    ]
}

/// Builtin loop type definitions (embedded in binary)
const BUILTIN_PLAN: &str = include_str!("builtin_types/plan.yml");
const BUILTIN_SPEC: &str = include_str!("builtin_types/spec.yml");
const BUILTIN_PHASE: &str = include_str!("builtin_types/phase.yml");
const BUILTIN_RALPH: &str = include_str!("builtin_types/ralph.yml");

/// Tracked file for hot-reload detection
#[derive(Debug, Clone)]
struct TrackedFile {
    path: PathBuf,
    modified: SystemTime,
}

/// Loader for loop type definitions with hot-reload support
pub struct LoopLoader {
    /// Loaded loop types by name (before inheritance resolution)
    raw_types: HashMap<String, LoopType>,

    /// Resolved loop types by name (after inheritance)
    types: HashMap<String, LoopType>,

    /// Tracked files for hot-reload
    tracked_files: Vec<TrackedFile>,

    /// Configuration used for loading
    config: LoopsConfig,
}

impl LoopLoader {
    /// Create a new loader using the given configuration
    pub fn new(config: &LoopsConfig) -> Result<Self> {
        debug!(?config, "LoopLoader::new: called");
        let mut loader = Self {
            raw_types: HashMap::new(),
            types: HashMap::new(),
            tracked_files: Vec::new(),
            config: config.clone(),
        };

        loader.load_all()?;
        debug!(type_count = loader.types.len(), "LoopLoader::new: complete");
        Ok(loader)
    }

    /// Load all types from configured sources
    fn load_all(&mut self) -> Result<()> {
        debug!("load_all: called");
        self.raw_types.clear();
        self.tracked_files.clear();

        // Load builtin types first (if enabled)
        if self.config.use_builtin() {
            debug!("load_all: loading builtin types");
            self.load_builtins()?;
        } else {
            debug!("load_all: builtin types disabled");
        }

        // Load from configured paths (later overrides earlier)
        for path in self.config.expanded_paths() {
            if path.exists() {
                debug!(?path, "load_all: loading from directory");
                self.load_from_directory(&path)?;
            } else {
                debug!(?path, "load_all: directory does not exist, skipping");
            }
        }

        // Resolve inheritance
        debug!("load_all: resolving inheritance");
        self.resolve_inheritance()?;

        info!(count = self.types.len(), "Loaded loop types");
        debug!("load_all: complete");
        Ok(())
    }

    /// Check if any tracked files have been modified
    pub fn has_changes(&self) -> bool {
        debug!(tracked_count = self.tracked_files.len(), "has_changes: called");
        for tracked in &self.tracked_files {
            if let Ok(metadata) = fs::metadata(&tracked.path)
                && let Ok(modified) = metadata.modified()
                && modified > tracked.modified
            {
                debug!(path = ?tracked.path, "has_changes: file modified");
                return true;
            }
        }

        // Also check for new files in tracked directories
        for path in self.config.expanded_paths() {
            if path.exists()
                && path.is_dir()
                && let Ok(entries) = fs::read_dir(&path)
            {
                for entry in entries.filter_map(|e| e.ok()) {
                    let file_path = entry.path();
                    if file_path
                        .extension()
                        .map(|e| e == "yml" || e == "yaml")
                        .unwrap_or(false)
                        && !self.tracked_files.iter().any(|t| t.path == file_path)
                    {
                        debug!(path = ?file_path, "has_changes: new file detected");
                        return true;
                    }
                }
            }
        }

        debug!("has_changes: no changes detected");
        false
    }

    /// Reload all types (hot-reload)
    pub fn reload(&mut self) -> Result<bool> {
        debug!("reload: called");
        if !self.has_changes() {
            debug!("reload: no changes, skipping");
            return Ok(false);
        }

        debug!("reload: changes detected, reloading");
        info!("Hot-reloading loop type configurations");
        self.load_all()?;
        debug!("reload: complete");
        Ok(true)
    }

    /// Load the builtin loop types
    fn load_builtins(&mut self) -> Result<()> {
        debug!("load_builtins: called");
        self.load_builtin_type("plan", BUILTIN_PLAN)?;
        self.load_builtin_type("spec", BUILTIN_SPEC)?;
        self.load_builtin_type("phase", BUILTIN_PHASE)?;
        self.load_builtin_type("ralph", BUILTIN_RALPH)?;
        debug!("load_builtins: loaded 4 builtin loop types");
        Ok(())
    }

    /// Load a single builtin type from embedded YAML
    fn load_builtin_type(&mut self, name: &str, yaml_content: &str) -> Result<()> {
        debug!(%name, "load_builtin_type: called");
        let loop_type: LoopType =
            serde_yaml::from_str(yaml_content).with_context(|| format!("Failed to parse builtin type: {}", name))?;
        self.raw_types.insert(name.to_string(), loop_type);
        debug!(%name, "load_builtin_type: inserted");
        Ok(())
    }

    /// Load all .yml files from a directory
    fn load_from_directory(&mut self, dir: &Path) -> Result<()> {
        debug!(?dir, "load_from_directory: called");

        let entries = fs::read_dir(dir).with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map(|e| e == "yml" || e == "yaml").unwrap_or(false) {
                debug!(?path, "load_from_directory: loading file");
                if let Err(e) = self.load_from_file(&path) {
                    debug!(?path, error = %e, "load_from_directory: failed to load file");
                    warn!(?path, error = %e, "Failed to load loop type file");
                }
            } else {
                debug!(?path, "load_from_directory: skipping non-yaml file");
            }
        }

        debug!(?dir, "load_from_directory: complete");
        Ok(())
    }

    /// Load a loop type from a YAML file
    fn load_from_file(&mut self, path: &Path) -> Result<()> {
        debug!(?path, "load_from_file: called");
        let content = fs::read_to_string(path).with_context(|| format!("Failed to read: {}", path.display()))?;
        debug!(?path, content_len = content.len(), "load_from_file: read content");

        // Track file for hot-reload
        if let Ok(metadata) = fs::metadata(path)
            && let Ok(modified) = metadata.modified()
        {
            debug!(?path, "load_from_file: tracking file for hot-reload");
            self.tracked_files.push(TrackedFile {
                path: path.to_path_buf(),
                modified,
            });
        } else {
            debug!(?path, "load_from_file: could not track file metadata");
        }

        // The file can contain a map of name -> definition, or just a definition
        // Try parsing as a map first (like taskdaemon.yml format)
        if let Ok(map) = serde_yaml::from_str::<HashMap<String, LoopType>>(&content) {
            debug!(?path, count = map.len(), "load_from_file: parsed as map");
            for (name, loop_type) in map {
                debug!(?path, %name, "load_from_file: inserting type from map");
                self.raw_types.insert(name, loop_type);
            }
            return Ok(());
        }

        debug!(?path, "load_from_file: parsing as single definition");
        // Fall back to single definition with filename as name
        let loop_type: LoopType =
            serde_yaml::from_str(&content).with_context(|| format!("Failed to parse: {}", path.display()))?;

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| eyre::eyre!("Invalid filename: {}", path.display()))?;

        debug!(?path, %name, "load_from_file: inserting single type");
        self.raw_types.insert(name.to_string(), loop_type);

        Ok(())
    }

    /// Resolve inheritance for all types
    fn resolve_inheritance(&mut self) -> Result<()> {
        debug!(raw_type_count = self.raw_types.len(), "resolve_inheritance: called");
        self.types.clear();

        // Clone raw_types since we need to iterate and look up parents
        let raw = self.raw_types.clone();

        for (name, raw_type) in &raw {
            debug!(%name, "resolve_inheritance: resolving type");
            let resolved = self.resolve_single_inheritance(name, raw_type, &raw, &mut vec![])?;
            self.types.insert(name.clone(), resolved);
        }

        debug!(resolved_count = self.types.len(), "resolve_inheritance: complete");
        Ok(())
    }

    /// Resolve inheritance for a single type (handles chains)
    fn resolve_single_inheritance(
        &self,
        name: &str,
        loop_type: &LoopType,
        all_raw: &HashMap<String, LoopType>,
        visited: &mut Vec<String>,
    ) -> Result<LoopType> {
        debug!(%name, extends = ?loop_type.extends, "resolve_single_inheritance: called");
        // Cycle detection
        if visited.contains(&name.to_string()) {
            debug!(%name, ?visited, "resolve_single_inheritance: cycle detected");
            return Err(eyre::eyre!(
                "Inheritance cycle detected: {} -> {}",
                visited.join(" -> "),
                name
            ));
        }
        visited.push(name.to_string());

        let mut resolved = loop_type.clone();

        if let Some(parent_name) = &loop_type.extends {
            if let Some(parent_raw) = all_raw.get(parent_name) {
                debug!(%name, %parent_name, "resolve_single_inheritance: resolving parent");
                // Recursively resolve parent's inheritance
                let parent = self.resolve_single_inheritance(parent_name, parent_raw, all_raw, visited)?;
                resolved.merge_parent(&parent);
                debug!(%name, extends = %parent_name, "resolve_single_inheritance: merged parent");
            } else {
                debug!(%name, %parent_name, "resolve_single_inheritance: parent not found");
                warn!(
                    name,
                    extends = %parent_name,
                    "Parent loop type not found, ignoring extends"
                );
            }
        } else {
            debug!(%name, "resolve_single_inheritance: no parent to extend");
        }

        // Clear extends field in resolved type
        resolved.extends = None;

        debug!(%name, "resolve_single_inheritance: complete");
        Ok(resolved)
    }

    /// Get a loop type by name
    pub fn get(&self, name: &str) -> Option<&LoopType> {
        debug!(%name, "LoopLoader::get: called");
        let result = self.types.get(name);
        debug!(%name, found = result.is_some(), "get: returning");
        result
    }

    /// Get all loaded loop type names
    pub fn names(&self) -> impl Iterator<Item = &str> {
        debug!(count = self.types.len(), "LoopLoader::names: called");
        self.types.keys().map(|s| s.as_str())
    }

    /// Get the number of loaded types
    pub fn len(&self) -> usize {
        debug!(count = self.types.len(), "LoopLoader::len: called");
        self.types.len()
    }

    /// Check if no types are loaded
    pub fn is_empty(&self) -> bool {
        debug!(is_empty = self.types.is_empty(), "LoopLoader::is_empty: called");
        self.types.is_empty()
    }

    /// Convert to a map for iteration
    pub fn iter(&self) -> impl Iterator<Item = (&str, &LoopType)> {
        debug!(count = self.types.len(), "LoopLoader::iter: called");
        self.types.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Find all loop types that have the given parent type
    /// Used for cascade logic - when a parent loop completes, spawn these child types
    pub fn children_of(&self, parent_type: &str) -> Vec<&str> {
        debug!(%parent_type, "LoopLoader::children_of: called");
        let result: Vec<&str> = self
            .types
            .iter()
            .filter(|(_, lt)| lt.parent.as_deref() == Some(parent_type))
            .map(|(name, _)| name.as_str())
            .collect();
        debug!(%parent_type, child_count = result.len(), "children_of: returning");
        result
    }

    /// Convert all loop types to LoopConfig format for the LoopManager
    pub fn to_configs(&self) -> HashMap<String, LoopConfig> {
        debug!(type_count = self.types.len(), "LoopLoader::to_configs: called");
        let result = self
            .types
            .iter()
            .map(|(name, loop_type)| {
                debug!(%name, "to_configs: converting type");
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
            .collect();
        debug!(config_count = self.types.len(), "to_configs: complete");
        result
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
        // Phase is a decomposition step (not implementation), so no bash
        assert!(!loop_type.tools.contains(&"bash".to_string()));
        assert_eq!(loop_type.parent, Some("spec".to_string()));
    }

    #[test]
    fn test_builtin_ralph_has_parent_phase() {
        let loop_type: LoopType = serde_yaml::from_str(BUILTIN_RALPH).unwrap();
        assert!(!loop_type.prompt_template.is_empty());
        // Ralph is the implementation step, needs bash
        assert!(loop_type.tools.contains(&"bash".to_string()));
        assert_eq!(loop_type.parent, Some("phase".to_string()));
    }

    #[test]
    fn test_builtin_ralph_parses() {
        let loop_type: LoopType = serde_yaml::from_str(BUILTIN_RALPH).unwrap();
        assert!(!loop_type.prompt_template.is_empty());
    }

    #[test]
    fn test_load_builtins() {
        let config = LoopsConfig::default();
        let loader = LoopLoader::new(&config).unwrap();

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

    #[test]
    fn test_loop_type_with_extends() {
        let yaml = r#"
extends: ralph
prompt-template: "Custom prompt"
"#;

        let loop_type: LoopType = serde_yaml::from_str(yaml).unwrap();

        assert_eq!(loop_type.extends, Some("ralph".to_string()));
        assert_eq!(loop_type.prompt_template, "Custom prompt");
    }

    #[test]
    fn test_merge_parent() {
        let parent_yaml = r#"
prompt-template: "Parent prompt"
validation-command: "make test"
max-iterations: 50
inputs:
  - input1
  - input2
tools:
  - read_file
  - custom_tool
"#;

        let child_yaml = r#"
extends: parent
prompt-template: "Child prompt"
inputs:
  - input3
"#;

        let parent: LoopType = serde_yaml::from_str(parent_yaml).unwrap();
        let mut child: LoopType = serde_yaml::from_str(child_yaml).unwrap();

        child.merge_parent(&parent);

        // Child prompt takes precedence
        assert_eq!(child.prompt_template, "Child prompt");
        // Validation command comes from parent (child used default)
        assert_eq!(child.validation_command, "make test");
        // Max iterations from parent
        assert_eq!(child.max_iterations, 50);
        // Inputs merged
        assert!(child.inputs.contains(&"input1".to_string()));
        assert!(child.inputs.contains(&"input2".to_string()));
        assert!(child.inputs.contains(&"input3".to_string()));
        // Tools from parent (child used default)
        assert!(child.tools.contains(&"read_file".to_string()));
        assert!(child.tools.contains(&"custom_tool".to_string()));
    }

    #[test]
    fn test_has_changes_no_files() {
        let config = LoopsConfig {
            paths: vec!["builtin".to_string()],
        };
        let loader = LoopLoader::new(&config).unwrap();

        // No external files tracked, so no changes
        assert!(!loader.has_changes());
    }

    #[test]
    fn test_four_level_hierarchy() {
        let config = LoopsConfig::default();
        let loader = LoopLoader::new(&config).unwrap();

        // Verify the 4-level hierarchy: Plan -> Spec -> Phase -> Ralph
        // Plan is root (no parent)
        let plan = loader.get("plan").unwrap();
        assert!(plan.parent.is_none(), "Plan should be root");

        // Spec has parent: plan
        let spec = loader.get("spec").unwrap();
        assert_eq!(spec.parent, Some("plan".to_string()));

        // Phase has parent: spec
        let phase = loader.get("phase").unwrap();
        assert_eq!(phase.parent, Some("spec".to_string()));

        // Ralph has parent: phase
        let ralph = loader.get("ralph").unwrap();
        assert_eq!(ralph.parent, Some("phase".to_string()));

        // children_of returns correct children
        assert_eq!(loader.children_of("plan"), vec!["spec"]);
        assert_eq!(loader.children_of("spec"), vec!["phase"]);
        assert_eq!(loader.children_of("phase"), vec!["ralph"]);
        assert!(loader.children_of("ralph").is_empty(), "Ralph has no children");
    }
}
