//! LLM error types

use std::time::Duration;
use thiserror::Error;
use tracing::debug;

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
        debug!(?self, "is_rate_limit: called");
        let result = matches!(self, LlmError::RateLimited { .. });
        if result {
            debug!("is_rate_limit: true - RateLimited variant");
        } else {
            debug!("is_rate_limit: false - not RateLimited variant");
        }
        result
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        debug!(?self, "is_retryable: called");
        match self {
            LlmError::RateLimited { .. } => {
                debug!("is_retryable: RateLimited - true");
                true
            }
            LlmError::ApiError { status, .. } => {
                let retryable = *status >= 500;
                if retryable {
                    debug!(%status, "is_retryable: ApiError 5xx - true");
                } else {
                    debug!(%status, "is_retryable: ApiError non-5xx - false");
                }
                retryable
            }
            LlmError::Network(_) => {
                debug!("is_retryable: Network - true");
                true
            }
            LlmError::Timeout(_) => {
                debug!("is_retryable: Timeout - true");
                true
            }
            LlmError::InvalidResponse(_) => {
                debug!("is_retryable: InvalidResponse - false");
                false
            }
            LlmError::Json(_) => {
                debug!("is_retryable: Json - false");
                false
            }
        }
    }

    /// Get the retry duration if this is a rate limit error
    pub fn retry_after(&self) -> Option<Duration> {
        debug!(?self, "retry_after: called");
        match self {
            LlmError::RateLimited { retry_after } => {
                debug!(?retry_after, "retry_after: RateLimited");
                Some(*retry_after)
            }
            _ => {
                debug!("retry_after: not RateLimited - None");
                None
            }
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
