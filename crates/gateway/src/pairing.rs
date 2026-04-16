//! DM Pairing System
//!
//! Pair code system for routing group/channel messages to DM sessions.

use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;

/// Pairing code configuration.
#[derive(Debug, Clone)]
pub struct PairingConfig {
    /// Code length.
    pub code_length: usize,

    /// Code format (numeric, alphanumeric).
    pub code_format: CodeFormat,

    /// Expiration time in seconds.
    pub expiration_seconds: u64,
}

impl Default for PairingConfig {
    fn default() -> Self {
        Self {
            code_length: 6,
            code_format: CodeFormat::Alphanumeric,
            expiration_seconds: 3600,  // 1 hour
        }
    }
}

/// Code format type.
#[derive(Debug, Clone, Copy)]
pub enum CodeFormat {
    /// Numeric only (0-9).
    Numeric,
    /// Alphanumeric (0-9, A-Z).
    Alphanumeric,
    /// Hex (0-9, A-F).
    Hex,
}

/// Pairing entry.
#[derive(Debug, Clone)]
pub struct PairingEntry {
    /// Pair code.
    pub code: String,

    /// Platform of the group/channel.
    pub platform: String,

    /// Group/channel ID.
    pub group_id: String,

    /// DM chat ID (where messages will be routed).
    pub dm_chat_id: Option<String>,

    /// Creation timestamp.
    pub created_at: u64,

    /// Expiration timestamp.
    pub expires_at: u64,
}

impl PairingEntry {
    /// Check if expired.
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now > self.expires_at
    }

    /// Check if paired (has DM chat).
    pub fn is_paired(&self) -> bool {
        self.dm_chat_id.is_some()
    }
}

/// Pairing manager.
///
/// Manages pair codes for routing messages from groups to DMs.
pub struct PairingManager {
    /// Configuration.
    config: PairingConfig,

    /// Active pairings by code.
    by_code: Arc<RwLock<HashMap<String, PairingEntry>>>,

    /// Active pairings by group.
    by_group: Arc<RwLock<HashMap<String, PairingEntry>>>,
}

