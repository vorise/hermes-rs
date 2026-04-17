use std::collections::HashMap;

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// OAuth 2.0 token response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    /// Access token.
    pub access_token: String,
    /// Token type (usually "Bearer").
    pub token_type: String,
    /// Refresh token (optional).
    pub refresh_token: Option<String>,
    /// Expires in seconds.
    pub expires_in: Option<u64>,
    /// Scope granted.
    pub scope: Option<String>,
    /// When the token was obtained (UTC epoch seconds).
    pub obtained_at: u64,
}

impl OAuthToken {
    /// Check if the token is expired.
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let expires_in = self.expires_in.unwrap_or(3600);
        now >= self.obtained_at + expires_in
    }

    /// Check if the token will expire within the given number of seconds.
    pub fn expires_within(&self, seconds: u64) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let expires_in = self.expires_in.unwrap_or(3600);
        now + seconds >= self.obtained_at + expires_in
    }

    /// Get the authorization header value (e.g., "Bearer <token>").
    pub fn auth_header(&self) -> String {
        format!("{} {}", self.token_type, self.access_token)
    }
}

/// OAuth server metadata (RFC 8414).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthMetadata {
    /// Authorization endpoint URL.
    pub authorization_endpoint: String,
    /// Token endpoint URL.
    pub token_endpoint: String,
    /// Registration endpoint URL (optional).
    pub registration_endpoint: Option<String>,
    /// Supported response types.
    pub response_types_supported: Vec<String>,
    /// Supported grant types.
    pub grant_types_supported: Vec<String>,
}

impl OAuthMetadata {
    /// Fetch OAuth metadata from the well-known URL.
    pub async fn fetch(base_url: &str) -> Result<Self> {
        let url = format!("{}/.well-known/oauth-authorization-server", base_url.trim_end_matches('/'));
        let client = reqwest::Client::new();
        let resp = client.get(&url).send().await
            .context("Failed to fetch OAuth metadata")?;

        if !resp.status().is_success() {
            return Err(anyhow!("OAuth metadata fetch failed: {}", resp.status()));
        }

        resp.json::<OAuthMetadata>().await
            .context("Failed to parse OAuth metadata")
    }
}

/// Configuration for an OAuth-protected MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpOAuthConfig {
    /// Server base URL.
    pub server_url: String,
    /// OAuth client ID.
    pub client_id: String,
    /// OAuth client secret (optional, for confidential clients).
    pub client_secret: Option<String>,
    /// Redirect URI for the OAuth callback.
    pub redirect_uri: String,
    /// Requested scopes.
    pub scopes: Vec<String>,
    /// OAuth metadata (can be auto-discovered).
    pub metadata: Option<OAuthMetadata>,
}

impl McpOAuthConfig {
    /// Create a new OAuth config.
    pub fn new(server_url: &str, client_id: &str) -> Self {
        Self {
            server_url: server_url.to_string(),
            client_id: client_id.to_string(),
            client_secret: None,
            redirect_uri: "http://localhost:9876/callback".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            metadata: None,
        }
    }

    /// With a custom redirect URI.
    pub fn with_redirect_uri(mut self, uri: &str) -> Self {
        self.redirect_uri = uri.to_string();
        self
    }

    /// With custom scopes.
    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }

    /// With a client secret.
    pub fn with_client_secret(mut self, secret: &str) -> Self {
        self.client_secret = Some(secret.to_string());
        self
    }
}

/// State for an in-progress OAuth authorization flow.
#[derive(Debug, Clone)]
pub struct OAuthState {
    /// Random state parameter for CSRF protection.
    pub state: String,
    /// Authorization URL to open in browser.
    pub authorization_url: String,
    /// Server this OAuth flow is for.
    pub server_name: String,
}

/// Generate a random state string for OAuth.
fn generate_state() -> String {
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let chars: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
        .chars()
        .collect();
    let mut state = String::with_capacity(32);
    let mut s = seed;
    for _ in 0..32 {
        let idx = (s as usize) % chars.len();
        state.push(chars[idx]);
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
    }
    state
}

