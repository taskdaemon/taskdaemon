//! OpenAI API client implementation
//!
//! Implements the LlmClient trait for OpenAI's Chat Completions API with
//! support for both blocking and streaming responses.

use async_trait::async_trait;
use futures::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use super::{
    CompletionRequest, CompletionResponse, ContentBlock, LlmClient, LlmError, Message, MessageContent, StopReason,
    StreamChunk, TokenUsage, ToolCall,
};
use crate::config::ResolvedLlmConfig;

/// Maximum number of retries for transient errors
const MAX_RETRIES: u32 = 3;

/// Initial backoff delay for retries
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Check if an HTTP status code is retryable
fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504)
}

/// OpenAI API client
pub struct OpenAIClient {
    model: String,
    api_key: String,
    base_url: String,
    http: Client,
    max_tokens: u32,
    #[allow(dead_code)]
    timeout: Duration,
}

impl OpenAIClient {
    /// Create a new client from resolved configuration
    ///
    /// Takes a ResolvedLlmConfig which contains all necessary fields.
    pub fn from_config(config: &ResolvedLlmConfig) -> Result<Self, LlmError> {
        debug!(?config, "from_config: called");
        let api_key = config
            .get_api_key()
            .map_err(|e| LlmError::InvalidResponse(e.to_string()))?;

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

    /// Build the request body for the OpenAI API
    fn build_request_body(&self, request: &CompletionRequest) -> serde_json::Value {
        debug!(%self.model, %request.max_tokens, "build_request_body: called");

        let mut messages = vec![serde_json::json!({
            "role": "system",
            "content": request.system_prompt,
        })];

        messages.extend(self.convert_messages(&request.messages));

        let max_tokens = request.max_tokens.min(self.max_tokens);

        // GPT-5.x and o1/o3 models use max_completion_tokens instead of max_tokens
        let uses_completion_tokens =
            self.model.starts_with("gpt-5") || self.model.starts_with("o1") || self.model.starts_with("o3");

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
        });

        if uses_completion_tokens {
            body["max_completion_tokens"] = serde_json::json!(max_tokens);
        } else {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }

        if !request.tools.is_empty() {
            debug!("build_request_body: tools not empty, adding tools");
            body["tools"] = serde_json::json!(request.tools.iter().map(|t| t.to_openai_schema()).collect::<Vec<_>>());
            body["tool_choice"] = serde_json::json!("auto");
        } else {
            debug!("build_request_body: no tools");
        }

