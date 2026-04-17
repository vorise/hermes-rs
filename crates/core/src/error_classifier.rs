use std::fmt;

/// Classification of an API error for determining recovery strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Rate limited — wait and retry.
    RateLimit {
        /// Seconds to wait before retrying (from Retry-After header or estimate).
        retry_after_secs: Option<u64>,
    },
    /// Context length exceeded — need to compress or truncate.
    ContextLength {
        /// Maximum allowed context length (if parseable from error).
        max_context: Option<u64>,
    },
    /// Output length exceeded — reduce max_tokens.
    OutputLength {
        /// Maximum allowed output tokens (if parseable).
        max_output: Option<u64>,
    },
    /// Authentication failure — token expired or invalid.
    AuthFailure,
    /// Temporary server error — retry with backoff.
    ServerError {
        /// HTTP status code.
        status_code: Option<u16>,
    },
    /// Timeout — request took too long.
    Timeout,
    /// Invalid request — bad parameters, won't succeed on retry.
    BadRequest,
    /// Quota exceeded — daily/monthly limit hit.
    QuotaExceeded,
    /// Content policy violation.
    ContentFilter,
    /// Unknown/unrecognized error.
    Unknown,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::RateLimit { retry_after_secs } => {
                if let Some(secs) = retry_after_secs {
                    write!(f, "rate_limit (retry after {secs}s)")
                } else {
                    write!(f, "rate_limit")
                }
            }
            ErrorCategory::ContextLength { max_context } => {
                if let Some(max) = max_context {
                    write!(f, "context_length_exceeded (max: {max})")
                } else {
                    write!(f, "context_length_exceeded")
                }
            }
            ErrorCategory::OutputLength { max_output } => {
                if let Some(max) = max_output {
                    write!(f, "output_length_exceeded (max: {max})")
                } else {
                    write!(f, "output_length_exceeded")
                }
            }
            ErrorCategory::AuthFailure => write!(f, "auth_failure"),
            ErrorCategory::ServerError { status_code } => {
                if let Some(code) = status_code {
                    write!(f, "server_error ({code})")
                } else {
                    write!(f, "server_error")
                }
            }
            ErrorCategory::Timeout => write!(f, "timeout"),
            ErrorCategory::BadRequest => write!(f, "bad_request"),
            ErrorCategory::QuotaExceeded => write!(f, "quota_exceeded"),
            ErrorCategory::ContentFilter => write!(f, "content_filter"),
            ErrorCategory::Unknown => write!(f, "unknown"),
        }
    }
}

impl ErrorCategory {
    /// Whether this error is recoverable with a retry.
    pub fn is_recoverable(&self) -> bool {
        match self {
            ErrorCategory::RateLimit { .. }
            | ErrorCategory::ServerError { .. }
            | ErrorCategory::Timeout => true,
            ErrorCategory::ContextLength { .. } => true, // Can compress
            ErrorCategory::OutputLength { .. } => true, // Can reduce max_tokens
            ErrorCategory::AuthFailure
            | ErrorCategory::BadRequest
            | ErrorCategory::QuotaExceeded
            | ErrorCategory::ContentFilter
            | ErrorCategory::Unknown => false,
        }
    }

    /// Recommended action for this error.
    pub fn recommended_action(&self) -> &'static str {
        match self {
            ErrorCategory::RateLimit { .. } => "wait and retry",
            ErrorCategory::ContextLength { .. } => "compress context or use truncation",
            ErrorCategory::OutputLength { .. } => "reduce max_tokens",
            ErrorCategory::AuthFailure => "refresh credentials",
            ErrorCategory::ServerError { .. } => "retry with exponential backoff",
            ErrorCategory::Timeout => "retry with increased timeout",
            ErrorCategory::BadRequest => "fix request parameters",
            ErrorCategory::QuotaExceeded => "switch to different credential or wait",
            ErrorCategory::ContentFilter => "modify prompt to avoid flagged content",
            ErrorCategory::Unknown => "check logs for details",
        }
    }
}

/// Classifies API errors into actionable categories.
pub struct ErrorClassifier;

