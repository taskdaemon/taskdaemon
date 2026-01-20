//! LLM Client module for TaskDaemon
//!
//! Provides LLM completion requests and utilities.

use std::sync::Arc;

use tracing::debug;

mod anthropic;
pub mod client;
mod error;
mod openai;
mod types;

pub use anthropic::AnthropicClient;
pub use client::LlmClient;
pub use error::LlmError;
pub use openai::OpenAIClient;
#[allow(unused_imports)]
pub use types::Role;
pub use types::{
    CompletionRequest, CompletionResponse, ContentBlock, Message, MessageContent, StopReason, StreamChunk, TokenUsage,
    ToolCall, ToolDefinition,
};

use crate::config::LlmConfig;

/// Create an LLM client based on the provider specified in config
///
/// Supports "anthropic" and "openai" providers.
pub fn create_client(config: &LlmConfig) -> Result<Arc<dyn LlmClient>, LlmError> {
    debug!(provider = %config.provider, model = %config.model, "create_client: called");
    match config.provider.as_str() {
        "anthropic" => {
            debug!("create_client: creating Anthropic client");
            Ok(Arc::new(AnthropicClient::from_config(config)?))
        }
        "openai" => {
            debug!("create_client: creating OpenAI client");
            Ok(Arc::new(OpenAIClient::from_config(config)?))
        }
        other => {
            debug!(provider = %other, "create_client: unknown provider");
            Err(LlmError::InvalidResponse(format!(
                "Unknown LLM provider: '{}'. Supported: anthropic, openai",
                other
            )))
        }
    }
}

/// Generate a short title from markdown/text content
///
/// Returns a 3-5 word lowercase hyphenated title like "oauth-database-schema"
pub async fn name_markdown(llm: &Arc<dyn LlmClient>, text: &str) -> Option<String> {
    debug!(text_len = text.len(), "name_markdown: called");

    let system_prompt = "Generate a 3-5 word title for this content. \
                         Output ONLY the title, nothing else. \
                         Use lowercase words separated by hyphens. \
                         Example: oauth-database-schema";

    let request = CompletionRequest {
        system_prompt: system_prompt.to_string(),
        messages: vec![Message::user(text.to_string())],
        max_tokens: 50,
        tools: vec![],
    };

    match llm.complete(request).await {
        Ok(response) => {
            let title = response.content.map(|t| {
                t.trim()
                    .to_lowercase()
                    .replace(' ', "-")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-')
                    .collect::<String>()
            });
            debug!(?title, "name_markdown: generated");
            title
        }
        Err(e) => {
            debug!(error = %e, "name_markdown: LLM call failed");
            None
        }
    }
}
