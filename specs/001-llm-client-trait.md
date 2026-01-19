# Spec: LLM Client Trait

**ID:** 001-llm-client-trait  
**Status:** Draft  
**Dependencies:** None

## Summary

Define and implement the `LlmClient` trait that provides an abstract interface for LLM providers. This trait will enable the system to support multiple LLM backends while maintaining a consistent API.

## Acceptance Criteria

1. **Trait Definition**
   - `LlmClient` trait defined with async methods for chat completions
   - Support for streaming and non-streaming responses
   - Tool/function calling support
   - Error handling with custom error types
   - Configuration abstraction for provider-specific settings

2. **Type Definitions**
   - Request/response types that are provider-agnostic
   - Message format supporting system, user, and assistant roles
   - Tool definition and result types
   - Streaming event types

3. **Testing Infrastructure**
   - Mock implementation for testing
   - Test utilities for simulating responses
   - Example usage demonstrating the trait

## Implementation Phases

### Phase 1: Core Trait Definition
- Define the `LlmClient` trait with basic methods
- Create request/response types
- Define error types
- Document all public APIs

### Phase 2: Type System
- Message types with role support
- Tool/function calling types
- Streaming event enumeration
- Configuration trait for provider settings

### Phase 3: Testing Support
- Mock client implementation
- Test helper functions
- Documentation examples
- Integration test scaffolding

## Technical Details

### Module Structure
```
src/llm/
├── mod.rs
├── client.rs      # LlmClient trait
├── types.rs       # Common types
├── errors.rs      # Error definitions
└── mock.rs        # Mock implementation
```

### Key Interfaces
```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    type Config: Send + Sync;
    type Error: std::error::Error + Send + Sync + 'static;
    
    async fn chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<ChatResponse, Self::Error>;
    
    async fn chat_completion_stream(
        &self,
        request: ChatRequest,
    ) -> Result<BoxStream<'static, Result<StreamEvent, Self::Error>>, Self::Error>;
}
```

## Notes

- The trait should be designed with performance in mind, avoiding unnecessary allocations
- Consider future extensions like embeddings or image support
- Error types should preserve provider-specific error details while presenting a uniform interface
- The mock implementation should support scripted responses for deterministic testing