impl ErrorClassifier {
    /// Classify an error based on its message and optional HTTP status code.
    pub fn classify(error: &str, status_code: Option<u16>) -> ErrorCategory {
        let lower = error.to_lowercase();

        // Rate limit
        if lower.contains("rate limit")
            || lower.contains("rate_limit")
            || lower.contains("too many requests")
            || lower.contains("429")
            || lower.contains("throttl")
        {
            let retry_after = parse_retry_after(error).or_else(|| {
                status_code.filter(|&c| c == 429).map(|_| 60) // Default 60s for 429
            });
            return ErrorCategory::RateLimit {
                retry_after_secs: retry_after,
            };
        }

        // Context length
        if lower.contains("context length")
            || lower.contains("context_window")
            || lower.contains("maximum context")
            || lower.contains("prompt is too long")
            || lower.contains("token limit")
            || lower.contains("exceeds") && lower.contains("context")
        {
            let max = parse_number_near(&lower, "limit")
                .or_else(|| parse_number_near(&lower, "maximum"))
                .or_else(|| parse_number_near(&lower, "context"));
            return ErrorCategory::ContextLength { max_context: max };
        }

        // Output length
        if lower.contains("max_tokens")
            || (lower.contains("output") && lower.contains("exceed"))
            || lower.contains("maximum output")
        {
            let max = parse_number_near(&lower, "max_tokens")
                .or_else(|| parse_number_near(&lower, "maximum"));
            return ErrorCategory::OutputLength { max_output: max };
        }

        // Auth failure
        if lower.contains("authentication")
            || lower.contains("unauthorized")
            || lower.contains("invalid api key")
            || lower.contains("invalid_api_key")
            || lower.contains("token expired")
            || lower.contains("401")
            || lower.contains("forbidden")
            || lower.contains("403")
        {
            return ErrorCategory::AuthFailure;
        }

        // Server errors
        if let Some(code) = status_code {
            if code >= 500 {
                return ErrorCategory::ServerError {
                    status_code: Some(code),
                };
            }
        }

        if lower.contains("internal server error")
            || lower.contains("500")
            || lower.contains("502")
            || lower.contains("503")
            || lower.contains("bad gateway")
            || lower.contains("service unavailable")
            || lower.contains("overloaded")
        {
            return ErrorCategory::ServerError { status_code };
        }

        // Timeout
        if lower.contains("timeout")
            || lower.contains("timed out")
            || lower.contains("deadline exceeded")
            || lower.contains("request timed out")
        {
            return ErrorCategory::Timeout;
        }

        // Bad request
        if let Some(code) = status_code {
            if code == 400 {
                return ErrorCategory::BadRequest;
            }
        }

        if lower.contains("bad request")
            || lower.contains("400")
            || lower.contains("invalid request")
            || lower.contains("validation error")
        {
            return ErrorCategory::BadRequest;
        }

        // Quota exceeded
        if lower.contains("quota")
            || lower.contains("billing")
            || lower.contains("insufficient balance")
            || lower.contains("credit")
        {
            return ErrorCategory::QuotaExceeded;
        }

        // Content filter
        if lower.contains("content policy")
            || lower.contains("flagged")
            || lower.contains("safety")
            || lower.contains("blocked")
            || lower.contains("moderation")
        {
            return ErrorCategory::ContentFilter;
        }

        ErrorCategory::Unknown
    }

    /// Classify from error message only (no status code).
    pub fn classify_message(error: &str) -> ErrorCategory {
        Self::classify(error, None)
    }
}

/// Parse a Retry-After header value or "retry after X seconds" from error text.
fn parse_retry_after(error: &str) -> Option<u64> {
    let lower = error.to_lowercase();
    if let Some(pos) = lower.find("retry after") {
        let rest = &error[pos + 11..];
        rest.trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .ok()
    } else {
        None
    }
}

