use std::io::Read;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Tirith security scanner verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TirithVerdict {
    Allow,
    Block,
    Warn,
}

/// A single threat finding from Tirith.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TirithFinding {
    pub rule_id: String,
    pub severity: String,
    pub description: String,
    pub detail: Option<String>,
}

/// Full Tirith scan result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TirithResult {
    pub verdict: TirithVerdict,
    pub findings: Vec<TirithFinding>,
    pub summary: String,
}

/// Configuration for the Tirith scanner.
#[derive(Debug, Clone)]
pub struct TirithConfig {
    pub enabled: bool,
    pub path: String,
    pub timeout: u64,
    pub fail_open: bool,
}

impl Default for TirithConfig {
    fn default() -> Self {
        Self {
            enabled: std::env::var("TIRITH_ENABLED")
                .ok()
                .map(|v| v.to_lowercase() != "false" && v != "0")
                .unwrap_or(true),
            path: std::env::var("TIRITH_BIN")
                .ok()
                .unwrap_or_else(|| "tirith".to_string()),
            timeout: std::env::var("TIRITH_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            fail_open: std::env::var("TIRITH_FAIL_OPEN")
                .ok()
                .map(|v| v.to_lowercase() != "false" && v != "0")
                .unwrap_or(true),
        }
    }
}

/// Resolved path to the Tirith binary.
static TIRITH_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Find the Tirith binary, attempting auto-install if not found.
pub fn resolve_tirith(config: &TirithConfig) -> Option<PathBuf> {
    TIRITH_PATH
        .get_or_init(|| {
            // 1. PATH lookup
            if let Some(path) = find_in_path(&config.path) {
                return Some(path);
            }

            // 2. $HERMES_HOME/bin/tirith
            let home = std::env::var("HERMES_HOME")
                .ok()
                .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.hermes")))
                .unwrap_or_default();
            let home_bin = PathBuf::from(&home).join("bin/tirith");
            if home_bin.exists() {
                return Some(home_bin);
            }

            // 3. Auto-install would go here (requires network + GitHub API)
            // For now, return None — caller decides fail-open/closed
            None
        })
        .clone()
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        if p.exists() {
            return Some(p);
        }
        return None;
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(name);
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

/// Run the Tirith security scanner on a shell command.
///
/// Returns `TirithVerdict::Allow` if:
/// - Scanner is disabled
/// - Scanner is unavailable and `fail_open` is true
/// - Scanner returns exit code 0
///
/// Returns `TirithVerdict::Block` if:
/// - Scanner is unavailable and `fail_open` is false
/// - Scanner returns exit code 1
///
/// Returns `TirithVerdict::Warn` if:
/// - Scanner returns exit code 2
pub fn scan_command(command: &str, config: &TirithConfig) -> TirithResult {
    if !config.enabled {
        return TirithResult {
            verdict: TirithVerdict::Allow,
            findings: Vec::new(),
            summary: "Scanner disabled".to_string(),
        };
    }

    let tirith_path = match resolve_tirith(config) {
        Some(p) => p,
        None => {
            // Scanner unavailable — respect fail_open
            return if config.fail_open {
                TirithResult {
                    verdict: TirithVerdict::Allow,
                    findings: Vec::new(),
                    summary: "Scanner not installed, fail-open".to_string(),
                }
            } else {
                TirithResult {
                    verdict: TirithVerdict::Block,
                    findings: Vec::new(),
                    summary: "Scanner not installed, fail-closed".to_string(),
                }
            };
        }
    };

    match run_tirith(&tirith_path, command, config.timeout) {
        Ok(result) => result,
        Err(e) => {
            // Operational failure — respect fail_open
            if config.fail_open {
                TirithResult {
                    verdict: TirithVerdict::Allow,
                    findings: Vec::new(),
                    summary: format!("Scanner error (fail-open): {e}"),
                }
            } else {
                TirithResult {
                    verdict: TirithVerdict::Block,
                    findings: Vec::new(),
                    summary: format!("Scanner error (fail-closed): {e}"),
                }
            }
        }
    }
}

fn run_tirith(
    tirith_path: &std::path::Path,
    command: &str,
    timeout_secs: u64,
) -> Result<TirithResult, String> {
    let mut child = std::process::Command::new(tirith_path)
        .args([
            "check",
            "--json",
            "--non-interactive",
            "--shell",
            "posix",
            "--",
            command,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn tirith: {e}"))?;

    // Wait with timeout
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let exit_code = status.code().unwrap_or(-1);
                let mut stdout = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut stdout);
                }

                let findings = parse_tirith_json(&stdout);
                let summary = findings
                    .iter()
                    .map(|f| f.description.as_str())
                    .collect::<Vec<_>>()
                    .join("; ");
                let summary = if summary.len() > 500 {
                    summary[..500].to_string()
                } else {
                    summary
                };

                let verdict = match exit_code {
                    0 => TirithVerdict::Allow,
                    1 => TirithVerdict::Block,
                    2 => TirithVerdict::Warn,
                    _ => return Err(format!("Unknown tirith exit code: {exit_code}")),
                };

                return Ok(TirithResult {
                    verdict,
                    findings,
                    summary,
                });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err("Tirith timed out".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Failed to wait for tirith: {e}")),
        }
    }
}

fn parse_tirith_json(stdout: &str) -> Vec<TirithFinding> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // Try to parse as JSON array of findings
    #[derive(Deserialize)]
    struct TirithOutput {
        #[serde(default)]
        findings: Vec<RawFinding>,
        #[serde(default)]
        summary: Option<String>,
    }

