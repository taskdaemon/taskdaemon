//! LLM Client module for TaskDaemon
//!
//! Provides stateless completion requests for Ralph loops. Each iteration gets
//! fresh context - no conversation state carried between calls.

mod anthropic;
pub mod client;
mod error;
mod types;

#[allow(unused_imports)]
pub use anthropic::AnthropicClient;
pub use client::LlmClient;
pub use error::LlmError;
#[allow(unused_imports)]
pub use types::Role;
pub use types::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, MessageContent, StopReason, StreamChunk, TokenUsage,
    ToolCall, ToolDefinition,
};
