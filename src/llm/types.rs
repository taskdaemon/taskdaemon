//! LLM request/response types for TaskDaemon
//!
//! These types model the Anthropic Messages API but are provider-agnostic enough
//! to support other providers in the future.

use serde::{Deserialize, Serialize};

/// A completion request - everything needed for one LLM call
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    /// System prompt (rendered from Handlebars template)
    pub system_prompt: String,

    /// User messages (typically just one for Ralph loops)
    pub messages: Vec<Message>,

    /// Available tools for this loop type
    pub tools: Vec<ToolDefinition>,

    /// Max tokens for response (from config)
    pub max_tokens: u32,
}

/// A message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
}

impl Message {
    /// Create a user message with text content
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(text.into()),
        }
    }

    /// Create an assistant message with text content
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(text.into()),
        }
    }

    /// Create a user message with multiple content blocks
    pub fn user_blocks(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Blocks(blocks),
        }
    }

    /// Create an assistant message with multiple content blocks
    pub fn assistant_blocks(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Blocks(blocks),
        }
    }
}

/// Message role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Message content - either plain text or structured blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl MessageContent {
    /// Get text content if this is a text message
    pub fn as_text(&self) -> Option<&str> {
        match self {
            MessageContent::Text(text) => Some(text),
            MessageContent::Blocks(_) => None,
        }
    }
}

/// A content block in a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

impl ContentBlock {
    /// Create a text content block
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    /// Create a tool result block
    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>, is_error: bool) -> Self {
        ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: content.into(),
            is_error,
        }
    }
}

/// Response from a completion request
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    /// Text content (if any)
    pub content: Option<String>,

    /// Tool calls requested by the model
    pub tool_calls: Vec<ToolCall>,

    /// Why the model stopped
    pub stop_reason: StopReason,

    /// Token usage for cost tracking
    pub usage: TokenUsage,
}

/// A tool call requested by the model
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Why the model stopped generating
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
}

impl StopReason {
    /// Parse from Anthropic API stop_reason string
    pub fn from_anthropic(s: &str) -> Self {
        match s {
            "end_turn" => StopReason::EndTurn,
            "tool_use" => StopReason::ToolUse,
            "max_tokens" => StopReason::MaxTokens,
            "stop_sequence" => StopReason::StopSequence,
            _ => StopReason::EndTurn,
        }
    }
}

/// Token usage for cost tracking
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

impl TokenUsage {
    /// Calculate cost in USD based on model pricing
    pub fn cost_usd(&self, model: &str) -> f64 {
        let (input_price, output_price) = match model {
            m if m.contains("opus") => (15.0, 75.0),
            m if m.contains("sonnet") => (3.0, 15.0),
            m if m.contains("haiku") => (0.25, 1.25),
            _ => (3.0, 15.0), // Default to sonnet pricing
        };

        let input_cost = (self.input_tokens as f64 / 1_000_000.0) * input_price;
        let output_cost = (self.output_tokens as f64 / 1_000_000.0) * output_price;

        // Cache reads are 90% cheaper
        let cache_cost = (self.cache_read_tokens as f64 / 1_000_000.0) * input_price * 0.1;

        input_cost + output_cost + cache_cost
    }
}

/// Tool definition for the LLM
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

impl ToolDefinition {
    /// Create a new tool definition
    pub fn new(name: impl Into<String>, description: impl Into<String>, input_schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }

    /// Convert to Anthropic API schema format
    pub fn to_anthropic_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.input_schema,
        })
    }
}

/// Streaming chunk for real-time TUI updates
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Message started with input token count (from message_start event)
    MessageStart { input_tokens: u64 },

    /// Text being generated
    TextDelta(String),

    /// Tool call starting
    ToolUseStart { id: String, name: String },

    /// Tool call JSON fragment
    ToolUseDelta { id: String, json_delta: String },

    /// Tool call complete
    ToolUseEnd { id: String },

    /// Message complete with final stats
    MessageDone { stop_reason: StopReason, usage: TokenUsage },

    /// Error during streaming
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_user() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert!(matches!(msg.content, MessageContent::Text(ref s) if s == "Hello"));
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("Hi there");
        assert_eq!(msg.role, Role::Assistant);
        assert!(matches!(msg.content, MessageContent::Text(ref s) if s == "Hi there"));
    }

    #[test]
    fn test_token_usage_cost_sonnet() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_read_tokens: 500_000,
            cache_creation_tokens: 0,
        };

        // Sonnet: $3/M input, $15/M output, 90% discount on cache
        let cost = usage.cost_usd("claude-sonnet-4");
        // $3 (input) + $1.50 (output) + $0.15 (cache @ 10%)
        assert!((cost - 4.65).abs() < 0.01);
    }

    #[test]
    fn test_token_usage_cost_opus() {
        let usage = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
        };

        // Opus: $15/M input, $75/M output
        let cost = usage.cost_usd("claude-opus-4");
        // $15 (input) + $7.50 (output)
        assert!((cost - 22.5).abs() < 0.01);
    }

    #[test]
    fn test_stop_reason_from_anthropic() {
        assert_eq!(StopReason::from_anthropic("end_turn"), StopReason::EndTurn);
        assert_eq!(StopReason::from_anthropic("tool_use"), StopReason::ToolUse);
        assert_eq!(StopReason::from_anthropic("max_tokens"), StopReason::MaxTokens);
        assert_eq!(StopReason::from_anthropic("stop_sequence"), StopReason::StopSequence);
        assert_eq!(StopReason::from_anthropic("unknown"), StopReason::EndTurn);
    }

    #[test]
    fn test_tool_definition_to_anthropic_schema() {
        let tool = ToolDefinition::new(
            "read",
            "Read a file",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        );

        let schema = tool.to_anthropic_schema();
        assert_eq!(schema["name"], "read");
        assert_eq!(schema["description"], "Read a file");
        assert!(schema["input_schema"].is_object());
    }

    #[test]
    fn test_content_block_text() {
        let block = ContentBlock::text("Hello world");
        match block {
            ContentBlock::Text { text } => assert_eq!(text, "Hello world"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_content_block_tool_result() {
        let block = ContentBlock::tool_result("tool_123", "Success", false);
        match block {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "tool_123");
                assert_eq!(content, "Success");
                assert!(!is_error);
            }
            _ => panic!("Expected ToolResult block"),
        }
    }
}
