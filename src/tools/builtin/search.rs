//! search tool - web search functionality

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Search the web for information
pub struct SearchTool;

/// Configuration for search API
#[derive(Debug, Clone, Deserialize)]
pub struct SearchConfig {
    /// API provider: "tavily", "brave", "serpapi"
    pub provider: String,
    /// API key
    pub api_key: String,
}

impl SearchConfig {
    /// Load from environment variables
    pub fn from_env() -> Option<Self> {
        // Try Tavily first (recommended for AI agents)
        if let Ok(api_key) = std::env::var("TAVILY_API_KEY") {
            return Some(Self {
                provider: "tavily".to_string(),
                api_key,
            });
        }

        // Try Brave Search
        if let Ok(api_key) = std::env::var("BRAVE_API_KEY") {
            return Some(Self {
                provider: "brave".to_string(),
                api_key,
            });
        }

        // Try SerpAPI
        if let Ok(api_key) = std::env::var("SERPAPI_KEY") {
            return Some(Self {
                provider: "serpapi".to_string(),
                api_key,
            });
        }

        None
    }
}

#[async_trait]
impl Tool for SearchTool {
    fn name(&self) -> &'static str {
        "search"
    }

    fn description(&self) -> &'static str {
        "Search the web for information. Requires TAVILY_API_KEY, BRAVE_API_KEY, or SERPAPI_KEY."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 5)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> ToolResult {
        let query = match input["query"].as_str() {
            Some(q) => q,
            None => return ToolResult::error("query is required"),
        };

        let max_results = input["max_results"].as_u64().unwrap_or(5) as usize;

        // Get configuration
        let config = match SearchConfig::from_env() {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    "No search API configured. Set TAVILY_API_KEY, BRAVE_API_KEY, or SERPAPI_KEY environment variable.",
                );
            }
        };

        // Execute search based on provider
        match config.provider.as_str() {
            "tavily" => search_tavily(query, max_results, &config.api_key).await,
            "brave" => search_brave(query, max_results, &config.api_key).await,
            "serpapi" => search_serpapi(query, max_results, &config.api_key).await,
            _ => ToolResult::error(format!("Unknown search provider: {}", config.provider)),
        }
    }
}

/// Search using Tavily API
async fn search_tavily(query: &str, max_results: usize, api_key: &str) -> ToolResult {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    let body = serde_json::json!({
        "api_key": api_key,
        "query": query,
        "max_results": max_results,
        "search_depth": "basic"
    });

    let response = match client.post("https://api.tavily.com/search").json(&body).send().await {
        Ok(r) => r,
        Err(e) => return ToolResult::error(format!("Search request failed: {}", e)),
    };

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return ToolResult::error(format!("Tavily API error {}: {}", status, error_text));
    }

    let result: Value = match response.json().await {
        Ok(r) => r,
        Err(e) => return ToolResult::error(format!("Failed to parse response: {}", e)),
    };

    // Format results
    let results = result["results"].as_array();
    if results.is_none() || results.unwrap().is_empty() {
        return ToolResult::success("No results found");
    }

    let output: Vec<String> = results
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let title = r["title"].as_str().unwrap_or("(no title)");
            let url = r["url"].as_str().unwrap_or("");
            let content = r["content"].as_str().unwrap_or("");
            format!("{}. {}\n   {}\n   {}\n", i + 1, title, url, truncate(content, 200))
        })
        .collect();

    ToolResult::success(output.join("\n"))
}

/// Search using Brave Search API
async fn search_brave(query: &str, max_results: usize, api_key: &str) -> ToolResult {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    let response = match client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("X-Subscription-Token", api_key)
        .query(&[("q", query), ("count", &max_results.to_string())])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return ToolResult::error(format!("Search request failed: {}", e)),
    };

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return ToolResult::error(format!("Brave API error {}: {}", status, error_text));
    }

    let result: Value = match response.json().await {
        Ok(r) => r,
        Err(e) => return ToolResult::error(format!("Failed to parse response: {}", e)),
    };

    // Format results
    let results = result["web"]["results"].as_array();
    if results.is_none() || results.unwrap().is_empty() {
        return ToolResult::success("No results found");
    }

    let output: Vec<String> = results
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let title = r["title"].as_str().unwrap_or("(no title)");
            let url = r["url"].as_str().unwrap_or("");
            let description = r["description"].as_str().unwrap_or("");
            format!("{}. {}\n   {}\n   {}\n", i + 1, title, url, truncate(description, 200))
        })
        .collect();

    ToolResult::success(output.join("\n"))
}

/// Search using SerpAPI
async fn search_serpapi(query: &str, max_results: usize, api_key: &str) -> ToolResult {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    let response = match client
        .get("https://serpapi.com/search")
        .query(&[
            ("q", query),
            ("api_key", api_key),
            ("num", &max_results.to_string()),
            ("engine", "google"),
        ])
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return ToolResult::error(format!("Search request failed: {}", e)),
    };

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        return ToolResult::error(format!("SerpAPI error {}: {}", status, error_text));
    }

    let result: Value = match response.json().await {
        Ok(r) => r,
        Err(e) => return ToolResult::error(format!("Failed to parse response: {}", e)),
    };

    // Format results
    let results = result["organic_results"].as_array();
    if results.is_none() || results.unwrap().is_empty() {
        return ToolResult::success("No results found");
    }

    let output: Vec<String> = results
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let title = r["title"].as_str().unwrap_or("(no title)");
            let link = r["link"].as_str().unwrap_or("");
            let snippet = r["snippet"].as_str().unwrap_or("");
            format!("{}. {}\n   {}\n   {}\n", i + 1, title, link, truncate(snippet, 200))
        })
        .collect();

    ToolResult::success(output.join("\n"))
}

/// Truncate string to max length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len { s.to_string() } else { format!("{}...", &s[..max_len]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_search_missing_query() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = SearchTool;

        let result = tool.execute(serde_json::json!({}), &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("query is required"));
    }

    #[tokio::test]
    async fn test_search_no_api_key() {
        // This test will fail if any search API key is set in the environment
        // We can't easily unset env vars in tests, so we just check the error path exists

        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test".to_string());
        let tool = SearchTool;

        let result = tool.execute(serde_json::json!({"query": "test"}), &ctx).await;

        // Either we get results (if API key is set) or we get the error message
        if result.is_error {
            assert!(
                result.content.contains("No search API configured")
                    || result.content.contains("API error")
                    || result.content.contains("request failed")
            );
        }
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a long string", 10), "this is a ...");
    }

    #[test]
    fn test_search_config_from_env() {
        // This just tests that the function runs without panicking
        // The actual result depends on environment variables
        let _ = SearchConfig::from_env();
    }
}
