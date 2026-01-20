//! fetch tool - fetch and process content from URLs

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tracing::debug;

use crate::llm::{CompletionRequest, LlmClient, Message};
use crate::tools::{Tool, ToolContext, ToolResult};

/// Fetch content from a URL, convert HTML to markdown, optionally summarize with LLM
pub struct FetchTool {
    /// Optional LLM client for post-processing with a prompt
    llm_client: Option<Arc<dyn LlmClient>>,
}

impl FetchTool {
    /// Create a FetchTool without LLM summarization
    pub fn new() -> Self {
        debug!("FetchTool::new: called");
        Self { llm_client: None }
    }

    /// Create a FetchTool with LLM summarization capability
    pub fn with_llm(client: Arc<dyn LlmClient>) -> Self {
        debug!("FetchTool::with_llm: called");
        Self {
            llm_client: Some(client),
        }
    }
}

impl Default for FetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for FetchTool {
    fn name(&self) -> &'static str {
        "fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch content from a URL. Converts HTML to markdown. Optionally summarize with a prompt."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional prompt to process/summarize the content (requires LLM)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        debug!(?input, "FetchTool::execute: called");
        let url = match input["url"].as_str() {
            Some(u) => {
                debug!(%u, "FetchTool::execute: url parameter found");
                u
            }
            None => {
                debug!("FetchTool::execute: missing url parameter");
                return ToolResult::error("url is required");
            }
        };

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            debug!("FetchTool::execute: invalid URL protocol");
            return ToolResult::error("URL must start with http:// or https://");
        }

        debug!("FetchTool::execute: URL protocol validated");

        let prompt = input["prompt"].as_str();
        debug!(has_prompt = %prompt.is_some(), "FetchTool::execute: prompt parameter");

        // Fetch the content with timeout
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("TaskDaemon/0.1 (fetch tool)")
            .build()
            .unwrap_or_default();

        debug!("FetchTool::execute: sending HTTP request");
        let response = match client.get(url).send().await {
            Ok(r) => {
                debug!(status = %r.status(), "FetchTool::execute: HTTP response received");
                r
            }
            Err(e) => {
                debug!(%e, "FetchTool::execute: HTTP request failed");
                return ToolResult::error(format!("Failed to fetch URL: {}", e));
            }
        };

        if !response.status().is_success() {
            debug!(status = %response.status(), "FetchTool::execute: HTTP error status");
            return ToolResult::error(format!("HTTP error: {}", response.status()));
        }

        debug!("FetchTool::execute: HTTP request successful");

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        debug!(%content_type, "FetchTool::execute: content type");

        let body = match response.text().await {
            Ok(b) => {
                debug!(body_len = %b.len(), "FetchTool::execute: response body read");
                b
            }
            Err(e) => {
                debug!(%e, "FetchTool::execute: failed to read response body");
                return ToolResult::error(format!("Failed to read response: {}", e));
            }
        };

        // Size limit
        if body.len() > 1_000_000 {
            debug!("FetchTool::execute: response too large");
            return ToolResult::error("Response too large (> 1MB)");
        }

        // Process based on content type
        let content = if content_type.contains("text/html") || content_type.contains("application/xhtml") {
            debug!("FetchTool::execute: converting HTML to markdown");
            // Convert HTML to Markdown using fast_html2md
            html2md::rewrite_html(&body, false)
        } else if content_type.contains("application/json") {
            debug!("FetchTool::execute: pretty-printing JSON");
            // Pretty-print JSON
            match serde_json::from_str::<Value>(&body) {
                Ok(json) => serde_json::to_string_pretty(&json).unwrap_or(body),
                Err(_) => body,
            }
        } else {
            debug!("FetchTool::execute: returning plain text");
            // Plain text or other
            body
        };

        // If prompt provided and we have an LLM client, summarize
        if let (Some(prompt_text), Some(llm)) = (prompt, &self.llm_client) {
            debug!("FetchTool::execute: summarizing with LLM");
            return self
                .summarize_with_llm(llm, &content, prompt_text, url, _ctx.max_tokens)
                .await;
        }

        // Truncate if too long (no LLM summarization)
        let max_chars = 50_000;
        let output = if content.len() > max_chars {
            debug!("FetchTool::execute: truncating long content");
            format!(
                "{}...\n\n[truncated, {} chars total]",
                &content[..max_chars],
                content.len()
            )
        } else {
            debug!("FetchTool::execute: content within size limit");
            content
        };

        ToolResult::success(output)
    }
}

