//! Prompt Loader
//!
//! Loads prompt templates from files or falls back to embedded defaults.

use std::path::{Path, PathBuf};

use eyre::{Result, eyre};
use handlebars::Handlebars;
use serde::Serialize;
use tracing::{debug, info};

use super::embedded;

/// Focus area for each review pass in the Rule of Five methodology
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    /// Pass 1: Are all required sections present?
    Completeness,
    /// Pass 2: Are there logical errors or invalid assumptions?
    Correctness,
    /// Pass 3: Are edge cases and error conditions handled?
    EdgeCases,
    /// Pass 4: Does it fit the existing architecture?
    Architecture,
    /// Pass 5: Is it clear and unambiguous?
    Clarity,
}

impl FocusArea {
    /// Get the focus area for a given pass number (1-5)
    pub fn from_pass(pass: u8) -> Option<Self> {
        match pass {
            1 => Some(Self::Completeness),
            2 => Some(Self::Correctness),
            3 => Some(Self::EdgeCases),
            4 => Some(Self::Architecture),
            5 => Some(Self::Clarity),
            _ => None,
        }
    }

    /// Get the display name for this focus area
    pub fn name(&self) -> &'static str {
        match self {
            Self::Completeness => "Completeness",
            Self::Correctness => "Correctness",
            Self::EdgeCases => "Edge Cases",
            Self::Architecture => "Architecture",
            Self::Clarity => "Clarity",
        }
    }

    /// Get the template name for this focus area
    pub fn template_name(&self) -> &'static str {
        match self {
            Self::Completeness => "plan-completeness",
            Self::Correctness => "plan-correctness",
            Self::EdgeCases => "plan-edge-cases",
            Self::Architecture => "plan-architecture",
            Self::Clarity => "plan-clarity",
        }
    }
}

impl std::fmt::Display for FocusArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Context for rendering prompt templates
#[derive(Debug, Clone, Serialize)]
pub struct PromptContext {
    /// The conversation history (for Pass 1)
    pub conversation: String,
    /// Current pass number (1-5)
    pub pass_number: u8,
    /// Output from previous pass (for Passes 2-5)
    pub previous_output: Option<String>,
    /// Focus area for this pass
    pub focus_area: String,
    /// Is this the first pass?
    pub is_first_pass: bool,
    /// Focus area booleans for conditional rendering
    pub focus_completeness: bool,
    pub focus_correctness: bool,
    pub focus_edge_cases: bool,
    pub focus_architecture: bool,
    pub focus_clarity: bool,
}

impl PromptContext {
    /// Create a context for the first pass
    pub fn first_pass(conversation: String) -> Self {
        Self {
            conversation,
            pass_number: 1,
            previous_output: None,
            focus_area: "Completeness".to_string(),
            is_first_pass: true,
            focus_completeness: true,
            focus_correctness: false,
            focus_edge_cases: false,
            focus_architecture: false,
            focus_clarity: false,
        }
    }

    /// Create a context for a review pass (2-5)
    pub fn review_pass(pass_number: u8, previous_output: String, focus: FocusArea) -> Self {
        Self {
            conversation: String::new(),
            pass_number,
            previous_output: Some(previous_output),
            focus_area: focus.name().to_string(),
            is_first_pass: false,
            focus_completeness: focus == FocusArea::Completeness,
            focus_correctness: focus == FocusArea::Correctness,
            focus_edge_cases: focus == FocusArea::EdgeCases,
            focus_architecture: focus == FocusArea::Architecture,
            focus_clarity: focus == FocusArea::Clarity,
        }
    }
}

/// Loads and renders prompt templates
pub struct PromptLoader {
    /// Handlebars template engine
    hbs: Handlebars<'static>,
    /// User override directory (e.g., `.taskdaemon/prompts/`)
    user_dir: Option<PathBuf>,
    /// Repo default directory (e.g., `prompts/`)
    repo_dir: Option<PathBuf>,
}

impl PromptLoader {
    /// Create a new prompt loader with the given directories
    ///
    /// # Arguments
    /// * `worktree` - The worktree root (used to find `.taskdaemon/prompts/` and `prompts/`)
    pub fn new(worktree: impl AsRef<Path>) -> Self {
        let worktree = worktree.as_ref();
        let user_dir = worktree.join(".taskdaemon/prompts");
        let repo_dir = worktree.join("prompts");

        Self {
            hbs: Handlebars::new(),
            user_dir: if user_dir.exists() { Some(user_dir) } else { None },
            repo_dir: if repo_dir.exists() { Some(repo_dir) } else { None },
        }
    }

