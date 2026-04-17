use std::collections::HashMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// A pairing code for DM-only mode.
///
/// Users in channels/groups must DM the bot and provide this code
/// to establish a session. Prevents unauthorized use in shared channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairCode {
    /// Unique pairing code (6 alphanumeric chars).
    pub code: String,
    /// Platform the code was generated for.
    pub platform: String,
    /// User ID that generated the code.
    pub created_by: String,
    /// Whether the code has been claimed.
    pub claimed: bool,
    /// Timestamp when the code was created (UTC epoch seconds).
    pub created_at: u64,
}

impl PairCode {
    pub fn new(platform: &str, user_id: &str) -> Self {
        Self {
            code: generate_code(),
            platform: platform.to_string(),
            created_by: user_id.to_string(),
            claimed: false,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        }
    }

    /// Check if the code is expired (older than 5 minutes).
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now - self.created_at > 300 // 5 minutes
    }
}

/// Generate a random 6-character alphanumeric code.
fn generate_code() -> String {
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let chars: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"
        .chars()
        .collect();
    let mut code = String::with_capacity(6);
    let mut s = seed;
    for _ in 0..6 {
        let idx = (s as usize) % chars.len();
        code.push(chars[idx]);
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
    }
    code
}

/// Tracks which users are allowed to use Hermes in DM-only mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedUser {
    pub user_id: String,
    pub platform: String,
    /// When the user was paired (UTC epoch seconds).
    pub paired_at: u64,
    /// Whether the user is verified.
    pub verified: bool,
}

/// DM Pairing manager.
///
/// Manages pairing codes and tracks paired users for DM-only mode.
pub struct PairingManager {
    /// Active pairing codes (code -> PairCode).
    codes: Mutex<HashMap<String, PairCode>>,
    /// Paired users ("platform:user_id" -> PairedUser).
    paired: Mutex<HashMap<String, PairedUser>>,
    /// Whether DM-only mode is enforced.
    dm_only: Mutex<bool>,
}

impl PairingManager {
    pub fn new() -> Self {
        Self {
            codes: Mutex::new(HashMap::new()),
            paired: Mutex::new(HashMap::new()),
            dm_only: Mutex::new(false),
        }
    }

    /// Enable or disable DM-only mode.
    pub fn set_dm_only(&self, enabled: bool) {
        *self.dm_only.lock() = enabled;
    }

    /// Check if DM-only mode is enforced.
    pub fn is_dm_only(&self) -> bool {
        *self.dm_only.lock()
    }

    /// Generate a new pairing code for a user.
    pub fn generate_code(&self, platform: &str, user_id: &str) -> PairCode {
        let code = PairCode::new(platform, user_id);
        self.codes.lock().insert(code.code.clone(), code.clone());
        code
    }

    /// Attempt to claim a pairing code.
    ///
    /// Returns true if the code was valid and claimed successfully.
    pub fn claim_code(&self, code: &str, user_id: &str) -> bool {
        let mut codes = self.codes.lock();
        if let Some(pair_code) = codes.get_mut(code) {
            if pair_code.claimed || pair_code.is_expired() {
                return false;
            }
            pair_code.claimed = true;

            // Register as paired user
            let paired_user = PairedUser {
                user_id: user_id.to_string(),
                platform: pair_code.platform.clone(),
                paired_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                verified: true,
            };
            let key = format!("{}:{}", pair_code.platform, user_id);
            self.paired.lock().insert(key, paired_user);
            true
        } else {
            false
        }
    }

    /// Check if a user is paired (allowed to use the bot).
    pub fn is_paired(&self, platform: &str, user_id: &str) -> bool {
        let key = format!("{platform}:{user_id}");
        self.paired.lock().contains_key(&key)
    }

    /// Get a paired user.
    pub fn get_paired(&self, platform: &str, user_id: &str) -> Option<PairedUser> {
        let key = format!("{platform}:{user_id}");
        self.paired.lock().get(&key).cloned()
    }

    /// Get all paired users for a platform.
    pub fn paired_users(&self, platform: &str) -> Vec<PairedUser> {
        self.paired
            .lock()
            .values()
            .filter(|u| u.platform == platform)
            .cloned()
            .collect()
    }

    /// Remove a paired user (unpair).
    pub fn unpair(&self, platform: &str, user_id: &str) -> bool {
        let key = format!("{platform}:{user_id}");
        self.paired.lock().remove(&key).is_some()
    }

    /// Clean up expired pairing codes.
    pub fn cleanup_expired_codes(&self) -> usize {
        let mut codes = self.codes.lock();
        let before = codes.len();
        codes.retain(|_, c| !c.is_expired() && !c.claimed);
        before - codes.len()
    }

    /// Get the number of active (unclaimed, non-expired) pairing codes.
    pub fn active_code_count(&self) -> usize {
        self.codes
            .lock()
            .values()
            .filter(|c| !c.claimed && !c.is_expired())
            .count()
    }

    /// Get the number of paired users.
    pub fn paired_count(&self) -> usize {
        self.paired.lock().len()
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
    fn test_pair_code_generation() {
        let code = PairCode::new("telegram", "user123");
        assert_eq!(code.code.len(), 6);
        assert_eq!(code.platform, "telegram");
        assert_eq!(code.created_by, "user123");
        assert!(!code.claimed);
    }

    #[test]
    fn test_pair_code_not_expired_when_new() {
        let code = PairCode::new("telegram", "user123");
        assert!(!code.is_expired());
    }

    #[test]
    fn test_generate_and_claim() {
        let manager = PairingManager::new();
        let code = manager.generate_code("telegram", "user1");

        assert!(manager.claim_code(&code.code, "user1"));
        assert!(manager.is_paired("telegram", "user1"));

        // Second claim should fail
        assert!(!manager.claim_code(&code.code, "user2"));
    }

    #[test]
    fn test_invalid_claim_code() {
        let manager = PairingManager::new();
        assert!(!manager.claim_code("INVALID", "user1"));
    }

    #[test]
    fn test_dm_only_mode() {
        let manager = PairingManager::new();
        assert!(!manager.is_dm_only());
        manager.set_dm_only(true);
        assert!(manager.is_dm_only());
        manager.set_dm_only(false);
        assert!(!manager.is_dm_only());
    }

    #[test]
    fn test_unpair() {
        let manager = PairingManager::new();
        let code = manager.generate_code("discord", "user1");
        manager.claim_code(&code.code, "user1");
        assert!(manager.is_paired("discord", "user1"));
        assert!(manager.unpair("discord", "user1"));
        assert!(!manager.is_paired("discord", "user1"));
    }

    #[test]
    fn test_cleanup_expired_codes() {
        let manager = PairingManager::new();
        manager.generate_code("telegram", "user1");
        manager.generate_code("telegram", "user2");
        assert_eq!(manager.active_code_count(), 2);

        // No codes are actually expired yet
        let cleaned = manager.cleanup_expired_codes();
        assert_eq!(cleaned, 0);
    }

    #[test]
    fn test_paired_users() {
        let manager = PairingManager::new();
        let code1 = manager.generate_code("telegram", "user1");
        let code2 = manager.generate_code("telegram", "user2");
        manager.claim_code(&code1.code, "user1");
        manager.claim_code(&code2.code, "user2");

        let users = manager.paired_users("telegram");
        assert_eq!(users.len(), 2);
    }
}
