use std::collections::HashSet;

use parking_lot::Mutex;
use tracing;

/// A mirror rule defines which platforms to mirror messages to.
#[derive(Debug, Clone)]
pub struct MirrorRule {
    /// Unique identifier for this rule.
    pub id: String,
    /// Source platform to mirror from.
    pub source_platform: String,
    /// Set of target platforms to mirror to.
    pub target_platforms: HashSet<String>,
    /// Whether to mirror DMs.
    pub mirror_dms: bool,
    /// Whether to mirror group/channel messages.
    pub mirror_groups: bool,
    /// Optional list of specific channel IDs to mirror (empty = all).
    pub channel_filter: HashSet<String>,
    /// Whether the rule is active.
    pub enabled: bool,
}

impl MirrorRule {
    pub fn new(id: &str, source: &str, targets: &[&str]) -> Self {
        Self {
            id: id.to_string(),
            source_platform: source.to_string(),
            target_platforms: targets.iter().map(|s| s.to_string()).collect(),
            mirror_dms: true,
            mirror_groups: true,
            channel_filter: HashSet::new(),
            enabled: true,
        }
    }

    /// Add a channel filter (only mirror messages from these channels).
    pub fn with_channels(mut self, channels: &[&str]) -> Self {
        self.channel_filter = channels.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Set whether to mirror DMs.
    pub fn mirror_dms(mut self, enabled: bool) -> Self {
        self.mirror_dms = enabled;
        self
    }

    /// Set whether to mirror group messages.
    pub fn mirror_groups(mut self, enabled: bool) -> Self {
        self.mirror_groups = enabled;
        self
    }

    /// Check if a message should be mirrored based on the rule.
    pub fn should_mirror(&self, is_dm: bool, channel_id: &str) -> bool {
        if !self.enabled {
            return false;
        }
        if is_dm && !self.mirror_dms {
            return false;
        }
        if !is_dm && !self.mirror_groups {
            return false;
        }
        if !self.channel_filter.is_empty() && !self.channel_filter.contains(channel_id) {
            return false;
        }
        true
    }
}

/// Mirrored message ready for delivery to a target platform.
#[derive(Debug, Clone)]
pub struct MirroredMessage {
    /// Original platform.
    pub source_platform: String,
    /// Target platform.
    pub target_platform: String,
    /// Message content.
    pub content: String,
    /// Whether this was a DM.
    pub is_dm: bool,
    /// Original channel/chat ID.
    pub source_channel: String,
    /// Sender identifier.
    pub sender_id: String,
}

/// Gateway mirror module for cross-platform message mirroring.
///
/// When a message arrives on a source platform, the mirror system
/// can forward it to other platforms for unified visibility.
///
/// Use cases:
/// - DMs from multiple platforms forwarded to a single admin channel
/// - Important channel messages mirrored to Discord for notifications
/// - Cross-platform audit logging
pub struct GatewayMirror {
    /// Mirror rules indexed by source platform.
    rules: Mutex<Vec<MirrorRule>>,
    /// Counter of mirrored messages.
    mirror_count: Mutex<u64>,
}

impl GatewayMirror {
    pub fn new() -> Self {
        Self {
            rules: Mutex::new(Vec::new()),
            mirror_count: Mutex::new(0),
        }
    }

    /// Add a mirror rule.
    pub fn add_rule(&self, rule: MirrorRule) {
        tracing::info!(
            rule_id = rule.id,
            source = rule.source_platform,
            targets = ?rule.target_platforms,
            "Adding mirror rule"
        );
        self.rules.lock().push(rule);
    }

    /// Remove a mirror rule by ID.
    pub fn remove_rule(&self, rule_id: &str) -> bool {
        let mut rules = self.rules.lock();
        let len_before = rules.len();
        rules.retain(|r| r.id != rule_id);
        let removed = len_before - rules.len();
        if removed > 0 {
            tracing::info!(rule_id = rule_id, "Removed mirror rule");
        }
        removed > 0
    }

    /// Get all mirror rules for a source platform.
    pub fn rules_for_platform(&self, platform: &str) -> Vec<MirrorRule> {
        self.rules
            .lock()
            .iter()
            .filter(|r| r.source_platform == platform)
            .cloned()
            .collect()
    }

