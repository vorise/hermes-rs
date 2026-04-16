use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// A single API credential entry.
#[derive(Debug, Clone)]
pub struct Credential {
    pub provider: String,
    pub api_key: String,
    pub base_url: String,
    pub label: String,
}

impl Credential {
    pub fn new(provider: &str, api_key: &str) -> Self {
        Self {
            provider: provider.to_string(),
            api_key: api_key.to_string(),
            base_url: String::new(),
            label: String::new(),
        }
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    pub fn with_label(mut self, label: &str) -> Self {
        self.label = label.to_string();
        self
    }
}

/// State of a credential in the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialState {
    /// Available for use.
    Active,
    /// Temporarily disabled (rate limit, error).
    Unavailable,
    /// Being re-tested for recovery.
    Recovering,
}

impl std::fmt::Display for CredentialState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialState::Active => write!(f, "active"),
            CredentialState::Unavailable => write!(f, "unavailable"),
            CredentialState::Recovering => write!(f, "recovering"),
        }
    }
}

/// Internal state for a credential entry.
struct CredentialEntry {
    credential: Credential,
    state: AtomicU64, // encodes CredentialState: 0=Active, 1=Unavailable, 2=Recovering
    failure_count: AtomicU64,
    last_failure: AtomicU64, // Unix timestamp seconds
}

impl CredentialEntry {
    fn new(credential: Credential) -> Self {
        Self {
            credential,
            state: AtomicU64::new(0), // Active
            failure_count: AtomicU64::new(0),
            last_failure: AtomicU64::new(0),
        }
    }

    fn get_state(&self) -> CredentialState {
        match self.state.load(Ordering::Relaxed) {
            0 => CredentialState::Active,
            1 => CredentialState::Unavailable,
            2 => CredentialState::Recovering,
            _ => CredentialState::Active,
        }
    }

    fn set_state(&self, state: CredentialState) {
        let val = match state {
            CredentialState::Active => 0,
            CredentialState::Unavailable => 1,
            CredentialState::Recovering => 2,
        };
        self.state.store(val, Ordering::Relaxed);
    }

    fn mark_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_failure.store(now, Ordering::Relaxed);
    }

    fn get_failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::Relaxed)
    }
}

/// A thread-safe pool of provider credentials with automatic failover.
///
/// Manages multiple API keys for the same provider, automatically
/// switching to the next available credential when one fails.
pub struct CredentialPool {
    entries: RwLock<Vec<Arc<CredentialEntry>>>,
    current_index: AtomicU64,
    recovery_interval_secs: AtomicU64,
}

impl CredentialPool {
    /// Create a new pool with the given credentials.
    pub fn new(credentials: Vec<Credential>) -> Self {
        let entries: Vec<Arc<CredentialEntry>> = credentials
            .into_iter()
            .map(|c| Arc::new(CredentialEntry::new(c)))
            .collect();
        Self {
            entries: RwLock::new(entries),
            current_index: AtomicU64::new(0),
            recovery_interval_secs: AtomicU64::new(300), // 5 minutes
        }
    }

    /// Create a pool from environment variables for a given provider.
    /// Reads `{PROVIDER}_API_KEY` and optionally `{PROVIDER}_API_KEY_2`, etc.
    pub fn from_env(provider: &str, base_url: &str) -> Self {
        let provider_upper = provider.to_uppercase();
        let mut credentials = Vec::new();

        // Primary key
        if let Ok(key) = std::env::var(format!("{provider_upper}_API_KEY")) {
            let mut cred = Credential::new(provider, &key);
            if !base_url.is_empty() {
                cred = cred.with_base_url(base_url);
            }
            credentials.push(cred);
        }

        // Additional keys: {PROVIDER}_API_KEY_2, {PROVIDER}_API_KEY_3, ...
        for i in 2..=10 {
            let var = format!("{provider_upper}_API_KEY_{i}");
            if let Ok(key) = std::env::var(&var) {
                let mut cred = Credential::new(provider, &key).with_label(&format!("{i}"));
                if !base_url.is_empty() {
                    cred = cred.with_base_url(base_url);
                }
                credentials.push(cred);
            }
        }

        Self::new(credentials)
    }

