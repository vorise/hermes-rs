//! Approval system for dangerous commands.
//!
//! Provides heuristic detection of destructive operations that
//! require user approval before execution.

/// Patterns that indicate destructive operations.
const DESTRUCTIVE_PATTERNS: &[&str] = &[
    // File deletion
    "rm",
    "rmdir",
    "unlink",
    // File modification/move
    "mv",
    "move",
    // In-place editing
    "sed -i",
    "perl -i",
    // Disk/file operations
    "truncate",
    "dd",
    "shred",
    // Git destructive operations
    "git reset",
    "git clean",
    "git checkout",
    // Output redirection (overwrite)
    ">",
    // Privilege escalation
    "sudo",
    "su",
    "chmod",
    "chown",
    // Package operations
    "apt remove",
    "apt purge",
    "yum remove",
    "dnf remove",
    "pip uninstall",
    "npm uninstall",
    // Database operations
    "DROP TABLE",
    "DROP DATABASE",
    "DELETE FROM",
    "TRUNCATE TABLE",
];

/// Additional patterns that are potentially dangerous but not always destructive.
const WARNING_PATTERNS: &[&str] = &[
    // Network operations
    "curl",
    "wget",
    "nc",
    "netcat",
    // Process operations
    "kill",
    "pkill",
    "killall",
    // System operations
    "systemctl",
    "service",
    // Environment modification
    "export",
    "unset",
    // Configuration changes
    "crontab",
    "iptables",
    "ufw",
];

/// Check if a command is potentially destructive.
///
/// Returns true if the command contains patterns that could
/// cause irreversible data loss or system changes.
pub fn is_destructive_command(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();

    // Check against destructive patterns
    for pattern in DESTRUCTIVE_PATTERNS {
        if cmd_lower.contains(pattern) {
            return true;
        }
    }

    false
}

/// Check if a command needs a warning (potentially risky).
///
/// Returns true for commands that could have side effects
/// but aren't necessarily destructive.
pub fn needs_warning(cmd: &str) -> bool {
    let cmd_lower = cmd.to_lowercase();

    // Check against warning patterns
    for pattern in WARNING_PATTERNS {
        if cmd_lower.contains(pattern) {
            return true;
        }
    }

    false
}

/// Get the risk level of a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    /// Safe operation, no approval needed.
    Safe,
    /// Potentially risky, may need review.
    Warning,
    /// Destructive, requires approval.
    Destructive,
}

/// Determine the risk level of a command.
pub fn assess_risk(cmd: &str) -> RiskLevel {
    if is_destructive_command(cmd) {
        RiskLevel::Destructive
    } else if needs_warning(cmd) {
        RiskLevel::Warning
    } else {
        RiskLevel::Safe
    }
}

/// Check if a file path operation is potentially destructive.
///
/// Returns true if the operation could modify or delete important files.
pub fn is_destructive_file_op(path: &str, operation: &str) -> bool {
    // Protect critical system paths
    let protected_paths = [
        "/etc/",
        "/usr/",
        "/bin/",
        "/sbin/",
        "/lib/",
        "/var/",
        "/root/",
        "~/.ssh/",
        "~/.gnupg/",
    ];

    let expanded_path = if path.starts_with("~") {
        // Expand home directory
        if let Some(home) = std::env::var("HOME").ok() {
            path.replace("~", &home)
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    // Check if path is in protected areas
    for protected in &protected_paths {
        let expanded_protected = if protected.starts_with("~") {
            if let Some(home) = std::env::var("HOME").ok() {
                protected.replace("~", &home)
            } else {
                protected.to_string()
            }
        } else {
            protected.to_string()
        };

        if expanded_path.starts_with(&expanded_protected) {
            return true;
        }
    }

    // Check operation type
    let destructive_ops = ["delete", "remove", "overwrite", "truncate"];
    for op in &destructive_ops {
        if operation.to_lowercase().contains(op) {
            return true;
        }
    }

    false
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_destructive_rm() {
        assert!(is_destructive_command("rm -rf /tmp/test"));
        assert!(is_destructive_command("rmdir empty_dir"));
    }

    #[test]
    fn test_destructive_git() {
        assert!(is_destructive_command("git reset --hard HEAD"));
        assert!(is_destructive_command("git clean -fdx"));
    }

    #[test]
    fn test_destructive_sed() {
        assert!(is_destructive_command("sed -i 's/old/new/g' file.txt"));
    }

    #[test]
    fn test_safe_commands() {
        assert!(!is_destructive_command("ls -la"));
        assert!(!is_destructive_command("cat file.txt"));
        assert!(!is_destructive_command("echo hello"));
        assert!(!is_destructive_command("pwd"));
    }

    #[test]
    fn test_warning_commands() {
        assert!(needs_warning("curl http://example.com"));
        assert!(needs_warning("wget file.tar.gz"));
        assert!(needs_warning("kill 1234"));
    }

    #[test]
    fn test_assess_risk() {
        assert_eq!(assess_risk("rm file.txt"), RiskLevel::Destructive);
        assert_eq!(assess_risk("curl url"), RiskLevel::Warning);
        assert_eq!(assess_risk("ls"), RiskLevel::Safe);
    }

    #[test]
    fn test_destructive_file_op() {
        assert!(is_destructive_file_op("/etc/passwd", "write"));
        assert!(is_destructive_file_op("/usr/bin/app", "delete"));
        // Home directory files are generally safe to modify
        assert!(!is_destructive_file_op("/tmp/test.txt", "write"));
    }
}