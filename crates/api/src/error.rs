use std::time::Duration;

use thiserror::Error;

/// Error classification for LLM API calls.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("context length exceeded: {0}")]
    ContextLengthExceeded(String),

    #[error("authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("provider error: {status} - {message}")]
    ProviderError { status: u16, message: String },

    #[error("connection error: {0}")]
    ConnectionError(#[from] reqwest::Error),

    #[error("stream parse error: {0}")]
    StreamParseError(String),

    #[error("timeout")]
    Timeout,

    #[error("unknown error: {0}")]
    Unknown(String),
}

/// Classify an HTTP error from the API response.
pub fn classify_error(status: u16, body: &str) -> ApiError {
    match status {
        401 | 403 => ApiError::AuthenticationFailed(body.to_string()),
        429 => ApiError::RateLimit(body.to_string()),
        400 => {
            if body.contains("context_length")
                || body.contains("maximum context length")
                || body.contains("prompt is too long")
            {
                ApiError::ContextLengthExceeded(body.to_string())
            } else {
                ApiError::ProviderError {
                    status,
                    message: body.to_string(),
                }
            }
        }
        500..=599 => ApiError::ProviderError {
            status,
            message: body.to_string(),
        },
        _ => ApiError::ProviderError {
            status,
            message: body.to_string(),
        },
    }
}

/// Whether an error is retryable and what backoff to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Do not retry.
    NoRetry,
    /// Retry after the given duration.
    RetryAfter(Duration),
}

impl ApiError {
    /// Decide whether to retry this error, and how long to wait.
    pub fn retry_decision(&self, attempt: u32, max_retries: u32) -> RetryDecision {
        if attempt >= max_retries {
            return RetryDecision::NoRetry;
        }

        match self {
            // Rate limit: exponential backoff starting at 1s
            ApiError::RateLimit(_) => {
                let delay = Duration::from_secs(2_u64.pow(attempt.min(5) as u32));
                RetryDecision::RetryAfter(delay)
            }
            // Server errors: exponential backoff starting at 500ms
            ApiError::ProviderError { status, .. } if *status >= 500 => {
                let delay = Duration::from_millis(500 * 2_u64.pow(attempt.min(4) as u32));
                RetryDecision::RetryAfter(delay)
            }
            // Connection errors: retry with short backoff
            ApiError::ConnectionError(_) => {
                let delay = Duration::from_millis(1000 * 2_u64.pow(attempt.min(3) as u32));
                RetryDecision::RetryAfter(delay)
            }
            // Timeout: retry with moderate backoff
            ApiError::Timeout => {
                let delay = Duration::from_secs(1 + attempt as u64);
                RetryDecision::RetryAfter(delay)
            }
            // Never retry these
            ApiError::ContextLengthExceeded(_)
            | ApiError::AuthenticationFailed(_)
            | ApiError::StreamParseError(_)
            | ApiError::Unknown(_) => RetryDecision::NoRetry,
            // Provider errors below 500 are not retryable
            ApiError::ProviderError { .. } => RetryDecision::NoRetry,
        }
    }
}

/// Configuration for retry behavior.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retries for transient errors.
    pub max_retries: u32,
    /// Whether to retry rate limits (429).
    pub retry_rate_limits: bool,
    /// Whether to retry server errors (5xx).
    pub retry_server_errors: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_rate_limits: true,
            retry_server_errors: true,
        }
    }
}

/// Execute an async operation with retry logic.
pub async fn with_retry<T, F, Fut>(config: &RetryConfig, mut f: F) -> Result<T, ApiError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, ApiError>>,
{
    let mut attempt = 0u32;
    loop {
        match f().await {
            Ok(value) => return Ok(value),
            Err(e) => {
                let decision = match &e {
                    ApiError::RateLimit(_) if !config.retry_rate_limits => RetryDecision::NoRetry,
                    ApiError::ProviderError { status, .. }
                        if *status >= 500 && !config.retry_server_errors =>
                    {
                        RetryDecision::NoRetry
                    }
                    _ => e.retry_decision(attempt, config.max_retries),
                };

                match decision {
                    RetryDecision::NoRetry => return Err(e),
                    RetryDecision::RetryAfter(delay) => {
                        tracing::warn!(
                            attempt,
                            delay_ms = delay.as_millis(),
                            error = %e,
                            "Retrying API call after transient error"
                        );
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rate_limit() {
        let err = classify_error(429, "rate limit");
        assert!(matches!(err, ApiError::RateLimit(_)));
    }

    #[test]
    fn test_classify_auth_failure() {
        let err = classify_error(401, "unauthorized");
        assert!(matches!(err, ApiError::AuthenticationFailed(_)));
        let err = classify_error(403, "forbidden");
        assert!(matches!(err, ApiError::AuthenticationFailed(_)));
    }

    #[test]
    fn test_classify_context_length() {
        let body = "context_length_exceeded";
        let err = classify_error(400, body);
        assert!(matches!(err, ApiError::ContextLengthExceeded(_)));
    }

    #[test]
    fn test_classify_server_error() {
        let err = classify_error(500, "internal error");
        assert!(matches!(err, ApiError::ProviderError { status: 500, .. }));
    }

    #[test]
    fn test_retry_decision_rate_limit() {
        let err = ApiError::RateLimit("test".to_string());
        let decision = err.retry_decision(0, 3);
        assert!(matches!(decision, RetryDecision::RetryAfter(d) if d > Duration::ZERO));
    }

    #[test]
    fn test_retry_decision_context_length_never_retries() {
        let err = ApiError::ContextLengthExceeded("test".to_string());
        assert_eq!(err.retry_decision(0, 10), RetryDecision::NoRetry);
    }

    #[test]
    fn test_retry_decision_auth_never_retries() {
        let err = ApiError::AuthenticationFailed("test".to_string());
        assert_eq!(err.retry_decision(0, 10), RetryDecision::NoRetry);
    }

    #[test]
    fn test_retry_decision_respects_max_retries() {
        let err = ApiError::RateLimit("test".to_string());
        assert_eq!(err.retry_decision(3, 3), RetryDecision::NoRetry);
        assert!(matches!(
            err.retry_decision(2, 3),
            RetryDecision::RetryAfter(_)
        ));
    }

    #[test]
    fn test_retry_decision_server_error_backoff() {
        let err = ApiError::ProviderError {
            status: 502,
            message: "bad gateway".to_string(),
        };
        let d0 = err.retry_decision(0, 3);
        let d1 = err.retry_decision(1, 3);
        let d2 = err.retry_decision(2, 3);
        // Backoff should increase
        let a = match d0 {
            RetryDecision::RetryAfter(d) => d,
            _ => panic!("expected RetryAfter"),
        };
        let b = match d1 {
            RetryDecision::RetryAfter(d) => d,
            _ => panic!("expected RetryAfter"),
        };
        let c = match d2 {
            RetryDecision::RetryAfter(d) => d,
            _ => panic!("expected RetryAfter"),
        };
        assert!(b > a);
        assert!(c > b);
    }
}
