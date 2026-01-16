//! Anthropic Claude API client implementation
//!
//! Implements the LlmClient trait for Anthropic's Messages API with
//! support for both blocking and streaming responses.

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use reqwest_eventsource::{Event, EventSource};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmClient, LlmError, Message, MessageContent, StopReason,
    StreamChunk, TokenUsage, ToolCall,
};
use crate::config::LlmConfig;

/// Anthropic Claude API client
pub struct AnthropicClient {
    model: String,
    api_key: String,
    base_url: String,
    http: Client,
    max_tokens: u32,
    #[allow(dead_code)]
    timeout: Duration,
}

impl AnthropicClient {
    /// Create a new client from configuration
    ///
    /// Reads the API key from the environment variable specified in config.
    pub fn from_config(config: &LlmConfig) -> Result<Self, LlmError> {
        let api_key = std::env::var(&config.api_key_env)
            .map_err(|_| LlmError::InvalidResponse(format!("Environment variable {} not set", config.api_key_env)))?;

        let timeout = Duration::from_millis(config.timeout_ms);

        let http = Client::builder().timeout(timeout).build().map_err(LlmError::Network)?;

        Ok(Self {
            model: config.model.clone(),
            api_key,
            base_url: config.base_url.clone(),
            http,
            max_tokens: config.max_tokens,
            timeout,
        })
    }

    /// Build the request body for the Anthropic API
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": request.max_tokens.min(self.max_tokens),
            "system": request.system_prompt,
            "messages": self.convert_messages(&request.messages),
        });

        if !request.tools.is_empty() {
            body["tools"] = serde_json::json!(
                request
                    .tools
                    .iter()
                    .map(|t| t.to_anthropic_schema())
                    .collect::<Vec<_>>()
            );
        }

        body
    }

    /// Convert internal Message types to Anthropic API format
    fn convert_messages(&self, messages: &[Message]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .map(|msg| {
                let content = match &msg.content {
                    MessageContent::Text(text) => serde_json::json!(text),
                    MessageContent::Blocks(blocks) => {
                        serde_json::json!(blocks.iter().map(|b| self.convert_content_block(b)).collect::<Vec<_>>())
                    }
                };

                serde_json::json!({
                    "role": msg.role,
                    "content": content,
                })
            })
            .collect()
    }

    /// Convert a ContentBlock to Anthropic API format
    fn convert_content_block(&self, block: &ContentBlock) -> serde_json::Value {
        match block {
            ContentBlock::Text { text } => {
                serde_json::json!({
                    "type": "text",
                    "text": text,
                })
            }
            ContentBlock::ToolUse { id, name, input } => {
                serde_json::json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                })
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": tool_use_id,
                    "content": content,
                    "is_error": is_error,
                })
            }
        }
    }

    /// Parse the Anthropic API response
    fn parse_response(&self, api_response: AnthropicResponse) -> CompletionResponse {
        let mut content = None;
        let mut tool_calls = Vec::new();

        for block in api_response.content {
            match block {
                AnthropicContentBlock::Text { text } => {
                    content = Some(text);
                }
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCall { id, name, input });
                }
            }
        }

        CompletionResponse {
            content,
            tool_calls,
            stop_reason: StopReason::from_anthropic(&api_response.stop_reason),
            usage: TokenUsage {
                input_tokens: api_response.usage.input_tokens,
                output_tokens: api_response.usage.output_tokens,
                cache_read_tokens: api_response.usage.cache_read_input_tokens.unwrap_or(0),
                cache_creation_tokens: api_response.usage.cache_creation_input_tokens.unwrap_or(0),
            },
        }
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let url = format!("{}/v1/messages", self.base_url);
        let body = self.build_request_body(&request);

        let response = self
            .http
            .post(url.clone())
            .header("x-api-key", self.api_key.clone())
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if response.status().as_u16() == 429 {
            // Rate limited - extract retry-after header
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);

            return Err(LlmError::RateLimited {
                retry_after: Duration::from_secs(retry_after),
            });
        }

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError { status, message: text });
        }

        let api_response: AnthropicResponse = response.json().await?;
        Ok(self.parse_response(api_response))
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        chunk_tx: mpsc::Sender<StreamChunk>,
    ) -> Result<CompletionResponse, LlmError> {
        let url = format!("{}/v1/messages", self.base_url);
        let mut body = self.build_request_body(&request);
        body["stream"] = serde_json::json!(true);

        let http_request = self
            .http
            .post(url)
            .header("x-api-key", self.api_key.clone())
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body);

        let mut es = EventSource::new(http_request).map_err(|e| LlmError::InvalidResponse(e.to_string()))?;

        let mut full_content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut current_tool: Option<(String, String, String)> = None; // (id, name, json_acc)
        let mut stop_reason = StopReason::EndTurn;
        let mut usage = TokenUsage::default();

        while let Some(event) = es.next().await {
            match event {
                Ok(Event::Message(msg)) => {
                    let data: serde_json::Value = serde_json::from_str(&msg.data).map_err(LlmError::Json)?;

                    match data["type"].as_str() {
                        Some("content_block_start") => {
                            if let Some(block) = data.get("content_block")
                                && block["type"] == "tool_use"
                            {
                                let id = block["id"].as_str().unwrap_or("").to_string();
                                let name = block["name"].as_str().unwrap_or("").to_string();
                                current_tool = Some((id.clone(), name.clone(), String::new()));
                                let _ = chunk_tx.send(StreamChunk::ToolUseStart { id, name }).await;
                            }
                        }
                        Some("content_block_delta") => {
                            if let Some(delta) = data.get("delta") {
                                if let Some(text) = delta["text"].as_str() {
                                    full_content.push_str(text);
                                    let _ = chunk_tx.send(StreamChunk::TextDelta(text.to_string())).await;
                                }
                                if let Some(json) = delta["partial_json"].as_str()
                                    && let Some((ref id, _, ref mut acc)) = current_tool
                                {
                                    acc.push_str(json);
                                    let _ = chunk_tx
                                        .send(StreamChunk::ToolUseDelta {
                                            id: id.clone(),
                                            json_delta: json.to_string(),
                                        })
                                        .await;
                                }
                            }
                        }
                        Some("content_block_stop") => {
                            if let Some((id, name, json)) = current_tool.take() {
                                let input: serde_json::Value =
                                    serde_json::from_str(&json).unwrap_or(serde_json::json!({}));
                                tool_calls.push(ToolCall {
                                    id: id.clone(),
                                    name,
                                    input,
                                });
                                let _ = chunk_tx.send(StreamChunk::ToolUseEnd { id }).await;
                            }
                        }
                        Some("message_delta") => {
                            if let Some(sr) = data["delta"]["stop_reason"].as_str() {
                                stop_reason = StopReason::from_anthropic(sr);
                            }
                            if let Some(u) = data.get("usage") {
                                usage.output_tokens = u["output_tokens"].as_u64().unwrap_or(0);
                            }
                        }
                        Some("message_start") => {
                            if let Some(u) = data["message"].get("usage") {
                                usage.input_tokens = u["input_tokens"].as_u64().unwrap_or(0);
                                usage.cache_read_tokens = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                                usage.cache_creation_tokens = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                            }
                        }
                        Some("message_stop") => {
                            break;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Open) => {}
                Err(e) => {
                    let _ = chunk_tx.send(StreamChunk::Error(e.to_string())).await;
                    return Err(LlmError::InvalidResponse(e.to_string()));
                }
            }
        }

        let _ = chunk_tx
            .send(StreamChunk::MessageDone {
                stop_reason: stop_reason.clone(),
                usage: usage.clone(),
            })
            .await;

        Ok(CompletionResponse {
            content: if full_content.is_empty() { None } else { Some(full_content) },
            tool_calls,
            stop_reason,
            usage,
        })
    }
}

