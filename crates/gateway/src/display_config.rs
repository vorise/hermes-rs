/// Platform-specific display configuration.
///
/// Defines how messages, emojis, stickers, and media should be rendered
/// on each platform, including length limits and feature flags.
#[derive(Debug, Clone)]
pub struct DisplayConfig {
    /// Platform name.
    pub platform: String,
    /// Whether Markdown formatting is supported.
    pub supports_markdown: bool,
    /// Whether HTML formatting is supported.
    pub supports_html: bool,
    /// Whether embeds (rich previews) are supported.
    pub supports_embeds: bool,
    /// Whether file attachments are supported.
    pub supports_files: bool,
    /// Whether animations/GIFs are supported.
    pub supports_animations: bool,
    /// Whether voice messages are supported.
    pub supports_voice: bool,
    /// Whether stickers/emoji are supported.
    pub supports_stickers: bool,
    /// Whether message editing is supported.
    pub supports_editing: bool,
    /// Whether typing indicators are supported.
    pub supports_typing: bool,
    /// Maximum message length (characters).
    pub max_message_length: usize,
    /// Maximum file size (bytes). None for unlimited.
    pub max_file_size: Option<usize>,
    /// Whether the platform uses Mrkdwn (Slack-style) formatting.
    pub uses_mrkdwn: bool,
}

