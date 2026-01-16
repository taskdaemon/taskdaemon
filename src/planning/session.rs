//! PlanningSession - multi-turn conversation for plan creation
//!
//! This session orchestrates a conversation with the user to refine
//! a vague idea into a concrete, actionable Plan.

use std::io::{self, BufRead, Write as IoWrite};
use std::path::PathBuf;
use std::sync::Arc;

use eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::domain::{Plan, PlanStatus};
use crate::llm::{CompletionRequest, CompletionResponse, LlmClient, Message, ToolDefinition};
use crate::state::StateManager;

/// Configuration for a planning session
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Maximum conversation turns before forcing completion
    pub max_turns: usize,

    /// Directory where plan markdown files are stored
    pub plans_dir: PathBuf,

    /// System prompt for the planning agent
    pub system_prompt: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_turns: 20,
            plans_dir: PathBuf::from("plans"),
            system_prompt: DEFAULT_PLANNING_PROMPT.to_string(),
        }
    }
}

/// State of the planning session
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionState {
    /// Gathering requirements via conversation
    Conversing,
    /// Plan has been finalized
    PlanFinalized,
    /// Session was cancelled by user
    Cancelled,
    /// Session hit max turns without completing
    MaxTurnsReached,
}

/// Draft plan being refined during conversation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanDraft {
    pub title: Option<String>,
    pub description: Option<String>,
    pub goals: Vec<String>,
    pub constraints: Vec<String>,
    pub context: Option<String>,
}

impl PlanDraft {
    /// Check if the draft has minimum required fields
    pub fn is_complete(&self) -> bool {
        self.title.is_some() && !self.goals.is_empty()
    }
}

/// Response from the LLM during planning
#[derive(Debug, Clone)]
struct PlanningResponse {
    /// Text content to show the user
    content: String,
    /// Whether the LLM considers the plan complete
    plan_complete: bool,
    /// Updated draft if provided
    draft_update: Option<PlanDraft>,
}

/// PlanningSession orchestrates multi-turn conversation for plan creation
pub struct PlanningSession {
    /// LLM client for conversation
    llm: Arc<dyn LlmClient>,

    /// State manager for persistence
    state: StateManager,

    /// Conversation history
    conversation: Vec<Message>,

    /// Current session state
    session_state: SessionState,

    /// Draft plan being refined
    draft: PlanDraft,

    /// Configuration
    config: SessionConfig,

    /// Turn count
    turn_count: usize,
}

impl PlanningSession {
    /// Create a new planning session
    pub fn new(llm: Arc<dyn LlmClient>, state: StateManager, config: SessionConfig) -> Self {
        Self {
            llm,
            state,
            conversation: Vec::new(),
            session_state: SessionState::Conversing,
            draft: PlanDraft::default(),
            config,
            turn_count: 0,
        }
    }