    /// Get the next active credential from the pool (round-robin among active).
    ///
    /// Returns `None` if no active credentials are available.
    pub fn get_active(&self) -> Option<Arc<Credential>> {
        let entries = self.entries.read().ok()?;
        let len = entries.len();
        if len == 0 {
            return None;
        }

        let start = self.current_index.load(Ordering::Relaxed) as usize % len;

        // Try up to `len` entries starting from current_index
        for offset in 0..len {
            let idx = (start + offset) % len;
            let entry = &entries[idx];
            if entry.get_state() == CredentialState::Active {
                // Advance the index for next call
                self.current_index.store((idx + 1) as u64, Ordering::Relaxed);
                return Some(Arc::new(entry.credential.clone()));
            }
        }

        None
    }

    /// Mark the given credential as failed (rate limit, server error).
    /// If failure count exceeds threshold, marks it unavailable.
    pub fn mark_failed(&self, credential: &Credential) {
        let entries = self.entries.read().ok().unwrap();
        for entry in entries.iter() {
            if entry.credential.provider == credential.provider
                && entry.credential.api_key == credential.api_key
            {
                entry.mark_failure();
                // After 3 failures, mark unavailable
                if entry.get_failure_count() >= 3 {
                    entry.set_state(CredentialState::Unavailable);
                }
                break;
            }
        }
    }

    /// Mark a credential as recovered (available again).
    pub fn mark_recovered(&self, credential: &Credential) {
        let entries = self.entries.read().ok().unwrap();
        for entry in entries.iter() {
            if entry.credential.provider == credential.provider
                && entry.credential.api_key == credential.api_key
            {
                entry.set_state(CredentialState::Active);
                entry.failure_count.store(0, Ordering::Relaxed);
                break;
            }
        }
    }

    /// Start recovery process for unavailable credentials.
    /// Moves them to Recovering state so they can be tested.
    pub fn start_recovery(&self) {
        let entries = self.entries.read().ok().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let interval = self.recovery_interval_secs.load(Ordering::Relaxed);

        for entry in entries.iter() {
            if entry.get_state() == CredentialState::Unavailable {
                let last_failure = entry.last_failure.load(Ordering::Relaxed);
                if now.saturating_sub(last_failure) >= interval {
                    entry.set_state(CredentialState::Recovering);
                }
            }
        }
    }

    /// Add a new credential to the pool.
    pub fn add(&self, credential: Credential) {
        let mut entries = self.entries.write().unwrap();
        entries.push(Arc::new(CredentialEntry::new(credential)));
    }

    /// Remove a credential from the pool by API key.
    pub fn remove(&self, api_key: &str) {
        let mut entries = self.entries.write().unwrap();
        entries.retain(|e| e.credential.api_key != api_key);
    }

    /// Get the count of active credentials.
    pub fn active_count(&self) -> usize {
        let entries = self.entries.read().unwrap();
        entries
            .iter()
            .filter(|e| e.get_state() == CredentialState::Active)
            .count()
    }