impl FetchTool {
    /// Summarize content using LLM
    async fn summarize_with_llm(
        &self,
        llm: &Arc<dyn LlmClient>,
        content: &str,
        prompt: &str,
        url: &str,
        max_tokens: u32,
    ) -> ToolResult {
        debug!(%url, content_len = %content.len(), max_tokens, "FetchTool::summarize_with_llm: called");
        // Truncate content for LLM context (leave room for prompt and response)
        let max_content = 100_000; // ~25k tokens roughly
        let truncated_content = if content.len() > max_content {
            debug!("FetchTool::summarize_with_llm: truncating content for LLM");
            format!(
                "{}...\n\n[Content truncated from {} chars]",
                &content[..max_content],
                content.len()
            )
        } else {
            debug!("FetchTool::summarize_with_llm: content within LLM limit");
            content.to_string()
        };

        let system_prompt = "You are a helpful assistant that processes web content. \
             The user has fetched content from a URL and wants you to process it according to their prompt. \
             Be concise and focused on what the user asked for."
            .to_string();

        let user_message = format!(
            "I fetched content from: {}\n\n\
             --- BEGIN CONTENT ---\n\
             {}\n\
             --- END CONTENT ---\n\n\
             {}",
            url, truncated_content, prompt
        );

        let request = CompletionRequest {
            system_prompt,
            messages: vec![Message::user(user_message)],
            tools: vec![], // No tools needed for summarization
            max_tokens,
        };

        debug!("FetchTool::summarize_with_llm: sending LLM request");
        match llm.complete(request).await {
            Ok(response) => {
                debug!("FetchTool::summarize_with_llm: LLM request successful");
                let summary = response.content.unwrap_or_else(|| "(no response)".to_string());
                ToolResult::success(summary)
            }
            Err(e) => {
                debug!(%e, "FetchTool::summarize_with_llm: LLM request failed");
                ToolResult::error(format!("LLM summarization failed: {}", e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_html_to_markdown() {
        let html = r#"
            <html>
                <body>
                    <h1>Hello World</h1>
                    <p>This is a paragraph.</p>
                    <ul>
                        <li>Item 1</li>
                        <li>Item 2</li>
                    </ul>
                </body>
            </html>
        "#;

        let md = html2md::rewrite_html(html, false);
        assert!(md.contains("Hello World"));
        assert!(md.contains("This is a paragraph"));
    }

    #[test]
    fn test_html_to_markdown_links() {
        let html = r#"<a href="https://example.com">Example Link</a>"#;
        let md = html2md::rewrite_html(html, false);
        assert!(md.contains("[Example Link]"));
        assert!(md.contains("https://example.com"));
    }

    #[test]
    fn test_html_to_markdown_code() {
        let html = r#"<pre><code>fn main() {}</code></pre>"#;
        let md = html2md::rewrite_html(html, false);
        assert!(md.contains("fn main()"));
    }

    #[tokio::test]
    async fn test_fetch_invalid_url() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = FetchTool::new();

        let result = tool.execute(serde_json::json!({"url": "not-a-url"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("http"));
    }

    #[tokio::test]
    async fn test_fetch_missing_url() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = FetchTool::new();

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("url is required"));
    }

    #[tokio::test]
    async fn test_fetch_prompt_without_llm() {
        // When prompt is provided but no LLM client, should just return content
        let tool = FetchTool::new(); // No LLM

        // This would need a real URL to test fully, but we can verify the tool creates ok
        assert_eq!(tool.name(), "fetch");
        assert!(tool.llm_client.is_none());
    }
}
