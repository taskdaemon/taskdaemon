//! Grep tool - search files using ripgrep library

use std::path::Path;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use serde_json::{Value, json};
use tracing::debug;
use walkdir::WalkDir;

use crate::tools::{Tool, ToolContext, ToolResult};

/// Grep tool - search for patterns in files using ripgrep library
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str {
        "grep"
    }

    fn description(&self) -> &'static str {
        "Search for patterns in files using ripgrep. Returns matching lines with context."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Path to search in (relative to worktree, default: '.')",
                    "default": "."
                },
                "file_pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g., '*.rs', '*.py')"
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines before and after match (default: 2)",
                    "default": 2
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case-insensitive search (default: false)",
                    "default": false
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matching lines to return (default: 50)",
                    "default": 50
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        debug!(?input, "GrepTool::execute: called");
        // Extract parameters
        let pattern = match input.get("pattern").and_then(|v| v.as_str()) {
            Some(p) => {
                debug!(%p, "GrepTool::execute: pattern parameter found");
                p
            }
            None => {
                debug!("GrepTool::execute: missing pattern parameter");
                return ToolResult::error("Missing required parameter: pattern");
            }
        };

        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let file_pattern = input.get("file_pattern").and_then(|v| v.as_str());
        let context_lines = input.get("context_lines").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
        let case_insensitive = input.get("case_insensitive").and_then(|v| v.as_bool()).unwrap_or(false);
        let max_results = input.get("max_results").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        debug!(%path, ?file_pattern, %context_lines, %case_insensitive, %max_results, "GrepTool::execute: parameters parsed");

        // Validate path is within worktree
        let search_path = match ctx.validate_path(Path::new(path)) {
            Ok(p) => {
                debug!(?p, "GrepTool::execute: search path validated");
                p
            }
            Err(e) => {
                debug!(%e, "GrepTool::execute: path validation failed");
                return ToolResult::error(format!("Invalid path: {}", e));
            }
        };

        // Build the regex matcher
        let matcher = match RegexMatcherBuilder::new()
            .case_insensitive(case_insensitive)
            .build(pattern)
        {
            Ok(m) => {
                debug!("GrepTool::execute: regex matcher built");
                m
            }
            Err(e) => {
                debug!(%e, "GrepTool::execute: invalid regex pattern");
                return ToolResult::error(format!("Invalid regex pattern: {}", e));
            }
        };

        // Build glob pattern matcher if specified
        let glob_matcher = file_pattern.and_then(|fp| glob::Pattern::new(fp).ok());
        debug!(has_glob_matcher = %glob_matcher.is_some(), "GrepTool::execute: glob matcher");

        // Collect results
        let results: Arc<Mutex<Vec<MatchResult>>> = Arc::new(Mutex::new(Vec::new()));
        let match_count = Arc::new(Mutex::new(0usize));

        // Build searcher with context
        let mut searcher_builder = SearcherBuilder::new();
        searcher_builder
            .binary_detection(BinaryDetection::quit(b'\x00'))
            .before_context(context_lines)
            .after_context(context_lines);

        // Walk the directory and search files
        let walker = if search_path.is_file() {
            debug!("GrepTool::execute: searching single file");
            // Single file search
            let files = vec![search_path.clone()];
            files.into_iter().collect::<Vec<_>>()
        } else {
            debug!("GrepTool::execute: searching directory");
            // Directory search
            WalkDir::new(&search_path)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .filter(|e| {
                    // Apply glob filter if specified
                    if let Some(ref glob) = glob_matcher {
                        if let Some(name) = e.path().file_name().and_then(|n| n.to_str()) {
                            return glob.matches(name);
                        }
                        return false;
                    }
                    true
                })
                .map(|e| e.path().to_path_buf())
                .collect::<Vec<_>>()
        };

        debug!(file_count = %walker.len(), "GrepTool::execute: files to search");

        for file_path in walker {
            // Check if we've hit max results
            {
                let count = match_count.lock().unwrap();
                if *count >= max_results {
                    debug!("GrepTool::execute: max results reached");
                    break;
                }
            }

            let mut searcher = searcher_builder.build();
            let file_results = Arc::clone(&results);
            let file_match_count = Arc::clone(&match_count);
            let max = max_results;

            // Get relative path for display
            let display_path = file_path
                .strip_prefix(&ctx.worktree)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();

            let search_result = searcher.search_path(
                &matcher,
                &file_path,
                UTF8(|line_num, line| {
                    let mut count = file_match_count.lock().unwrap();
                    if *count >= max {
                        return Ok(false); // Stop searching
                    }

                    // Check if this line actually matches (not just context)
                    let is_match = matcher.is_match(line.as_bytes()).unwrap_or(false);

                    let mut results = file_results.lock().unwrap();
                    results.push(MatchResult {
                        file: display_path.clone(),
                        line_num,
                        line: line.trim_end().to_string(),
                        is_context: !is_match,
                    });

                    if is_match {
                        *count += 1;
                    }

                    Ok(true)
                }),
            );

            if let Err(e) = search_result {
                // Skip files that can't be searched (binary, permissions, etc.)
                debug!(?file_path, %e, "GrepTool::execute: skipping file");
            }
        }

        // Format results
        let results = results.lock().unwrap();
        debug!(results_count = %results.len(), "GrepTool::execute: search complete");

        if results.is_empty() {
            debug!("GrepTool::execute: no matches found");
            return ToolResult::success("No matches found.");
        }

        debug!("GrepTool::execute: formatting results");
        let output = format_results(&results, max_results);
        ToolResult::success(output)
    }
}

