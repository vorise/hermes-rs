use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Cache TTL — re-read config every 30 seconds to avoid reparsing YAML
/// on every URL check during web crawls with many pages.
const CACHE_TTL_SECS: u64 = 30;

/// Configuration for website blocklist enforcement.
#[derive(Debug, Clone, Default)]
pub struct WebsitePolicyConfig {
    /// Whether blocklist enforcement is active.
    pub enabled: bool,
    /// Inline domain patterns to block.
    pub domains: Vec<String>,
    /// Paths to external blocklist files (one domain per line).
    pub shared_files: Vec<String>,
}

/// Cached policy state with TTL.
struct CachedPolicy {
    config: WebsitePolicyConfig,
    normalized_rules: Vec<String>,
    loaded_at: Instant,
}

static POLICY_CACHE: OnceLock<std::sync::Mutex<Option<CachedPolicy>>> = OnceLock::new();

/// Load the website policy config from the Hermes config file.
fn load_config() -> WebsitePolicyConfig {
    let config_path = std::env::var("HERMES_HOME")
        .ok()
        .map(|h| PathBuf::from(h).join("config.yaml"))
        .or_else(|| {
            std::env::var("HOME").ok().map(|h| {
                PathBuf::from(h).join(".hermes/config.yaml")
            })
        });

    let Some(path) = config_path else {
        return WebsitePolicyConfig::default();
    };

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return WebsitePolicyConfig::default(),
    };

    let config: serde_yaml::Value = match serde_yaml::from_str(&content) {
        Ok(v) => v,
        Err(_) => return WebsitePolicyConfig::default(),
    };

    let website_blocklist = config
        .get("website_blocklist")
        .and_then(|v| v.as_mapping());

    let Some(map) = website_blocklist else {
        return WebsitePolicyConfig::default();
    };

    let enabled = map
        .get(&serde_yaml::Value::String("enabled".to_string()))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let domains = map
        .get(&serde_yaml::Value::String("domains".to_string()))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let shared_files = map
        .get(&serde_yaml::Value::String("shared_files".to_string()))
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    WebsitePolicyConfig {
        enabled,
        domains,
        shared_files,
    }
}

/// Normalize a single blocklist rule into a canonical domain form.
///
/// Handles:
/// - Full URLs: `https://www.example.com/path` → `example.com`
/// - Wildcards: `*.example.com` → `example.com`
/// - Comments: Lines starting with `#` are skipped
/// - Strips whitespace, protocols, `www.` prefix
fn normalize_rule(rule: &str) -> Option<String> {
    let trimmed = rule.trim();

    // Skip comments and empty lines
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    // Strip protocol
    let without_protocol = if let Some(pos) = trimmed.find("://") {
        &trimmed[pos + 3..]
    } else {
        trimmed
    };

    // Strip path (domain only)
    let domain = without_protocol
        .split('/')
        .next()
        .unwrap_or(without_protocol);

    // Strip port
    let domain = domain
        .split(':')
        .next()
        .unwrap_or(domain);

    // Strip www. prefix
    let domain = domain.strip_prefix("www.").unwrap_or(domain);

    // Strip wildcard prefix
    let domain = domain.strip_prefix('*').unwrap_or(domain);
    let domain = domain.strip_prefix('.').unwrap_or(domain);

    Some(domain.to_lowercase())
}

/// Load rules from a shared blocklist file.
///
/// Plain text, one domain per line. Missing/unreadable files log a warning.
fn load_blocklist_file(path: &str) -> Vec<String> {
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to read blocklist file '{path}': {e}");
            return Vec::new();
        }
    };

    content
        .lines()
        .filter_map(normalize_rule)
        .collect()
}

/// Get or refresh the cached policy.
fn get_policy() -> WebsitePolicyConfig {
    let cache = POLICY_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = cache.lock().unwrap();

    let needs_refresh = match guard.as_ref() {
        None => true,
        Some(cached) => cached.loaded_at.elapsed() > Duration::from_secs(CACHE_TTL_SECS),
    };

    if needs_refresh {
        let config = load_config();
        *guard = Some(CachedPolicy {
            config: config.clone(),
            normalized_rules: Vec::new(), // Will be built on demand
            loaded_at: Instant::now(),
        });
        return config;
    }

    guard.as_ref().unwrap().config.clone()
}