/// MCP OAuth manager.
///
/// Handles the OAuth 2.0 authorization code flow for MCP servers:
/// 1. Generate authorization URL
/// 2. Wait for callback with authorization code
/// 3. Exchange code for access token
/// 4. Store and refresh tokens as needed
pub struct McpOAuthManager {
    /// OAuth configs per server.
    configs: Mutex<HashMap<String, McpOAuthConfig>>,
    /// Stored tokens per server.
    tokens: Mutex<HashMap<String, OAuthToken>>,
    /// Pending OAuth flows (state -> server_name).
    pending_flows: Mutex<HashMap<String, String>>,
    /// Callback receiver for authorization codes.
    code_rx: Mutex<Option<mpsc::Receiver<(String, String)>>>,
    /// Callback sender.
    code_tx: mpsc::Sender<(String, String)>,
}

impl McpOAuthManager {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<(String, String)>(32);
        Self {
            configs: Mutex::new(HashMap::new()),
            tokens: Mutex::new(HashMap::new()),
            pending_flows: Mutex::new(HashMap::new()),
            code_rx: Mutex::new(Some(rx)),
            code_tx: tx,
        }
    }

    /// Register an OAuth config for an MCP server.
    pub fn register_config(&self, server_name: &str, config: McpOAuthConfig) {
        self.configs.lock().insert(server_name.to_string(), config);
    }

    /// Generate an authorization URL for the given server.
    ///
    /// Returns the URL to open in a browser and the CSRF state.
    pub async fn begin_authorization(&self, server_name: &str) -> Result<OAuthState> {
        let (server_url, client_id, redirect_uri, scopes, cached_metadata) = {
            let configs = self.configs.lock();
            let config = configs.get(server_name)
                .ok_or_else(|| anyhow!("No OAuth config found for server: {server_name}"))?;
            (
                config.server_url.clone(),
                config.client_id.clone(),
                config.redirect_uri.clone(),
                config.scopes.clone(),
                config.metadata.clone(),
            )
        };

        // Fetch metadata if not already cached
        let metadata = if let Some(ref meta) = cached_metadata {
            meta.clone()
        } else {
            // Try to fetch from well-known URL
            match OAuthMetadata::fetch(&server_url).await {
                Ok(meta) => {
                    // Cache the discovered metadata
                    if let Some(cfg) = self.configs.lock().get_mut(server_name) {
                        cfg.metadata = Some(meta.clone());
                    }
                    meta
                }
                Err(_) => {
                    // Fall back to default paths
                    OAuthMetadata {
                        authorization_endpoint: format!("{}/authorize", server_url.trim_end_matches('/')),
                        token_endpoint: format!("{}/token", server_url.trim_end_matches('/')),
                        registration_endpoint: None,
                        response_types_supported: vec!["code".to_string()],
                        grant_types_supported: vec!["authorization_code".to_string()],
                    }
                }
            }
        };

        let state = generate_state();
        let scope = scopes.join(" ");

        let mut url = metadata.authorization_endpoint;
        url.push_str(&format!(
            "?response_type=code&client_id={}&redirect_uri={}&state={}&scope={}",
            url_encode(&client_id),
            url_encode(&redirect_uri),
            url_encode(&state),
            url_encode(&scope),
        ));

        // Store the pending flow
        self.pending_flows.lock().insert(state.clone(), server_name.to_string());

        Ok(OAuthState {
            state,
            authorization_url: url,
            server_name: server_name.to_string(),
        })
    }

    /// Handle an OAuth callback (authorization code received from browser).
    ///
    /// This exchanges the authorization code for an access token.
    pub async fn handle_callback(&self, code: &str, state: &str) -> Result<()> {
        // Verify the state matches a pending flow
        let server_name = {
            let mut flows = self.pending_flows.lock();
            flows.remove(state)
                .ok_or_else(|| anyhow!("Invalid OAuth state: no pending flow"))?
        };

        // Extract config fields, releasing the lock immediately
        let result: Option<(Option<String>, String, Option<String>, String)> = {
            let configs = self.configs.lock();
            configs.get(&server_name).map(|cfg| (
                cfg.metadata.as_ref().map(|m| m.token_endpoint.clone()),
                cfg.client_id.clone(),
                cfg.client_secret.clone(),
                cfg.redirect_uri.clone(),
            ))
        };

        let (token_endpoint, client_id, client_secret, redirect_uri) = match result {
            Some((Some(ep), cid, cs, ruri)) => (ep, cid, cs, ruri),
            Some((None, _, _, _)) => return Err(anyhow!("No OAuth metadata for server: {server_name}")),
            None => return Err(anyhow!("No OAuth config for server: {server_name}")),
        };

        let mut params = HashMap::new();
        params.insert("grant_type", "authorization_code".to_string());
        params.insert("code", code.to_string());
        params.insert("redirect_uri", redirect_uri);
        params.insert("client_id", client_id);
        if let Some(secret) = client_secret {
            params.insert("client_secret", secret);
        }

        let client = reqwest::Client::new();
        let resp = client.post(&token_endpoint)
            .form(&params)
            .send()
            .await
            .context("Failed to exchange authorization code for token")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Token exchange failed: {body}"));
        }

        let token_response: OAuthTokenResponse = resp.json().await
            .context("Failed to parse token response")?;

        let token = OAuthToken {
            access_token: token_response.access_token,
            token_type: token_response.token_type.unwrap_or_else(|| "Bearer".to_string()),
            refresh_token: token_response.refresh_token,
            expires_in: token_response.expires_in,
            scope: token_response.scope,
            obtained_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        self.tokens.lock().insert(server_name, token);
        Ok(())
    }

    /// Get a valid access token for a server, refreshing if needed.
    pub async fn get_token(&self, server_name: &str) -> Result<OAuthToken> {
        let tokens = self.tokens.lock();
        let token = tokens.get(server_name);

        // Return existing token if valid
        if let Some(t) = token {
            if !t.is_expired() {
                return Ok(t.clone());
            }
        }

        // Token expired or missing — try to refresh
        drop(tokens);
        self.refresh_token(server_name).await
    }

    /// Refresh an access token using the refresh token.
    async fn refresh_token(&self, server_name: &str) -> Result<OAuthToken> {
        let refresh_token: Option<String> = {
            let tokens = self.tokens.lock();
            tokens.get(server_name).and_then(|t| t.refresh_token.clone())
        };
        let refresh = refresh_token
            .ok_or_else(|| anyhow!("No refresh token available for server: {server_name}"))?;

        let configs = self.configs.lock();
        let config = configs.get(server_name).cloned();
        drop(configs);
        let config = config
            .ok_or_else(|| anyhow!("No OAuth config for server: {server_name}"))?;

        let metadata = config.metadata.clone()
            .ok_or_else(|| anyhow!("No OAuth metadata for server: {server_name}"))?;

        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token".to_string());
        params.insert("refresh_token", refresh);
        params.insert("client_id", config.client_id);
        if let Some(secret) = config.client_secret {
            params.insert("client_secret", secret);
        }

        let client = reqwest::Client::new();
        let resp = client.post(&metadata.token_endpoint)
            .form(&params)
            .send()
            .await
            .context("Failed to refresh token")?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Token refresh failed: {body}"));
        }

        let token_response: OAuthTokenResponse = resp.json().await
            .context("Failed to parse refresh token response")?;

        let token = OAuthToken {
            access_token: token_response.access_token,
            token_type: token_response.token_type.unwrap_or_else(|| "Bearer".to_string()),
            refresh_token: token_response.refresh_token.or_else(|| {
                // Keep the old refresh token if not returned
                self.tokens.lock().get(server_name)
                    .and_then(|t| t.refresh_token.clone())
            }),
            expires_in: token_response.expires_in,
            scope: token_response.scope,
            obtained_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        self.tokens.lock().insert(server_name.to_string(), token.clone());
        Ok(token)
    }

    /// Get the authorization header for a server (auto-refreshes if needed).
    pub async fn auth_header(&self, server_name: &str) -> Result<String> {
        let token = self.get_token(server_name).await?;
        Ok(token.auth_header())
    }

    /// Check if a server has a valid (non-expired) token.
    pub fn has_valid_token(&self, server_name: &str) -> bool {
        self.tokens.lock().get(server_name)
            .map(|t| !t.is_expired())
            .unwrap_or(false)
    }

    /// Remove stored tokens and pending flows for a server.
    pub fn revoke(&self, server_name: &str) {
        self.tokens.lock().remove(server_name);
        self.pending_flows.lock().retain(|_, v| v != server_name);
    }

    /// Get the receiver for OAuth callback codes.
    ///
    /// Call this once and spawn a task that feeds (code, state) pairs
    /// into the channel when the HTTP callback receives them.
    pub fn take_code_receiver(&self) -> Option<mpsc::Receiver<(String, String)>> {
        self.code_rx.lock().take()
    }

    /// Feed an authorization code into the OAuth manager.
    ///
    /// This should be called by the HTTP callback handler when
    /// the browser redirects back with the authorization code.
    pub async fn submit_code(&self, code: String, state: String) -> Result<()> {
        self.code_tx.send((code, state)).await
            .map_err(|_| anyhow!("OAuth manager dropped"))
    }
}