    /// Process an incoming message and determine mirror targets.
    ///
    /// Returns a list of `MirroredMessage` for each target platform.
    pub fn process_message(
        &self,
        source_platform: &str,
        content: &str,
        is_dm: bool,
        channel_id: &str,
        sender_id: &str,
    ) -> Vec<MirroredMessage> {
        let rules = self.rules_for_platform(source_platform);
        let mut mirrored = Vec::new();

        for rule in &rules {
            if !rule.should_mirror(is_dm, channel_id) {
                continue;
            }

            for target in &rule.target_platforms {
                let msg = MirroredMessage {
                    source_platform: source_platform.to_string(),
                    target_platform: target.clone(),
                    content: content.to_string(),
                    is_dm,
                    source_channel: channel_id.to_string(),
                    sender_id: sender_id.to_string(),
                };
                mirrored.push(msg);
            }
        }

        if !mirrored.is_empty() {
            let count = mirrored.len();
            *self.mirror_count.lock() += count as u64;
            tracing::debug!(
                source = source_platform,
                targets = count,
                "Message mirrored to {count} platform(s)"
            );
        }

        mirrored
    }

    /// Enable a mirror rule by ID.
    pub fn enable_rule(&self, rule_id: &str) -> bool {
        self.rules
            .lock()
            .iter_mut()
            .find(|r| r.id == rule_id)
            .map(|r| {
                r.enabled = true;
                true
            })
            .unwrap_or(false)
    }

    /// Disable a mirror rule by ID.
    pub fn disable_rule(&self, rule_id: &str) -> bool {
        self.rules
            .lock()
            .iter_mut()
            .find(|r| r.id == rule_id)
            .map(|r| {
                r.enabled = false;
                true
            })
            .unwrap_or(false)
    }

    /// Get total number of mirror rules.
    pub fn rule_count(&self) -> usize {
        self.rules.lock().len()
    }

    /// Get total number of messages mirrored.
    pub fn mirror_count(&self) -> u64 {
        *self.mirror_count.lock()
    }

    /// Reset the mirror counter.
    pub fn reset_counter(&self) {
        *self.mirror_count.lock() = 0;
    }

    /// Get all source platforms that have mirror rules.
    pub fn source_platforms(&self) -> HashSet<String> {
        self.rules
            .lock()
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.source_platform.clone())
            .collect()
    }

    /// Get all target platforms across all rules.
    pub fn target_platforms(&self) -> HashSet<String> {
        self.rules
            .lock()
            .iter()
            .filter(|r| r.enabled)
            .flat_map(|r| r.target_platforms.iter().cloned())
            .collect()
    }
}

impl Default for GatewayMirror {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rule() -> MirrorRule {
        MirrorRule::new("rule1", "telegram", &["discord", "slack"])
    }

    #[test]
    fn test_mirror_rule_new() {
        let rule = test_rule();
        assert_eq!(rule.id, "rule1");
        assert_eq!(rule.source_platform, "telegram");
        assert!(rule.target_platforms.contains("discord"));
        assert!(rule.target_platforms.contains("slack"));
        assert!(rule.enabled);
    }

    #[test]
    fn test_mirror_rule_with_channels() {
        let rule = MirrorRule::new("rule1", "telegram", &["discord"])
            .with_channels(&["channel_a", "channel_b"]);
        assert!(rule.should_mirror(false, "channel_a"));
        assert!(rule.should_mirror(false, "channel_b"));
        assert!(!rule.should_mirror(false, "channel_c"));
    }

    #[test]
    fn test_mirror_rule_dm_filter() {
        let rule = MirrorRule::new("rule1", "telegram", &["discord"])
            .mirror_dms(false)
            .mirror_groups(true);
        assert!(!rule.should_mirror(true, "any"));
        assert!(rule.should_mirror(false, "any"));
    }

    #[test]
    fn test_mirror_rule_group_filter() {
        let rule = MirrorRule::new("rule1", "telegram", &["discord"])
            .mirror_dms(true)
            .mirror_groups(false);
        assert!(rule.should_mirror(true, "any"));
        assert!(!rule.should_mirror(false, "any"));
    }

