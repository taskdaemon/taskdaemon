# Spec: Anthropic Client Implementation

**ID:** 002-anthropic-client
**Status:** Draft
**Dependencies:** [001-llm-client-trait]

## Summary

Implement `AnthropicClient` as a concrete implementation of the `LlmClient` trait, providing full support for Claude models with streaming responses, tool calling, and robust error handling.

## Acceptance Criteria

1. **Client Implementation**
   - Full implementation of `LlmClient` trait for Anthropic
   - Support for all Claude model variants
   - Streaming and non-streaming completions
   - Tool/function calling with proper formatting
   - Rate limiting and retry logic

2. **Configuration**
   - API key management (environment variable support)
   - Model selection and parameters
   - Custom endpoint configuration
   - Timeout and retry settings

3. **Error Handling**
   - Proper error mapping from Anthropic API errors
   - Network error handling with retries
   - Rate limit detection and backoff
   - Graceful degradation strategies

4. **Testing**
   - Integration tests with mock server
   - Unit tests for all public methods
   - Error scenario coverage
   - Performance benchmarks

## Implementation Phases

### Phase 1: Basic Client Setup
- Create `AnthropicClient` struct
- Implement configuration loading
- Set up HTTP client with retries
- Basic chat completion (non-streaming)

### Phase 2: Streaming Support
- Implement SSE parsing for streams
- Stream event mapping to common types
- Error handling in streaming context
- Backpressure management

### Phase 3: Advanced Features
- Tool/function calling support
- System message handling
- Token counting utilities
- Response caching layer

### Phase 4: Production Hardening
- Rate limiting implementation
- Retry logic with exponential backoff
- Metrics and logging
- Connection pooling optimization

## Technical Details

### Module Structure
```
src/llm/anthropic/
├── mod.rs
├── client.rs      # AnthropicClient implementation
├── config.rs      # Configuration types
├── types.rs       # Anthropic-specific types
├── streaming.rs   # SSE streaming logic
└── errors.rs      # Error mapping
```

### Configuration
```rust
pub struct AnthropicConfig {
    pub api_key: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub timeout: Duration,
    pub max_retries: u32,
}
```

### Dependencies
- `reqwest` with streaming support
- `tokio` for async runtime
- `serde` for JSON handling
- `futures` for stream utilities
- `tracing` for logging

## Notes

- API key should never be logged or included in error messages
- Implement proper request/response logging for debugging (with sensitive data redacted)
- Consider implementing a request queue to handle rate limits gracefully
- Support for vision models should be designed in but can be implemented later