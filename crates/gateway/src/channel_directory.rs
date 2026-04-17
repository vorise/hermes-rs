use std::collections::HashMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// Metadata for a single channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelInfo {
    /// Platform this channel belongs to.
    pub platform: String,
    /// Platform-specific channel ID.
    pub channel_id: String,
    /// Human-readable channel name.
    pub name: String,
    /// Whether this is a group/channel (false = DM).
    pub is_group: bool,
    /// Timestamp when the channel was first seen (UTC epoch seconds).
    pub first_seen: u64,
    /// Timestamp of last activity (UTC epoch seconds).
    pub last_activity: u64,
    /// Message count in this channel.
    pub message_count: u64,
}

impl ChannelInfo {
    pub fn new(platform: &str, channel_id: &str, name: &str, is_group: bool) -> Self {
        let now = now_secs();
        Self {
            platform: platform.to_string(),
            channel_id: channel_id.to_string(),
            name: name.to_string(),
            is_group,
            first_seen: now,
            last_activity: now,
            message_count: 0,
        }
    }

    pub fn record_activity(&mut self) {
        self.last_activity = now_secs();
        self.message_count += 1;
    }
}

/// Channel directory — tracks all channels across platforms.
///
/// Used for routing, management, and status reporting.
pub struct ChannelDirectory {
    channels: Mutex<HashMap<String, ChannelInfo>>,
}

impl ChannelDirectory {
    pub fn new() -> Self {
        Self {
            channels: Mutex::new(HashMap::new()),
        }
    }

    /// Register or update a channel.
    pub fn register(&self, info: ChannelInfo) {
        let key = channel_key(&info.platform, &info.channel_id);
        self.channels.lock().insert(key, info);
    }

    /// Get a channel by platform and ID.
    pub fn get(&self, platform: &str, channel_id: &str) -> Option<ChannelInfo> {
        let key = channel_key(platform, channel_id);
        self.channels.lock().get(&key).cloned()
    }

    /// Record activity on a channel.
    pub fn record_activity(&self, platform: &str, channel_id: &str) {
        let key = channel_key(platform, channel_id);
        if let Some(channel) = self.channels.lock().get_mut(&key) {
            channel.record_activity();
        }
    }

    /// List all channels for a platform.
    pub fn channels_for(&self, platform: &str) -> Vec<ChannelInfo> {
        self.channels
            .lock()
            .values()
            .filter(|c| c.platform == platform)
            .cloned()
            .collect()
    }

    /// List all group channels (non-DM) for a platform.
    pub fn groups_for(&self, platform: &str) -> Vec<ChannelInfo> {
        self.channels
            .lock()
            .values()
            .filter(|c| c.platform == platform && c.is_group)
            .cloned()
            .collect()
    }

    /// List all DM channels for a platform.
    pub fn dms_for(&self, platform: &str) -> Vec<ChannelInfo> {
        self.channels
            .lock()
            .values()
            .filter(|c| c.platform == platform && !c.is_group)
            .cloned()
            .collect()
    }

    /// Remove a channel.
    pub fn remove(&self, platform: &str, channel_id: &str) -> bool {
        let key = channel_key(platform, channel_id);
        self.channels.lock().remove(&key).is_some()
    }

    /// Get the total number of tracked channels.
    pub fn len(&self) -> usize {
        self.channels.lock().len()
    }

    /// Get the total message count across all channels.
    pub fn total_messages(&self) -> u64 {
        self.channels.lock().values().map(|c| c.message_count).sum()
    }

    /// List all platforms that have registered channels.
    pub fn platforms(&self) -> Vec<String> {
        let mut platforms: Vec<String> = self.channels
            .lock()
            .values()
            .map(|c| c.platform.clone())
            .collect();
        platforms.sort();
        platforms.dedup();
        platforms
    }
}

impl Default for ChannelDirectory {
    fn default() -> Self {
        Self::new()
    }
}

fn channel_key(platform: &str, channel_id: &str) -> String {
    format!("{platform}:{channel_id}")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_info_new() {
        let ch = ChannelInfo::new("telegram", "123", "Test Group", true);
        assert_eq!(ch.platform, "telegram");
        assert_eq!(ch.channel_id, "123");
        assert!(ch.is_group);
        assert_eq!(ch.message_count, 0);
    }

    #[test]
    fn test_channel_info_record_activity() {
        let mut ch = ChannelInfo::new("discord", "456", "General", true);
        ch.record_activity();
        ch.record_activity();
        assert_eq!(ch.message_count, 2);
    }

    #[test]
    fn test_register_and_get() {
        let dir = ChannelDirectory::new();
        let ch = ChannelInfo::new("telegram", "ch1", "Main", false);
        dir.register(ch);

        let got = dir.get("telegram", "ch1").unwrap();
        assert_eq!(got.name, "Main");
        assert!(!got.is_group);
    }

    #[test]
    fn test_channels_for_platform() {
        let dir = ChannelDirectory::new();
        dir.register(ChannelInfo::new("telegram", "1", "DM", false));
        dir.register(ChannelInfo::new("telegram", "2", "Group", true));
        dir.register(ChannelInfo::new("discord", "3", "Discord Ch", true));

        let tg_channels = dir.channels_for("telegram");
        assert_eq!(tg_channels.len(), 2);

        let dc_channels = dir.channels_for("discord");
        assert_eq!(dc_channels.len(), 1);
    }

    #[test]
    fn test_groups_and_dms() {
        let dir = ChannelDirectory::new();
        dir.register(ChannelInfo::new("telegram", "1", "DM", false));
        dir.register(ChannelInfo::new("telegram", "2", "Group", true));
        dir.register(ChannelInfo::new("telegram", "3", "Another Group", true));

        let groups = dir.groups_for("telegram");
        assert_eq!(groups.len(), 2);

        let dms = dir.dms_for("telegram");
        assert_eq!(dms.len(), 1);
    }

    #[test]
    fn test_remove_channel() {
        let dir = ChannelDirectory::new();
        dir.register(ChannelInfo::new("slack", "ch1", "Random", true));
        assert_eq!(dir.len(), 1);
        assert!(dir.remove("slack", "ch1"));
        assert_eq!(dir.len(), 0);
    }

    #[test]
    fn test_total_messages() {
        let dir = ChannelDirectory::new();
        let mut ch1 = ChannelInfo::new("telegram", "1", "DM", false);
        ch1.record_activity();
        ch1.record_activity();
        dir.register(ch1);

        let mut ch2 = ChannelInfo::new("telegram", "2", "Group", true);
        ch2.record_activity();
        dir.register(ch2);

        assert_eq!(dir.total_messages(), 3);
    }

    #[test]
    fn test_platforms() {
        let dir = ChannelDirectory::new();
        dir.register(ChannelInfo::new("telegram", "1", "DM", false));
        dir.register(ChannelInfo::new("discord", "2", "Ch", true));
        dir.register(ChannelInfo::new("telegram", "3", "Group", true));

        let platforms = dir.platforms();
        assert_eq!(platforms, vec!["discord".to_string(), "telegram".to_string()]);
    }
}
