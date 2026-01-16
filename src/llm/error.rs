//! LLM error types

use std::time::Duration;
use thiserror::Error;

/// Errors that can occur during LLM operations
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("Rate limited, retry after {retry_after:?}")]
    RateLimited { retry_after: Duration },

    #[error("API error {status}: {message}")]
    ApiError { status: u16, message: String },

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Timeout after {0:?}")]
    Timeout(Duration),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

impl LlmError {
    /// Check if this is a rate limit error
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, LlmError::RateLimited { .. })
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            LlmError::RateLimited { .. } => true,
            LlmError::ApiError { status, .. } => *status >= 500,
            LlmError::Network(_) => true,
            LlmError::Timeout(_) => true,
            LlmError::InvalidResponse(_) => false,
            LlmError::Json(_) => false,
        }
    }

    /// Get the retry duration if this is a rate limit error
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            LlmError::RateLimited { retry_after } => Some(*retry_after),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_rate_limit() {
        let err = LlmError::RateLimited {
            retry_after: Duration::from_secs(60),
        };
        assert!(err.is_rate_limit());

        let err = LlmError::ApiError {
            status: 500,
            message: "Server error".to_string(),
        };
        assert!(!err.is_rate_limit());
    }

    #[test]
    fn test_is_retryable() {
        // Rate limited should be retryable
        assert!(
            LlmError::RateLimited {
                retry_after: Duration::from_secs(60)
            }
            .is_retryable()
        );

        // 5xx errors should be retryable
        assert!(
            LlmError::ApiError {
                status: 500,
                message: "Server error".to_string()
            }
            .is_retryable()
        );

        assert!(
            LlmError::ApiError {
                status: 502,
                message: "Bad gateway".to_string()
            }
            .is_retryable()
        );

        // 4xx errors should not be retryable
        assert!(
            !LlmError::ApiError {
                status: 400,
                message: "Bad request".to_string()
            }
            .is_retryable()
        );

        // Timeout should be retryable
        assert!(LlmError::Timeout(Duration::from_secs(30)).is_retryable());

        // Invalid response should not be retryable
        assert!(!LlmError::InvalidResponse("Bad JSON".to_string()).is_retryable());
    }

    #[test]
    fn test_retry_after() {
        let err = LlmError::RateLimited {
            retry_after: Duration::from_secs(42),
        };
        assert_eq!(err.retry_after(), Some(Duration::from_secs(42)));

        let err = LlmError::ApiError {
            status: 500,
            message: "Server error".to_string(),
        };
        assert_eq!(err.retry_after(), None);
    }
}