    /// Run interactive planning session (reads from stdin, writes to stdout)
    ///
    /// Returns the finalized Plan if successful, None if cancelled.
    pub async fn run_interactive(&mut self, initial_task: &str) -> Result<Option<Plan>> {
        info!("Starting planning session");

        // Seed conversation with initial task
        self.conversation.push(Message::user(initial_task));

        let stdin = io::stdin();
        let mut stdout = io::stdout();

        loop {
            // Check turn limit
            if self.turn_count >= self.config.max_turns {
                self.session_state = SessionState::MaxTurnsReached;
                warn!("Max turns ({}) reached, forcing plan completion", self.config.max_turns);
                return self.force_finalize_plan().await;
            }

            // Get LLM response
            let response = self.get_llm_response().await?;
            self.turn_count += 1;

            // Display response to user
            println!("\n{}\n", response.content);

            // Update draft if provided
            if let Some(draft_update) = response.draft_update {
                self.merge_draft(draft_update);
            }

            // Check if plan is complete
            if response.plan_complete {
                self.session_state = SessionState::PlanFinalized;
                return self.finalize_plan().await.map(Some);
            }

            // Get user input
            print!("> ");
            stdout.flush()?;

            let handle = stdin.lock();

            let input = match handle.lines().next() {
                Some(Ok(line)) => line,
                Some(Err(e)) => return Err(e.into()),
                None => {
                    // EOF - treat as quit
                    self.session_state = SessionState::Cancelled;
                    println!("\nSession cancelled.");
                    return Ok(None);
                }
            };

            let input = input.trim();

            // Handle special commands
            match input.to_lowercase().as_str() {
                "quit" | "exit" | "/quit" | "/exit" | "q" => {
                    self.session_state = SessionState::Cancelled;
                    println!("Session cancelled.");
                    return Ok(None);
                }
                "/done" | "/finalize" => {
                    // User forcing completion
                    self.session_state = SessionState::PlanFinalized;
                    return self.finalize_plan().await.map(Some);
                }
                "/draft" => {
                    // Show current draft
                    self.show_draft();
                    continue;
                }
                "/help" => {
                    self.show_help();
                    continue;
                }
                "" => {
                    // Empty input - prompt again
                    continue;
                }
                _ => {}
            }

            // Add user message to conversation
            self.conversation.push(Message::user(input));
        }
    }

    /// Get a response from the LLM
    async fn get_llm_response(&mut self) -> Result<PlanningResponse> {
        let request = CompletionRequest {
            system_prompt: self.build_system_prompt(),
            messages: self.conversation.clone(),
            tools: self.build_tools(),
            max_tokens: 4096,
        };

        let response = self.llm.complete(request).await.context("Failed to get LLM response")?;

        self.parse_response(response)
    }

    /// Build the system prompt with current draft state
    fn build_system_prompt(&self) -> String {
        let mut prompt = self.config.system_prompt.clone();

        // Inject current draft state
        if self.draft.title.is_some() || !self.draft.goals.is_empty() {
            prompt.push_str("\n\n## Current Plan Draft\n");
            if let Some(title) = &self.draft.title {
                prompt.push_str(&format!("Title: {}\n", title));
            }
            if let Some(desc) = &self.draft.description {
                prompt.push_str(&format!("Description: {}\n", desc));
            }
            if !self.draft.goals.is_empty() {
                prompt.push_str("Goals:\n");
                for goal in &self.draft.goals {
                    prompt.push_str(&format!("- {}\n", goal));
                }
            }
            if !self.draft.constraints.is_empty() {
                prompt.push_str("Constraints:\n");
                for constraint in &self.draft.constraints {
                    prompt.push_str(&format!("- {}\n", constraint));
                }
            }
        }

        prompt
    }

