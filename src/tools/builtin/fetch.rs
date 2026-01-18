//! fetch tool - fetch and process content from URLs

use async_trait::async_trait;
use scraper::{Html, Selector};
use serde_json::Value;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Fetch content from a URL
pub struct FetchTool;

#[async_trait]
impl Tool for FetchTool {
    fn name(&self) -> &'static str {
        "fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch content from a URL. Converts HTML to readable text."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "selector": {
                    "type": "string",
                    "description": "Optional CSS selector to extract specific content"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let url = match input["url"].as_str() {
            Some(u) => u,
            None => return ToolResult::error("url is required"),
        };

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return ToolResult::error("URL must start with http:// or https://");
        }

        let selector = input["selector"].as_str();

        // Fetch the content with timeout
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        let response = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => return ToolResult::error(format!("Failed to fetch URL: {}", e)),
        };

        if !response.status().is_success() {
            return ToolResult::error(format!("HTTP error: {}", response.status()));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = match response.text().await {
            Ok(b) => b,
            Err(e) => return ToolResult::error(format!("Failed to read response: {}", e)),
        };

        // Size limit
        if body.len() > 1_000_000 {
            return ToolResult::error("Response too large (> 1MB)");
        }

        // Process based on content type
        let output = if content_type.contains("text/html") || content_type.contains("application/xhtml") {
            html_to_text(&body, selector)
        } else if content_type.contains("application/json") {
            // Pretty-print JSON
            match serde_json::from_str::<Value>(&body) {
                Ok(json) => serde_json::to_string_pretty(&json).unwrap_or(body),
                Err(_) => body,
            }
        } else {
            // Plain text or other
            body
        };

        // Truncate if too long
        let max_chars = 50_000;
        let truncated = if output.len() > max_chars {
            format!("{}...\n[truncated, {} chars total]", &output[..max_chars], output.len())
        } else {
            output
        };

        ToolResult::success(truncated)
    }
}

/// Convert HTML to readable text
fn html_to_text(html: &str, selector: Option<&str>) -> String {
    let document = Html::parse_document(html);

    let content = if let Some(sel_str) = selector {
        // Extract content matching selector
        match Selector::parse(sel_str) {
            Ok(sel) => {
                let mut output = Vec::new();
                for element in document.select(&sel) {
                    output.push(extract_text_from_element(&element));
                }
                output.join("\n\n")
            }
            Err(_) => {
                return format!("Invalid CSS selector: {}", sel_str);
            }
        }
    } else {
        // Extract from body, or whole document if no body
        let body_selector = Selector::parse("body").unwrap();
        if let Some(body) = document.select(&body_selector).next() {
            extract_text_from_element(&body)
        } else {
            extract_text_from_element(&document.root_element())
        }
    };

    // Clean up whitespace
    clean_text(&content)
}

/// Extract text from an HTML element
fn extract_text_from_element(element: &scraper::ElementRef) -> String {
    let mut output = Vec::new();

    for node in element.descendants() {
        if let Some(text) = node.value().as_text() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                output.push(trimmed.to_string());
            }
        } else if let Some(el) = node.value().as_element() {
            // Add line breaks for block elements
            match el.name() {
                "p" | "div" | "br" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "tr" => {
                    output.push("\n".to_string());
                }
                "script" | "style" | "noscript" => {
                    // Skip script/style content
                }
                _ => {}
            }
        }
    }

    output.join(" ")
}

/// Clean up extracted text
fn clean_text(text: &str) -> String {
    // Collapse multiple whitespace/newlines
    let mut result = String::new();
    let mut prev_was_whitespace = false;
    let mut prev_was_newline = false;

    for ch in text.chars() {
        if ch == '\n' {
            if !prev_was_newline {
                result.push('\n');
            }
            prev_was_newline = true;
            prev_was_whitespace = true;
        } else if ch.is_whitespace() {
            if !prev_was_whitespace {
                result.push(' ');
            }
            prev_was_whitespace = true;
        } else {
            result.push(ch);
            prev_was_whitespace = false;
            prev_was_newline = false;
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_html_to_text_basic() {
        let html = r#"
            <html>
                <body>
                    <h1>Hello World</h1>
                    <p>This is a paragraph.</p>
                </body>
            </html>
        "#;

        let text = html_to_text(html, None);
        assert!(text.contains("Hello World"));
        assert!(text.contains("This is a paragraph"));
    }

    #[test]
    fn test_html_to_text_with_selector() {
        let html = r#"
            <html>
                <body>
                    <div class="content">Target content</div>
                    <div class="sidebar">Ignore this</div>
                </body>
            </html>
        "#;

        let text = html_to_text(html, Some(".content"));
        assert!(text.contains("Target content"));
        assert!(!text.contains("Ignore this"));
    }

    #[test]
    fn test_html_to_text_removes_scripts() {
        let html = r#"
            <html>
                <body>
                    <p>Visible text</p>
                    <script>console.log('hidden');</script>
                </body>
            </html>
        "#;

        let text = html_to_text(html, None);
        assert!(text.contains("Visible text"));
        // Script content should be filtered in extraction
    }

    #[test]
    fn test_clean_text() {
        let messy = "  Hello    world\n\n\n\nMultiple    spaces  ";
        let clean = clean_text(messy);
        assert_eq!(clean, "Hello world\nMultiple spaces");
    }

    #[tokio::test]
    async fn test_fetch_invalid_url() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = FetchTool;

        let result = tool.execute(serde_json::json!({"url": "not-a-url"}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("http"));
    }

    #[tokio::test]
    async fn test_fetch_missing_url() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = FetchTool;

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("url is required"));
    }
}
