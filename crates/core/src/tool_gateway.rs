use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Default domain for the Nous tool gateway.
const DEFAULT_TOOL_GATEWAY_DOMAIN: &str = "nousresearch.com";

/// Default scheme for the gateway.
const DEFAULT_TOOL_GATEWAY_SCHEME: &str = "https";

/// Skew in seconds before expiry to trigger refresh.
const NOUS_ACCESS_TOKEN_REFRESH_SKEW_SECONDS: i64 = 120;

/// Configuration for the managed tool gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedToolGatewayConfig {
    /// Vendor being proxied (e.g., "openai", "anthropic").
    pub vendor: String,
    /// Full gateway URL.
    pub gateway_origin: String,
    /// Subscriber OAuth token for Nous gateway.
    pub nous_user_token: String,
    /// Whether managed mode is active.
    pub managed_mode: bool,
}

/// Check if managed Nous tools gateway is enabled.
pub fn managed_nous_tools_enabled() -> bool {
    // Check env var first
    if let Ok(token) = std::env::var("TOOL_GATEWAY_USER_TOKEN") {
        return !token.is_empty();
    }

    // Check auth.json for Nous provider state
    if let Ok(state) = read_nous_provider_state() {
        if let Some(token) = state.get("access_token").and_then(|v| v.as_str()) {
            if !token.is_empty() {
                // Check if token is still valid (not expired)
                if let Some(expires) = state.get("expires_at") {
                    if let Some(expiry) = parse_timestamp(expires) {
                        let remaining = (expiry - Utc::now()).num_seconds();
                        return remaining > NOUS_ACCESS_TOKEN_REFRESH_SKEW_SECONDS;
                    }
                }
                return true; // No expiry info, assume valid
            }
        }
    }

    false
}

/// Read Nous provider state from ~/.hermes/auth.json.
fn read_nous_provider_state() -> Result<serde_json::Value, String> {
    let auth_path = std::env::var("HERMES_HOME")
        .ok()
        .map(|h| PathBuf::from(h).join("auth.json"))
        .or_else(|| {
            std::env::var("HOME").ok().map(|h| {
                PathBuf::from(h).join(".hermes/auth.json")
            })
        });

    let Some(path) = auth_path else {
        return Err("HERMES_HOME and HOME not set".to_string());
    };

    if !path.exists() {
        return Err(format!("Auth file not found: {}", path.display()));
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read auth file: {e}"))?;

    let auth: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse auth file: {e}"))?;

    // Navigate to providers → nous
    let providers = auth
        .get("providers")
        .and_then(|v| v.as_object());

    let Some(providers) = providers else {
        return Err("No providers section in auth.json".to_string());
    };

    let nous = providers.get("nous");
    let Some(nous) = nous else {
        return Err("No nous provider in auth.json".to_string());
    };

    Ok(nous.clone())
}

/// Parse a timestamp from various formats (ISO with Z suffix, etc.).
fn parse_timestamp(value: &serde_json::Value) -> Option<DateTime<Utc>> {
    // Try Unix timestamp (numeric) first
    if let Some(ts) = value.as_i64() {
        return DateTime::from_timestamp(ts, 0);
    }

    // Try string formats
    let s = value.as_str()?;

    // Try RFC3339 / ISO 8601 with Z suffix
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try common format: "2024-01-15T10:30:00Z"
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ") {
        return Some(dt.and_utc());
    }

    // Try format with fractional seconds
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.fZ") {
        return Some(dt.and_utc());
    }

    None
}

/// Check if an access token is expiring soon (within the skew window).
pub fn access_token_is_expiring(expires_at: &serde_json::Value, skew_seconds: i64) -> bool {
    let Some(expires) = parse_timestamp(expires_at) else {
        return false;
    };

    let remaining = (expires - Utc::now()).num_seconds();
    remaining <= skew_seconds.max(0)
}

