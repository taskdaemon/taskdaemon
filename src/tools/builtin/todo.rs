//! todo tool - task list management for agents

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Task status in the todo list
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoStatus::Pending => write!(f, "pending"),
            TodoStatus::InProgress => write!(f, "in_progress"),
            TodoStatus::Completed => write!(f, "completed"),
        }
    }
}

/// A single todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: usize,
    pub task: String,
    pub status: TodoStatus,
    pub created_at: i64,
}

/// Shared todo list state
pub type TodoList = Arc<Mutex<Vec<TodoItem>>>;

/// Create a new shared todo list
pub fn new_todo_list() -> TodoList {
    Arc::new(Mutex::new(Vec::new()))
}

/// Manage a task list
pub struct TodoTool {
    todos: TodoList,
}

impl TodoTool {
    /// Create a new TodoTool with its own todo list
    pub fn new() -> Self {
        Self { todos: new_todo_list() }
    }

    /// Create a TodoTool with a shared todo list
    pub fn with_list(todos: TodoList) -> Self {
        Self { todos }
    }
}

impl Default for TodoTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &'static str {
        "todo"
    }

    fn description(&self) -> &'static str {
        "Manage a task list. Actions: add, complete, list, clear, set_status"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "complete", "list", "clear", "set_status"],
                    "description": "Action to perform"
                },
                "task": {
                    "type": "string",
                    "description": "Task description (for add) or task ID (for complete/set_status)"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed"],
                    "description": "New status (for set_status action)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let action = match input["action"].as_str() {
            Some(a) => a,
            None => return ToolResult::error("action is required"),
        };

        match action {
            "add" => {
                let task = match input["task"].as_str() {
                    Some(t) => t,
                    None => return ToolResult::error("task is required for add action"),
                };

                let mut todos = self.todos.lock().await;
                let id = todos.len() + 1;
                let item = TodoItem {
                    id,
                    task: task.to_string(),
                    status: TodoStatus::Pending,
                    created_at: taskstore::now_ms(),
                };
                todos.push(item);

                ToolResult::success(format!("Added task #{}: {}", id, task))
            }
            "complete" => {
                let task_id = match input["task"].as_str() {
                    Some(t) => match t.parse::<usize>() {
                        Ok(id) => id,
                        Err(_) => return ToolResult::error("task must be a valid task ID number"),
                    },
                    None => return ToolResult::error("task (ID) is required for complete action"),
                };

                let mut todos = self.todos.lock().await;
                if let Some(item) = todos.iter_mut().find(|t| t.id == task_id) {
                    item.status = TodoStatus::Completed;
                    ToolResult::success(format!("Completed task #{}: {}", task_id, item.task))
                } else {
                    ToolResult::error(format!("Task #{} not found", task_id))
                }
            }
            "set_status" => {
                let task_id = match input["task"].as_str() {
                    Some(t) => match t.parse::<usize>() {
                        Ok(id) => id,
                        Err(_) => return ToolResult::error("task must be a valid task ID number"),
                    },
                    None => return ToolResult::error("task (ID) is required for set_status action"),
                };

                let status = match input["status"].as_str() {
                    Some("pending") => TodoStatus::Pending,
                    Some("in_progress") => TodoStatus::InProgress,
                    Some("completed") => TodoStatus::Completed,
                    Some(s) => return ToolResult::error(format!("Invalid status: {}", s)),
                    None => return ToolResult::error("status is required for set_status action"),
                };

                let mut todos = self.todos.lock().await;
                if let Some(item) = todos.iter_mut().find(|t| t.id == task_id) {
                    item.status = status.clone();
                    ToolResult::success(format!("Set task #{} status to {}", task_id, status))
                } else {
                    ToolResult::error(format!("Task #{} not found", task_id))
                }
            }
            "list" => {
                let todos = self.todos.lock().await;
                if todos.is_empty() {
                    return ToolResult::success("No tasks in the list");
                }

                let output: Vec<String> = todos
                    .iter()
                    .map(|t| {
                        let status_marker = match t.status {
                            TodoStatus::Pending => "[ ]",
                            TodoStatus::InProgress => "[~]",
                            TodoStatus::Completed => "[x]",
                        };
                        format!("{} #{}: {}", status_marker, t.id, t.task)
                    })
                    .collect();

                ToolResult::success(output.join("\n"))
            }
            "clear" => {
                let mut todos = self.todos.lock().await;
                let count = todos.len();
                todos.clear();
                ToolResult::success(format!("Cleared {} task(s)", count))
            }
            _ => ToolResult::error(format!("Unknown action: {}", action)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_todo_add() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TodoTool::new();

        let result = tool
            .execute(serde_json::json!({"action": "add", "task": "Write tests"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("#1"));
        assert!(result.content.contains("Write tests"));
    }

    #[tokio::test]
    async fn test_todo_list() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TodoTool::new();

        // Add some tasks
        tool.execute(serde_json::json!({"action": "add", "task": "Task 1"}), &ctx)
            .await;
        tool.execute(serde_json::json!({"action": "add", "task": "Task 2"}), &ctx)
            .await;

        let result = tool.execute(serde_json::json!({"action": "list"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("Task 1"));
        assert!(result.content.contains("Task 2"));
        assert!(result.content.contains("[ ]")); // Pending status
    }

    #[tokio::test]
    async fn test_todo_complete() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TodoTool::new();

        // Add a task
        tool.execute(serde_json::json!({"action": "add", "task": "Task 1"}), &ctx)
            .await;

        // Complete it
        let result = tool
            .execute(serde_json::json!({"action": "complete", "task": "1"}), &ctx)
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("Completed"));

        // Verify it's completed in the list
        let list_result = tool.execute(serde_json::json!({"action": "list"}), &ctx).await;
        assert!(list_result.content.contains("[x]"));
    }

    #[tokio::test]
    async fn test_todo_set_status() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TodoTool::new();

        // Add a task
        tool.execute(serde_json::json!({"action": "add", "task": "Task 1"}), &ctx)
            .await;

        // Set to in_progress
        let result = tool
            .execute(
                serde_json::json!({"action": "set_status", "task": "1", "status": "in_progress"}),
                &ctx,
            )
            .await;

        assert!(!result.is_error);
        assert!(result.content.contains("in_progress"));

        // Verify status in the list
        let list_result = tool.execute(serde_json::json!({"action": "list"}), &ctx).await;
        assert!(list_result.content.contains("[~]"));
    }

    #[tokio::test]
    async fn test_todo_clear() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TodoTool::new();

        // Add tasks
        tool.execute(serde_json::json!({"action": "add", "task": "Task 1"}), &ctx)
            .await;
        tool.execute(serde_json::json!({"action": "add", "task": "Task 2"}), &ctx)
            .await;

        // Clear
        let result = tool.execute(serde_json::json!({"action": "clear"}), &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("Cleared 2"));

        // Verify empty
        let list_result = tool.execute(serde_json::json!({"action": "list"}), &ctx).await;
        assert!(list_result.content.contains("No tasks"));
    }

    #[tokio::test]
    async fn test_todo_complete_not_found() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TodoTool::new();

        let result = tool
            .execute(serde_json::json!({"action": "complete", "task": "999"}), &ctx)
            .await;

        assert!(result.is_error);
        assert!(result.content.contains("not found"));
    }

    #[tokio::test]
    async fn test_todo_missing_action() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = TodoTool::new();

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("action is required"));
    }
}