    /// Build tools available for planning
    fn build_tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition::new(
                "update_plan_draft",
                "Update the plan draft with new information gathered from the conversation. Call this as you gather requirements.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Title for the plan (max 256 chars)"
                        },
                        "description": {
                            "type": "string",
                            "description": "Brief description of what the plan accomplishes"
                        },
                        "goals": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "List of goals/outcomes the plan should achieve"
                        },
                        "constraints": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Constraints or limitations to consider"
                        },
                        "context": {
                            "type": "string",
                            "description": "Additional context or background"
                        }
                    }
                }),
            ),
            ToolDefinition::new(
                "finalize_plan",
                "Call this when you have gathered enough information and the plan is ready to be finalized. The user has agreed to the plan.",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "description": "Final title for the plan"
                        },
                        "description": {
                            "type": "string",
                            "description": "Final description"
                        },
                        "goals": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Final list of goals"
                        },
                        "constraints": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Final constraints"
                        }
                    },
                    "required": ["title", "goals"]
                }),
            ),
        ]
    }

    /// Parse LLM response into structured planning response
    fn parse_response(&mut self, response: CompletionResponse) -> Result<PlanningResponse> {
        let mut content = response.content.unwrap_or_default();
        let mut plan_complete = false;
        let mut draft_update: Option<PlanDraft> = None;

        // Process tool calls
        for tool_call in &response.tool_calls {
            match tool_call.name.as_str() {
                "update_plan_draft" => {
                    let update = self.parse_draft_update(&tool_call.input)?;
                    draft_update = Some(update);
                }
                "finalize_plan" => {
                    let final_draft = self.parse_draft_update(&tool_call.input)?;
                    draft_update = Some(final_draft);
                    plan_complete = true;
                }
                _ => {
                    debug!("Unknown tool call: {}", tool_call.name);
                }
            }
        }

        // If no content but we have tool calls, generate a status message
        if content.is_empty() && draft_update.is_some() {
            if plan_complete {
                content = "I've gathered enough information. Let me finalize the plan.".to_string();
            } else {
                content = "I've updated the plan draft with that information.".to_string();
            }
        }

        // Add assistant response to conversation
        self.conversation.push(Message::assistant(&content));

        Ok(PlanningResponse {
            content,
            plan_complete,
            draft_update,
        })
    }

    /// Parse a draft update from tool input
    fn parse_draft_update(&self, input: &serde_json::Value) -> Result<PlanDraft> {
        Ok(PlanDraft {
            title: input.get("title").and_then(|v| v.as_str()).map(String::from),
            description: input.get("description").and_then(|v| v.as_str()).map(String::from),
            goals: input
                .get("goals")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            constraints: input
                .get("constraints")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            context: input.get("context").and_then(|v| v.as_str()).map(String::from),
        })
    }

    /// Merge a draft update into the current draft
    fn merge_draft(&mut self, update: PlanDraft) {
        if update.title.is_some() {
            self.draft.title = update.title;
        }
        if update.description.is_some() {
            self.draft.description = update.description;
        }
        if !update.goals.is_empty() {
            self.draft.goals = update.goals;
        }
        if !update.constraints.is_empty() {
            self.draft.constraints = update.constraints;
        }
        if update.context.is_some() {
            self.draft.context = update.context;
        }
    }

    /// Finalize the plan and persist to state
    async fn finalize_plan(&mut self) -> Result<Plan> {
        if !self.draft.is_complete() {
            bail!("Plan draft is incomplete - missing title or goals");
        }

        let title = self.draft.title.as_ref().unwrap();

        // Create plan markdown file
        let file_path = self.create_plan_file(title)?;

        // Create Plan domain object
        let mut plan = Plan::new(title, file_path.display().to_string());
        plan.set_status(PlanStatus::Ready);

        // Persist to state
        self.state
            .create_plan(plan.clone())
            .await
            .context("Failed to persist plan")?;

        info!(plan_id = %plan.id, "Plan created and persisted");

        println!("\nPlan created: {}", plan.id);
        println!("File: {}", file_path.display());

        Ok(plan)
    }

    /// Force finalize when max turns reached
    async fn force_finalize_plan(&mut self) -> Result<Option<Plan>> {
        // If we have a title and at least one goal, finalize
        if self.draft.is_complete() {
            warn!("Force-finalizing plan due to max turns");
            return self.finalize_plan().await.map(Some);
        }

        // Otherwise, give up
        println!("\nMax conversation turns reached without completing plan.");
        println!("Please restart with a more specific task description.");
        Ok(None)
    }

    /// Create the plan markdown file
    fn create_plan_file(&self, title: &str) -> Result<PathBuf> {
        // Ensure plans directory exists
        std::fs::create_dir_all(&self.config.plans_dir).context("Failed to create plans directory")?;

        // Generate filename from title
        let slug = slugify(title);
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let filename = format!("{}-{}.md", timestamp, slug);
        let file_path = self.config.plans_dir.join(&filename);

        // Build markdown content
        let content = self.build_plan_markdown();

        std::fs::write(&file_path, content).context("Failed to write plan file")?;

        Ok(file_path)
    }

    /// Build markdown content for the plan file
    fn build_plan_markdown(&self) -> String {
        let mut md = String::new();

        // Title
        if let Some(title) = &self.draft.title {
            md.push_str(&format!("# {}\n\n", title));
        }

        // Description
        if let Some(desc) = &self.draft.description {
            md.push_str(&format!("{}\n\n", desc));
        }

        // Goals
        if !self.draft.goals.is_empty() {
            md.push_str("## Goals\n\n");
            for goal in &self.draft.goals {
                md.push_str(&format!("- {}\n", goal));
            }
            md.push('\n');
        }

        // Constraints
        if !self.draft.constraints.is_empty() {
            md.push_str("## Constraints\n\n");
            for constraint in &self.draft.constraints {
                md.push_str(&format!("- {}\n", constraint));
            }
            md.push('\n');
        }

        // Context
        if let Some(ctx) = &self.draft.context {
            md.push_str("## Context\n\n");
            md.push_str(ctx);
            md.push_str("\n\n");
        }

        // Conversation summary
        md.push_str("## Planning Conversation\n\n");
        md.push_str(&format!("_Completed in {} turns_\n", self.turn_count));

        md
    }

    /// Show the current draft to the user
    fn show_draft(&self) {
        println!("\n--- Current Plan Draft ---");
        if let Some(title) = &self.draft.title {
            println!("Title: {}", title);
        } else {
            println!("Title: (not set)");
        }
        if let Some(desc) = &self.draft.description {
            println!("Description: {}", desc);
        }
        if !self.draft.goals.is_empty() {
            println!("Goals:");
            for goal in &self.draft.goals {
                println!("  - {}", goal);
            }
        } else {
            println!("Goals: (none)");
        }
        if !self.draft.constraints.is_empty() {
            println!("Constraints:");
            for constraint in &self.draft.constraints {
                println!("  - {}", constraint);
            }
        }
        println!("--------------------------\n");
    }

    /// Show help for session commands
    fn show_help(&self) {
        println!("\n--- Planning Session Commands ---");
        println!("  /draft    - Show current plan draft");
        println!("  /done     - Finalize plan with current draft");
        println!("  /help     - Show this help");
        println!("  quit      - Cancel session");
        println!("---------------------------------\n");
    }

    /// Get current session state
    pub fn state(&self) -> &SessionState {
        &self.session_state
    }

    /// Get current draft
    pub fn draft(&self) -> &PlanDraft {
        &self.draft
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

/// Default system prompt for planning sessions
const DEFAULT_PLANNING_PROMPT: &str = r#"You are a planning assistant helping to refine a software development task into a concrete plan.

Your job is to:
1. Understand what the user wants to accomplish
2. Ask clarifying questions to fill in gaps
3. Identify constraints and requirements
4. Build a clear, actionable plan

Guidelines:
- Ask ONE focused question at a time
- Don't ask about things the user has already explained
- When you have enough information (title, clear goals, key constraints), finalize the plan
- Use the update_plan_draft tool as you gather information
- Use the finalize_plan tool when the plan is ready

Keep responses concise and focused. Avoid lengthy explanations.

When you call finalize_plan, the conversation ends and work begins.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_draft_is_complete() {
        let mut draft = PlanDraft::default();
        assert!(!draft.is_complete());

        draft.title = Some("Test Plan".to_string());
        assert!(!draft.is_complete());

        draft.goals.push("Goal 1".to_string());
        assert!(draft.is_complete());
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Add OAuth 2.0 Support"), "add-oauth-2-0-support");
        assert_eq!(slugify("  Multiple   Spaces  "), "multiple-spaces");
        assert_eq!(slugify("Special!@#$%Characters"), "special-characters");
    }

    #[test]
    fn test_session_config_default() {
        let config = SessionConfig::default();
        assert_eq!(config.max_turns, 20);
        assert!(!config.system_prompt.is_empty());
    }
}
