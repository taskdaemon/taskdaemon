//! ToolExecutor - manages tool execution for a loop

use std::collections::HashMap;

use crate::llm::{ToolCall, ToolDefinition};

use super::builtin::{
    CompleteTaskTool, EditFileTool, FetchTool, GlobTool, GrepTool, ListDirectoryTool, QueryTool, ReadFileTool,
    RunCommandTool, SearchTool, ShareTool, TodoTool, TreeTool, WriteFileTool,
};
use super::{Tool, ToolContext, ToolResult};

/// Manages tool execution for a loop
pub struct ToolExecutor {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolExecutor {
    /// Create executor with standard tools
    pub fn standard() -> Self {
        let mut tools: HashMap<String, Box<dyn Tool>> = HashMap::new();

        // File system tools
        tools.insert("read".into(), Box::new(ReadFileTool));
        tools.insert("write".into(), Box::new(WriteFileTool));
        tools.insert("edit".into(), Box::new(EditFileTool));
        tools.insert("list".into(), Box::new(ListDirectoryTool));
        tools.insert("glob".into(), Box::new(GlobTool));
        tools.insert("grep".into(), Box::new(GrepTool));

        // Command execution
        tools.insert("bash".into(), Box::new(RunCommandTool));

        // New tools
        tools.insert("tree".into(), Box::new(TreeTool));
        tools.insert("todo".into(), Box::new(TodoTool::new()));
        tools.insert("fetch".into(), Box::new(FetchTool::new()));
        tools.insert("search".into(), Box::new(SearchTool));

        // Task completion
        tools.insert("complete_task".into(), Box::new(CompleteTaskTool));

        // Coordination tools (require coordinator handle in context)
        tools.insert("query".into(), Box::new(QueryTool));
        tools.insert("share".into(), Box::new(ShareTool));

        Self { tools }
    }

    /// Create an empty executor (for testing)
    pub fn empty() -> Self {
        Self { tools: HashMap::new() }
    }

    /// Add a tool to the executor
    pub fn add_tool(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Get tool definitions for LLM
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Get definitions for a subset of tools by name
    pub fn definitions_for(&self, tool_names: &[String]) -> Vec<ToolDefinition> {
        tool_names
            .iter()
            .filter_map(|name| self.tools.get(name))
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect()
    }

    /// Execute a tool call
    pub async fn execute(&self, tool_call: &ToolCall, ctx: &ToolContext) -> ToolResult {
        match self.tools.get(&tool_call.name) {
            Some(tool) => tool.execute(tool_call.input.clone(), ctx).await,
            None => ToolResult::error(format!("Unknown tool: {}", tool_call.name)),
        }
    }

    /// Execute multiple tool calls
    pub async fn execute_all(&self, tool_calls: &[ToolCall], ctx: &ToolContext) -> Vec<(String, ToolResult)> {
        let mut results = Vec::with_capacity(tool_calls.len());

        for call in tool_calls {
            let result = self.execute(call, ctx).await;
            results.push((call.id.clone(), result));
        }

        results
    }

    /// Check if a tool exists
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Get tool names
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

impl Default for ToolExecutor {
    fn default() -> Self {
        Self::standard()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_standard_executor_has_basic_tools() {
        let executor = ToolExecutor::standard();

        assert!(executor.has_tool("read"));
        assert!(executor.has_tool("write"));
        assert!(executor.has_tool("edit"));
        assert!(executor.has_tool("bash"));
        assert!(executor.has_tool("list"));
        assert!(executor.has_tool("glob"));
    }

    #[test]
    fn test_definitions_returns_all_tools() {
        let executor = ToolExecutor::standard();
        let defs = executor.definitions();

        assert!(!defs.is_empty());
        assert!(defs.iter().any(|d| d.name == "read"));
    }

    #[test]
    fn test_definitions_for_subset() {
        let executor = ToolExecutor::standard();
        let defs = executor.definitions_for(&["read".to_string(), "write".to_string()]);

        assert_eq!(defs.len(), 2);
        assert!(defs.iter().any(|d| d.name == "read"));
        assert!(defs.iter().any(|d| d.name == "write"));
    }

    #[tokio::test]
    async fn test_execute_unknown_tool() {
        let executor = ToolExecutor::standard();
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());

        let call = ToolCall {
            id: "call_1".to_string(),
            name: "unknown_tool".to_string(),
            input: serde_json::json!({}),
        };

        let result = executor.execute(&call, &ctx).await;
        assert!(result.is_error);
        assert!(result.content.contains("Unknown tool"));
    }
}
