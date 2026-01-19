//! Tool trait definition

use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use super::context::ToolContext;

/// A tool that can be called by the LLM
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (matches LLM tool_use name)
    fn name(&self) -> &'static str;

    /// Human-readable description
    fn description(&self) -> &'static str;

    /// JSON Schema for input parameters
    fn input_schema(&self) -> Value;

    /// Execute the tool
    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult;
}

/// Result of a tool execution
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
}

impl ToolResult {
    /// Create a successful result
    pub fn success(content: impl Into<String>) -> Self {
        debug!("ToolResult::success: called");
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    /// Create an error result
    pub fn error(content: impl Into<String>) -> Self {
        debug!("ToolResult::error: called");
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("File written successfully");
        assert!(!result.is_error);
        assert_eq!(result.content, "File written successfully");
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("File not found");
        assert!(result.is_error);
        assert_eq!(result.content, "File not found");
    }
}
