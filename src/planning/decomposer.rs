//! PlanDecomposer - LLM-driven decomposition of Plans into Specs
//!
//! Takes an agreed-upon Plan and breaks it into executable Specs
//! with dependencies, phases, and loop type assignments.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::domain::{Phase, Plan, PlanStatus, Spec};
use crate::llm::{CompletionRequest, CompletionResponse, LlmClient, Message, ToolDefinition};
use crate::r#loop::validate_dependency_graph;
use crate::state::StateManager;

/// Result of plan decomposition
#[derive(Debug, Clone)]
pub struct DecomposedPlan {
    /// The original plan
    pub plan: Plan,
    /// Generated specs
    pub specs: Vec<Spec>,
    /// Any warnings during decomposition
    pub warnings: Vec<String>,
}

/// Configuration for decomposition
#[derive(Debug, Clone)]
pub struct DecomposerConfig {
    /// Directory for spec markdown files
    pub specs_dir: PathBuf,
    /// Available loop types for assignment
    pub loop_types: Vec<String>,
    /// System prompt for decomposition
    pub system_prompt: String,
}

impl Default for DecomposerConfig {
    fn default() -> Self {
        Self {
            specs_dir: PathBuf::from("specs"),
            loop_types: vec!["spec".to_string(), "phase".to_string()],
            system_prompt: DEFAULT_DECOMPOSE_PROMPT.to_string(),
        }
    }
}

/// LLM output schema for a spec
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpecOutput {
    /// Spec title
    title: String,
    /// Brief description
    description: String,
    /// IDs of specs this depends on (within this decomposition)
    depends_on: Vec<String>,
    /// Phases within this spec
    phases: Vec<PhaseOutput>,
    /// Loop type to use (must be in available loop_types)
    loop_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PhaseOutput {
    name: String,
    description: String,
}

/// Full decomposition output from LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DecompositionOutput {
    specs: Vec<SpecOutput>,
}

/// PlanDecomposer breaks Plans into executable Specs
pub struct PlanDecomposer {
    llm: Arc<dyn LlmClient>,
    state: StateManager,
    config: DecomposerConfig,
}

impl PlanDecomposer {
    /// Create a new decomposer
    pub fn new(llm: Arc<dyn LlmClient>, state: StateManager, config: DecomposerConfig) -> Self {
        Self { llm, state, config }
    }

    /// Decompose a Plan into Specs
    ///
    /// This uses the LLM to break down the plan, then validates and persists
    /// the resulting specs.
    pub async fn decompose(&self, plan: &Plan) -> Result<DecomposedPlan> {
        info!(plan_id = %plan.id, "Decomposing plan into specs");

        // Read plan file content
        let plan_content = std::fs::read_to_string(&plan.file).context("Failed to read plan file")?;

        // Get decomposition from LLM
        let output = self.get_decomposition(&plan_content).await?;

        // Validate and convert to Spec objects
        let (specs, warnings) = self.build_specs(plan, output).await?;

        // Validate dependency graph
        validate_dependency_graph(&specs).map_err(|cycle| eyre::eyre!("Circular dependency detected: {:?}", cycle))?;

        // Persist specs to state
        for spec in &specs {
            self.state
                .create_spec(spec.clone())
                .await
                .context("Failed to persist spec")?;
        }

        // Update plan status to InProgress
        let mut updated_plan = plan.clone();
        updated_plan.set_status(PlanStatus::InProgress);
        self.state
            .update_plan(updated_plan.clone())
            .await
            .context("Failed to update plan status")?;

        info!(
            plan_id = %plan.id,
            spec_count = specs.len(),
            "Plan decomposed into {} specs",
            specs.len()
        );

        Ok(DecomposedPlan {
            plan: updated_plan,
            specs,
            warnings,
        })
    }

    /// Get decomposition from LLM
    async fn get_decomposition(&self, plan_content: &str) -> Result<DecompositionOutput> {
        let request = CompletionRequest {
            system_prompt: self.build_system_prompt(),
            messages: vec![Message::user(format!(
                "Decompose this plan into specs:\n\n{}",
                plan_content
            ))],
            tools: self.build_tools(),
            max_tokens: 8192,
        };

        let response = self
            .llm
            .complete(request)
            .await
            .context("Failed to get LLM response for decomposition")?;

        self.parse_decomposition_response(response)
    }

