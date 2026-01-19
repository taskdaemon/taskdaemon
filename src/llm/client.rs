//! LlmClient trait definition

use async_trait::async_trait;
use tokio::sync::mpsc;
#[allow(unused_imports)]
use tracing::debug;

use super::{CompletionRequest, CompletionResponse, LlmError, StreamChunk};

/// Stateless LLM client - each call is independent (fresh context)
///
/// This is the core abstraction for interacting with language models.
/// Each completion request is independent - no conversation state is
/// maintained between calls. This is intentional: the Ralph Wiggum
/// pattern requires fresh context windows to prevent context rot.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a single completion request (blocking until complete)
    ///
    /// This is the primary method for Ralph loop iterations.
    /// Each call starts a new conversation with fresh context.
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError>;

    /// Streaming completion for TUI progress display
    ///
    /// Sends chunks to the provided channel as they arrive.
    /// Returns the final complete response.
    async fn stream(
        &self,
        request: CompletionRequest,
        chunk_tx: mpsc::Sender<StreamChunk>,
    ) -> Result<CompletionResponse, LlmError>;
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing::debug;

    /// Mock LLM client for unit tests
    pub struct MockLlmClient {
        responses: Vec<CompletionResponse>,
        call_count: AtomicUsize,
    }

    impl MockLlmClient {
        pub fn new(responses: Vec<CompletionResponse>) -> Self {
            debug!(response_count = %responses.len(), "MockLlmClient::new: called");
            Self {
                responses,
                call_count: AtomicUsize::new(0),
            }
        }

        pub fn call_count(&self) -> usize {
            debug!("MockLlmClient::call_count: called");
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LlmClient for MockLlmClient {
        async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
            debug!("MockLlmClient::complete: called");
            let idx = self.call_count.fetch_add(1, Ordering::SeqCst);
            debug!(%idx, "MockLlmClient::complete: fetching response");
            self.responses.get(idx).cloned().ok_or_else(|| {
                debug!("MockLlmClient::complete: no more mock responses");
                LlmError::InvalidResponse("No more mock responses".to_string())
            })
        }

        async fn stream(
            &self,
            request: CompletionRequest,
            _chunk_tx: mpsc::Sender<StreamChunk>,
        ) -> Result<CompletionResponse, LlmError> {
            debug!("MockLlmClient::stream: called");
            // For mock, just return complete response without streaming
            self.complete(request).await
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::llm::{StopReason, TokenUsage};

        #[tokio::test]
        async fn test_mock_client_returns_responses() {
            let responses = vec![
                CompletionResponse {
                    content: Some("Response 1".to_string()),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                },
                CompletionResponse {
                    content: Some("Response 2".to_string()),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                    usage: TokenUsage::default(),
                },
            ];

            let client = MockLlmClient::new(responses);

            let req = CompletionRequest {
                system_prompt: "Test".to_string(),
                messages: vec![],
                tools: vec![],
                max_tokens: 1000,
            };

            let resp1 = client.complete(req.clone()).await.unwrap();
            assert_eq!(resp1.content, Some("Response 1".to_string()));

            let resp2 = client.complete(req.clone()).await.unwrap();
            assert_eq!(resp2.content, Some("Response 2".to_string()));

            assert_eq!(client.call_count(), 2);
        }

        #[tokio::test]
        async fn test_mock_client_errors_when_exhausted() {
            let client = MockLlmClient::new(vec![]);

            let req = CompletionRequest {
                system_prompt: "Test".to_string(),
                messages: vec![],
                tools: vec![],
                max_tokens: 1000,
            };

            let result = client.complete(req).await;
            assert!(result.is_err());
        }
    }
}