/// Try to find a number near a keyword in the error text.
fn parse_number_near(error: &str, keyword: &str) -> Option<u64> {
    if let Some(pos) = error.find(keyword) {
        // Look forward from the keyword position
        let rest = &error[pos..];
        let digits: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_detection() {
        let cat = ErrorClassifier::classify_message("Rate limit exceeded. Please retry after 30 seconds.");
        assert_eq!(cat, ErrorCategory::RateLimit { retry_after_secs: Some(30) });
    }

    #[test]
    fn test_rate_limit_429() {
        let cat = ErrorClassifier::classify("Too many requests", Some(429));
        assert!(matches!(cat, ErrorCategory::RateLimit { .. }));
    }

    #[test]
    fn test_context_length() {
        let cat = ErrorClassifier::classify_message(
            "This model's maximum context length is 128000 tokens"
        );
        assert!(matches!(cat, ErrorCategory::ContextLength { max_context: Some(128000) }));
    }

    #[test]
    fn test_context_length_too_long() {
        let cat = ErrorClassifier::classify_message("prompt is too long: 150000 > 128000");
        assert!(matches!(cat, ErrorCategory::ContextLength { .. }));
    }

    #[test]
    fn test_output_length() {
        let cat = ErrorClassifier::classify_message(
            "max_tokens 16384 is greater than the context window"
        );
        assert!(matches!(cat, ErrorCategory::OutputLength { max_output: Some(16384) }));
    }

    #[test]
    fn test_auth_failure() {
        let cat = ErrorClassifier::classify("Invalid API key", Some(401));
        assert_eq!(cat, ErrorCategory::AuthFailure);
    }

    #[test]
    fn test_server_error() {
        let cat = ErrorClassifier::classify("Internal server error", Some(500));
        assert_eq!(cat, ErrorCategory::ServerError { status_code: Some(500) });
    }

    #[test]
    fn test_timeout() {
        let cat = ErrorClassifier::classify_message("Request timed out after 60s");
        assert_eq!(cat, ErrorCategory::Timeout);
    }

    #[test]
    fn test_bad_request() {
        let cat = ErrorClassifier::classify("Bad request: invalid parameter", Some(400));
        assert_eq!(cat, ErrorCategory::BadRequest);
    }

    #[test]
    fn test_quota_exceeded() {
        let cat = ErrorClassifier::classify_message("You have exceeded your monthly quota");
        assert_eq!(cat, ErrorCategory::QuotaExceeded);
    }

    #[test]
    fn test_content_filter() {
        let cat = ErrorClassifier::classify_message("Content policy violation: flagged content");
        assert_eq!(cat, ErrorCategory::ContentFilter);
    }

    #[test]
    fn test_unknown() {
        let cat = ErrorClassifier::classify_message("Something went very wrong");
        assert_eq!(cat, ErrorCategory::Unknown);
    }

    #[test]
    fn test_is_recoverable() {
        assert!(ErrorCategory::RateLimit { retry_after_secs: None }.is_recoverable());
        assert!(ErrorCategory::ContextLength { max_context: None }.is_recoverable());
        assert!(ErrorCategory::ServerError { status_code: None }.is_recoverable());
        assert!(ErrorCategory::Timeout.is_recoverable());
        assert!(!ErrorCategory::AuthFailure.is_recoverable());
        assert!(!ErrorCategory::BadRequest.is_recoverable());
        assert!(!ErrorCategory::QuotaExceeded.is_recoverable());
        assert!(!ErrorCategory::ContentFilter.is_recoverable());
    }

    #[test]
    fn test_recommended_action() {
        assert_eq!(
            ErrorCategory::RateLimit { retry_after_secs: None }.recommended_action(),
            "wait and retry"
        );
        assert_eq!(
            ErrorCategory::AuthFailure.recommended_action(),
            "refresh credentials"
        );
        assert_eq!(
            ErrorCategory::ContextLength { max_context: None }.recommended_action(),
            "compress context or use truncation"
        );
    }

    #[test]
    fn test_display() {
        let cat = ErrorCategory::RateLimit { retry_after_secs: Some(30) };
        assert_eq!(format!("{cat}"), "rate_limit (retry after 30s)");

        let cat = ErrorCategory::ServerError { status_code: Some(502) };
        assert_eq!(format!("{cat}"), "server_error (502)");
    }

    #[test]
    fn test_throttling_detection() {
        let cat = ErrorClassifier::classify_message("Requests are being throttled");
        assert!(matches!(cat, ErrorCategory::RateLimit { .. }));
    }
}