// Anthropic API response types

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    stop_reason: String,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_body_basic() {
        // We can't easily test from_config without env vars, but we can test
        // the internal methods with a manually constructed client
        let client = AnthropicClient {
            model: "claude-sonnet-4".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            http: Client::new(),
            max_tokens: 8192,
            timeout: Duration::from_secs(300),
        };

        let request = CompletionRequest {
            system_prompt: "You are helpful".to_string(),
            messages: vec![Message::user("Hello")],
            tools: vec![],
            max_tokens: 1000,
        };

        let body = client.build_request_body(&request);

        assert_eq!(body["model"], "claude-sonnet-4");
        assert_eq!(body["max_tokens"], 1000);
        assert_eq!(body["system"], "You are helpful");
        assert!(body["messages"].is_array());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_build_request_body_with_tools() {
        use crate::llm::ToolDefinition;

        let client = AnthropicClient {
            model: "claude-sonnet-4".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            http: Client::new(),
            max_tokens: 8192,
            timeout: Duration::from_secs(300),
        };

        let request = CompletionRequest {
            system_prompt: "You are helpful".to_string(),
            messages: vec![Message::user("Read a file")],
            tools: vec![ToolDefinition::new(
                "read_file",
                "Read a file",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" }
                    }
                }),
            )],
            max_tokens: 1000,
        };

        let body = client.build_request_body(&request);

        assert!(body["tools"].is_array());
        assert_eq!(body["tools"][0]["name"], "read_file");
    }

    #[test]
    fn test_max_tokens_capped() {
        let client = AnthropicClient {
            model: "claude-sonnet-4".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://api.anthropic.com".to_string(),
            http: Client::new(),
            max_tokens: 1000, // Client configured with 1000 max
            timeout: Duration::from_secs(300),
        };

        let request = CompletionRequest {
            system_prompt: "Test".to_string(),
            messages: vec![],
            tools: vec![],
            max_tokens: 5000, // Request asks for 5000
        };

        let body = client.build_request_body(&request);

        // Should be capped to client max
        assert_eq!(body["max_tokens"], 1000);
    }
}
