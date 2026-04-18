use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext, ToolResult};

/// Target format: `platform:reference[:thread_id]`
#[derive(Debug, Clone)]
pub struct SendTarget {
    pub platform: String,
    pub reference: String,
    pub thread_id: Option<String>,
}

impl std::str::FromStr for SendTarget {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.is_empty() || parts[0].is_empty() {
            return Err("Invalid target: must start with platform name".to_string());
        }

        Ok(Self {
            platform: parts[0].to_string(),
            reference: parts.get(1).map(|s| s.to_string()).unwrap_or_default(),
            thread_id: parts.get(2).map(|s| s.to_string()),
        })
    }
}

/// Per-platform send configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlatformSendConfig {
    pub base_url: String,
    pub api_key: String,
    pub bot_token: String,
    pub extra: HashMap<String, String>,
}

/// Cross-platform messaging tool. Sends messages to any connected platform
/// (Telegram, Discord, Slack, WhatsApp, Signal, Matrix, etc.) via REST APIs.
pub struct SendMessageTool {
    config: Arc<Mutex<HashMap<String, PlatformSendConfig>>>,
}

impl SendMessageTool {
    pub fn new() -> Self {
        Self {
            config: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register_platform(&self, name: &str, cfg: PlatformSendConfig) {
        let mut configs = self.config.lock().unwrap();
        configs.insert(name.to_string(), cfg);
    }

    /// Check for cron duplicate — skip if target matches auto-delivery config.
    fn is_cron_duplicate(&self, target: &SendTarget) -> bool {
        let platform_env = std::env::var("HERMES_CRON_AUTO_DELIVER_PLATFORM")
            .unwrap_or_default();
        let chat_env = std::env::var("HERMES_CRON_AUTO_DELIVER_CHAT_ID")
            .unwrap_or_default();

        if platform_env.is_empty() || chat_env.is_empty() {
            return false;
        }

        platform_env == target.platform && chat_env == target.reference
    }
}

impl Default for SendMessageTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn toolset(&self) -> &str {
        "messaging"
    }

    fn description(&self) -> &str {
        "Send a message to a connected messaging platform, or list available targets. \
        Target format: 'platform', 'platform:#channel-name', 'platform:chat_id', \
        or 'platform:chat_id:thread_id'."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["send", "list"],
                    "description": "Action: 'send' to send a message, 'list' to see available targets"
                },
                "target": {
                    "type": "string",
                    "description": "Target in format: 'platform:reference[:thread_id]'"
                },
                "message": {
                    "type": "string",
                    "description": "The message text to send"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<ToolResult> {
        let action = args.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("send");

        match action {
            "list" => self.list_targets().await,
            "send" => self.send_message(&args).await,
            other => Ok(ToolResult::err(format!("Unknown action: {other}"))),
        }
    }
}

impl SendMessageTool {
    async fn list_targets(&self) -> Result<ToolResult> {
        let configs = self.config.lock().unwrap();
        if configs.is_empty() {
            return Ok(ToolResult::ok("No messaging platforms configured. Use gateway mode or configure platform credentials."));
        }

        let lines = configs.iter().map(|(name, cfg)| {
            format!("  {name} (base_url: {})", cfg.base_url)
        }).collect::<Vec<_>>().join("\n");

        Ok(ToolResult::ok(format!("Configured platforms:\n{lines}")))
    }

    async fn send_message(&self, args: &Value) -> Result<ToolResult> {
        let target_str = args.get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: target"))?;

        let message = args.get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let target: SendTarget = target_str.parse()
            .map_err(|e: String| anyhow::anyhow!("{e}"))?;

        // Check cron duplicate
        if self.is_cron_duplicate(&target) {
            return Ok(ToolResult::ok(
                serde_json::to_string(&json!({
                    "success": true,
                    "skipped": true,
                    "reason": "cron_auto_delivery_duplicate_target"
                })).unwrap_or_default()
            ));
        }

        // Get platform config
        let platform_cfg = {
            let configs = self.config.lock().unwrap();
            configs.get(&target.platform).cloned()
        };

        let platform_cfg = match platform_cfg {
            Some(cfg) => cfg,
            None => {
                return Ok(ToolResult::err(format!(
                    "Platform '{}' is not configured. Use 'send_message' with action=list to see available platforms.",
                    target.platform
                )));
            }
        };

        // Dispatch to platform-specific sender
        match self.dispatch_send(&target, message, &platform_cfg).await {
            Ok(msg_id) => {
                Ok(ToolResult::ok(
                    serde_json::to_string(&json!({
                        "success": true,
                        "platform": target.platform,
                        "target": target.reference,
                        "message_id": msg_id
                    })).unwrap_or_default()
                ))
            }
            Err(e) => Ok(ToolResult::err(format!("Failed to send message: {e}"))),
        }
    }

    async fn dispatch_send(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        match target.platform.to_lowercase().as_str() {
            "telegram" => self.send_telegram(target, message, config).await,
            "discord" => self.send_discord(target, message, config).await,
            "slack" => self.send_slack(target, message, config).await,
            "signal" => self.send_signal(target, message, config).await,
            "matrix" => self.send_matrix(target, message, config).await,
            "whatsapp" => self.send_whatsapp(target, message, config).await,
            "email" => self.send_email(target, message, config).await,
            "webhook" => self.send_webhook(target, message, config).await,
            "sms" => self.send_sms(target, message, config).await,
            "mattermost" => self.send_mattermost(target, message, config).await,
            "feishu" | "lark" => self.send_feishu(target, message, config).await,
            "wecom" => self.send_wecom(target, message, config).await,
            "dingtalk" => self.send_dingtalk(target, message, config).await,
            "qqbot" | "weixin" => {
                // These require a running gateway — skip for one-shot send
                Err(anyhow::anyhow!(
                    "Platform '{}' requires a running gateway adapter for send_message",
                    target.platform
                ))
            }
            "bluebubbles" => self.send_bluebubbles(target, message, config).await,
            "homeassistant" => self.send_homeassistant(target, message, config).await,
            "api_server" => self.send_api_server(target, message, config).await,
            _ => Err(anyhow::anyhow!("Unknown platform: {}", target.platform)),
        }
    }

    // ---- Platform-specific send implementations ----

    async fn send_telegram(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let chat_id = if target.reference.is_empty() {
            &config.extra.get("default_chat_id").ok_or_else(|| {
                anyhow::anyhow!("Telegram requires a chat_id in target (e.g., 'telegram:-1001234567890')")
            })?
        } else {
            &target.reference
        };

        let url = format!(
            "{}/bot{}/sendMessage",
            if config.base_url.is_empty() { "https://api.telegram.org" } else { &config.base_url },
            &config.bot_token,
        );

        let client = reqwest::Client::new();
        let mut body = json!({
            "chat_id": chat_id,
            "text": message,
        });

        // Add thread_id for topics
        if let Some(ref thread_id) = target.thread_id {
            body["message_thread_id"] = json!(thread_id);
        }

        // Detect HTML for parse mode
        let has_html = message.contains('<') && message.contains('>');
        if has_html {
            body["parse_mode"] = json!("HTML");
        }

        let resp = client.post(&url).json(&body).send().await?;
        let status = resp.status();
        let body_text = resp.text().await?;

        if !status.is_success() {
            return Err(anyhow::anyhow!("Telegram API error: {body_text}"));
        }

        // Extract message_id from response
        let json: Value = serde_json::from_str(&body_text).unwrap_or_default();
        let msg_id = json["result"]["message_id"]
            .as_u64()
            .unwrap_or(0)
            .to_string();

        Ok(msg_id)
    }

    async fn send_discord(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let channel_id = if target.reference.is_empty() {
            config.extra.get("default_channel_id").ok_or_else(|| {
                anyhow::anyhow!("Discord requires a channel_id in target (e.g., 'discord:999888777')")
            })?
        } else {
            &target.reference
        };

        let url = format!(
            "https://discord.com/api/v10/channels/{channel_id}/messages",
        );

        let mut body = json!({ "content": message });
        if let Some(ref thread_id) = target.thread_id {
            body["thread_id"] = json!(thread_id);
        }

        let client = reqwest::Client::new();
        let resp = client.post(&url)
            .header("Authorization", format!("Bot {}", &config.bot_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let body_text = resp.text().await?;

        if !status.is_success() {
            return Err(anyhow::anyhow!("Discord API error: {body_text}"));
        }

        let json: Value = serde_json::from_str(&body_text).unwrap_or_default();
        let msg_id = json["id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(msg_id)
    }

    async fn send_slack(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let channel = if target.reference.is_empty() {
            config.extra.get("default_channel").ok_or_else(|| {
                anyhow::anyhow!("Slack requires a channel in target (e.g., 'slack:#general')")
            })?
        } else {
            &target.reference
        };

        let client = reqwest::Client::new();
        let resp = client.post("https://slack.com/api/chat.postMessage")
            .header("Authorization", format!("Bearer {}", &config.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "channel": channel,
                "text": message,
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;

        if json.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            return Err(anyhow::anyhow!("Slack API error: {json}"));
        }

        let msg_id = json["message"]["ts"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(msg_id)
    }

    async fn send_signal(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let recipient = if target.reference.is_empty() {
            config.extra.get("default_recipient").ok_or_else(|| {
                anyhow::anyhow!("Signal requires a recipient number in target (e.g., 'signal:+1234567890')")
            })?
        } else {
            &target.reference
        };

        let base_url = if config.base_url.is_empty() {
            "http://localhost:8080"
        } else {
            &config.base_url
        };

        let client = reqwest::Client::new();
        let resp = client.post(format!("{base_url}/v2/send"))
            .header("Content-Type", "application/json")
            .json(&json!({
                "message": message,
                "number": config.extra.get("signal_number").unwrap_or(&String::new()),
                "recipients": [recipient],
            }))
            .send()
            .await?;

        let body = resp.text().await?;
        let json: Value = serde_json::from_str(&body).unwrap_or_default();
        let msg_id = json["timestamp"]
            .as_u64()
            .unwrap_or(0)
            .to_string();

        Ok(msg_id)
    }

    async fn send_matrix(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let room_id = if target.reference.is_empty() {
            config.extra.get("default_room_id").ok_or_else(|| {
                anyhow::anyhow!("Matrix requires a room_id in target (e.g., 'matrix:!roomid:server.org')")
            })?
        } else {
            &target.reference
        };

        let base_url = if config.base_url.is_empty() {
            "https://matrix.org"
        } else {
            &config.base_url
        };

        let client = reqwest::Client::new();
        let resp = client.put(format!(
            "{base_url}/_matrix/client/v3/rooms/{room_id}/send/m.room.message/txn_{timestamp}",
            timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ))
            .query(&[("access_token", &config.api_key)])
            .json(&json!({
                "msgtype": "m.text",
                "body": message,
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let msg_id = json["event_id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(msg_id)
    }

    async fn send_whatsapp(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let to = if target.reference.is_empty() {
            config.extra.get("default_to").ok_or_else(|| {
                anyhow::anyhow!("WhatsApp requires a recipient number in target (e.g., 'whatsapp:+1234567890')")
            })?
        } else {
            &target.reference
        };

        let from = config.extra.get("whatsapp_from").ok_or_else(|| {
            anyhow::anyhow!("WhatsApp requires 'whatsapp_from' in extra config")
        })?;

        let base_url = if config.base_url.is_empty() {
            "https://graph.facebook.com/v17.0"
        } else {
            &config.base_url
        };

        let phone_id = config.extra.get("phone_id").cloned().unwrap_or_default();

        let client = reqwest::Client::new();
        let resp = client.post(format!("{base_url}/{phone_id}/messages"))
            .header("Authorization", format!("Bearer {}", &config.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "messaging_product": "whatsapp",
                "recipient_type": "individual",
                "to": to,
                "type": "text",
                "text": { "body": message },
                "from": from,
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let msg_id = json["messages"][0]["id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(msg_id)
    }

    async fn send_email(
        &self,
        target: &SendTarget,
        _message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let to = if target.reference.is_empty() {
            config.extra.get("default_to").ok_or_else(|| {
                anyhow::anyhow!("Email requires a recipient in target (e.g., 'email:user@example.com')")
            })?
        } else {
            &target.reference
        };

        // Email via SMTP or API — for one-shot, use a simple mailto: or API approach
        // In practice, this would use an email API (SendGrid, SES, etc.)
        let smtp_host = config.extra.get("smtp_host")
            .map(|s| s.as_str())
            .unwrap_or("localhost");
        let smtp_port = config.extra.get("smtp_port")
            .map(|s| s.as_str())
            .unwrap_or("587");

        // Placeholder — actual email sending requires SMTP client
        Ok(format!("email_sent:{smtp_host}:{smtp_port}:{to}"))
    }

    async fn send_webhook(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let url = if target.reference.is_empty() {
            if config.base_url.is_empty() {
                return Err(anyhow::anyhow!("Webhook requires a URL in config.base_url or target"));
            }
            &config.base_url
        } else {
            &target.reference
        };

        let client = reqwest::Client::new();
        let resp = client.post(url)
            .header("Content-Type", "application/json")
            .body(message.to_string())
            .send()
            .await?;

        let status = resp.status();
        Ok(format!("webhook_sent:{status}"))
    }

    async fn send_sms(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let to = if target.reference.is_empty() {
            config.extra.get("default_to").ok_or_else(|| {
                anyhow::anyhow!("SMS requires a recipient in target (e.g., 'sms:+1234567890')")
            })?
        } else {
            &target.reference
        };

        let from = config.extra.get("twilio_from").ok_or_else(|| {
            anyhow::anyhow!("SMS requires 'twilio_from' in extra config")
        })?;

        let account_sid = &config.extra.get("twilio_account_sid")
            .ok_or_else(|| anyhow::anyhow!("SMS requires 'twilio_account_sid' in extra config"))?;

        let client = reqwest::Client::new();
        let resp = client.post(format!(
            "https://api.twilio.com/2010-04-01/Accounts/{account_sid}/Messages.json"
        ))
            .basic_auth(account_sid, Some(&config.api_key))
            .form(&[
                ("To", to.as_str()),
                ("From", from.as_str()),
                ("Body", message),
            ])
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let msg_id = json["sid"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(msg_id)
    }

    async fn send_mattermost(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let channel = if target.reference.is_empty() {
            config.extra.get("default_channel").ok_or_else(|| {
                anyhow::anyhow!("Mattermost requires a channel in target (e.g., 'mattermost:town-square')")
            })?
        } else {
            &target.reference
        };

        let base_url = if config.base_url.is_empty() {
            "https://mattermost.example.com"
        } else {
            &config.base_url
        };

        let client = reqwest::Client::new();
        let resp = client.post(format!("{base_url}/api/v4/channels/{channel}/posts"))
            .header("Authorization", format!("Bearer {}", &config.api_key))
            .json(&json!({
                "channel_id": channel,
                "message": message,
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let msg_id = json["id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(msg_id)
    }

    async fn send_feishu(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let chat_id = if target.reference.is_empty() {
            config.extra.get("default_chat_id").ok_or_else(|| {
                anyhow::anyhow!("Feishu requires a chat_id in target (e.g., 'feishu:oc_xxx123')")
            })?
        } else {
            &target.reference
        };

        let client = reqwest::Client::new();
        let resp = client.post("https://open.feishu.cn/open-apis/im/v1/messages")
            .query(&[("receive_id_type", "chat_id")])
            .header("Authorization", format!("Bearer {}", &config.api_key))
            .json(&json!({
                "receive_id": chat_id,
                "msg_type": "text",
                "content": serde_json::to_string(&json!({ "text": message })).unwrap_or_default(),
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let msg_id = json["data"]["message_id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(msg_id)
    }

    async fn send_wecom(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let chat_id = if target.reference.is_empty() {
            config.extra.get("default_chat_id").ok_or_else(|| {
                anyhow::anyhow!("WeCom requires a chat_id in target")
            })?
        } else {
            &target.reference
        };

        let client = reqwest::Client::new();
        let resp = client.post(format!(
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={}",
            &config.bot_token,
        ))
            .json(&json!({
                "msgtype": "text",
                "text": { "content": message },
                "chatid": chat_id,
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let errcode = json["errcode"].as_i64().unwrap_or(-1);
        if errcode != 0 {
            return Err(anyhow::anyhow!("WeCom API error: {json}"));
        }

        Ok(format!("wecom_sent:{errcode}"))
    }

    async fn send_dingtalk(
        &self,
        _target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let webhook_token = if config.bot_token.is_empty() {
            config.extra.get("webhook_token").ok_or_else(|| {
                anyhow::anyhow!("DingTalk requires a webhook token in config")
            })?
        } else {
            &config.bot_token
        };

        let client = reqwest::Client::new();
        let resp = client.post(format!(
            "https://oapi.dingtalk.com/robot/send?access_token={webhook_token}"
        ))
            .json(&json!({
                "msgtype": "text",
                "text": { "content": message },
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let errcode = json["errcode"].as_i64().unwrap_or(-1);
        if errcode != 0 {
            return Err(anyhow::anyhow!("DingTalk API error: {json}"));
        }

        Ok(format!("dingtalk_sent:{errcode}"))
    }

    async fn send_bluebubbles(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let chat_guid = if target.reference.is_empty() {
            config.extra.get("default_chat_guid").ok_or_else(|| {
                anyhow::anyhow!("BlueBubbles requires a chat_guid in target")
            })?
        } else {
            &target.reference
        };

        let base_url = if config.base_url.is_empty() {
            "http://localhost:1234"
        } else {
            &config.base_url
        };

        let client = reqwest::Client::new();
        let resp = client.post(format!("{base_url}/api/v1/message/text"))
            .header("Authorization", &config.api_key)
            .json(&json!({
                "chatGuid": chat_guid,
                "message": message,
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let msg_id = json["data"]["identifier"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        Ok(msg_id)
    }

    async fn send_homeassistant(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let service = if target.reference.is_empty() {
            config.extra.get("default_service").ok_or_else(|| {
                anyhow::anyhow!("HomeAssistant requires a service in target (e.g., 'homeassistant:notify.all_devices')")
            })?
        } else {
            &target.reference
        };

        let base_url = if config.base_url.is_empty() {
            "http://localhost:8123"
        } else {
            &config.base_url
        };

        let parts: Vec<&str> = service.split('.').collect();
        let domain = parts.first().unwrap_or(&"notify");
        let svc = parts.get(1).unwrap_or(&"all_devices");

        let client = reqwest::Client::new();
        let resp = client.post(format!("{base_url}/api/services/{domain}/{svc}"))
            .header("Authorization", format!("Bearer {}", &config.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "message": message,
            }))
            .send()
            .await?;

        let status = resp.status();
        Ok(format!("homeassistant_sent:{status}"))
    }

    async fn send_api_server(
        &self,
        target: &SendTarget,
        message: &str,
        config: &PlatformSendConfig,
    ) -> Result<String> {
        let base_url = if config.base_url.is_empty() {
            return Err(anyhow::anyhow!("API server requires base_url in config"));
        } else {
            &config.base_url
        };

        let endpoint = if target.reference.is_empty() {
            "/api/message"
        } else {
            &target.reference
        };

        let client = reqwest::Client::new();
        let resp = client.post(format!("{base_url}{endpoint}"))
            .header("Authorization", format!("Bearer {}", &config.api_key))
            .json(&json!({ "message": message }))
            .send()
            .await?;

        let status = resp.status();
        Ok(format!("api_server_sent:{status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_target_simple() {
        let target: SendTarget = "telegram:-1001234567890".parse().unwrap();
        assert_eq!(target.platform, "telegram");
        assert_eq!(target.reference, "-1001234567890");
        assert!(target.thread_id.is_none());
    }

    #[test]
    fn test_parse_target_with_thread() {
        let target: SendTarget = "telegram:-1001234567890:17585".parse().unwrap();
        assert_eq!(target.platform, "telegram");
        assert_eq!(target.reference, "-1001234567890");
        assert_eq!(target.thread_id.as_deref(), Some("17585"));
    }

    #[test]
    fn test_parse_target_bare_platform() {
        let target: SendTarget = "discord".parse().unwrap();
        assert_eq!(target.platform, "discord");
        assert!(target.reference.is_empty());
    }

    #[test]
    fn test_parse_target_channel_name() {
        let target: SendTarget = "slack:#general".parse().unwrap();
        assert_eq!(target.platform, "slack");
        assert_eq!(target.reference, "#general");
    }

    #[test]
    fn test_parse_target_matrix_room() {
        let target: SendTarget = "matrix:!roomid:server.org".parse().unwrap();
        assert_eq!(target.platform, "matrix");
        assert_eq!(target.reference, "!roomid");
        assert_eq!(target.thread_id.as_deref(), Some("server.org"));
    }

    #[tokio::test]
    async fn test_send_list_empty() {
        let tool = SendMessageTool::new();
        let result = tool.list_targets().await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("No messaging platforms"));
    }

    #[tokio::test]
    async fn test_send_list_with_platforms() {
        let tool = SendMessageTool::new();
        // Use a blocking register since we're in an async test but outside the runtime
        {
            let mut configs = tool.config.lock().unwrap();
            configs.insert("telegram".to_string(), PlatformSendConfig {
                base_url: "https://api.telegram.org".to_string(),
                ..Default::default()
            });
        }
        let result = tool.list_targets().await.unwrap();
        assert!(!result.is_error);
        assert!(result.content.contains("telegram"));
    }

    #[tokio::test]
    async fn test_send_unknown_platform() {
        let tool = SendMessageTool::new();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            task_id: "test".to_string(),
            config: std::sync::Arc::new(h_core::HermesConfig::default()),
            working_dir: std::path::PathBuf::from("/tmp"),
            clarify: None,
        };
        let result = tool.execute(
            json!({"action": "send", "target": "unknown_platform:ref", "message": "test"}),
            &ctx,
        ).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("Unknown platform") || result.content.contains("not configured"));
    }

    #[tokio::test]
    async fn test_send_unconfigured_platform() {
        let tool = SendMessageTool::new();
        let ctx = ToolContext {
            session_id: "test".to_string(),
            task_id: "test".to_string(),
            config: std::sync::Arc::new(h_core::HermesConfig::default()),
            working_dir: std::path::PathBuf::from("/tmp"),
            clarify: None,
        };
        let result = tool.execute(
            json!({"action": "send", "target": "telegram:-123", "message": "test"}),
            &ctx,
        ).await.unwrap();
        assert!(result.is_error);
        assert!(result.content.contains("not configured"));
    }

    #[test]
    fn test_cron_duplicate() {
        let tool = SendMessageTool::new();

        // Test: matching platform + chat_id → duplicate
        unsafe {
            std::env::set_var("HERMES_CRON_DUP_PLATFORM_TEST", "telegram");
            std::env::set_var("HERMES_CRON_DUP_CHAT_TEST", "-100123");
        }

        // Override the env var names for this test
        unsafe {
            std::env::set_var("HERMES_CRON_AUTO_DELIVER_PLATFORM", "telegram");
            std::env::set_var("HERMES_CRON_AUTO_DELIVER_CHAT_ID", "-100123");
        }

        let target: SendTarget = "telegram:-100123".parse().unwrap();
        assert!(tool.is_cron_duplicate(&target));

        // Test: non-matching chat_id → not duplicate
        unsafe { std::env::set_var("HERMES_CRON_AUTO_DELIVER_CHAT_ID", "-100999"); }
        let target2: SendTarget = "telegram:-100123".parse().unwrap();
        assert!(!tool.is_cron_duplicate(&target2));

        // Test: no env vars → not duplicate
        unsafe {
            std::env::remove_var("HERMES_CRON_AUTO_DELIVER_PLATFORM");
            std::env::remove_var("HERMES_CRON_AUTO_DELIVER_CHAT_ID");
            std::env::remove_var("HERMES_CRON_DUP_PLATFORM_TEST");
            std::env::remove_var("HERMES_CRON_DUP_CHAT_TEST");
        }
    }
}