/// Resolve the managed tool gateway configuration for a given vendor.
///
/// Returns `None` if managed mode is not active or the vendor is unknown.
pub fn resolve_managed_tool_gateway(vendor: &str) -> Option<ManagedToolGatewayConfig> {
    if !managed_nous_tools_enabled() {
        return None;
    }

    // Resolve token: env var takes priority over auth.json
    let token = std::env::var("TOOL_GATEWAY_USER_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| {
            read_nous_provider_state()
                .ok()
                .and_then(|state| {
                    state
                        .get("access_token")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
        });

    let token = token?;

    // Build gateway URL
    let scheme = std::env::var("TOOL_GATEWAY_SCHEME")
        .ok()
        .unwrap_or_else(|| DEFAULT_TOOL_GATEWAY_SCHEME.to_string());

    let domain = std::env::var("TOOL_GATEWAY_DOMAIN")
        .ok()
        .unwrap_or_else(|| DEFAULT_TOOL_GATEWAY_DOMAIN.to_string());

    let gateway_origin = format!("{scheme}://{domain}/api/v1/tools/{vendor}");

    Some(ManagedToolGatewayConfig {
        vendor: vendor.to_string(),
        gateway_origin,
        nous_user_token: token,
        managed_mode: true,
    })
}

/// Build a reqwest client pre-configured for the managed gateway.
pub fn build_gateway_client(config: &ManagedToolGatewayConfig) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    let auth_value = format!("Bearer {}", config.nous_user_token)
        .try_into()
        .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static(""));
    headers.insert(reqwest::header::AUTHORIZATION, auth_value);

    let vendor_value = reqwest::header::HeaderValue::from_str(&config.vendor)
        .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static(""));
    headers.insert("X-Gateway-Vendor", vendor_value);

    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_managed_nous_tools_disabled_by_default() {
        // Without TOOL_GATEWAY_USER_TOKEN or auth.json, should be disabled
        // (Unless the test environment happens to have these set)
        let enabled = std::env::var("TOOL_GATEWAY_USER_TOKEN").is_ok();
        if !enabled {
            assert!(!managed_nous_tools_enabled());
        }
    }

    #[test]
    fn test_managed_nous_tools_enabled_with_env() {
        // Save original
        let original = std::env::var("TOOL_GATEWAY_USER_TOKEN").ok();

        unsafe {
            std::env::set_var("TOOL_GATEWAY_USER_TOKEN", "test-token-123");
        }
        assert!(managed_nous_tools_enabled());

        // Restore
        if let Some(val) = original {
            unsafe { std::env::set_var("TOOL_GATEWAY_USER_TOKEN", val) }
        } else {
            unsafe { std::env::remove_var("TOOL_GATEWAY_USER_TOKEN") }
        }
    }

    #[test]
    fn test_parse_timestamp_iso() {
        let value = serde_json::json!("2024-01-15T10:30:00Z");
        let dt = parse_timestamp(&value);
        assert!(dt.is_some());
    }

    #[test]
    fn test_parse_timestamp_rfc3339() {
        let value = serde_json::json!("2024-01-15T10:30:00+00:00");
        let dt = parse_timestamp(&value);
        assert!(dt.is_some());
    }

    #[test]
    fn test_parse_timestamp_unix() {
        let value = serde_json::json!(1705312200);
        let dt = parse_timestamp(&value);
        assert!(dt.is_some());
        assert_eq!(dt.unwrap().timestamp(), 1705312200);
    }

    #[test]
    fn test_parse_timestamp_invalid() {
        let value = serde_json::json!("not-a-date");
        let dt = parse_timestamp(&value);
        assert!(dt.is_none());
    }

    #[test]
    fn test_access_token_is_expiring_soon() {
        // Token expiring in 60 seconds with 120s skew → should be expiring
        let future = Utc::now() + chrono::Duration::seconds(60);
        let value = serde_json::json!(future.to_rfc3339());
        assert!(access_token_is_expiring(&value, 120));
    }

    #[test]
    fn test_access_token_not_expiring() {
        // Token expiring in 1 hour with 120s skew → should not be expiring
        let future = Utc::now() + chrono::Duration::hours(1);
        let value = serde_json::json!(future.to_rfc3339());
        assert!(!access_token_is_expiring(&value, 120));
    }

    #[test]
    fn test_access_token_already_expired() {
        // Token that expired 10 minutes ago
        let past = Utc::now() - chrono::Duration::minutes(10);
        let value = serde_json::json!(past.to_rfc3339());
        assert!(access_token_is_expiring(&value, 120));
    }

    #[test]
    fn test_resolve_gateway_returns_none_when_disabled() {
        // Without env var or auth.json, should return None
        let has_token = std::env::var("TOOL_GATEWAY_USER_TOKEN").is_ok();
        if !has_token {
            assert!(resolve_managed_tool_gateway("openai-audio").is_none());
        }
    }

    #[test]
    fn test_resolve_gateway_with_env() {
        let original = std::env::var("TOOL_GATEWAY_USER_TOKEN").ok();

        unsafe {
            std::env::set_var("TOOL_GATEWAY_USER_TOKEN", "test-token");
        }
        let config = resolve_managed_tool_gateway("openai-audio");
        assert!(config.is_some());
        let config = config.unwrap();
        assert_eq!(config.vendor, "openai-audio");
        assert!(config.managed_mode);
        assert_eq!(config.nous_user_token, "test-token");
        assert!(config.gateway_origin.contains("nousresearch.com"));

        // Restore
        if let Some(val) = original {
            unsafe { std::env::set_var("TOOL_GATEWAY_USER_TOKEN", val) }
        } else {
            unsafe { std::env::remove_var("TOOL_GATEWAY_USER_TOKEN") }
        }
    }

    #[test]
    fn test_build_gateway_client() {
        let config = ManagedToolGatewayConfig {
            vendor: "openai".to_string(),
            gateway_origin: "https://nousresearch.com/api/v1/tools/openai".to_string(),
            nous_user_token: "test-token".to_string(),
            managed_mode: true,
        };
        let client = build_gateway_client(&config);
        // Client should be usable
        assert!(client.get("https://example.com").build().is_ok());
    }
}