        body
    }

    /// Convert internal Message types to OpenAI API format
    ///
    /// OpenAI requires one message per tool result, so a single internal message
    /// with multiple tool results becomes multiple OpenAI messages.
    fn convert_messages(&self, messages: &[Message]) -> Vec<serde_json::Value> {
        debug!(message_count = %messages.len(), "convert_messages: called");
        let mut result = Vec::new();

        for msg in messages {
            let role = match msg.role {
                super::types::Role::User => "user",
                super::types::Role::Assistant => "assistant",
            };

            match &msg.content {
                MessageContent::Text(text) => {
                    debug!("convert_messages: text content");
                    result.push(serde_json::json!({
                        "role": role,
                        "content": text,
                    }));
                }
                MessageContent::Blocks(blocks) => {
                    debug!(block_count = %blocks.len(), "convert_messages: blocks content");
                    // For blocks, we need to handle tool calls and tool results specially
                    let mut tool_calls = Vec::new();
                    let mut tool_results = Vec::new();
                    let mut text_content = String::new();

                    for block in blocks {
                        match block {
                            ContentBlock::Text { text } => {
                                text_content.push_str(text);
                            }
                            ContentBlock::ToolUse { id, name, input } => {
                                tool_calls.push(serde_json::json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": input.to_string(),
                                    }
                                }));
                            }
                            ContentBlock::ToolResult {
                                tool_use_id, content, ..
                            } => {
                                tool_results.push((tool_use_id.clone(), content.clone()));
                            }
                        }
                    }

                    // OpenAI requires one message per tool result
                    if !tool_results.is_empty() {
                        for (tool_call_id, content) in tool_results {
                            result.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_call_id,
                                "content": content,
                            }));
                        }
                        continue;
                    }

                    if !tool_calls.is_empty() {
                        // Assistant message with tool calls
                        let mut msg = serde_json::json!({
                            "role": "assistant",
                            "tool_calls": tool_calls,
                        });
                        if !text_content.is_empty() {
                            msg["content"] = serde_json::json!(text_content);
                        }
                        result.push(msg);
                        continue;
                    }

                    // Plain text message
                    result.push(serde_json::json!({
                        "role": role,
                        "content": text_content,
                    }));
                }
            }
        }

        result
    }

    /// Parse the OpenAI API response
    fn parse_response(&self, api_response: OpenAIResponse) -> CompletionResponse {
        debug!(?api_response.choices, "parse_response: called");
        let choice = api_response.choices.into_iter().next();

        let (content, tool_calls, stop_reason) = match choice {
            Some(c) => {
                let content = c.message.content;
                let tool_calls = c
                    .message
                    .tool_calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|tc| ToolCall {
                        id: tc.id,
                        name: tc.function.name,
                        input: serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::json!({})),
                    })
                    .collect();
                let stop_reason = match c.finish_reason.as_deref() {
                    Some("stop") => StopReason::EndTurn,
                    Some("tool_calls") => StopReason::ToolUse,
                    Some("length") => StopReason::MaxTokens,
                    _ => StopReason::EndTurn,
                };
                (content, tool_calls, stop_reason)
            }
            None => (None, vec![], StopReason::EndTurn),
        };

        CompletionResponse {
            content,
            tool_calls,
            stop_reason,
            usage: TokenUsage {
                input_tokens: api_response.usage.prompt_tokens,
                output_tokens: api_response.usage.completion_tokens,
                cache_read_tokens: 0,
                cache_creation_tokens: 0,
            },
        }
    }
}

#[async_trait]
impl LlmClient for OpenAIClient {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        debug!(%self.model, %request.max_tokens, "complete: called");
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = self.build_request_body(&request);