    /// Build system prompt with available loop types
    fn build_system_prompt(&self) -> String {
        let mut prompt = self.config.system_prompt.clone();

        prompt.push_str("\n\n## Available Loop Types\n");
        for lt in &self.config.loop_types {
            prompt.push_str(&format!("- {}\n", lt));
        }

        prompt
    }

    /// Build tools for decomposition
    fn build_tools(&self) -> Vec<ToolDefinition> {
        vec![ToolDefinition::new(
            "submit_decomposition",
            "Submit the plan decomposition. Call this once with all specs.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "specs": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": {
                                    "type": "string",
                                    "description": "Short title for this spec"
                                },
                                "description": {
                                    "type": "string",
                                    "description": "What this spec accomplishes"
                                },
                                "depends_on": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Titles of other specs this depends on"
                                },
                                "phases": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "name": { "type": "string" },
                                            "description": { "type": "string" }
                                        },
                                        "required": ["name", "description"]
                                    },
                                    "description": "Sequential phases within this spec"
                                },
                                "loop_type": {
                                    "type": "string",
                                    "description": "Loop type to use for execution (optional)"
                                }
                            },
                            "required": ["title", "description", "phases"]
                        }
                    }
                },
                "required": ["specs"]
            }),
        )]
    }

    /// Parse decomposition response from LLM
    fn parse_decomposition_response(&self, response: CompletionResponse) -> Result<DecompositionOutput> {
        // Look for submit_decomposition tool call
        for tool_call in &response.tool_calls {
            if tool_call.name == "submit_decomposition" {
                return self.parse_decomposition_input(&tool_call.input);
            }
        }

        // If no tool call, try to parse content as JSON (fallback)
        if let Some(content) = &response.content
            && let Ok(output) = serde_json::from_str::<DecompositionOutput>(content)
        {
            return Ok(output);
        }

        bail!("LLM did not produce a valid decomposition")
    }

    /// Parse the tool input into DecompositionOutput
    fn parse_decomposition_input(&self, input: &serde_json::Value) -> Result<DecompositionOutput> {
        let specs_json = input
            .get("specs")
            .ok_or_else(|| eyre::eyre!("Missing 'specs' in decomposition output"))?;

        let specs: Vec<SpecOutput> =
            serde_json::from_value(specs_json.clone()).context("Failed to parse specs from decomposition")?;

        if specs.is_empty() {
            bail!("Decomposition produced zero specs");
        }

        Ok(DecompositionOutput { specs })
    }

    /// Build Spec objects from decomposition output
    async fn build_specs(&self, plan: &Plan, output: DecompositionOutput) -> Result<(Vec<Spec>, Vec<String>)> {
        let mut specs = Vec::new();
        let mut warnings = Vec::new();

        // First pass: create specs and collect title -> id mapping
        let mut title_to_id: HashMap<String, String> = HashMap::new();

        // Ensure specs directory exists
        std::fs::create_dir_all(&self.config.specs_dir).context("Failed to create specs directory")?;

        for spec_out in &output.specs {
            // Create spec file
            let file_path = self.create_spec_file(spec_out)?;

            // Create Spec object
            let mut spec = Spec::new(&plan.id, &spec_out.title, file_path.display().to_string());

            // Add phases
            for (idx, phase_out) in spec_out.phases.iter().enumerate() {
                let phase_name = if phase_out.name.starts_with("Phase") {
                    phase_out.name.clone()
                } else {
                    format!("Phase {}: {}", idx + 1, phase_out.name)
                };
                spec.add_phase(Phase::new(phase_name, &phase_out.description));
            }

            // Validate loop type if specified
            if let Some(lt) = &spec_out.loop_type
                && !self.config.loop_types.contains(lt)
            {
                warnings.push(format!(
                    "Spec '{}' has unknown loop_type '{}', using default",
                    spec_out.title, lt
                ));
            }

            title_to_id.insert(spec_out.title.clone(), spec.id.clone());
            specs.push(spec);
        }

        // Second pass: resolve dependencies (title -> id)
        for (idx, spec_out) in output.specs.iter().enumerate() {
            for dep_title in &spec_out.depends_on {
                if let Some(dep_id) = title_to_id.get(dep_title) {
                    specs[idx].add_dependency(dep_id);
                } else {
                    warnings.push(format!(
                        "Spec '{}' depends on unknown spec '{}'",
                        spec_out.title, dep_title
                    ));
                }
            }
        }

        Ok((specs, warnings))
    }

    /// Create markdown file for a spec
    fn create_spec_file(&self, spec: &SpecOutput) -> Result<PathBuf> {
        let slug = slugify(&spec.title);
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let filename = format!("{}-{}.md", timestamp, slug);
        let file_path = self.config.specs_dir.join(&filename);

        let content = self.build_spec_markdown(spec);
        std::fs::write(&file_path, content).context("Failed to write spec file")?;

        Ok(file_path)
    }

    /// Build markdown content for a spec file
    fn build_spec_markdown(&self, spec: &SpecOutput) -> String {
        let mut md = String::new();

        md.push_str(&format!("# {}\n\n", spec.title));
        md.push_str(&format!("{}\n\n", spec.description));

        if !spec.depends_on.is_empty() {
            md.push_str("## Dependencies\n\n");
            for dep in &spec.depends_on {
                md.push_str(&format!("- {}\n", dep));
            }
            md.push('\n');
        }

        md.push_str("## Phases\n\n");
        for (idx, phase) in spec.phases.iter().enumerate() {
            md.push_str(&format!("### Phase {}: {}\n\n", idx + 1, phase.name));
            md.push_str(&format!("{}\n\n", phase.description));
        }

        if let Some(lt) = &spec.loop_type {
            md.push_str(&format!("## Loop Type\n\n{}\n", lt));
        }

        md
    }
}