impl DisplayConfig {
    /// Get display config for a known platform.
    pub fn for_platform(platform: &str) -> Self {
        match platform {
            "telegram" => Self {
                platform: "telegram".to_string(),
                supports_markdown: true,
                supports_html: true,
                supports_embeds: true,
                supports_files: true,
                supports_animations: true,
                supports_voice: true,
                supports_stickers: true,
                supports_editing: true,
                supports_typing: true,
                max_message_length: 4_096,
                max_file_size: Some(50 * 1024 * 1024), // 50MB
                uses_mrkdwn: false,
            },
            "discord" => Self {
                platform: "discord".to_string(),
                supports_markdown: true,
                supports_html: false,
                supports_embeds: true,
                supports_files: true,
                supports_animations: true,
                supports_voice: true,
                supports_stickers: true,
                supports_editing: true,
                supports_typing: true,
                max_message_length: 2_000,
                max_file_size: Some(25 * 1024 * 1024), // 25MB (8MB free, 25MB Nitro)
                uses_mrkdwn: false,
            },
            "slack" => Self {
                platform: "slack".to_string(),
                supports_markdown: true,
                supports_html: false,
                supports_embeds: true,
                supports_files: true,
                supports_animations: true,
                supports_voice: false,
                supports_stickers: false,
                supports_editing: true,
                supports_typing: true,
                max_message_length: 40_000,
                max_file_size: Some(1024 * 1024 * 1024), // 1GB
                uses_mrkdwn: true,
            },
            "whatsapp" => Self {
                platform: "whatsapp".to_string(),
                supports_markdown: true,
                supports_html: false,
                supports_embeds: false,
                supports_files: true,
                supports_animations: true,
                supports_voice: true,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: true,
                max_message_length: 65_536,
                max_file_size: Some(16 * 1024 * 1024), // 16MB
                uses_mrkdwn: false,
            },
            "signal" => Self {
                platform: "signal".to_string(),
                supports_markdown: true,
                supports_html: false,
                supports_embeds: false,
                supports_files: true,
                supports_animations: false,
                supports_voice: true,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: true,
                max_message_length: 2_000,
                max_file_size: Some(100 * 1024 * 1024), // 100MB
                uses_mrkdwn: false,
            },
            "matrix" => Self {
                platform: "matrix".to_string(),
                supports_markdown: true,
                supports_html: true,
                supports_embeds: false,
                supports_files: true,
                supports_animations: true,
                supports_voice: true,
                supports_stickers: true,
                supports_editing: true,
                supports_typing: true,
                max_message_length: 65_536,
                max_file_size: None, // Depends on homeserver
                uses_mrkdwn: false,
            },
            "mattermost" => Self {
                platform: "mattermost".to_string(),
                supports_markdown: true,
                supports_html: false,
                supports_embeds: true,
                supports_files: true,
                supports_animations: true,
                supports_voice: false,
                supports_stickers: false,
                supports_editing: true,
                supports_typing: true,
                max_message_length: 16_383,
                max_file_size: Some(100 * 1024 * 1024), // 100MB
                uses_mrkdwn: false,
            },
            "homeassistant" => Self {
                platform: "homeassistant".to_string(),
                supports_markdown: false,
                supports_html: false,
                supports_embeds: false,
                supports_files: false,
                supports_animations: false,
                supports_voice: false,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: false,
                max_message_length: 255, // Notification title limit
                max_file_size: None,
                uses_mrkdwn: false,
            },
            "webhook" => Self {
                platform: "webhook".to_string(),
                supports_markdown: true,
                supports_html: false,
                supports_embeds: false,
                supports_files: false,
                supports_animations: false,
                supports_voice: false,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: false,
                max_message_length: 65_536,
                max_file_size: None,
                uses_mrkdwn: false,
            },
            "sms" => Self {
                platform: "sms".to_string(),
                supports_markdown: false,
                supports_html: false,
                supports_embeds: false,
                supports_files: false,
                supports_animations: false,
                supports_voice: false,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: false,
                max_message_length: 1_600, // Concatenated SMS limit
                max_file_size: None,
                uses_mrkdwn: false,
            },
            "email" => Self {
                platform: "email".to_string(),
                supports_markdown: true,
                supports_html: true,
                supports_embeds: true,
                supports_files: true,
                supports_animations: true,
                supports_voice: true,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: false,
                max_message_length: 0, // No practical limit
                max_file_size: Some(25 * 1024 * 1024), // 25MB typical email limit
                uses_mrkdwn: false,
            },
            "bluebubbles" => Self {
                platform: "bluebubbles".to_string(),
                supports_markdown: false,
                supports_html: false,
                supports_embeds: false,
                supports_files: true,
                supports_animations: true,
                supports_voice: true,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: true,
                max_message_length: 0, // iMessage has no practical limit
                max_file_size: None,
                uses_mrkdwn: false,
            },
            "dingtalk" => Self {
                platform: "dingtalk".to_string(),
                supports_markdown: true,
                supports_html: false,
                supports_embeds: false,
                supports_files: true,
                supports_animations: false,
                supports_voice: false,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: false,
                max_message_length: 20_000,
                max_file_size: Some(20 * 1024 * 1024), // 20MB
                uses_mrkdwn: false,
            },
            "feishu" => Self {
                platform: "feishu".to_string(),
                supports_markdown: true,
                supports_html: false,
                supports_embeds: true,
                supports_files: true,
                supports_animations: true,
                supports_voice: false,
                supports_stickers: false,
                supports_editing: true,
                supports_typing: false,
                max_message_length: 20_000,
                max_file_size: Some(50 * 1024 * 1024), // 50MB
                uses_mrkdwn: false,
            },
            "qqbot" => Self {
                platform: "qqbot".to_string(),
                supports_markdown: false,
                supports_html: false,
                supports_embeds: true,
                supports_files: true,
                supports_animations: true,
                supports_voice: true,
                supports_stickers: false,
                supports_editing: true,
                supports_typing: false,
                max_message_length: 4_096,
                max_file_size: Some(25 * 1024 * 1024), // 25MB
                uses_mrkdwn: false,
            },
            "wecom" => Self {
                platform: "wecom".to_string(),
                supports_markdown: false,
                supports_html: false,
                supports_embeds: false,
                supports_files: true,
                supports_animations: false,
                supports_voice: true,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: false,
                max_message_length: 4_096,
                max_file_size: Some(20 * 1024 * 1024), // 20MB
                uses_mrkdwn: false,
            },
            "weixin" => Self {
                platform: "weixin".to_string(),
                supports_markdown: false,
                supports_html: false,
                supports_embeds: false,
                supports_files: true,
                supports_animations: false,
                supports_voice: true,
                supports_stickers: false,
                supports_editing: false,
                supports_typing: false,
                max_message_length: 2_048,
                max_file_size: Some(10 * 1024 * 1024), // 10MB
                uses_mrkdwn: false,
            },
            "api_server" => Self {
                platform: "api_server".to_string(),
                supports_markdown: false,
                supports_html: false,
                supports_embeds: false,
                supports_files: true,
                supports_animations: true,
                supports_voice: true,
                supports_stickers: false,
                supports_editing: true,
                supports_typing: false,
                max_message_length: 65_536,
                max_file_size: None,
                uses_mrkdwn: false,
            },
            _ => Self::default(),
        }
    }

    /// Get the appropriate message format for this platform.
    pub fn message_format(&self) -> &'static str {
        if self.uses_mrkdwn {
            "mrkdwn"
        } else if self.supports_html {
            "html"
        } else if self.supports_markdown {
            "markdown"
        } else {
            "plain"
        }
    }
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            platform: "unknown".to_string(),
            supports_markdown: true,
            supports_html: false,
            supports_embeds: false,
            supports_files: true,
            supports_animations: false,
            supports_voice: false,
            supports_stickers: false,
            supports_editing: true,
            supports_typing: true,
            max_message_length: 4_096,
            max_file_size: Some(10 * 1024 * 1024),
            uses_mrkdwn: false,
        }
    }
}