        let mut last_error = None;
        for attempt in 0..=MAX_RETRIES {
            if attempt > 0 {
                let backoff = INITIAL_BACKOFF_MS * 2u64.pow(attempt - 1);
                warn!(
                    attempt,
                    backoff_ms = backoff,
                    "complete: retrying after transient error"
                );
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }

            let response = match self
                .http
                .post(url.clone())
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    debug!(attempt, error = %e, "complete: network error");
                    last_error = Some(LlmError::Network(e));
                    continue;
                }
            };

            let status = response.status().as_u16();

            if status == 429 {
                debug!("complete: rate limited (429)");
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

            if is_retryable_status(status) && attempt < MAX_RETRIES {
                let text = response.text().await.unwrap_or_default();
                debug!(attempt, status, "complete: retryable error");
                last_error = Some(LlmError::ApiError { status, message: text });
                continue;
            }

            if !response.status().is_success() {
                debug!(%status, "complete: API error");
                let text = response.text().await.unwrap_or_default();
                return Err(LlmError::ApiError { status, message: text });
            }

            debug!("complete: success");
            let api_response: OpenAIResponse = response.json().await?;
            return Ok(self.parse_response(api_response));
        }

        Err(last_error.unwrap_or_else(|| LlmError::InvalidResponse("Max retries exceeded".to_string())))
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        chunk_tx: mpsc::Sender<StreamChunk>,
    ) -> Result<CompletionResponse, LlmError> {
        debug!(%self.model, %request.max_tokens, "stream: called");
        let url = format!("{}/v1/chat/completions", self.base_url);
        let mut body = self.build_request_body(&request);
        body["stream"] = serde_json::json!(true);

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(LlmError::Network)?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError { status, message: text });
        }

        let mut stream = response.bytes_stream();
        let mut full_content = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        let mut current_tool_calls: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new(); // index -> (id, name, args)
        let mut stop_reason = StopReason::EndTurn;
        let mut usage = TokenUsage::default();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(LlmError::Network)?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            // Process complete SSE lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line == "data: [DONE]" {
                    continue;
                }

                if let Some(data) = line.strip_prefix("data: ")
                    && let Ok(chunk_data) = serde_json::from_str::<OpenAIStreamChunk>(data)
                {
                    if let Some(choice) = chunk_data.choices.first() {
                        // Handle content delta
                        if let Some(content) = &choice.delta.content {
                            full_content.push_str(content);
                            let _ = chunk_tx.send(StreamChunk::TextDelta(content.clone())).await;
                        }

                        // Handle tool calls
                        if let Some(tcs) = &choice.delta.tool_calls {
                            for tc in tcs {
                                let entry = current_tool_calls
                                    .entry(tc.index)
                                    .or_insert_with(|| (String::new(), String::new(), String::new()));

                                if let Some(id) = &tc.id {
                                    entry.0 = id.clone();
                                }
                                if let Some(func) = &tc.function {
                                    if let Some(name) = &func.name {
                                        entry.1 = name.clone();
                                        let _ = chunk_tx
                                            .send(StreamChunk::ToolUseStart {
                                                id: entry.0.clone(),
                                                name: name.clone(),
                                            })
                                            .await;
                                    }
                                    if let Some(args) = &func.arguments {
                                        entry.2.push_str(args);
                                        let _ = chunk_tx
                                            .send(StreamChunk::ToolUseDelta {
                                                id: entry.0.clone(),
                                                json_delta: args.clone(),
                                            })
                                            .await;
                                    }
                                }
                            }
                        }

                        // Handle finish reason
                        if let Some(reason) = &choice.finish_reason {
                            stop_reason = match reason.as_str() {
                                "stop" => StopReason::EndTurn,
                                "tool_calls" => StopReason::ToolUse,
                                "length" => StopReason::MaxTokens,
                                _ => StopReason::EndTurn,
                            };
                        }
                    }

                    // Handle usage (OpenAI sends this in the final chunk with stream_options)
                    if let Some(u) = chunk_data.usage {
                        usage.input_tokens = u.prompt_tokens;
                        usage.output_tokens = u.completion_tokens;
                    }
                }
            }
        }

        // Finalize tool calls
        for (_, (id, name, args)) in current_tool_calls {
            let input = serde_json::from_str(&args).unwrap_or(serde_json::json!({}));
            tool_calls.push(ToolCall {
                id: id.clone(),
                name,
                input,
            });
            let _ = chunk_tx.send(StreamChunk::ToolUseEnd { id }).await;
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

// OpenAI API response types

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    choices: Vec<OpenAIChoice>,
    usage: OpenAIUsage,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIToolCall {
    id: String,
    function: OpenAIFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAIFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

// Streaming types

#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    choices: Vec<OpenAIStreamChoice>,
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: OpenAIStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAIStreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamToolCall {
    index: usize,
    id: Option<String>,
    function: Option<OpenAIStreamFunction>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_body_basic() {
        let client = OpenAIClient {
            model: "gpt-4o".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://api.openai.com".to_string(),
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

        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["max_tokens"], 1000);
        assert!(body["messages"].is_array());
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "You are helpful");
        assert_eq!(body["messages"][1]["role"], "user");
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_max_tokens_capped() {
        let client = OpenAIClient {
            model: "gpt-4o".to_string(),
            api_key: "test-key".to_string(),
            base_url: "https://api.openai.com".to_string(),
            http: Client::new(),
            max_tokens: 1000,
            timeout: Duration::from_secs(300),
        };

        let request = CompletionRequest {
            system_prompt: "Test".to_string(),
            messages: vec![],
            tools: vec![],
            max_tokens: 5000,
        };

        let body = client.build_request_body(&request);
        assert_eq!(body["max_tokens"], 1000);
    }
}