impl Default for McpOAuthManager {
    fn default() -> Self {
        Self::new()
    }
}

/// OAuth token response.
#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

fn url_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_token_not_expired() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: Some(3600),
            scope: None,
            obtained_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };
        assert!(!token.is_expired());
    }

    #[test]
    fn test_oauth_token_expired() {
        let token = OAuthToken {
            access_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: Some(1),
            scope: None,
            obtained_at: 0, // long ago
        };
        assert!(token.is_expired());
    }

    #[test]
    fn test_auth_header() {
        let token = OAuthToken {
            access_token: "abc123".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: None,
            scope: None,
            obtained_at: 0,
        };
        assert_eq!(token.auth_header(), "Bearer abc123");
    }

    #[test]
    fn test_oauth_config_builder() {
        let config = McpOAuthConfig::new("https://mcp.example.com", "client-123")
            .with_redirect_uri("http://localhost:8080/callback")
            .with_scopes(vec!["read".to_string()])
            .with_client_secret("secret-456");

        assert_eq!(config.server_url, "https://mcp.example.com");
        assert_eq!(config.client_id, "client-123");
        assert_eq!(config.redirect_uri, "http://localhost:8080/callback");
        assert_eq!(config.scopes, vec!["read".to_string()]);
        assert_eq!(config.client_secret, Some("secret-456".to_string()));
    }

    #[test]
    fn test_generate_state() {
        let s1 = generate_state();
        let s2 = generate_state();
        assert_eq!(s1.len(), 32);
        assert_eq!(s2.len(), 32);
        assert_ne!(s1, s2);
    }

    #[test]
    fn test_manager_no_config_returns_error() {
        let manager = McpOAuthManager::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(manager.begin_authorization("unknown"));
        assert!(result.is_err());
    }

    #[test]
    fn test_manager_revoke() {
        let manager = McpOAuthManager::new();
        manager.revoke("test-server");
        assert!(!manager.has_valid_token("test-server"));
    }

    #[test]
    fn test_expires_within() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let token = OAuthToken {
            access_token: "test".to_string(),
            token_type: "Bearer".to_string(),
            refresh_token: None,
            expires_in: Some(60),
            scope: None,
            obtained_at: now,
        };
        assert!(token.expires_within(120));
        assert!(!token.expires_within(10));
    }
}