    /// Create a loader that only uses embedded prompts (for testing)
    pub fn embedded_only() -> Self {
        Self {
            hbs: Handlebars::new(),
            user_dir: None,
            repo_dir: None,
        }
    }

    /// Load a template by name
    ///
    /// Checks in order:
    /// 1. User override: `.taskdaemon/prompts/{name}.pmt`
    /// 2. Repo default: `prompts/{name}.pmt`
    /// 3. Embedded fallback
    fn load_template(&self, name: &str) -> Result<String> {
        // Try user override first
        if let Some(ref user_dir) = self.user_dir {
            let path = user_dir.join(format!("{}.pmt", name));
            if path.exists() {
                debug!("Loading prompt from user override: {:?}", path);
                return std::fs::read_to_string(&path)
                    .map_err(|e| eyre!("Failed to read user prompt {}: {}", path.display(), e));
            }
        }

        // Try repo default
        if let Some(ref repo_dir) = self.repo_dir {
            let path = repo_dir.join(format!("{}.pmt", name));
            if path.exists() {
                debug!("Loading prompt from repo: {:?}", path);
                return std::fs::read_to_string(&path)
                    .map_err(|e| eyre!("Failed to read repo prompt {}: {}", path.display(), e));
            }
        }

        // Fall back to embedded
        if let Some(content) = embedded::get_embedded(name) {
            debug!("Using embedded prompt: {}", name);
            return Ok(content.to_string());
        }

        Err(eyre!("Prompt template not found: {}", name))
    }

    /// Render a template with the given context
    pub fn render(&self, template_name: &str, context: &PromptContext) -> Result<String> {
        let template = self.load_template(template_name)?;
        info!(
            "Rendering template '{}' for pass {} (focus: {})",
            template_name, context.pass_number, context.focus_area
        );

        self.hbs
            .render_template(&template, context)
            .map_err(|e| eyre!("Failed to render template {}: {}", template_name, e))
    }

    /// Get the consolidated plan prompt (includes Rule of Five)
    pub fn plan_prompt(&self) -> Result<String> {
        self.load_template("plan")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_area_from_pass() {
        assert_eq!(FocusArea::from_pass(1), Some(FocusArea::Completeness));
        assert_eq!(FocusArea::from_pass(2), Some(FocusArea::Correctness));
        assert_eq!(FocusArea::from_pass(3), Some(FocusArea::EdgeCases));
        assert_eq!(FocusArea::from_pass(4), Some(FocusArea::Architecture));
        assert_eq!(FocusArea::from_pass(5), Some(FocusArea::Clarity));
        assert_eq!(FocusArea::from_pass(0), None);
        assert_eq!(FocusArea::from_pass(6), None);
    }

    #[test]
    fn test_focus_area_name() {
        assert_eq!(FocusArea::Completeness.name(), "Completeness");
        assert_eq!(FocusArea::Correctness.name(), "Correctness");
        assert_eq!(FocusArea::EdgeCases.name(), "Edge Cases");
        assert_eq!(FocusArea::Architecture.name(), "Architecture");
        assert_eq!(FocusArea::Clarity.name(), "Clarity");
    }

    #[test]
    fn test_prompt_context_first_pass() {
        let ctx = PromptContext::first_pass("test conversation".to_string());
        assert_eq!(ctx.pass_number, 1);
        assert!(ctx.is_first_pass);
        assert!(ctx.focus_completeness);
        assert!(!ctx.focus_correctness);
        assert!(ctx.previous_output.is_none());
    }

    #[test]
    fn test_prompt_context_review_pass() {
        let ctx = PromptContext::review_pass(3, "previous output".to_string(), FocusArea::EdgeCases);
        assert_eq!(ctx.pass_number, 3);
        assert!(!ctx.is_first_pass);
        assert!(!ctx.focus_completeness);
        assert!(ctx.focus_edge_cases);
        assert_eq!(ctx.previous_output, Some("previous output".to_string()));
    }

    #[test]
    fn test_prompt_loader_plan() {
        let loader = PromptLoader::embedded_only();

        // Should load consolidated plan prompt
        let plan = loader.plan_prompt();
        assert!(plan.is_ok());
        let content = plan.unwrap();
        assert!(content.contains("software architect"));
        assert!(content.contains("Rule of Five"));
    }

    #[test]
    fn test_prompt_loader_unknown_template() {
        let loader = PromptLoader::embedded_only();
        let result = loader.load_template("nonexistent-template");
        assert!(result.is_err());
    }
}