#[derive(Debug)]
struct MatchResult {
    file: String,
    line_num: u64,
    line: String,
    is_context: bool,
}

fn format_results(results: &[MatchResult], max_results: usize) -> String {
    debug!(results_count = %results.len(), %max_results, "format_results: called");
    let mut output = String::new();
    let mut current_file = String::new();
    let mut match_count = 0;

    for result in results {
        // Add file header when file changes
        if result.file != current_file {
            if !current_file.is_empty() {
                output.push('\n');
            }
            current_file = result.file.clone();
        }

        // Format line: file:line_num:content or file-line_num-content for context
        let separator = if result.is_context { "-" } else { ":" };
        output.push_str(&format!(
            "{}{}{}{}{}",
            result.file, separator, result.line_num, separator, result.line
        ));
        output.push('\n');

        if !result.is_context {
            match_count += 1;
        }
    }

    if match_count >= max_results {
        debug!("format_results: output truncated at max results");
        output.push_str(&format!("\n... (truncated at {} matches)", max_results));
    }

    output.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use tokio::fs;

    #[tokio::test]
    async fn test_grep_basic() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        // Create test file
        let test_file = temp.path().join("test.txt");
        fs::write(&test_file, "hello world\nfoo bar\nhello again")
            .await
            .unwrap();

        let input = json!({
            "pattern": "hello",
            "path": "."
        });

        let tool = GrepTool;
        let result = tool.execute(input, &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("hello"));
    }

    #[tokio::test]
    async fn test_grep_case_insensitive() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let test_file = temp.path().join("test.txt");
        fs::write(&test_file, "Hello World\nHELLO AGAIN").await.unwrap();

        let input = json!({
            "pattern": "hello",
            "case_insensitive": true
        });

        let tool = GrepTool;
        let result = tool.execute(input, &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("Hello"));
        assert!(result.content.contains("HELLO"));
    }

    #[tokio::test]
    async fn test_grep_no_matches() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let test_file = temp.path().join("test.txt");
        fs::write(&test_file, "foo bar baz").await.unwrap();

        let input = json!({
            "pattern": "notfound"
        });

        let tool = GrepTool;
        let result = tool.execute(input, &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("No matches found"));
    }

    #[tokio::test]
    async fn test_grep_file_pattern() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        // Create test files
        fs::write(temp.path().join("test.rs"), "fn main() { hello }")
            .await
            .unwrap();
        fs::write(temp.path().join("test.txt"), "hello world").await.unwrap();

        let input = json!({
            "pattern": "hello",
            "file_pattern": "*.rs"
        });

        let tool = GrepTool;
        let result = tool.execute(input, &ctx).await;

        assert!(!result.is_error);
        assert!(result.content.contains("test.rs"));
        assert!(!result.content.contains("test.txt"));
    }

    #[tokio::test]
    async fn test_grep_invalid_regex() {
        let temp = tempdir().unwrap();
        let ctx = ToolContext::new(temp.path().to_path_buf(), "test-exec".to_string());

        let input = json!({
            "pattern": "[invalid"
        });

        let tool = GrepTool;
        let result = tool.execute(input, &ctx).await;

        assert!(result.is_error);
        assert!(result.content.contains("Invalid regex"));
    }

    #[test]
    fn test_format_results() {
        let results = vec![
            MatchResult {
                file: "test.rs".to_string(),
                line_num: 1,
                line: "hello world".to_string(),
                is_context: false,
            },
            MatchResult {
                file: "test.rs".to_string(),
                line_num: 2,
                line: "context line".to_string(),
                is_context: true,
            },
        ];

        let output = format_results(&results, 50);
        assert!(output.contains("test.rs:1:hello world"));
        assert!(output.contains("test.rs-2-context line"));
    }
}