/// Slugify a string for use in filenames
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(50)
        .collect()
}

/// Default system prompt for decomposition
const DEFAULT_DECOMPOSE_PROMPT: &str = r#"You are a software architect decomposing a development plan into executable specifications (specs).

Your job is to:
1. Read the plan and understand the goals
2. Break it into logical, sequential specs
3. Identify dependencies between specs
4. Define phases within each spec

Guidelines:
- Create 2-5 specs for a typical plan (more for complex plans)
- Each spec should be independently executable by an AI agent
- Specs should have clear, testable completion criteria
- Dependencies should form a DAG (no cycles)
- Phases within a spec are executed sequentially
- Keep spec titles short and descriptive

For dependencies:
- Reference specs by their title
- A spec can only depend on specs defined before it
- Don't create circular dependencies

Call submit_decomposition with all specs at once.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("OAuth Endpoints"), "oauth-endpoints");
        assert_eq!(slugify("Add User Auth"), "add-user-auth");
    }

    #[test]
    fn test_spec_output_deserialize() {
        let json = r#"{
            "title": "Test Spec",
            "description": "A test spec",
            "depends_on": [],
            "phases": [
                {"name": "Setup", "description": "Set things up"}
            ]
        }"#;

        let spec: SpecOutput = serde_json::from_str(json).unwrap();
        assert_eq!(spec.title, "Test Spec");
        assert_eq!(spec.phases.len(), 1);
    }

    #[test]
    fn test_decomposition_output_deserialize() {
        let json = r#"{
            "specs": [
                {
                    "title": "Spec 1",
                    "description": "First spec",
                    "depends_on": [],
                    "phases": [{"name": "Phase 1", "description": "Do stuff"}]
                },
                {
                    "title": "Spec 2",
                    "description": "Second spec",
                    "depends_on": ["Spec 1"],
                    "phases": [{"name": "Phase 1", "description": "Do more stuff"}]
                }
            ]
        }"#;

        let output: DecompositionOutput = serde_json::from_str(json).unwrap();
        assert_eq!(output.specs.len(), 2);
        assert_eq!(output.specs[1].depends_on, vec!["Spec 1"]);
    }

    #[test]
    fn test_decomposer_config_default() {
        let config = DecomposerConfig::default();
        assert!(!config.loop_types.is_empty());
        assert!(!config.system_prompt.is_empty());
    }
}