/// Check if a URL is allowed by the website blocklist policy.
///
/// Returns `(allowed, reason_if_blocked)`.
pub fn is_url_allowed(url: &str) -> (bool, Option<String>) {
    let config = get_policy();

    if !config.enabled {
        return (true, None);
    }

    if config.domains.is_empty() && config.shared_files.is_empty() {
        return (true, None);
    }

    // Normalize the URL to extract domain
    let url_domain = match normalize_rule(url) {
        Some(d) => d,
        None => return (true, None), // Can't parse URL → allow
    };

    // Build the full rule set (inline domains + shared files)
    let mut rules: Vec<String> = config
        .domains
        .iter()
        .filter_map(|r| normalize_rule(r))
        .collect();

    for file_path in &config.shared_files {
        rules.extend(load_blocklist_file(file_path));
    }

    // Check for exact match or wildcard match
    for rule in &rules {
        if url_domain == *rule {
            return (false, Some(format!("URL blocked by domain rule: {rule}")));
        }

        // Wildcard: rule is a parent domain
        if url_domain.ends_with(&format!(".{rule}")) {
            return (false, Some(format!("URL blocked by wildcard rule: *.{rule}")));
        }
    }

    (true, None)
}

/// Check multiple URLs at once.
///
/// Returns `(allowed_count, blocked_count, first_block_reason)`.
pub fn check_urls(urls: &[String]) -> (usize, usize, Option<String>) {
    let mut allowed = 0;
    let mut blocked = 0;
    let mut first_block_reason = None;

    for url in urls {
        let (ok, reason) = is_url_allowed(url);
        if ok {
            allowed += 1;
        } else {
            blocked += 1;
            if first_block_reason.is_none() {
                first_block_reason = reason;
            }
        }
    }

    (allowed, blocked, first_block_reason)
}

/// Check a single URL, returning a Result-style message.
pub fn check_url(url: &str) -> Result<(), String> {
    let (allowed, reason) = is_url_allowed(url);
    if allowed {
        Ok(())
    } else {
        Err(reason.unwrap_or_else(|| "URL blocked by policy".to_string()))
    }
}

/// Reset the policy cache (useful for testing).
pub fn reset_cache() {
    if let Some(cache) = POLICY_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            *guard = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_rule_full_url() {
        assert_eq!(
            normalize_rule("https://www.example.com/path/to/page"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_normalize_rule_wildcard() {
        assert_eq!(
            normalize_rule("*.example.com"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_normalize_rule_bare_domain() {
        assert_eq!(
            normalize_rule("example.com"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_normalize_rule_comment() {
        assert!(normalize_rule("# this is a comment").is_none());
    }

    #[test]
    fn test_normalize_rule_empty() {
        assert!(normalize_rule("  ").is_none());
    }

    #[test]
    fn test_normalize_rule_with_port() {
        assert_eq!(
            normalize_rule("http://example.com:8080/path"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_normalize_rule_www_stripped() {
        assert_eq!(
            normalize_rule("www.malicious-site.com"),
            Some("malicious-site.com".to_string())
        );
    }

    #[test]
    fn test_is_url_allowed_disabled() {
        // Blocklist is disabled by default (no config file in test env)
        let (allowed, reason) = is_url_allowed("https://evil.com/malware");
        assert!(allowed);
        assert!(reason.is_none());
    }

    #[test]
    fn test_check_urls_empty() {
        let (allowed, blocked, reason) = check_urls(&[]);
        assert_eq!(allowed, 0);
        assert_eq!(blocked, 0);
        assert!(reason.is_none());
    }

    #[test]
    fn test_check_urls_all_allowed() {
        // Without config file, all URLs are allowed
        let urls = vec![
            "https://google.com/search".to_string(),
            "https://github.com/repo".to_string(),
        ];
        let (allowed, blocked, reason) = check_urls(&urls);
        assert_eq!(allowed, 2);
        assert_eq!(blocked, 0);
        assert!(reason.is_none());
    }

    #[test]
    fn test_check_url_result() {
        // Without config file, all URLs are allowed
        assert!(check_url("https://any-site.com").is_ok());
    }

    #[test]
    fn test_reset_cache() {
        reset_cache();
        let cache = POLICY_CACHE.get_or_init(|| std::sync::Mutex::new(None));
        let guard = cache.lock().unwrap();
        assert!(guard.is_none());
    }
}