    #[derive(Deserialize)]
    struct RawFinding {
        #[serde(default)]
        rule_id: String,
        #[serde(default)]
        severity: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        detail: Option<String>,
    }

    // Try parsing as { findings: [...] }
    if let Ok(output) = serde_json::from_str::<TirithOutput>(trimmed) {
        return output
            .findings
            .into_iter()
            .take(50) // Cap at 50 findings
            .map(|f| TirithFinding {
                rule_id: f.rule_id,
                severity: f.severity,
                description: f.description,
                detail: f.detail,
            })
            .collect();
    }

    // Try parsing as direct array
    if let Ok(findings) = serde_json::from_str::<Vec<RawFinding>>(trimmed) {
        return findings
            .into_iter()
            .take(50)
            .map(|f| TirithFinding {
                rule_id: f.rule_id,
                severity: f.severity,
                description: f.description,
                detail: f.detail,
            })
            .collect();
    }

    Vec::new()
}

/// Check all command guards: Tirith scanner + heuristic dangerous command detection.
///
/// Returns the combined verdict with all findings.
pub fn check_all_command_guards(
    command: &str,
    tirith_config: &TirithConfig,
) -> TirithResult {
    // Run Tirith scanner
    let mut result = scan_command(command, tirith_config);

    // Also run heuristic check (always available)
    if crate::approval::is_destructive_command(command) {
        result.findings.push(TirithFinding {
            rule_id: "heuristic-destructive".to_string(),
            severity: "high".to_string(),
            description: "Command matches heuristic dangerous pattern".to_string(),
            detail: None,
        });
        // Heuristic detection alone doesn't block — adds a finding
        if result.verdict == TirithVerdict::Allow {
            result.verdict = TirithVerdict::Warn;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tirith_disabled() {
        let config = TirithConfig {
            enabled: false,
            ..Default::default()
        };
        let result = scan_command("ls -la", &config);
        assert_eq!(result.verdict, TirithVerdict::Allow);
        assert!(result.summary.contains("disabled"));
    }

    #[test]
    fn test_tirith_fail_open() {
        let config = TirithConfig {
            enabled: true,
            fail_open: true,
            path: "nonexistent-tirith-binary".to_string(),
            ..Default::default()
        };
        let result = scan_command("rm -rf /", &config);
        assert_eq!(result.verdict, TirithVerdict::Allow);
        assert!(result.summary.contains("fail-open"));
    }

    #[test]
    fn test_tirith_fail_closed() {
        let config = TirithConfig {
            enabled: true,
            fail_open: false,
            path: "nonexistent-tirith-binary".to_string(),
            ..Default::default()
        };
        let result = scan_command("rm -rf /", &config);
        assert_eq!(result.verdict, TirithVerdict::Block);
        assert!(result.summary.contains("fail-closed"));
    }

    #[test]
    fn test_parse_empty_json() {
        let findings = parse_tirith_json("");
        assert!(findings.is_empty());
    }

    #[test]
    fn test_parse_findings_array() {
        let json = r#"[{"rule_id": "test-1", "severity": "high", "description": "test finding"}]"#;
        let findings = parse_tirith_json(json);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "test-1");
    }

    #[test]
    fn test_parse_findings_object() {
        let json = r#"{"findings": [{"rule_id": "test-2", "severity": "medium", "description": "obj finding"}], "summary": "test"}"#;
        let findings = parse_tirith_json(json);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule_id, "test-2");
    }

    #[test]
    fn test_check_all_guards_heuristic() {
        let config = TirithConfig {
            enabled: false, // Disable tirith so we only test heuristic
            ..Default::default()
        };
        let result = check_all_command_guards("rm -rf /tmp", &config);
        // Heuristic should add a finding
        assert!(!result.findings.is_empty());
    }

    #[test]
    fn test_check_all_guards_safe_command() {
        let config = TirithConfig {
            enabled: false,
            ..Default::default()
        };
        let result = check_all_command_guards("ls -la", &config);
        assert!(result.findings.is_empty());
        assert_eq!(result.verdict, TirithVerdict::Allow);
    }
}
