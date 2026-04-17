use crate::error::ApiError;

/// High-level reason for a failover decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailoverReason {
    /// Rate limit hit (429).
    RateLimit,
    /// Context length exceeded (400).
    ContextLength,
    /// Invalid request body or parameters.
    InvalidRequest,
    /// Network or connectivity error.
    NetworkError,
    /// Provider returned an unexpected response.
    ProviderUnavailable,
    /// Authentication or authorization failure.
    AuthFailure,
    /// Request timed out.
    Timeout,
    /// Provider returned success but with an empty or meaningless response.
    EmptyResponse,
}

impl std::fmt::Display for FailoverReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailoverReason::RateLimit => write!(f, "rate_limit"),
            FailoverReason::ContextLength => write!(f, "context_length"),
            FailoverReason::InvalidRequest => write!(f, "invalid_request"),
            FailoverReason::NetworkError => write!(f, "network_error"),
            FailoverReason::ProviderUnavailable => write!(f, "provider_unavailable"),
            FailoverReason::AuthFailure => write!(f, "auth_failure"),
            FailoverReason::Timeout => write!(f, "timeout"),
            FailoverReason::EmptyResponse => write!(f, "empty_response"),
        }
    }
}

/// Recovery action to take for a given failover reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// Retry with exponential backoff.
    RetryWithBackoff,
    /// Compress context and retry.
    TriggerCompression,
    /// Abort the request — no recovery possible.
    Abort,
    /// Failover to the next credential in the pool.
    FailoverToNextCredential,
    /// Retry with a longer timeout.
    RetryWithTimeout,
}

impl std::fmt::Display for RecoveryStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecoveryStrategy::RetryWithBackoff => write!(f, "retry_with_backoff"),
            RecoveryStrategy::TriggerCompression => write!(f, "trigger_compression"),
            RecoveryStrategy::Abort => write!(f, "abort"),
            RecoveryStrategy::FailoverToNextCredential => write!(f, "failover_to_next_credential"),
            RecoveryStrategy::RetryWithTimeout => write!(f, "retry_with_timeout"),
        }
    }
}

/// Classify an `ApiError` into a high-level `FailoverReason`.
///
/// Uses the error variant and optional response body for richer classification.
pub fn classify_failover_reason(error: &ApiError, body: &str) -> FailoverReason {
    match error {
        ApiError::RateLimit(_) => FailoverReason::RateLimit,
        ApiError::ContextLengthExceeded(_) => FailoverReason::ContextLength,
        ApiError::AuthenticationFailed(_) => FailoverReason::AuthFailure,
        ApiError::Timeout => FailoverReason::Timeout,
        ApiError::StreamParseError(_) => FailoverReason::ProviderUnavailable,
        ApiError::ConnectionError(_) => FailoverReason::NetworkError,
        ApiError::ProviderError { status, message } => {
            // Check body for more specific classification
            let lower = body.to_lowercase();
            if lower.contains("context")
                || lower.contains("maximum context length")
                || lower.contains("prompt is too long")
                || lower.contains("token limit")
            {
                return FailoverReason::ContextLength;
            }
            if lower.contains("auth") || lower.contains("token") || lower.contains("key") {
                return FailoverReason::AuthFailure;
            }
            if lower.contains("empty") || lower.contains("no response") {
                return FailoverReason::EmptyResponse;
            }

            match status {
                400 | 422 => FailoverReason::InvalidRequest,
                401 | 403 => FailoverReason::AuthFailure,
                429 => FailoverReason::RateLimit,
                500..=599 => FailoverReason::ProviderUnavailable,
                _ => {
                    if message.is_empty() {
                        FailoverReason::EmptyResponse
                    } else {
                        FailoverReason::ProviderUnavailable
                    }
                }
            }
        }
        ApiError::Unknown(msg) => {
            let lower = msg.to_lowercase();
            if lower.contains("timeout") {
                FailoverReason::Timeout
            } else if lower.contains("context") {
                FailoverReason::ContextLength
            } else {
                FailoverReason::NetworkError
            }
        }
    }
}