/// Sticker/emoji cache for messaging platforms.
///
/// Caches platform-specific stickers and emojis so they can be
/// reused efficiently without re-fetching from the API.
#[derive(Debug, Default)]
pub struct StickerCache {
    /// Cached stickers keyed by platform and sticker name.
    stickers: parking_lot::Mutex<std::collections::HashMap<String, std::collections::HashMap<String, StickerEntry>>>,
    /// Whether caching is enabled per platform.
    cache_enabled: parking_lot::Mutex<std::collections::HashMap<String, bool>>,
}

#[derive(Debug, Clone)]
pub struct StickerEntry {
    /// Platform-specific sticker ID or reference.
    pub id: String,
    /// URL or path to the sticker asset.
    pub asset_url: Option<String>,
    /// Emoji representation (fallback for platforms without sticker support).
    pub emoji: String,
    /// Usage count (for LRU eviction).
    pub last_used: std::time::Instant,
}

impl StickerCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable caching for a platform.
    pub fn set_enabled(&self, platform: &str, enabled: bool) {
        self.cache_enabled.lock().insert(platform.to_string(), enabled);
    }

    /// Check if caching is enabled for a platform.
    pub fn is_enabled(&self, platform: &str) -> bool {
        self.cache_enabled
            .lock()
            .get(platform)
            .copied()
            .unwrap_or(true)
    }

    /// Store a sticker in the cache.
    pub fn store(&self, platform: &str, name: &str, entry: StickerEntry) {
        let mut stickers = self.stickers.lock();
        stickers
            .entry(platform.to_string())
            .or_default()
            .insert(name.to_string(), entry);
    }

    /// Retrieve a sticker from the cache.
    pub fn get(&self, platform: &str, name: &str) -> Option<StickerEntry> {
        let mut stickers = self.stickers.lock();
        let platform_map = stickers.get_mut(platform)?;
        if let Some(entry) = platform_map.get_mut(name) {
            let result = entry.clone();
            entry.last_used = std::time::Instant::now();
            Some(result)
        } else {
            None
        }
    }

    /// Get the emoji fallback for a sticker name.
    pub fn get_emoji(&self, platform: &str, name: &str) -> Option<String> {
        self.get(platform, name).map(|e| e.emoji)
    }

    /// Remove a sticker from the cache.
    pub fn remove(&self, platform: &str, name: &str) -> bool {
        let mut stickers = self.stickers.lock();
        stickers
            .get_mut(platform)
            .and_then(|m| m.remove(name))
            .is_some()
    }

    /// Clear all stickers for a platform.
    pub fn clear_platform(&self, platform: &str) {
        self.stickers.lock().remove(platform);
    }

    /// Clear all cached stickers.
    pub fn clear_all(&self) {
        self.stickers.lock().clear();
    }

    /// Get the number of cached stickers for a platform.
    pub fn count(&self, platform: &str) -> usize {
        self.stickers
            .lock()
            .get(platform)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Get total cached stickers across all platforms.
    pub fn total_count(&self) -> usize {
        self.stickers
            .lock()
            .values()
            .map(|m| m.len())
            .sum()
    }

    /// Evict least-recently-used entries if cache exceeds capacity.
    pub fn evict_lru(&self, platform: &str, max_entries: usize) {
        let mut stickers = self.stickers.lock();
        let Some(map) = stickers.get_mut(platform) else { return };
        if map.len() <= max_entries {
            return;
        }
        // Collect and sort entries to find LRU candidates
        let mut entries: Vec<_> = map.iter().map(|(k, e)| (k.clone(), e.last_used)).collect();
        entries.sort_by_key(|(_, last_used)| *last_used);
        let to_remove = map.len() - max_entries;
        // Now iterate over owned keys, no borrow conflict
        for (name, _) in entries.iter().take(to_remove) {
            map.remove(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telegram_config() {
        let cfg = DisplayConfig::for_platform("telegram");
        assert!(cfg.supports_markdown);
        assert!(cfg.supports_html);
        assert!(cfg.supports_stickers);
        assert!(cfg.supports_editing);
        assert_eq!(cfg.max_message_length, 4_096);
        assert!(!cfg.uses_mrkdwn);
    }

    #[test]
    fn test_discord_config() {
        let cfg = DisplayConfig::for_platform("discord");
        assert!(cfg.supports_markdown);
        assert!(!cfg.supports_html);
        assert!(cfg.supports_stickers);
        assert_eq!(cfg.max_message_length, 2_000);
    }

    #[test]
    fn test_slack_uses_mrkdwn() {
        let cfg = DisplayConfig::for_platform("slack");
        assert!(cfg.uses_mrkdwn);
        assert!(!cfg.supports_stickers);
        assert_eq!(cfg.max_message_length, 40_000);
    }

    #[test]
    fn test_signal_no_editing() {
        let cfg = DisplayConfig::for_platform("signal");
        assert!(!cfg.supports_editing);
        assert!(!cfg.supports_animations);
        assert_eq!(cfg.max_message_length, 2_000);
    }

    #[test]
    fn test_matrix_full_features() {
        let cfg = DisplayConfig::for_platform("matrix");
        assert!(cfg.supports_html);
        assert!(cfg.supports_stickers);
        assert!(cfg.supports_editing);
        assert!(cfg.max_file_size.is_none());
    }

    #[test]
    fn test_homeassistant_minimal() {
        let cfg = DisplayConfig::for_platform("homeassistant");
        assert!(!cfg.supports_markdown);
        assert!(!cfg.supports_html);
        assert!(!cfg.supports_files);
        assert!(!cfg.supports_editing);
        assert!(!cfg.supports_typing);
    }

    #[test]
    fn test_unknown_defaults() {
        let cfg = DisplayConfig::for_platform("unknown_platform");
        assert_eq!(cfg.platform, "unknown");
    }

    #[test]
    fn test_message_format() {
        assert_eq!(DisplayConfig::for_platform("telegram").message_format(), "html");
        assert_eq!(DisplayConfig::for_platform("discord").message_format(), "markdown");
        assert_eq!(DisplayConfig::for_platform("slack").message_format(), "mrkdwn");
        assert_eq!(DisplayConfig::for_platform("signal").message_format(), "markdown");
        assert_eq!(DisplayConfig::for_platform("homeassistant").message_format(), "plain");
    }

    #[test]
    fn test_sticker_cache_store_and_get() {
        let cache = StickerCache::new();
        cache.store("telegram", "thumbsup", StickerEntry {
            id: "sticker-123".to_string(),
            asset_url: Some("https://example.com/sticker.png".to_string()),
            emoji: "\u{1f44d}".to_string(),
            last_used: std::time::Instant::now(),
        });

        let entry = cache.get("telegram", "thumbsup").unwrap();
        assert_eq!(entry.id, "sticker-123");
        assert_eq!(entry.emoji, "\u{1f44d}");
    }

    #[test]
    fn test_sticker_cache_miss() {
        let cache = StickerCache::new();
        assert!(cache.get("telegram", "nonexistent").is_none());
    }

    #[test]
    fn test_sticker_cache_remove() {
        let cache = StickerCache::new();
        cache.store("telegram", "test", StickerEntry {
            id: "1".to_string(),
            asset_url: None,
            emoji: "?".to_string(),
            last_used: std::time::Instant::now(),
        });
        assert!(cache.remove("telegram", "test"));
        assert!(!cache.remove("telegram", "test"));
    }

    #[test]
    fn test_sticker_cache_count() {
        let cache = StickerCache::new();
        cache.store("telegram", "a", StickerEntry {
            id: "1".to_string(), asset_url: None, emoji: "1".to_string(),
            last_used: std::time::Instant::now(),
        });
        cache.store("telegram", "b", StickerEntry {
            id: "2".to_string(), asset_url: None, emoji: "2".to_string(),
            last_used: std::time::Instant::now(),
        });
        assert_eq!(cache.count("telegram"), 2);
        assert_eq!(cache.total_count(), 2);
    }

    #[test]
    fn test_sticker_cache_clear() {
        let cache = StickerCache::new();
        cache.store("telegram", "x", StickerEntry {
            id: "1".to_string(), asset_url: None, emoji: "x".to_string(),
            last_used: std::time::Instant::now(),
        });
        cache.clear_all();
        assert_eq!(cache.total_count(), 0);
    }

    #[test]
    fn test_sticker_cache_evict_lru() {
        let cache = StickerCache::new();
        use std::time::Duration;

        // Store 5 entries with different usage times
        for i in 0..5 {
            let mut entry = StickerEntry {
                id: format!("{i}"), asset_url: None, emoji: format!("{i}"),
                last_used: std::time::Instant::now(),
            };
            // Make some appear older by adjusting last_used
            entry.last_used -= Duration::from_secs((5 - i) as u64);
            cache.store("telegram", &format!("key_{i}"), entry);
        }

        cache.evict_lru("telegram", 3);
        assert_eq!(cache.count("telegram"), 3);
    }

    #[test]
    fn test_sticker_cache_platform_toggle() {
        let cache = StickerCache::new();
        assert!(cache.is_enabled("telegram"));
        cache.set_enabled("telegram", false);
        assert!(!cache.is_enabled("telegram"));
        cache.set_enabled("telegram", true);
        assert!(cache.is_enabled("telegram"));
    }

    #[test]
    fn test_sticker_cache_clear_platform() {
        let cache = StickerCache::new();
        cache.store("telegram", "a", StickerEntry {
            id: "1".to_string(), asset_url: None, emoji: "a".to_string(),
            last_used: std::time::Instant::now(),
        });
        cache.clear_platform("telegram");
        assert_eq!(cache.count("telegram"), 0);
    }
}
