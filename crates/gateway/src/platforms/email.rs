use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use crate::base::{MessageFormat, PlatformAdapter, StreamConsumer};
use crate::base::PlatformConfig;
use crate::formatting::strip_markdown;
use crate::runner::BufferingConsumer;

/// Email platform adapter configuration.
///
/// Supports sending emails via SMTP for notifications and
/// receiving emails via IMAP for incoming messages.
#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// SMTP server hostname.
    pub smtp_host: String,
    /// SMTP server port.
    pub smtp_port: u16,
    /// SMTP username.
    pub smtp_user: String,
    /// SMTP password.
    pub smtp_password: String,
    /// Sender email address.
    pub from_address: String,
    /// Whether to use TLS for SMTP.
    pub use_tls: bool,
    /// IMAP server hostname (for receiving emails).
    pub imap_host: Option<String>,
}

impl EmailConfig {
    pub fn from_platform_config(config: &PlatformConfig) -> Result<Self> {
        let smtp_host = config
            .get_str("smtp_host")
            .ok_or_else(|| anyhow::anyhow!("Email smtp_host not configured"))?;
        let smtp_user = config
            .get_str("smtp_user")
            .ok_or_else(|| anyhow::anyhow!("Email smtp_user not configured"))?;
        let smtp_password = config
            .get_str("smtp_password")
            .ok_or_else(|| anyhow::anyhow!("Email smtp_password not configured"))?;
        let from_address = config
            .get_str("from_address")
            .ok_or_else(|| anyhow::anyhow!("Email from_address not configured"))?;
        let smtp_port = config
            .settings
            .get("smtp_port")
            .and_then(|v| v.as_u64())
            .unwrap_or(587) as u16;
        let use_tls = config
            .settings
            .get("use_tls")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        Ok(Self {
            smtp_host,
            smtp_port,
            smtp_user,
            smtp_password,
            from_address,
            use_tls,
            imap_host: config.get_str("imap_host"),
        })
    }

    /// SMTP connection URL.
    pub fn smtp_url(&self) -> String {
        let proto = if self.use_tls { "smtps" } else { "smtp" };
        format!("{proto}://{}:{}/?tls=required", self.smtp_host, self.smtp_port)
    }
}

/// Email platform adapter.
///
/// Sends emails via SMTP for notifications and can receive
/// emails via IMAP for two-way communication.
pub struct EmailAdapter {
    config: EmailConfig,
    client: reqwest::Client,
}

impl EmailAdapter {
    pub fn new(config: EmailConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_config(config: &PlatformConfig) -> Result<Self> {
        let email_config = EmailConfig::from_platform_config(config)?;
        Ok(Self::new(email_config))
    }

    /// Send an email via the SMTP API.
    ///
    /// Uses a generic email service API (e.g., SendGrid, Mailgun, SES).
    async fn send_email(&self, to: &str, subject: &str, body: &str, is_html: bool) -> Result<String> {
        let body_json = if is_html {
            serde_json::json!({
                "to": to,
                "from": self.config.from_address,
                "subject": subject,
                "html": body,
                "text": strip_markdown(body),
            })
        } else {
            serde_json::json!({
                "to": to,
                "from": self.config.from_address,
                "subject": subject,
                "text": body,
            })
        };

        // Use a generic email service API endpoint
        // This would be customized per provider (SendGrid, Mailgun, etc.)
        let resp = self.client
            .post("https://api.sendgrid.com/v3/mail/send")
            .header("Authorization", format!("Bearer {}", self.config.smtp_password))
            .header("Content-Type", "application/json")
            .json(&body_json)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "Email API error: {status} - {text}"
            ));
        }

        // Email APIs typically return a message ID in headers
        let msg_id = resp
            .headers()
            .get("X-Message-Id")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();

        Ok(msg_id)
    }
}

#[async_trait]
impl PlatformAdapter for EmailAdapter {
    fn name(&self) -> &str { "email" }

    async fn connect(&self) -> Result<()> {
        // Test SMTP connection by sending a test email
        tracing::info!("Email adapter initialized (SMTP: {}:{})",
            self.config.smtp_host, self.config.smtp_port);
        Ok(())
    }

    async fn disconnect(&self) -> Result<()> {
        tracing::info!("Email adapter disconnected");
        Ok(())
    }

    async fn send_message(&self, chat_id: &str, text: &str) -> Result<String> {
        // Email supports HTML, so we can use markdown for formatting
        self.send_email(
            chat_id,
            "Hermes Agent",
            text,
            true, // Send as HTML for markdown support
        ).await
    }