/// Map a `FailoverReason` to the appropriate `RecoveryStrategy`.
pub fn recovery_strategy(reason: FailoverReason) -> RecoveryStrategy {
    match reason {
        FailoverReason::RateLimit => RecoveryStrategy::RetryWithBackoff,
        FailoverReason::ContextLength => RecoveryStrategy::TriggerCompression,
        FailoverReason::InvalidRequest => RecoveryStrategy::Abort,
        FailoverReason::NetworkError => RecoveryStrategy::RetryWithTimeout,
        FailoverReason::ProviderUnavailable => RecoveryStrategy::FailoverToNextCredential,
        FailoverReason::AuthFailure => RecoveryStrategy::Abort,
        FailoverReason::Timeout => RecoveryStrategy::RetryWithTimeout,
        FailoverReason::EmptyResponse => RecoveryStrategy::RetryWithBackoff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rate_limit() {
        let err = ApiError::RateLimit("too many requests".to_string());
        assert_eq!(classify_failover_reason(&err, ""), FailoverReason::RateLimit);
        assert_eq!(
            recovery_strategy(FailoverReason::RateLimit),
            RecoveryStrategy::RetryWithBackoff
        );
    }

    #[test]
    fn test_classify_context_length() {
        let err = ApiError::ContextLengthExceeded("too long".to_string());
        assert_eq!(
            classify_failover_reason(&err, ""),
            FailoverReason::ContextLength
        );
        assert_eq!(
            recovery_strategy(FailoverReason::ContextLength),
            RecoveryStrategy::TriggerCompression
        );
    }

    #[test]
    fn test_classify_auth_failure() {
        let err = ApiError::AuthenticationFailed("bad key".to_string());
        assert_eq!(
            classify_failover_reason(&err, ""),
            FailoverReason::AuthFailure
        );
        assert_eq!(
            recovery_strategy(FailoverReason::AuthFailure),
            RecoveryStrategy::Abort
        );
    }

    #[test]
    fn test_classify_timeout() {
        let err = ApiError::Timeout;
        assert_eq!(classify_failover_reason(&err, ""), FailoverReason::Timeout);
        assert_eq!(
            recovery_strategy(FailoverReason::Timeout),
            RecoveryStrategy::RetryWithTimeout
        );
    }

    #[test]
    fn test_classify_connection_error() {
        // Can't construct a real reqwest::Error, use Unknown instead
        let err = ApiError::Unknown("connection refused".to_string());
        assert_eq!(
            classify_failover_reason(&err, ""),
            FailoverReason::NetworkError
        );
    }

    #[test]
    fn test_classify_provider_error_500() {
        let err = ApiError::ProviderError {
            status: 500,
            message: "internal error".to_string(),
        };
        assert_eq!(
            classify_failover_reason(&err, ""),
            FailoverReason::ProviderUnavailable
        );
        assert_eq!(
            recovery_strategy(FailoverReason::ProviderUnavailable),
            RecoveryStrategy::FailoverToNextCredential
        );
    }

    #[test]
    fn test_classify_provider_error_400() {
        let err = ApiError::ProviderError {
            status: 400,
            message: "bad request".to_string(),
        };
        assert_eq!(
            classify_failover_reason(&err, ""),
            FailoverReason::InvalidRequest
        );
        assert_eq!(
            recovery_strategy(FailoverReason::InvalidRequest),
            RecoveryStrategy::Abort
        );
    }

    #[test]
    fn test_classify_provider_error_429() {
        let err = ApiError::ProviderError {
            status: 429,
            message: "rate limited".to_string(),
        };
        assert_eq!(classify_failover_reason(&err, ""), FailoverReason::RateLimit);
    }

    #[test]
    fn test_classify_provider_error_body_context() {
        let err = ApiError::ProviderError {
            status: 400,
            message: "bad request".to_string(),
        };
        assert_eq!(
            classify_failover_reason(&err, "context_length_exceeded"),
            FailoverReason::ContextLength
        );
    }

    #[test]
    fn test_classify_provider_error_body_auth() {
        let err = ApiError::ProviderError {
            status: 401,
            message: "error".to_string(),
        };
        assert_eq!(
            classify_failover_reason(&err, "invalid auth token"),
            FailoverReason::AuthFailure
        );
    }

    #[test]
    fn test_classify_provider_error_empty_response() {
        let err = ApiError::ProviderError {
            status: 200,
            message: String::new(),
        };
        assert_eq!(
            classify_failover_reason(&err, ""),
            FailoverReason::EmptyResponse
        );
    }

    #[test]
    fn test_classify_stream_parse_error() {
        let err = ApiError::StreamParseError("invalid SSE".to_string());
        assert_eq!(
            classify_failover_reason(&err, ""),
            FailoverReason::ProviderUnavailable
        );
    }

    #[test]
    fn test_failover_reason_display() {
        assert_eq!(FailoverReason::RateLimit.to_string(), "rate_limit");
        assert_eq!(FailoverReason::ContextLength.to_string(), "context_length");
        assert_eq!(RecoveryStrategy::Abort.to_string(), "abort");
    }

    #[test]
    fn test_recovery_strategy_display() {
        assert_eq!(
            RecoveryStrategy::RetryWithBackoff.to_string(),
            "retry_with_backoff"
        );
        assert_eq!(
            RecoveryStrategy::FailoverToNextCredential.to_string(),
            "failover_to_next_credential"
        );
    }

    #[test]
    fn test_invalid_request_aborts() {
        assert_eq!(
            recovery_strategy(FailoverReason::InvalidRequest),
            RecoveryStrategy::Abort
        );
    }
}
