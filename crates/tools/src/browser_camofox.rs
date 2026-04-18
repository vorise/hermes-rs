use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Default Camofox REST API URL.
const DEFAULT_CAMOFOX_URL: &str = "http://localhost:9377";

/// Max characters for snapshot responses (Camofox paginates at this limit).
const _SNAPSHOT_MAX_CHARS: usize = 80_000;

/// Camofox REST API client.
///
/// Routes browser operations through a self-hosted Camoufox instance
/// (Firefox fork with C++ fingerprint spoofing for anti-detection).
#[derive(Debug, Clone)]
pub struct CamofoxClient {
    client: Client,
    base_url: String,
    vnc_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CamofoxHealth {
    pub status: String,
    pub vnc_port: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct SnapshotResult {
    pub content: String,
    pub truncated: bool,
}

impl CamofoxClient {
    /// Create a new client. Resolves URL from `CAMOFOX_URL` env var,
    /// falling back to `http://localhost:9377`.
    pub fn new() -> Self {
        let base_url = std::env::var("CAMOFOX_URL")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_CAMOFOX_URL.to_string());

        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url,
            vnc_url: None,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Check if Camofox is available.
    pub async fn health(&self) -> Result<CamofoxHealth, String> {
        let resp = self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .map_err(|e| format!("Camofox health check failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("Camofox health returned: {}", resp.status()));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse health response: {e}"))?;

        let status = body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let vnc_port = body.get("vncPort").and_then(|v| v.as_u64()).map(|p| p as u16);

        Ok(CamofoxHealth { status, vnc_port })
    }

    /// Navigate to a URL.
    pub async fn navigate(&self, url: &str) -> Result<String, String> {
        let resp = self
            .client
            .post(format!("{}/navigate", self.base_url))
            .header("content-type", "application/json")
            .json(&serde_json::json!({ "url": url }))
            .send()
            .await
            .map_err(|e| format!("Camofox navigate failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Navigate failed: {status}: {body}"));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse navigate response: {e}"))?;

        Ok(body
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or(url)
            .to_string())
    }

    /// Get accessibility snapshot with element refs.
    pub async fn snapshot(&self) -> Result<SnapshotResult, String> {
        let resp = self
            .client
            .post(format!("{}/snapshot", self.base_url))
            .header("content-type", "application/json")
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| format!("Camofox snapshot failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Snapshot failed: {status}: {body}"));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse snapshot response: {e}"))?;

        let content = body
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let truncated = content.len() > _SNAPSHOT_MAX_CHARS;

        // Truncate to max chars
        let content = if content.len() > _SNAPSHOT_MAX_CHARS {
            content.chars().take(_SNAPSHOT_MAX_CHARS).collect()
        } else {
            content
        };

        Ok(SnapshotResult {
            content,
            truncated,
        })
    }

    /// Click an element by ref/selector.
    pub async fn click(&self, selector: &str) -> Result<String, String> {
        let resp = self
            .client
            .post(format!("{}/click", self.base_url))
            .header("content-type", "application/json")
            .json(&serde_json::json!({ "selector": selector }))
            .send()
            .await
            .map_err(|e| format!("Camofox click failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Click failed: {status}: {body}"));
        }

        Ok(format!("Clicked element: {selector}"))
    }

    /// Type text into an element by ref/selector.
    pub async fn r#type(&self, selector: &str, text: &str) -> Result<String, String> {
        let resp = self
            .client
            .post(format!("{}/type", self.base_url))
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "selector": selector,
                "text": text
            }))
            .send()
            .await
            .map_err(|e| format!("Camofox type failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Type failed: {status}: {body}"));
        }

        Ok(format!("Typed into {selector}: \"{text}\""))
    }

    /// Scroll the viewport.
    pub async fn scroll(&self, dx: i64, dy: i64) -> Result<String, String> {
        let resp = self
            .client
            .post(format!("{}/scroll", self.base_url))
            .header("content-type", "application/json")
            .json(&serde_json::json!({
                "dx": dx,
                "dy": dy
            }))
            .send()
            .await
            .map_err(|e| format!("Camofox scroll failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Scroll failed: {status}: {body}"));
        }

        Ok(format!("Scrolled by ({dx}, {dy})"))
    }

    /// Take a full page screenshot (returns base64 PNG).
    pub async fn screenshot(&self) -> Result<String, String> {
        let resp = self
            .client
            .post(format!("{}/screenshot", self.base_url))
            .header("content-type", "application/json")
            .json(&serde_json::json!({}))
            .send()
            .await
            .map_err(|e| format!("Camofox screenshot failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Screenshot failed: {status}: {body}"));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse screenshot response: {e}"))?;

        let data = body
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        Ok(data.to_string())
    }

    /// Check if Camofox is configured/enabled.
    pub fn is_enabled() -> bool {
        std::env::var("CAMOFOX_URL")
            .ok()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_enabled_false_by_default() {
        // CAMOFOX_URL is not set by default in tests
        let enabled = std::env::var("CAMOFOX_URL").is_ok();
        assert!(!enabled);
    }

    #[test]
    fn test_client_default_url() {
        let client = CamofoxClient::new();
        assert_eq!(client.base_url(), DEFAULT_CAMOFOX_URL);
    }
}