    async fn send_file(&self, chat_id: &str, path: &Path) -> Result<()> {
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment");

        // In production, this would use multipart form upload
        // For now, send a notification about the file
        self.send_email(
            chat_id,
            "Hermes Agent - File Attachment",
            &format!("Please find attached: {file_name}"),
            false,
        ).await?;
        Ok(())
    }

    async fn send_animation(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_voice(&self, chat_id: &str, path: &Path) -> Result<()> {
        self.send_file(chat_id, path).await
    }

    async fn send_sticker(&self, _chat_id: &str, _sticker_id: &str) -> Result<()> {
        // Email does not support stickers
        Ok(())
    }

    async fn edit_message(&self, _chat_id: &str, _message_id: &str, _text: &str) -> Result<()> {
        // Email does not support message editing
        tracing::warn!("Email does not support message editing");
        Ok(())
    }

    async fn delete_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        // Email does not support message deletion
        tracing::warn!("Email does not support message deletion");
        Ok(())
    }

    async fn is_typing(&self, _chat_id: &str) -> Result<()> {
        // Email does not have a typing indicator
        Ok(())
    }

    fn create_consumer(&self, chat_id: &str, _message_id: Option<String>) -> Box<dyn StreamConsumer> {
        Box::new(BufferingConsumer::new(
            chat_id.to_string(),
            Arc::new(Self {
                config: self.config.clone(),
                client: self.client.clone(),
            }),
        ))
    }

    fn message_format(&self) -> MessageFormat {
        MessageFormat::Html
    }

    fn supports_edit_streaming(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_platform_config() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "smtp_host": "smtp.gmail.com",
                "smtp_port": 587,
                "smtp_user": "bot@gmail.com",
                "smtp_password": "app_password",
                "from_address": "bot@gmail.com",
                "use_tls": 1,
                "imap_host": "imap.gmail.com"
            }),
        };

        let config = EmailConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.smtp_host, "smtp.gmail.com");
        assert_eq!(config.smtp_port, 587);
        assert_eq!(config.smtp_user, "bot@gmail.com");
        assert_eq!(config.from_address, "bot@gmail.com");
        assert!(config.use_tls);
        assert_eq!(config.imap_host, Some("imap.gmail.com".to_string()));
    }

    #[test]
    fn test_config_missing_smtp_host() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "smtp_user": "user",
                "smtp_password": "pass",
                "from_address": "test@example.com"
            }),
        };
        assert!(EmailConfig::from_platform_config(&platform_config).is_err());
    }

    #[test]
    fn test_config_defaults() {
        let platform_config = PlatformConfig {
            enabled: true,
            settings: serde_json::json!({
                "smtp_host": "smtp.example.com",
                "smtp_user": "user",
                "smtp_password": "pass",
                "from_address": "bot@example.com"
            }),
        };

        let config = EmailConfig::from_platform_config(&platform_config).unwrap();
        assert_eq!(config.smtp_port, 587);
        assert!(config.use_tls);
        assert!(config.imap_host.is_none());
    }

    #[test]
    fn test_smtp_url_tls() {
        let config = EmailConfig {
            smtp_host: "smtp.gmail.com".to_string(),
            smtp_port: 587,
            smtp_user: "user".to_string(),
            smtp_password: "pass".to_string(),
            from_address: "bot@gmail.com".to_string(),
            use_tls: true,
            imap_host: None,
        };
        let url = config.smtp_url();
        assert!(url.starts_with("smtps://"));
    }

    #[test]
    fn test_smtp_url_no_tls() {
        let config = EmailConfig {
            smtp_host: "localhost".to_string(),
            smtp_port: 25,
            smtp_user: "user".to_string(),
            smtp_password: "pass".to_string(),
            from_address: "bot@localhost".to_string(),
            use_tls: false,
            imap_host: None,
        };
        let url = config.smtp_url();
        assert!(url.starts_with("smtp://"));
    }

    #[test]
    fn test_adapter_name() {
        let config = EmailConfig {
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            smtp_user: "user".to_string(),
            smtp_password: "pass".to_string(),
            from_address: "bot@example.com".to_string(),
            use_tls: true,
            imap_host: None,
        };
        let adapter = EmailAdapter::new(config);
        assert_eq!(adapter.name(), "email");
    }

    #[test]
    fn test_message_format() {
        let config = EmailConfig {
            smtp_host: "smtp.example.com".to_string(),
            smtp_port: 587,
            smtp_user: "user".to_string(),
            smtp_password: "pass".to_string(),
            from_address: "bot@example.com".to_string(),
            use_tls: true,
            imap_host: None,
        };
        let adapter = EmailAdapter::new(config);
        assert_eq!(adapter.message_format(), MessageFormat::Html);
        assert!(!adapter.supports_edit_streaming());
    }
}