impl PairingManager {
    /// Create new pairing manager.
    pub fn new() -> Self {
        Self {
            config: PairingConfig::default(),
            by_code: Arc::new(RwLock::new(HashMap::new())),
            by_group: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with custom config.
    pub fn with_config(config: PairingConfig) -> Self {
        Self {
            config,
            by_code: Arc::new(RwLock::new(HashMap::new())),
            by_group: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Generate a new pair code.
    pub fn generate_code(&self) -> String {
        let chars: &[char] = match self.config.code_format {
            CodeFormat::Numeric => &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'],
            CodeFormat::Alphanumeric => &[
                '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
                'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'M', 'N', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
            ],
            CodeFormat::Hex => &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F'],
        };

        // Use uuid for randomness source
        let uuid = uuid::Uuid::new_v4();
        let uuid_bytes = uuid.as_bytes();
        let mut idx = 0;
        let code: String = (0..self.config.code_length)
            .map(|_| {
                idx = (idx + 1) % uuid_bytes.len();
                chars[uuid_bytes[idx] as usize % chars.len()]
            })
            .collect();

        code
    }

    /// Create a pairing for a group.
    pub fn create_pairing(&self, platform: &str, group_id: &str) -> PairingEntry {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let code = self.generate_code();

        let entry = PairingEntry {
            code,
            platform: platform.to_string(),
            group_id: group_id.to_string(),
            dm_chat_id: None,
            created_at: now,
            expires_at: now + self.config.expiration_seconds,
        };

        // Store by code
        {
            let mut by_code = self.by_code.write();
            by_code.insert(entry.code.clone(), entry.clone());
        }

        // Store by group
        {
            let mut by_group = self.by_group.write();
            let key = format!("{}:{}", platform, group_id);
            by_group.insert(key, entry.clone());
        }

        entry
    }

    /// Complete pairing with DM chat.
    pub fn complete_pairing(&self, code: &str, dm_chat_id: &str) -> Option<PairingEntry> {
        let mut by_code = self.by_code.write();

        if let Some(entry) = by_code.get_mut(code) {
            if entry.is_expired() {
                by_code.remove(code);
                return None;
            }

            entry.dm_chat_id = Some(dm_chat_id.to_string());

            // Update by_group
            let mut by_group = self.by_group.write();
            let key = format!("{}:{}", entry.platform, entry.group_id);
            if let Some(group_entry) = by_group.get_mut(&key) {
                group_entry.dm_chat_id = Some(dm_chat_id.to_string());
            }

            return Some(entry.clone());
        }

        None
    }

    /// Get pairing by code.
    pub fn get_by_code(&self, code: &str) -> Option<PairingEntry> {
        let by_code = self.by_code.read();
        by_code.get(code)
            .filter(|e| !e.is_expired())
            .cloned()
    }

    /// Get pairing by group.
    pub fn get_by_group(&self, platform: &str, group_id: &str) -> Option<PairingEntry> {
        let key = format!("{}:{}", platform, group_id);
        let by_group = self.by_group.read();
        by_group.get(&key)
            .filter(|e| !e.is_expired())
            .cloned()
    }

    /// Check if a code is valid.
    pub fn is_valid_code(&self, code: &str) -> bool {
        self.get_by_code(code).is_some()
    }

    /// Get DM chat for a group.
    pub fn get_dm_chat(&self, platform: &str, group_id: &str) -> Option<String> {
        self.get_by_group(platform, group_id)
            .and_then(|e| e.dm_chat_id)
    }

    /// Remove expired pairings.
    pub fn cleanup_expired(&self) {
        let mut by_code = self.by_code.write();
        let mut by_group = self.by_group.write();

        let expired_codes: Vec<String> = by_code.values()
            .filter(|e| e.is_expired())
            .map(|e| e.code.clone())
            .collect();

        for code in expired_codes {
            if let Some(entry) = by_code.remove(&code) {
                let key = format!("{}:{}", entry.platform, entry.group_id);
                by_group.remove(&key);
            }
        }
    }

    /// Count active pairings.
    pub fn count(&self) -> usize {
        let by_code = self.by_code.read();
        by_code.values().filter(|e| !e.is_expired()).count()
    }

    /// Clear all pairings.
    pub fn clear(&self) {
        let mut by_code = self.by_code.write();
        let mut by_group = self.by_group.write();
        by_code.clear();
        by_group.clear();
    }
}

impl Default for PairingManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_code() {
        let manager = PairingManager::new();
        let code = manager.generate_code();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn test_generate_code_numeric() {
        let config = PairingConfig {
            code_length: 8,
            code_format: CodeFormat::Numeric,
            expiration_seconds: 3600,
        };
        let manager = PairingManager::with_config(config);
        let code = manager.generate_code();
        assert_eq!(code.len(), 8);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_create_pairing() {
        let manager = PairingManager::new();
        let entry = manager.create_pairing("telegram", "group-123");

        assert_eq!(entry.platform, "telegram");
        assert_eq!(entry.group_id, "group-123");
        assert!(entry.dm_chat_id.is_none());
        assert!(!entry.is_expired());
    }

    #[test]
    fn test_complete_pairing() {
        let manager = PairingManager::new();
        let entry = manager.create_pairing("telegram", "group-123");

        let completed = manager.complete_pairing(&entry.code, "dm-456");
        assert!(completed.is_some());
        assert_eq!(completed.unwrap().dm_chat_id, Some("dm-456".to_string()));
    }

    #[test]
    fn test_get_by_code() {
        let manager = PairingManager::new();
        let entry = manager.create_pairing("telegram", "group-123");

        let found = manager.get_by_code(&entry.code);
        assert!(found.is_some());
        assert_eq!(found.unwrap().group_id, "group-123");
    }

    #[test]
    fn test_get_by_group() {
        let manager = PairingManager::new();
        let _entry = manager.create_pairing("telegram", "group-123");

        let found = manager.get_by_group("telegram", "group-123");
        assert!(found.is_some());
    }

    #[test]
    fn test_get_dm_chat() {
        let manager = PairingManager::new();
        let entry = manager.create_pairing("telegram", "group-123");
        manager.complete_pairing(&entry.code, "dm-456");

        let dm = manager.get_dm_chat("telegram", "group-123");
        assert_eq!(dm, Some("dm-456".to_string()));
    }

    #[test]
    fn test_cleanup_expired() {
        let config = PairingConfig {
            code_length: 6,
            code_format: CodeFormat::Alphanumeric,
            expiration_seconds: 1,  // Short expiration
        };
        let manager = PairingManager::with_config(config);

        // Create pairing that will be expired
        manager.create_pairing("telegram", "group-123");

        // Wait for expiration (1 second)
        std::thread::sleep(std::time::Duration::from_secs(2));

        // Cleanup
        manager.cleanup_expired();

        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_pairing_entry_is_expired() {
        let entry = PairingEntry {
            code: "ABC123".to_string(),
            platform: "telegram".to_string(),
            group_id: "group-123".to_string(),
            dm_chat_id: None,
            created_at: 0,
            expires_at: 1,  // Very short expiration
        };

        assert!(entry.is_expired());
    }
}