    #[test]
    fn test_mirror_rule_disabled() {
        let mut rule = test_rule();
        rule.enabled = false;
        assert!(!rule.should_mirror(true, "any"));
        assert!(!rule.should_mirror(false, "any"));
    }

    #[test]
    fn test_mirror_empty_no_rules() {
        let mirror = GatewayMirror::new();
        let result = mirror.process_message("telegram", "hello", true, "dm", "user1");
        assert!(result.is_empty());
    }

    #[test]
    fn test_mirror_single_rule() {
        let mirror = GatewayMirror::new();
        mirror.add_rule(MirrorRule::new("r1", "telegram", &["discord"]));

        let result = mirror.process_message("telegram", "hello", true, "dm", "user1");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].target_platform, "discord");
        assert_eq!(result[0].content, "hello");
        assert_eq!(result[0].source_platform, "telegram");
    }

    #[test]
    fn test_mirror_multiple_targets() {
        let mirror = GatewayMirror::new();
        mirror.add_rule(MirrorRule::new("r1", "telegram", &["discord", "slack", "email"]));

        let result = mirror.process_message("telegram", "hello", true, "dm", "user1");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_mirror_multiple_rules() {
        let mirror = GatewayMirror::new();
        mirror.add_rule(MirrorRule::new("r1", "telegram", &["discord"]));
        mirror.add_rule(MirrorRule::new("r2", "telegram", &["slack"]));

        let result = mirror.process_message("telegram", "hello", true, "dm", "user1");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_mirror_enable_disable() {
        let mirror = GatewayMirror::new();
        mirror.add_rule(MirrorRule::new("r1", "telegram", &["discord"]));

        assert!(mirror.disable_rule("r1"));
        let result = mirror.process_message("telegram", "hello", true, "dm", "user1");
        assert!(result.is_empty());

        assert!(mirror.enable_rule("r1"));
        let result = mirror.process_message("telegram", "hello", true, "dm", "user1");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_mirror_remove_rule() {
        let mirror = GatewayMirror::new();
        mirror.add_rule(MirrorRule::new("r1", "telegram", &["discord"]));
        assert_eq!(mirror.rule_count(), 1);

        assert!(mirror.remove_rule("r1"));
        assert_eq!(mirror.rule_count(), 0);
    }

    #[test]
    fn test_mirror_remove_nonexistent() {
        let mirror = GatewayMirror::new();
        assert!(!mirror.remove_rule("nonexistent"));
    }

    #[test]
    fn test_mirror_counter() {
        let mirror = GatewayMirror::new();
        mirror.add_rule(MirrorRule::new("r1", "telegram", &["discord", "slack"]));

        mirror.process_message("telegram", "msg1", true, "dm", "user1");
        mirror.process_message("telegram", "msg2", true, "dm", "user2");
        assert_eq!(mirror.mirror_count(), 4); // 2 messages x 2 targets

        mirror.reset_counter();
        assert_eq!(mirror.mirror_count(), 0);
    }

    #[test]
    fn test_mirror_source_platforms() {
        let mirror = GatewayMirror::new();
        mirror.add_rule(MirrorRule::new("r1", "telegram", &["discord"]));
        mirror.add_rule(MirrorRule::new("r2", "whatsapp", &["discord"]));

        let sources = mirror.source_platforms();
        assert_eq!(sources.len(), 2);
        assert!(sources.contains("telegram"));
        assert!(sources.contains("whatsapp"));
    }

    #[test]
    fn test_mirror_target_platforms() {
        let mirror = GatewayMirror::new();
        mirror.add_rule(MirrorRule::new("r1", "telegram", &["discord", "slack"]));
        mirror.add_rule(MirrorRule::new("r2", "whatsapp", &["email"]));

        let targets = mirror.target_platforms();
        assert_eq!(targets.len(), 3);
        assert!(targets.contains("discord"));
        assert!(targets.contains("slack"));
        assert!(targets.contains("email"));
    }

    #[test]
    fn test_default_mirror() {
        let mirror = GatewayMirror::default();
        assert_eq!(mirror.rule_count(), 0);
        assert_eq!(mirror.mirror_count(), 0);
    }
}