    /// Get the total credential count.
    pub fn total_count(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    /// Get all credentials and their states.
    pub fn list_all(&self) -> Vec<(Credential, CredentialState)> {
        let entries = self.entries.read().unwrap();
        entries
            .iter()
            .map(|e| (e.credential.clone(), e.get_state()))
            .collect()
    }

    /// Set the recovery interval (how long before an unavailable credential is re-tested).
    pub fn set_recovery_interval(&self, interval: Duration) {
        self.recovery_interval_secs
            .store(interval.as_secs(), Ordering::Relaxed);
    }

    /// Get all active credentials as a vector.
    pub fn get_all_active(&self) -> Vec<Arc<Credential>> {
        let entries = self.entries.read().unwrap();
        entries
            .iter()
            .filter(|e| e.get_state() == CredentialState::Active)
            .map(|e| Arc::new(e.credential.clone()))
            .collect()
    }
}

/// Empty pool fallback when no credentials are configured.
impl Default for CredentialPool {
    fn default() -> Self {
        Self::new(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_credential(key: &str) -> Credential {
        Credential::new("test-provider", key)
    }

    #[test]
    fn test_pool_empty() {
        let pool = CredentialPool::new(vec![]);
        assert_eq!(pool.total_count(), 0);
        assert_eq!(pool.active_count(), 0);
        assert!(pool.get_active().is_none());
    }

    #[test]
    fn test_pool_single_credential() {
        let pool = CredentialPool::new(vec![test_credential("key1")]);
        assert_eq!(pool.total_count(), 1);
        assert_eq!(pool.active_count(), 1);
        let cred = pool.get_active().unwrap();
        assert_eq!(cred.api_key, "key1");
    }

    #[test]
    fn test_pool_round_robin() {
        let pool = CredentialPool::new(vec![
            test_credential("key1"),
            test_credential("key2"),
            test_credential("key3"),
        ]);

        // First call should return key1
        let c1 = pool.get_active().unwrap();
        assert_eq!(c1.api_key, "key1");
        // Second call should return key2 (round-robin)
        let c2 = pool.get_active().unwrap();
        assert_eq!(c2.api_key, "key2");
        // Third call should return key3
        let c3 = pool.get_active().unwrap();
        assert_eq!(c3.api_key, "key3");
    }

    #[test]
    fn test_pool_failover() {
        let pool = CredentialPool::new(vec![
            test_credential("key1"),
            test_credential("key2"),
        ]);

        // Mark key1 as failed 3 times
        let cred1 = test_credential("key1");
        pool.mark_failed(&cred1);
        pool.mark_failed(&cred1);
        pool.mark_failed(&cred1);

        // key1 should now be unavailable
        assert_eq!(pool.active_count(), 1);

        // get_active should return key2
        let active = pool.get_active().unwrap();
        assert_eq!(active.api_key, "key2");
    }

    #[test]
    fn test_pool_recovery() {
        let pool = CredentialPool::new(vec![
            test_credential("key1"),
            test_credential("key2"),
        ]);

        // Mark key1 unavailable
        let cred1 = test_credential("key1");
        for _ in 0..3 {
            pool.mark_failed(&cred1);
        }
        assert_eq!(pool.active_count(), 1);

        // Mark recovered
        pool.mark_recovered(&cred1);
        assert_eq!(pool.active_count(), 2);
    }

    #[test]
    fn test_pool_add_remove() {
        let pool = CredentialPool::new(vec![test_credential("key1")]);
        assert_eq!(pool.total_count(), 1);

        pool.add(test_credential("key2"));
        assert_eq!(pool.total_count(), 2);

        pool.remove("key2");
        assert_eq!(pool.total_count(), 1);
    }

    #[test]
    fn test_pool_list_all() {
        let pool = CredentialPool::new(vec![
            test_credential("key1"),
            test_credential("key2"),
        ]);

        let all = pool.list_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].0.api_key, "key1");
        assert_eq!(all[0].1, CredentialState::Active);
    }

    #[test]
    fn test_pool_all_unavailable_returns_none() {
        let pool = CredentialPool::new(vec![
            test_credential("key1"),
            test_credential("key2"),
        ]);

        let cred1 = test_credential("key1");
        let cred2 = test_credential("key2");
        for _ in 0..3 {
            pool.mark_failed(&cred1);
            pool.mark_failed(&cred2);
        }

        assert!(pool.get_active().is_none());
    }

    #[test]
    fn test_credential_builder() {
        let cred = Credential::new("anthropic", "sk-ant-xxx")
            .with_base_url("https://api.anthropic.com")
            .with_label("primary");
        assert_eq!(cred.provider, "anthropic");
        assert_eq!(cred.api_key, "sk-ant-xxx");
        assert_eq!(cred.base_url, "https://api.anthropic.com");
        assert_eq!(cred.label, "primary");
    }

    #[test]
    fn test_credential_state_display() {
        assert_eq!(CredentialState::Active.to_string(), "active");
        assert_eq!(CredentialState::Unavailable.to_string(), "unavailable");
        assert_eq!(CredentialState::Recovering.to_string(), "recovering");
    }

    #[test]
    fn test_pool_get_all_active() {
        let pool = CredentialPool::new(vec![
            test_credential("key1"),
            test_credential("key2"),
        ]);

        let all = pool.get_all_active();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_pool_default() {
        let pool = CredentialPool::default();
        assert_eq!(pool.total_count(), 0);
        assert!(pool.get_active().is_none());
    }
}
